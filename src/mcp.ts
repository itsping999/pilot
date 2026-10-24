import { readFileSync, existsSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import {
  StdioTransport,
  StreamableHttpTransport,
  type McpTransport,
  type JsonRpcMessage,
  type AuthProvider,
} from "@earendil-works/pi-mcp";
import type { McpExtensionOptions } from "@earendil-works/pi-coding-agent";
import type { Principal } from "./auth.js";
import { ApiError } from "./errors.js";

type LoadedConfig = ReturnType<NonNullable<McpExtensionOptions["loadConfig"]>>;
type Entry = LoadedConfig["servers"][number];
type Value =
  | string
  | {
      env?: string;
      context?: "user_id" | "tenant_id" | "session_id";
      prefix?: string;
    };
interface Definition {
  name: string;
  command?: string;
  args?: string[];
  url?: string;
  values: Record<string, Value>;
  allow: "*" | { tenant_id: string; user_id: string }[];
  enabled: boolean;
  timeout: number;
  toolExposure?: Record<string, "direct" | "hidden">;
}

const configError = () =>
  new ApiError(500, "configuration_error", "MCP 配置无效或所需环境变量缺失。");
const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export class McpCatalog {
  readonly definitions: Definition[] = [];
  constructor(
    filename: string,
    readonly env: NodeJS.ProcessEnv = process.env,
    configuration?: unknown,
  ) {
    if (configuration === undefined && !existsSync(filename)) return;
    try {
      const raw: unknown =
        configuration ?? JSON.parse(readFileSync(filename, "utf8"));
      if (!record(raw) || !record(raw.mcpServers)) throw configError();
      const namespaces = new Set<string>();
      for (const [name, value] of Object.entries(raw.mcpServers)) {
        if (
          !/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(name) ||
          !record(value) ||
          namespaces.has(name.replaceAll("-", "_"))
        )
          throw configError();
        namespaces.add(name.replaceAll("-", "_"));
        const command = value.command;
        const url = value.url;
        if ((typeof command === "string") === (typeof url === "string"))
          throw configError();
        if (
          typeof command === "string" &&
          (!command ||
            (value.args !== undefined &&
              (!Array.isArray(value.args) ||
                value.args.some((arg) => typeof arg !== "string"))))
        )
          throw configError();
        if (typeof url === "string") {
          const parsed = new URL(url);
          if (
            !["http:", "https:"].includes(parsed.protocol) ||
            parsed.username ||
            parsed.password ||
            parsed.hash
          )
            throw configError();
        }
        const values = (command ? value.env : value.headers) ?? {};
        if (!record(values)) throw configError();
        for (const spec of Object.values(values)) {
          if (typeof spec === "string") {
            if (spec.startsWith("!")) throw configError();
            continue;
          }
          if (
            !record(spec) ||
            (typeof spec.env === "string") ===
              (typeof spec.context === "string") ||
            (spec.env !== undefined &&
              !/^[A-Za-z_][A-Za-z0-9_]*$/.test(String(spec.env))) ||
            (spec.context !== undefined &&
              !["user_id", "tenant_id", "session_id"].includes(
                String(spec.context),
              )) ||
            (spec.prefix !== undefined && typeof spec.prefix !== "string")
          )
            throw configError();
        }
        const allow = value.allow ?? [];
        if (
          allow !== "*" &&
          (!Array.isArray(allow) ||
            allow.some(
              (entry) =>
                !record(entry) ||
                typeof entry.user_id !== "string" ||
                typeof entry.tenant_id !== "string",
            ))
        )
          throw configError();
        const timeout = value.timeout ?? 30;
        if (
          typeof timeout !== "number" ||
          !Number.isSafeInteger(timeout) ||
          timeout < 1 ||
          timeout > 300
        )
          throw configError();
        if (value.enabled !== undefined && typeof value.enabled !== "boolean")
          throw configError();
        if (
          value.toolExposure !== undefined &&
          (!record(value.toolExposure) ||
            Object.values(value.toolExposure).some(
              (entry) => entry !== "direct" && entry !== "hidden",
            ))
        )
          throw configError();
        this.definitions.push({
          name,
          command: typeof command === "string" ? command : undefined,
          url: typeof url === "string" ? url : undefined,
          args: value.args as string[] | undefined,
          values: values as Record<string, Value>,
          allow: allow as Definition["allow"],
          enabled: value.enabled !== false,
          timeout,
          toolExposure: value.toolExposure as Definition["toolExposure"],
        });
      }
    } catch {
      throw configError();
    }
  }

  allowed(owner: Principal): Definition[] {
    return this.definitions.filter(
      (entry) =>
        entry.enabled &&
        (entry.allow === "*" ||
          entry.allow.some(
            (rule) =>
              rule.tenant_id === owner.tenantId &&
              rule.user_id === owner.userId,
          )),
    );
  }

  catalogue(owner: Principal) {
    return this.allowed(owner).map((entry) => ({
      name: entry.name,
      transport: entry.command ? "stdio" : "http",
    }));
  }

  entries(owner: Principal, sessionId: string): Entry[] {
    const context = {
      user_id: owner.userId,
      tenant_id: owner.tenantId,
      session_id: sessionId,
    };
    return this.allowed(owner).map((definition) => {
      const values: Record<string, string> = {};
      for (const [key, spec] of Object.entries(definition.values)) {
        let value: string | undefined;
        if (typeof spec === "string")
          value = spec.replace(
            /\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g,
            (_, variable: string) => {
              const found = this.env[variable];
              if (found === undefined || found === "") throw configError();
              return found;
            },
          );
        else value = spec.context ? context[spec.context] : this.env[spec.env!];
        if (
          value === undefined ||
          (typeof spec === "object" && spec.env && value === "")
        )
          throw configError();
        values[key] =
          (typeof spec === "object" ? (spec.prefix ?? "") : "") + value;
      }
      return {
        name: definition.name,
        source: "service-configuration",
        scope: "global",
        config: definition.command
          ? {
              type: "stdio",
              command: definition.command,
              args: definition.args,
              env: values,
              exposure: "direct",
              timeout: definition.timeout,
              toolExposure: definition.toolExposure,
            }
          : {
              type: "http",
              url: definition.url!,
              headers: values,
              exposure: "direct",
              timeout: definition.timeout,
              toolExposure: definition.toolExposure,
            },
      };
    });
  }
}

/** Observe only protocol readiness, never tool arguments, results, or credentials. */
class ObservedTransport implements McpTransport {
  readonly ready: Promise<void>;
  private resolve!: () => void;
  private reject!: (error: Error) => void;
  private requests = new Map<string | number, string>();
  private settled = false;
  constructor(readonly base: McpTransport) {
    this.ready = new Promise((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
    });
    void this.ready.catch(() => undefined);
    base.onMessage((message) => {
      if (!("id" in message) || message.id === null || "method" in message)
        return;
      const method = this.requests.get(message.id);
      this.requests.delete(message.id);
      if ("error" in message) {
        this.fail();
        return;
      }
      if (method === "initialize" && "result" in message) {
        const result = message.result as { capabilities?: { tools?: unknown } };
        if (!result.capabilities?.tools) this.finish();
      }
      if (method === "tools/list" && "result" in message) this.finish();
    });
    base.onError(() => this.fail());
    base.onClose(() => this.fail());
  }
  private finish() {
    if (!this.settled) {
      this.settled = true;
      this.resolve();
    }
  }
  private fail() {
    if (!this.settled) {
      this.settled = true;
      this.reject(new Error("MCP unavailable"));
    }
  }
  start() {
    return this.base.start().catch((error: unknown) => {
      this.fail();
      throw error;
    });
  }
  send(message: JsonRpcMessage) {
    if ("method" in message && "id" in message && message.id !== null)
      this.requests.set(message.id, message.method);
    return this.base.send(message).catch((error: unknown) => {
      this.fail();
      throw error;
    });
  }
  close() {
    this.fail();
    return this.base.close();
  }
  onMessage(listener: Parameters<McpTransport["onMessage"]>[0]) {
    return this.base.onMessage(listener);
  }
  onError(listener: Parameters<McpTransport["onError"]>[0]) {
    return this.base.onError(listener);
  }
  onClose(listener: Parameters<McpTransport["onClose"]>[0]) {
    return this.base.onClose(listener);
  }
  setProtocolVersion(version: string) {
    this.base.setProtocolVersion?.(version);
  }
}

export class McpConnections {
  private transports: ObservedTransport[] = [];
  private created: Promise<void>;
  private resolveCreated!: () => void;
  private closed = false;
  constructor(
    readonly workspace: string,
    readonly expected: number,
  ) {
    this.created = new Promise((resolve) => {
      this.resolveCreated = resolve;
    });
    if (expected === 0) this.resolveCreated();
  }
  create(
    entry: Entry,
    _cwd: string,
    authProvider: AuthProvider | undefined,
  ): McpTransport {
    if (this.closed) throw new Error("MCP connection closed");
    const config = entry.config;
    let base: McpTransport;
    if ("command" in config) {
      const temporary = join(this.workspace, "tmp");
      mkdirSync(temporary, { recursive: true, mode: 0o700 });
      base = new StdioTransport({
        command: config.command,
        args: config.args,
        cwd: this.workspace,
        inheritEnv: false,
        env: {
          PATH: process.env.PATH ?? dirname(process.execPath),
          HOME: this.workspace,
          TMPDIR: temporary,
          LANG: "C.UTF-8",
          ...config.env,
        },
        stderr: "pipe",
        onStderr: () => undefined,
      });
    } else
      base = new StreamableHttpTransport({
        url: config.url,
        headers: config.headers,
        authProvider,
      });
    const transport = new ObservedTransport(base);
    this.transports.push(transport);
    if (this.transports.length >= this.expected) this.resolveCreated();
    return transport;
  }

  async waitReady(signal: AbortSignal): Promise<void> {
    if (signal.aborted)
      throw new ApiError(409, "run_cancelled", "执行已取消。");
    let timer: ReturnType<typeof setTimeout> | undefined;
    let listener: (() => void) | undefined;
    try {
      await Promise.race([
        (async () => {
          await this.created;
          await Promise.all(
            this.transports.map((transport) => transport.ready),
          );
        })(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error("MCP unavailable")), 10000);
          listener = () =>
            reject(new ApiError(409, "run_cancelled", "执行已取消。"));
          signal.addEventListener("abort", listener, { once: true });
        }),
      ]);
    } catch (error) {
      if (error instanceof ApiError) throw error;
      throw new ApiError(502, "mcp_unavailable", "配置的 MCP 服务暂时不可用。");
    } finally {
      if (timer) clearTimeout(timer);
      if (listener) signal.removeEventListener("abort", listener);
    }
  }

  async close(): Promise<void> {
    this.closed = true;
    await Promise.allSettled(
      this.transports.map((transport) => transport.close()),
    );
  }
}
