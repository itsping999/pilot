import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { waitFor, json, SERVICE_TOKEN } from "./helpers.js";

test("real server entrypoint serves authenticated APIs and releases its data lock on SIGTERM", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "agent-bootstrap-"));
  const child = spawn(
    process.execPath,
    [new URL("../src/main.js", import.meta.url).pathname],
    {
      env: {
        ...process.env,
        AGENT_ADDR: "127.0.0.1:0",
        AGENT_DATA_DIR: join(root, "data"),
        AGENT_API_TOKEN: SERVICE_TOKEN,
        AGENT_ADMIN_TOKEN: "",
        AGENT_SKILLS_DIR: new URL("../../skills", import.meta.url).pathname,
        AGENT_MCP_CONFIG_PATH: join(root, "none.json"),
        AGENT_LLM_PROVIDER: "none",
        AGENT_LLM_MODEL: "",
        AGENT_LLM_API_KEY: "",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let stdout = "",
    stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk.toString();
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });
  const exited = new Promise<number | null>((resolve) =>
    child.once("exit", resolve),
  );
  t.after(async () => {
    if (child.exitCode === null) child.kill("SIGTERM");
    await exited;
    await rm(root, { recursive: true, force: true });
  });
  await waitFor(
    () => stdout.includes('"event":"listening"') || child.exitCode !== null,
  );
  assert.equal(child.exitCode, null, stdout + stderr);
  const line = stdout
    .trim()
    .split("\n")
    .find((entry) => entry.includes('"event":"listening"'))!;
  const port = (JSON.parse(line) as { port: number }).port;
  const base = `http://127.0.0.1:${port}`;
  assert.equal((await fetch(base + "/health")).status, 200);
  const headers = {
    Authorization: `Bearer ${SERVICE_TOKEN}`,
    "X-User-Id": "alice",
    "Content-Type": "application/json",
  };
  const created = await fetch(base + "/api/v1/sessions", {
    method: "POST",
    headers,
    body: "{}",
  });
  assert.equal(created.status, 201);
  const id = (await json(created)).data.id as string;
  const response = await fetch(
    `${base}/api/v1/sessions/${id}/messages/stream`,
    { method: "POST", headers, body: JSON.stringify({ message: "hello" }) },
  );
  assert.equal(response.status, 503);
  assert.equal((await json(response)).code, "model_not_configured");
  const reloadGeneration = stdout;
  child.kill("SIGHUP");
  await waitFor(
    () =>
      stdout !== reloadGeneration &&
      stdout.includes('"event":"capabilities_reloaded"'),
  );
  assert.equal((await fetch(base + "/health")).status, 200);
  child.kill("SIGTERM");
  assert.equal(await exited, 0);
  assert.ok(stdout.includes('"event":"stopped"'));
});
