import test from "node:test";
import assert from "node:assert/strict";
import { writeFile, symlink, readFile, unlink } from "node:fs/promises";
import { join } from "node:path";
import { fixture, json, events, waitFor, SERVICE_TOKEN } from "./helpers.js";
import { principal } from "../src/auth.js";

type Fixture = Awaited<ReturnType<typeof fixture>>;
async function session(f: Fixture, user = "alice", tenant = "default") {
  return (
    await json(await f.request("/api/v1/sessions", user, "POST", {}, tenant))
  ).data.id as string;
}
async function ask(
  f: Fixture,
  id: string,
  message: string,
  user = "alice",
  tenant = "default",
  skill?: string,
) {
  const response = await f.request(
    `/api/v1/sessions/${id}/messages/stream`,
    user,
    "POST",
    { message, ...(skill ? { skill } : {}) },
    tenant,
  );
  assert.equal(response.status, 200);
  const result = events(await response.text());
  assert.equal(result.at(-1)?.status, "completed", JSON.stringify(result));
  return result
    .filter((event) => event.type === "token")
    .map((event) => event.content)
    .join("");
}

test("trusted identity is required, and all conversation/run operations enforce user and tenant ownership", async (t) => {
  const f = await fixture();
  t.after(() => f.cleanup());
  assert.equal((await fetch(f.url + "/api/v1/sessions")).status, 401);
  assert.equal(
    (
      await fetch(f.url + "/api/v1/sessions", {
        headers: { Authorization: `Bearer ${SERVICE_TOKEN}` },
      })
    ).status,
    400,
  );
  const a = await session(f),
    b = await session(f, "bob");
  await ask(f, a, "ALICE_PRIVATE");
  const listed = (await json(await f.request("/api/v1/sessions", "bob"))).data
    .sessions as { id: string }[];
  assert.deepEqual(
    listed.map((entry) => entry.id),
    [b],
  );
  const started = (
    await json(
      await f.request(`/api/v1/sessions/${a}/messages`, "alice", "POST", {
        message: "slow owner",
      }),
    )
  ).data.run as { id: string };
  for (const [method, path, body] of [
    ["GET", `/api/v1/sessions/${a}`, undefined],
    ["GET", `/api/v1/sessions/${a}/messages`, undefined],
    ["POST", `/api/v1/sessions/${a}/messages`, { message: "forged" }],
    ["POST", `/api/v1/sessions/${a}/messages/stream`, { message: "forged" }],
    ["GET", `/api/v1/sessions/${a}/runs/${started.id}`, undefined],
    ["GET", `/api/v1/sessions/${a}/runs/${started.id}/events`, undefined],
    ["POST", `/api/v1/sessions/${a}/runs/${started.id}/cancel`, {}],
    ["DELETE", `/api/v1/sessions/${a}`, undefined],
    ["GET", `/api/v1/sessions/${b}/runs/${started.id}`, undefined],
  ] as const) {
    const response = await f.request(path, "bob", method, body);
    assert.equal(response.status, 404, path);
    assert.ok(!(await response.text()).includes("ALICE_PRIVATE"));
  }
  assert.equal(
    (
      await f.request(
        `/api/v1/sessions/${a}`,
        "alice",
        "GET",
        undefined,
        "other-tenant",
      )
    ).status,
    404,
  );
  await f.request(
    `/api/v1/sessions/${a}/runs/${started.id}/cancel`,
    "alice",
    "POST",
    {},
  );
});

test("same-user conversations, different users, and different tenants have separate model context; restart restores only the owned conversation", async (t) => {
  const f = await fixture();
  t.after(() => f.cleanup());
  const a = await session(f),
    second = await session(f),
    b = await session(f, "bob"),
    tenant = await session(f, "alice", "other");
  await ask(f, a, "ALICE_ONLY_SENTINEL");
  assert.ok(!(await ask(f, second, "memory")).includes("ALICE_ONLY_SENTINEL"));
  assert.ok(
    !(await ask(f, b, "memory", "bob")).includes("ALICE_ONLY_SENTINEL"),
  );
  assert.ok(
    !(await ask(f, tenant, "memory", "alice", "other")).includes(
      "ALICE_ONLY_SENTINEL",
    ),
  );
  await f.restart();
  assert.match(await ask(f, a, "after restart"), /ALICE_ONLY_SENTINEL/);
  assert.ok(
    !(await ask(f, second, "after restart")).includes("ALICE_ONLY_SENTINEL"),
  );
  const messages = (
    await json(await f.request(`/api/v1/sessions/${a}/messages`))
  ).data.messages as { role: string; content: string }[];
  assert.equal(messages.filter((entry) => entry.role === "user").length, 2);
  assert.ok(
    messages.every(
      (entry) => !entry.content.includes("GLOBAL_INJECTION_SENTINEL"),
    ),
  );
});

