import { randomUUID } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
} from "node:fs";
import { join } from "node:path";
import type { Config } from "./config.js";
import { ApiError } from "./errors.js";
import { McpCatalog } from "./mcp.js";
import { Skills, contained } from "./skills.js";

export interface CapabilitySnapshot {
  generation: string;
  skills: Skills;
  mcp: McpCatalog;
  directory: string;
  references: number;
}

const invalidConfiguration = () =>
  new ApiError(500, "configuration_error", "能力配置无效，未发布任何变更。");
const MAX_FILE_BYTES = 1024 * 1024;

/** Copy only the configured tree, reject escapes/cycles, and bound snapshot work. */
function copyTree(
  source: string,
  target: string,
  root: string,
  budget: { files: number; bytes: number },
  ancestors = new Set<string>(),
) {
  const actual = realpathSync(source);
  if (!contained(root, actual) || ancestors.has(actual))
    throw invalidConfiguration();
  const info = statSync(actual);
  if (++budget.files > 5000) throw invalidConfiguration();
  if (info.isDirectory()) {
    mkdirSync(target, { recursive: true, mode: 0o700 });
    const parents = new Set(ancestors).add(actual);
    for (const entry of readdirSync(actual)) {
      if ([".git", "node_modules"].includes(entry)) continue;
      copyTree(join(actual, entry), join(target, entry), root, budget, parents);
    }
  } else if (info.isFile()) {
    budget.bytes += info.size;
    if (info.size > MAX_FILE_BYTES || budget.bytes > 32 * MAX_FILE_BYTES)
      throw invalidConfiguration();
    copyFileSync(actual, target);
  } else throw invalidConfiguration();
}

/** Pure configuration-driven registry for Skills and MCP with atomic snapshot reloading. */
export class CapabilityRegistry {
  readonly directory: string;
  private readonly snapshots = new Set<CapabilitySnapshot>();
  private current: CapabilitySnapshot;
  private readonly listeners = new Set<() => void>();

  constructor(
    readonly config: Config,
    readonly env: NodeJS.ProcessEnv = process.env,
  ) {
    mkdirSync(join(config.dataDir, "capabilities"), {
      recursive: true,
      mode: 0o700,
    });
    this.directory = realpathSync(join(config.dataDir, "capabilities"));
    if (!contained(realpathSync(config.dataDir), this.directory))
      throw invalidConfiguration();
    this.current = this.prepare();
  }

  get snapshot() {
    return this.current;
  }
  acquire(snapshot = this.current) {
    snapshot.references++;
    return snapshot;
  }
  release(snapshot: CapabilitySnapshot) {
    snapshot.references--;
    if (snapshot.references < 0) throw new Error("Unbalanced capability lease");
    if (snapshot !== this.current && snapshot.references === 0)
      this.remove(snapshot);
  }
  onChange(listener: () => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  reload() {
    const next = this.prepare();
    const previous = this.current;
    this.current = next;
    if (previous.references === 0) this.remove(previous);
    for (const listener of this.listeners) listener();
  }

  close() {
    for (const snapshot of this.snapshots) this.remove(snapshot);
    this.listeners.clear();
  }

  private prepare(): CapabilitySnapshot {
    const directory = mkdtempSync(join(this.directory, ".snapshot-"));
    try {
      const sourceRoot = realpathSync(this.config.skillsDir);
      const active = join(directory, "active");
      copyTree(sourceRoot, active, sourceRoot, { files: 0, bytes: 0 });
      const skills = new Skills(active);
      if (skills.entries.length > 200) throw invalidConfiguration();

      const mcp = existsSync(this.config.mcpConfigPath)
        ? new McpCatalog(this.config.mcpConfigPath, { ...this.env })
        : new McpCatalog("", { ...this.env }, { mcpServers: {} });
      if (mcp.definitions.length > 200) throw invalidConfiguration();

      const snapshot: CapabilitySnapshot = {
        generation: randomUUID(),
        directory,
        references: 0,
        skills,
        mcp,
      };
      this.snapshots.add(snapshot);
      return snapshot;
    } catch {
      rmSync(directory, { recursive: true, force: true });
      throw invalidConfiguration();
    }
  }

  private remove(snapshot: CapabilitySnapshot) {
    if (this.snapshots.delete(snapshot))
      rmSync(snapshot.directory, { recursive: true, force: true });
  }
}
