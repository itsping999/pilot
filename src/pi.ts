import { mkdirSync, existsSync, realpathSync } from "node:fs";
import { join } from "node:path";
import {
  InMemoryCredentialStore,
  InMemoryModelsStore,
} from "@earendil-works/pi-ai";
import {
  createAgentSession,
  createMcpExtension,
  DefaultResourceLoader,
  ModelRuntime,
  SessionManager,
  SettingsManager,
  type AgentSession,
  type McpExtensionOptions,
  type ToolDefinition,
} from "@earendil-works/pi-coding-agent";
import type { Config } from "./config.js";
import { ensureModelConfigured } from "./config.js";
import type { Principal } from "./auth.js";
import type { SessionRecord } from "./contracts.js";
import { ApiError } from "./errors.js";
import { contained } from "./skills.js";
import { McpConnections } from "./mcp.js";
import type { CapabilitySnapshot } from "./capabilities.js";

export type RuntimeEvent =
  | { type: "context_persisted" }
  | { type: "token"; content: string }
  | {
      type: "tool_start" | "tool_end";
      tool: string;
      toolCallId: string;
      isError?: boolean;
    }
  | { type: "turn_start" }
  | { type: "assistant_final"; content: string; stopReason: string };

export interface AgentRuntime {
  readonly file: string;
  readonly workspace: string;
  subscribe(listener: (event: RuntimeEvent) => void): () => void;
  prompt(
    text: string,
    skill: string | undefined,
    signal: AbortSignal,
  ): Promise<void>;
  abort(): Promise<void>;
  close(): Promise<void>;
}

export interface RuntimeFactory {
  ensureConfigured(): void;
  create(
    owner: Principal,
    record: SessionRecord,
    snapshot: CapabilitySnapshot,
  ): Promise<AgentRuntime>;
}

class PiRuntime implements AgentRuntime {
  constructor(
    readonly session: AgentSession,
    readonly file: string,
    readonly workspace: string,
    readonly connections: McpConnections,
    readonly context: { signal?: AbortSignal },
  ) {}
  subscribe(listener: (event: RuntimeEvent) => void): () => void {
    return this.session.subscribe((event) => {
      if (
        event.type === "message_update" &&
        event.assistantMessageEvent.type === "text_delta"
      )
        listener({ type: "token", content: event.assistantMessageEvent.delta });
      else if (
        event.type === "tool_execution_start" ||
        event.type === "tool_execution_end"
      )
        listener({
          type:
            event.type === "tool_execution_start" ? "tool_start" : "tool_end",
          tool: event.toolName,
          toolCallId: event.toolCallId,
          isError:
            event.type === "tool_execution_end" ? event.isError : undefined,
        });
      else if (event.type === "turn_start") listener({ type: "turn_start" });
      else if (event.type === "message_end") {
        if (existsSync(this.file)) listener({ type: "context_persisted" });
        if (event.message.role === "assistant")
          listener({
            type: "assistant_final",
            content: event.message.content
              .filter((item) => item.type === "text")
              .map((item) => item.text)
              .join(""),
            stopReason: event.message.stopReason,
          });
      }
    });
  }

  async prompt(
    text: string,
    skill: string | undefined,
    signal: AbortSignal,
  ): Promise<void> {
    this.context.signal = signal;
    await this.connections.waitReady(signal);
    if (signal.aborted)
      throw new ApiError(409, "run_cancelled", "执行已取消。");
    const cancel = () => {
      void this.session.abort().catch(() => undefined);
    };
    signal.addEventListener("abort", cancel, { once: true });
    try {
      await this.session.prompt(skill ? `/skill:${skill} ${text}` : text, {
        expandPromptTemplates: skill !== undefined,
        source: "rpc",
      });
    } finally {
      signal.removeEventListener("abort", cancel);
      this.context.signal = undefined;
    }
  }
  abort(): Promise<void> {
    return this.session.abort();
  }
  async close(): Promise<void> {
    this.session.dispose();
    await this.connections.close();
  }
}

export class PiFactory implements RuntimeFactory {
  constructor(readonly config: Config) {}
  ensureConfigured() {
    ensureModelConfigured(this.config);
  }