test("missing model fails before run/message persistence, callbacks, or MCP startup", async (t) => {
  const f = await fixture({ configured: false, mcp: true });
  t.after(() => f.cleanup());
  const id = await session(f);
  for (const endpoint of ["messages", "messages/stream"]) {
    const response = await f.request(
      `/api/v1/sessions/${id}/${endpoint}`,
      "alice",
      "POST",
      { message: "use-mcp" },
    );
    assert.equal(response.status, 503);
    assert.equal((await json(response)).code, "model_not_configured");
  }
  assert.equal(f.modelRequests.length, 0);
  assert.equal(f.mcpRequests, 0);
  assert.deepEqual(
    (await json(await f.request(`/api/v1/sessions/${id}/messages`))).data
      .messages,
    [],
  );
  assert.equal(
    f.service.store.session(principal("default", "alice"), id).pi_file,
    null,
  );
});

test("explicit Skills expand in the real SDK while public history keeps user text; SDK management commands are treated as ordinary chat", async (t) => {
  const f = await fixture();
  t.after(() => f.cleanup());
  const id = await session(f);
  const configBefore = await readFile(f.config.mcpConfigPath, "utf8");
  await ask(f, id, "hello", "alice", "default", "test-flow");
  assert.ok(JSON.stringify(f.modelRequests[0]).includes("SKILL_MARKER"));
  const messages = (
    await json(await f.request(`/api/v1/sessions/${id}/messages`))
  ).data.messages as { content: string }[];
  assert.equal(messages[0]?.content, "hello");
  await ask(f, id, "/mcp login demo");
  assert.equal(await readFile(f.config.mcpConfigPath, "utf8"), configBefore);
  const unknown = await f.request(
    `/api/v1/sessions/${id}/messages`,
    "alice",
    "POST",
    { message: "hello", skill: "unknown" },
  );
  assert.equal(unknown.status, 400);
});

test("read tool confines files to the owned workspace and configured skills, including symlink targets", async (t) => {
  const f = await fixture();
  t.after(() => f.cleanup());
  const a = await session(f),
    b = await session(f, "bob");
  await ask(f, a, "prepare");
  await ask(f, b, "prepare", "bob");
  const own = join(f.config.dataDir, "sessions", a, "workspace", "own.txt");
  const other = join(f.config.dataDir, "sessions", b, "workspace", "other.txt");
  await writeFile(own, "OWN_CONTENT");
  await writeFile(other, "BOB_PRIVATE_FILE_SENTINEL");
  await symlink(
    other,
    join(f.config.dataDir, "sessions", a, "workspace", "escape.txt"),
  );
  assert.match(await ask(f, a, `read-path:${own}`), /OWN_CONTENT/);
  assert.ok(
    !(await ask(f, a, `read-path:${other}`)).includes(
      "BOB_PRIVATE_FILE_SENTINEL",
    ),
  );
  assert.ok(
    !(
      await ask(
        f,
        a,
        `read-path:${join(f.config.dataDir, "sessions", a, "workspace", "escape.txt")}`,
      )
    ).includes("BOB_PRIVATE_FILE_SENTINEL"),
  );
  const privateFile = join(f.root, "server-secret.txt");
  await writeFile(privateFile, "OUTSIDE_SECRET_SENTINEL");
  assert.ok(
    !(await ask(f, a, `read-path:${privateFile}`)).includes(
      "OUTSIDE_SECRET_SENTINEL",
    ),
  );
});

