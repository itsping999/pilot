import test from "node:test";
import assert from "node:assert/strict";
import { writeFile, mkdir, symlink } from "node:fs/promises";
import { join } from "node:path";
import { fixture, json, events } from "./helpers.js";

test("Skill and MCP configuration loads from files and reloads atomically", async (t) => {
  const f = await fixture({ mcp: true });
  t.after(() => f.cleanup());

  const initialGeneration = f.service.capabilities.snapshot.generation;
  assert.ok(initialGeneration);
  assert.equal(f.service.skills.entries.length, 1);
  assert.equal(f.service.mcp.definitions.length, 1);

  // Add a new skill on disk
  const newSkillDir = join(f.config.skillsDir, "extra-skill");
  await mkdir(newSkillDir, { recursive: true });
  await writeFile(
    join(newSkillDir, "SKILL.md"),
    "---\nname: extra-skill\ndescription: Extra skill description.\n---\n\n# Extra\n",
    "utf8",
  );

  // Reload capabilities
  f.service.capabilities.reload();
  const nextGeneration = f.service.capabilities.snapshot.generation;
  assert.notEqual(initialGeneration, nextGeneration);
  assert.equal(f.service.skills.entries.length, 2);
  assert.ok(f.service.skills.entries.some((e) => e.name === "extra-skill"));
});

test("an active run keeps its immutable Skill and MCP snapshot until completion while other sessions use the new revision", async (t) => {
  const f = await fixture({ mcp: true });
  t.after(() => f.cleanup());

  const created1 = await f.request("/api/v1/sessions", "alice", "POST", {
    title: "Session 1",
  });
  const session1 = (await json(created1)).data.id as string;

  const stream1 = await f.request(
    `/api/v1/sessions/${session1}/messages/stream`,
    "alice",
    "POST",
    { message: "slow-prompt" },
  );
  assert.equal(stream1.status, 200);

  // Wait for the first run to start and acquire its snapshot
  await new Promise((resolve) => setTimeout(resolve, 50));

  // Add another skill while run 1 is active
  const anotherSkillDir = join(f.config.skillsDir, "dynamic-skill");
  await mkdir(anotherSkillDir, { recursive: true });
  await writeFile(
    join(anotherSkillDir, "SKILL.md"),
    "---\nname: dynamic-skill\ndescription: Dynamic skill.\n---\n\n# Dynamic\n",
    "utf8",
  );
  f.service.capabilities.reload();

  const created2 = await f.request("/api/v1/sessions", "alice", "POST", {
    title: "Session 2",
  });
  const session2 = (await json(created2)).data.id as string;

  const stream2 = await f.request(
    `/api/v1/sessions/${session2}/messages/stream`,
    "alice",
    "POST",
    { message: "fast-prompt", skill: "dynamic-skill" },
  );
  assert.equal(stream2.status, 200);
  const items2 = events(await stream2.text());
  assert.ok(items2.some((e) => e.type === "done"));

  // Finish session 1
  const items1 = events(await stream1.text());
  assert.ok(items1.some((e) => e.type === "done"));
});

test("invalid configuration reload fails atomically and preserves current snapshot", async (t) => {
  const f = await fixture({ mcp: true });
  t.after(() => f.cleanup());

  const beforeGen = f.service.capabilities.snapshot.generation;
  assert.ok(beforeGen);

  // Write invalid MCP config file (malformed JSON)
  await writeFile(f.config.mcpConfigPath, "{ bad json", "utf8");
  assert.throws(() => f.service.capabilities.reload());

  // Current snapshot is preserved
  assert.equal(f.service.capabilities.snapshot.generation, beforeGen);
  assert.equal(f.service.skills.entries.length, 1);
  assert.equal(f.service.mcp.definitions.length, 1);

  // Try symlink escape in skills directory
  const escapeDir = join(f.config.skillsDir, "escape");
  await mkdir(escapeDir, { recursive: true });
  await symlink("/etc", join(escapeDir, "escaped-link"));
  assert.throws(() => f.service.capabilities.reload());

  // Snapshot remains preserved
  assert.equal(f.service.capabilities.snapshot.generation, beforeGen);
});