  async create(
    owner: Principal,
    record: SessionRecord,
    snapshot: CapabilitySnapshot,
  ): Promise<AgentRuntime> {
    this.ensureConfigured();
    const { skills, mcp } = snapshot;
    const root = join(this.config.dataDir, "sessions", record.id);
    const workspace = join(root, "workspace");
    const agentDir = join(root, "runtime");
    const historyDir = join(root, "history");
    for (const directory of [root, workspace, agentDir, historyDir])
      mkdirSync(directory, { recursive: true, mode: 0o700 });
    if (
      record.pi_file &&
      ((record.pi_ready === 1 && !existsSync(record.pi_file)) ||
        (existsSync(record.pi_file) &&
          !contained(realpathSync(historyDir), realpathSync(record.pi_file))))
    )
      throw new ApiError(
        500,
        "session_storage_error",
        "会话上下文存储不可用。",
      );

    const manager =
      record.pi_file && existsSync(record.pi_file)
        ? SessionManager.open(record.pi_file, historyDir, workspace)
        : SessionManager.create(workspace, historyDir);
    const settings = SettingsManager.inMemory({
      retry: { enabled: false },
      compaction: { enabled: true },
      cacheWarming: "off",
    });
    const modelRuntime = await ModelRuntime.create({
      credentials: new InMemoryCredentialStore(),
      modelsStore: new InMemoryModelsStore(),
      modelsPath: null,
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    const providerId = "agent-service-llm";
    const api =
      this.config.llm.provider === "anthropic"
        ? "anthropic-messages"
        : "openai-completions";
    modelRuntime.registerProvider(providerId, {
      api,
      baseUrl: this.config.llm.baseUrl,
      authHeader: !!this.config.llm.apiKey,
      models: [
        {
          id: this.config.llm.model,
          name: this.config.llm.model,
          api,
          input: ["text"],
          reasoning: false,
          contextWindow: this.config.llm.contextWindow,
          maxTokens: this.config.llm.maxTokens,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          compat:
            api === "openai-completions"
              ? {
                  supportsStore: false,
                  supportsDeveloperRole: false,
                  supportsReasoningEffort: false,
                }
              : undefined,
        },
      ],
    });
    await modelRuntime.setRuntimeApiKey(
      providerId,
      this.config.llm.apiKey ?? "local-endpoint",
    );
    const model = modelRuntime.getModel(providerId, this.config.llm.model);
    if (!model)
      throw new ApiError(503, "model_not_configured", "模型配置不可用。");

    const entries = mcp.entries(owner, record.id);
    const connections = new McpConnections(workspace, entries.length);
    const context: { signal?: AbortSignal } = {};
    const mcpOptions: McpExtensionOptions = {
      loadConfig: () => ({
        servers: entries,
        errors: [],
        autoEnableCodemode: false,
      }),
      createTransport: (entry, cwd, authProvider) =>
        connections.create(entry, cwd, authProvider),
      logPath: process.platform === "win32" ? "NUL" : "/dev/null",
      startupWaitMs: 10000,
      openUrl: () => {
        throw new ApiError(
          502,
          "mcp_auth_required",
          "MCP 服务需要管理员配置认证。",
        );
      },
      updateConfig: () => {
        throw new ApiError(403, "forbidden", "无法通过会话修改 MCP 配置。");
      },
    };
    const loader = new DefaultResourceLoader({
      cwd: workspace,
      agentDir,
      settingsManager: settings,
      noExtensions: true,
      noSkills: true,
      noPromptTemplates: true,
      noThemes: true,
      noContextFiles: true,
      systemPrompt: this.config.systemPrompt,
      skillsOverride: () => ({ skills: skills.entries, diagnostics: [] }),
      extensionFactories: [
        createMcpExtension(mcpOptions),
        (pi) => {
          pi.on("tool_call", () =>
            context.signal && !context.signal.aborted
              ? undefined
              : { block: true, reason: "Run is not active." },
          );
        },
      ],
    });
    try {
      await loader.reload();
      const { session } = await createAgentSession({
        cwd: workspace,
        agentDir,
        model,
        modelRuntime,
        thinkingLevel: "off",
        sessionManager: manager,
        settingsManager: settings,
        resourceLoader: loader,
        tools: [
          "read",
          "mcp__*",
          "list_mcp_resources",
          "list_mcp_resource_templates",
          "read_mcp_resource",
        ],
        // SDK's heterogeneous tool array erases the built-in read renderer's schema type.
        customTools: [skills.readTool(workspace) as unknown as ToolDefinition],
      });
      try {
        await session.bindExtensions({ mode: "rpc", onError: () => undefined });
        const file = manager.getSessionFile();
        if (!file)
          throw new ApiError(
            500,
            "session_storage_error",
            "无法创建会话上下文。",
          );
        return new PiRuntime(session, file, workspace, connections, context);
      } catch (error) {
        session.dispose();
        throw error;
      }
    } catch (error) {
      await connections.close();
      throw error;
    }
  }
}