test("concurrent sessions and streams do not mix; same-session reentry is rejected and stale cancellation cannot cancel a newer run", async (t) => {
  const f = await fixture();
  t.after(() => f.cleanup());
  const a = await session(f),
    b = await session(f, "bob");
  const started = (
    await json(
      await f.request(`/api/v1/sessions/${a}/messages`, "alice", "POST", {
        message: "slow ALICE_STREAM",
      }),
    )
  ).data.run as { id: string };
  const streamB = await f.request(
    `/api/v1/sessions/${b}/messages/stream`,
    "bob",
    "POST",
    { message: "slow BOB_STREAM" },
  );
  const bodyB = streamB.text();
  await waitFor(() =>
    f.service
      .events(principal("default", "alice"), a, started.id, 0)
      .some((entry) => entry.type === "token"),
  );
  assert.equal(
    (
      await f.request(`/api/v1/sessions/${a}/messages`, "alice", "POST", {
        message: "reentry",
      })
    ).status,
    409,
  );
  await f.request(
    `/api/v1/sessions/${a}/runs/${started.id}/cancel`,
    "alice",
    "POST",
    {},
  );
  await waitFor(
    () =>
      f.service.getRun(principal("default", "alice"), a, started.id).status ===
      "cancelled",
  );
  const parsedB = events(await bodyB);
  assert.equal(parsedB.at(-1)?.status, "completed");
  assert.ok(parsedB.every((entry) => entry.session_id === b));
  assert.ok(!JSON.stringify(parsedB).includes("ALICE_STREAM"));
  const newer = (
    await json(
      await f.request(`/api/v1/sessions/${a}/messages`, "alice", "POST", {
        message: "slow NEW_RUN",
      }),
    )
  ).data.run as { id: string };
  await f.request(
    `/api/v1/sessions/${a}/runs/${started.id}/cancel`,
    "alice",
    "POST",
    {},
  );
  assert.equal(
    f.service.getRun(principal("default", "alice"), a, newer.id).status,
    "running",
  );
  await f.request(
    `/api/v1/sessions/${a}/runs/${newer.id}/cancel`,
    "alice",
    "POST",
    {},
  );
});

test("stdio MCP receives only scoped environment and a distinct workspace for each conversation", async (t) => {
  const previous = process.env.AGENT_API_TOKEN;
  process.env.AGENT_API_TOKEN = "PARENT_CREDENTIAL_SENTINEL";
  const f = await fixture({ stdio: true });
  t.after(async () => {
    await f.cleanup();
    if (previous === undefined) delete process.env.AGENT_API_TOKEN;
    else process.env.AGENT_API_TOKEN = previous;
  });
  const a = await session(f),
    b = await session(f, "bob");
  const answerA = await ask(f, a, "use-mcp alice"),
    answerB = await ask(f, b, "use-mcp bob", "bob");
  const dataA = JSON.parse(answerA.slice("tool-result:".length)) as {
    user: string;
    session: string;
    cwd: string;
    inherited_service_token: boolean;
  };
  const dataB = JSON.parse(
    answerB.slice("tool-result:".length),
  ) as typeof dataA;
  assert.equal(dataA.user, "alice");
  assert.equal(dataB.user, "bob");
  assert.equal(dataA.session, a);
  assert.equal(dataB.session, b);
  assert.notEqual(dataA.cwd, dataB.cwd);
  assert.equal(dataA.inherited_service_token, false);
  assert.equal(dataB.inherited_service_token, false);
});

test("tool budget aborts before executing an extra MCP call; failed provider details stay out of API output", async (t) => {
  const f = await fixture({ mcp: true, overrides: { maxToolCalls: 1 } });
  t.after(() => f.cleanup());
  const id = await session(f);
  const limited = events(
    await (
      await f.request(
        `/api/v1/sessions/${id}/messages/stream`,
        "alice",
        "POST",
        { message: "auto-skill" },
      )
    ).text(),
  );
  assert.equal(limited.at(-1)?.status, "failed");
  assert.ok(limited.some((entry) => entry.code === "run_limit_exceeded"));
  assert.equal(f.mcpCalls.length, 0);
  const failed = await f.request(
    `/api/v1/sessions/${id}/messages/stream`,
    "alice",
    "POST",
    { message: "provider-error" },
  );
  const value = await failed.text();
  assert.equal(events(value).at(-1)?.status, "failed");
  assert.ok(!value.includes("PRIVATE_PROVIDER_SENTINEL"));
});

