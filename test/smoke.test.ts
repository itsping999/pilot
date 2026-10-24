import test from "node:test";
import assert from "node:assert/strict";
import { fixture, json, events } from "./helpers.js";

test("real Pi SDK streams a reply, discovers a Skill, invokes scoped HTTP MCP, and persists history", async (t) => {
  const f = await fixture({ mcp: true });
  t.after(() => f.cleanup());
  const health = await f.request("/health");
  assert.equal(health.status, 200);
  const created = await f.request("/api/v1/sessions", "alice", "POST", {
    title: "SDK verification",
  });
  assert.equal(created.status, 201);
  const session = (await json(created)).data;
  const id = session.id as string;
  assert.ok(id);
  assert.equal(session.pi_file, undefined);
  const response = await f.request(
    `/api/v1/sessions/${id}/messages/stream`,
    "alice",
    "POST",
    { message: "auto-skill" },
  );
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type")!, /text\/event-stream/);
  const stream = events(await response.text());
  assert.equal(stream[0]?.type, "run_started");
  assert.equal(stream.at(-1)?.status, "completed", JSON.stringify(stream));
  assert.ok(
    stream.some(
      (event) => event.type === "tool_start" && event.tool === "read",
    ),
  );
  assert.ok(
    stream.some(
      (event) =>
        event.type === "tool_start" && event.tool?.startsWith("mcp__demo__"),
    ),
  );
  assert.ok(
    stream.some(
      (event) => event.type === "token" && event.content?.includes("alice"),
    ),
  );
  assert.equal(f.mcpCalls.length, 1);
  assert.equal(f.mcpCalls[0]?.user, "alice");
  assert.equal(f.mcpCalls[0]?.session, id);
  assert.equal(f.mcpCalls[0]?.authorization, "Bearer test-token-alice");
  assert.ok(
    f.modelRequests[0]?.tools?.every(
      (tool) =>
        tool.function.name === "read" || tool.function.name.startsWith("mcp__"),
    ),
  );
  assert.ok(
    !JSON.stringify(f.modelRequests[0]).includes("GLOBAL_INJECTION_SENTINEL"),
  );
  assert.ok(!JSON.stringify(f.modelRequests[0]).includes("SKILL_MARKER"));
  const messages = (
    await json(await f.request(`/api/v1/sessions/${id}/messages`))
  ).data.messages as { role: string; content: string }[];
  assert.equal(messages.length, 2);
  assert.equal(messages[0]?.content, "auto-skill");
  assert.match(messages[1]!.content, /alice/);
  const runId = stream[0]!.run_id;
  const replay = events(
    await (
      await f.request(`/api/v1/sessions/${id}/runs/${runId}/events?after=1`)
    ).text(),
  );
  assert.ok(replay.every((event) => event.sequence > 1));
  assert.equal(replay.at(-1)?.status, "completed");
  const run = (
    await json(await f.request(`/api/v1/sessions/${id}/runs/${runId}`))
  ).data;
  assert.equal(run.status, "completed");
  assert.match(String(run.reply), /alice/);
  for (const legacy of [
    "/documents",
    "/rag/query",
    "/rag/query/stream",
    "/bot/chat",
    "/bot/chat/stream",
    "/qa",
  ]) {
    assert.equal((await f.request(legacy)).status, 404);
  }
});
