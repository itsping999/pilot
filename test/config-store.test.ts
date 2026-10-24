import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadConfig, ensureModelConfigured } from "../src/config.js";
import { principal, authenticate } from "../src/auth.js";
import { ApiError } from "../src/errors.js";
import { Store } from "../src/store.js";
import { McpCatalog } from "../src/mcp.js";

test("configuration has no implicit model, isolates data paths, validates limits, and supports explicit provider aliases", () => {
  const config = loadConfig({}, "/tmp/example-agent");
  assert.equal(config.host, "127.0.0.1");
  assert.equal(config.port, 8080);
  assert.equal(config.dataDir, "/tmp/example-agent/data/agent");
  assert.equal(config.llm.provider, "none");
  assert.equal(config.llm.model, "");
  assert.throws(
    () => ensureModelConfigured(config),
    (error: unknown) =>
      error instanceof ApiError && error.code === "model_not_configured",
  );
  assert.throws(() => loadConfig({ AGENT_MAX_TURNS: "-1" }), /AGENT_MAX_TURNS/);
  assert.throws(
    () => loadConfig({ AGENT_ADDR: "127.0.0.1:70000" }),
    /AGENT_ADDR/,
  );
  assert.throws(
    () => loadConfig({ AGENT_LLM_PROVIDER: "typo" }),
    /AGENT_LLM_PROVIDER/,
  );
  assert.throws(
    () =>
      loadConfig({
        AGENT_LLM_BASE_URL: "https://user:password@example.invalid",
      }),
    /AGENT_LLM_BASE_URL/,
  );
  const local = loadConfig({
    AGENT_ADDR: "[::1]:0",
    AGENT_LLM_PROVIDER: "custom",
    AGENT_LLM_MODEL: "installed-model",
    AGENT_LLM_BASE_URL: "http://127.0.0.1:11434/v1",
  });
  assert.equal(local.host, "::1");
  assert.equal(local.llm.provider, "openai-compatible");
  assert.doesNotThrow(() => ensureModelConfigured(local));
  const claude = loadConfig({
    AGENT_LLM_PROVIDER: "claude",
    AGENT_LLM_MODEL: "installed-model",
  });
  assert.throws(
    () => ensureModelConfigured(claude),
    (error: unknown) =>
      error instanceof ApiError && error.code === "model_not_configured",
  );
  assert.throws(
    () =>
      authenticate(
        { authorization: "Bearer wrong", "x-user-id": "alice" },
        { ...config, apiToken: "correct" },
      ),
    (error: unknown) => error instanceof ApiError && error.status === 401,
  );
  assert.throws(
    () =>
      authenticate(
        { authorization: "Bearer correct", "x-user-id": "../../other" },
        { ...config, apiToken: "correct" },
      ),
    (error: unknown) => error instanceof ApiError && error.status === 400,
  );
  assert.notEqual(principal("a:b", "c").key, principal("a", "b:c").key);
});

test("SQLite process lock prevents concurrent writers, and restart marks an unfinished run interrupted instead of replaying it", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "agent-store-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const owner = principal("default", "alice"),
    other = principal("default", "bob");
  const first = new Store(directory);
  const session = first.createSession(owner, "Interrupted run");
  const run = first.startRun(
    owner,
    session.id,
    "Never replay a business operation",
  );
  assert.throws(
    () => new Store(directory),
    (error: unknown) =>
      error instanceof ApiError && error.code === "data_directory_in_use",
  );
  assert.throws(
    () => first.session(other, session.id),
    (error: unknown) => error instanceof ApiError && error.status === 404,
  );
  first.close();
  const restored = new Store(directory);
  try {
    assert.equal(restored.run(owner, session.id, run.id).status, "interrupted");
    assert.equal(
      restored.run(owner, session.id, run.id).error_code,
      "service_restarted",
    );
    assert.equal(
      restored.events(owner, session.id, run.id).at(-1)?.status,
      "interrupted",
    );
    assert.equal(restored.messages(owner, session.id).length, 1);
  } finally {
    restored.close();
  }
});

test("MCP access defaults closed, respects tenant/user rules, and keeps credentials out of catalogues", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "agent-mcp-policy-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const file = join(directory, "mcp.json");
  await writeFile(
    file,
    JSON.stringify({
      mcpServers: {
        restricted: {
          url: "https://example.invalid/mcp",
          allow: [{ tenant_id: "default", user_id: "alice" }],
          headers: {
            Authorization: { env: "MCP_TOKEN", prefix: "Bearer " },
            "X-User-Id": { context: "user_id" },
          },
        },
        unavailable: { url: "https://example.invalid/mcp" },
      },
    }),
  );
  const catalog = new McpCatalog(file, { MCP_TOKEN: "PRIVATE_TOKEN_SENTINEL" });
  const alice = principal("default", "alice");
  assert.equal(catalog.catalogue(alice).length, 1);
  assert.equal(catalog.catalogue(principal("default", "bob")).length, 0);
  assert.equal(catalog.catalogue(principal("other", "alice")).length, 0);
  assert.ok(
    !JSON.stringify(catalog.catalogue(alice)).includes(
      "PRIVATE_TOKEN_SENTINEL",
    ),
  );
  const entry = catalog.entries(alice, "example-session")[0]!;
  assert.ok("headers" in entry.config);
  if ("headers" in entry.config) {
    assert.equal(
      entry.config.headers?.Authorization,
      "Bearer PRIVATE_TOKEN_SENTINEL",
    );
    assert.equal(entry.config.headers?.["X-User-Id"], "alice");
  }
  assert.throws(
    () => new McpCatalog(file, {}).entries(alice, "session"),
    (error: unknown) =>
      error instanceof ApiError && error.code === "configuration_error",
  );
  await writeFile(
    file,
    JSON.stringify({
      mcpServers: {
        "a-b": { url: "https://example.invalid" },
        a_b: { url: "https://example.invalid" },
      },
    }),
  );
  assert.throws(() => new McpCatalog(file), /MCP/);
});