test("idle runtime eviction preserves context and deletion hides a conversation without exposing internal files", async (t) => {
  const f = await fixture({
    overrides: { idleTimeoutMs: 50, maxActiveSessions: 1 },
  });
  t.after(() => f.cleanup());
  const a = await session(f),
    b = await session(f);
  await ask(f, a, "EVICTED_MEMORY_SENTINEL");
  await ask(f, b, "another session");
  assert.match(await ask(f, a, "restore"), /EVICTED_MEMORY_SENTINEL/);
  const response = await f.request(`/api/v1/sessions/${a}`, "alice", "DELETE");
  assert.equal(response.status, 200);
  assert.equal((await f.request(`/api/v1/sessions/${a}`)).status, 404);
  assert.equal((await f.request(`/api/v1/sessions/${a}/messages`)).status, 404);
  assert.deepEqual(
    (
      (await json(await f.request("/api/v1/sessions"))).data.sessions as {
        id: string;
      }[]
    ).map((entry) => entry.id),
    [b],
  );
});

test("request limits and invalid JSON return JSON errors before creating conversation state", async (t) => {
  const f = await fixture({ overrides: { bodyLimit: 256, messageLimit: 20 } });
  t.after(() => f.cleanup());
  const id = await session(f);
  const large = await f.request(
    `/api/v1/sessions/${id}/messages`,
    "alice",
    "POST",
    { message: "x".repeat(1024) },
  );
  assert.equal(large.status, 413);
  assert.equal((await json(large)).code, "payload_too_large");
  const message = await f.request(
    `/api/v1/sessions/${id}/messages`,
    "alice",
    "POST",
    { message: "x".repeat(21) },
  );
  assert.equal(message.status, 400);
  const malformed = await fetch(f.url + "/api/v1/sessions", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${SERVICE_TOKEN}`,
      "X-User-Id": "alice",
      "Content-Type": "application/json",
    },
    body: "{",
  });
  assert.equal(malformed.status, 400);
  assert.deepEqual(
    (await json(await f.request(`/api/v1/sessions/${id}/messages`))).data
      .messages,
    [],
  );
});

test("unavailable MCP fails before the model is called and a fresh connection can recover the next run", async (t) => {
  const options = { mcp: true, mcpFailure: true };
  const f = await fixture(options);
  t.after(() => f.cleanup());
  const id = await session(f);
  const response = await f.request(
    `/api/v1/sessions/${id}/messages/stream`,
    "alice",
    "POST",
    { message: "use-mcp" },
  );
  const value = await response.text(),
    stream = events(value);
  assert.equal(stream.at(-1)?.status, "failed");
  assert.ok(stream.some((event) => event.code === "mcp_unavailable"));
  assert.ok(!value.includes("PRIVATE_MCP_SENTINEL"));
  assert.equal(f.modelRequests.length, 0);
  options.mcpFailure = false;
  assert.match(await ask(f, id, "use-mcp recover"), /alice/);
});

test("run deadline stops the current execution and leaves the session usable", async (t) => {
  const f = await fixture({ overrides: { runTimeoutMs: 200 } });
  t.after(() => f.cleanup());
  const id = await session(f);
  const response = await f.request(
    `/api/v1/sessions/${id}/messages/stream`,
    "alice",
    "POST",
    { message: "slow deadline" },
  );
  const stream = events(await response.text());
  assert.equal(stream.at(-1)?.status, "failed");
  assert.ok(stream.some((event) => event.code === "run_timeout"));
  assert.match(await ask(f, id, "next request"), /next request/);
});

test("Anthropic-compatible configuration uses the real SDK messages protocol", async (t) => {
  const f = await fixture({ anthropic: true });
  t.after(() => f.cleanup());
  const id = await session(f);
  assert.equal(await ask(f, id, "hello"), "anthropic-compatible-ok");
});

test("missing established Pi context reports storage failure instead of silently resetting conversation history", async (t) => {
  const f = await fixture();
  t.after(() => f.cleanup());
  const id = await session(f);
  await ask(f, id, "PERSISTED_HISTORY");
  const stored = f.service.store.session(principal("default", "alice"), id);
  assert.equal(stored.pi_ready, 1);
  await f.restart();
  await unlink(stored.pi_file!);
  const response = await f.request(
    `/api/v1/sessions/${id}/messages/stream`,
    "alice",
    "POST",
    { message: "must not reset" },
  );
  const stream = events(await response.text());
  assert.equal(stream.at(-1)?.status, "failed");
  assert.ok(stream.some((event) => event.code === "session_storage_error"));
  assert.equal(f.modelRequests.length, 1);
});
