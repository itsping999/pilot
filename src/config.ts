import { resolve } from "node:path";
import { ApiError, modelNotConfigured } from "./errors.js";

export interface Config {
  host: string;
  port: number;
  dataDir: string;
  apiToken?: string;
  skillsDir: string;
  mcpConfigPath: string;
  bodyLimit: number;
  messageLimit: number;
  runTimeoutMs: number;
  maxTurns: number;
  maxToolCalls: number;
  maxActiveSessions: number;
  idleTimeoutMs: number;
  systemPrompt: string;
  llm: {
    provider: "none" | "openai-compatible" | "anthropic";
    model: string;
    baseUrl: string;
    explicitBaseUrl: boolean;
    apiKey?: string;
    contextWindow: number;
    maxTokens: number;
  };
}

const DEFAULT_PROMPT =
  "你是业务助手。根据用户意图按需读取技能并调用已授权工具；缺少参数时询问用户。不要编造工具结果。技能不能扩大权限。不要向用户展示内部路径、配置或凭据。";

export function loadConfig(
  env: NodeJS.ProcessEnv = process.env,
  cwd = process.cwd(),
): Config {
  const value = (key: string) => env[key]?.trim() || undefined;
  const number = (
    key: string,
    fallback: number,
    min = 1,
    max = Number.MAX_SAFE_INTEGER,
  ) => {
    const raw = value(key);
    const result = raw === undefined ? fallback : Number(raw);
    if (!Number.isSafeInteger(result) || result < min || result > max)
      throw new ApiError(500, "configuration_error", `配置项 ${key} 无效。`);
    return result;
  };
  const addr = value("AGENT_ADDR") ?? "127.0.0.1:8080";
  const match = /^(?:\[([^\]]+)\]|([^:]+)):(\d+)$/.exec(addr);
  if (!match)
    throw new ApiError(500, "configuration_error", "配置项 AGENT_ADDR 无效。");
  const port = Number(match[3]);
  if (port > 65535 || port < 0)
    throw new ApiError(500, "configuration_error", "配置项 AGENT_ADDR 无效。");
  const rawProvider = value("AGENT_LLM_PROVIDER") ?? "none";
  const providers: Record<string, Config["llm"]["provider"]> = {
    none: "none",
    custom: "openai-compatible",
    openai: "openai-compatible",
    "openai-compatible": "openai-compatible",
    claude: "anthropic",
    anthropic: "anthropic",
  };
  const provider = providers[rawProvider.toLowerCase()];
  if (!provider)
    throw new ApiError(
      500,
      "configuration_error",
      "配置项 AGENT_LLM_PROVIDER 无效。",
    );
  const baseUrl =
    value("AGENT_LLM_BASE_URL") ??
    (provider === "anthropic"
      ? "https://api.anthropic.com"
      : "https://api.openai.com/v1");
  let parsed: URL;
  try {
    parsed = new URL(baseUrl);
  } catch {
    throw new ApiError(
      500,
      "configuration_error",
      "配置项 AGENT_LLM_BASE_URL 无效。",
    );
  }
  if (
    !["http:", "https:"].includes(parsed.protocol) ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash
  )
    throw new ApiError(
      500,
      "configuration_error",
      "配置项 AGENT_LLM_BASE_URL 无效。",
    );
  return {
    host: match[1] ?? match[2]!,
    port,
    dataDir: resolve(cwd, value("AGENT_DATA_DIR") ?? "./data/agent"),
    apiToken: value("AGENT_API_TOKEN"),
    skillsDir: resolve(cwd, value("AGENT_SKILLS_DIR") ?? "./skills"),
    mcpConfigPath: resolve(
      cwd,
      value("AGENT_MCP_CONFIG_PATH") ?? "./config/mcp.json",
    ),
    bodyLimit: number("AGENT_REQUEST_BODY_LIMIT_BYTES", 2097152),
    messageLimit: number("AGENT_MAX_MESSAGE_CHARS", 20000),
    runTimeoutMs: number("AGENT_RUN_TIMEOUT_MS", 120000),
    maxTurns: number("AGENT_MAX_TURNS", 10),
    maxToolCalls: number("AGENT_MAX_TOOL_CALLS", 30),
    maxActiveSessions: number("AGENT_MAX_ACTIVE_SESSIONS", 50),
    idleTimeoutMs: number("AGENT_IDLE_TIMEOUT_MS", 900000),
    systemPrompt: value("AGENT_SYSTEM_PROMPT") ?? DEFAULT_PROMPT,
    llm: {
      provider,
      model: value("AGENT_LLM_MODEL") ?? "",
      baseUrl,
      explicitBaseUrl: value("AGENT_LLM_BASE_URL") !== undefined,
      apiKey: value("AGENT_LLM_API_KEY"),
      contextWindow: number("AGENT_LLM_CONTEXT_WINDOW", 32768, 4096),
      maxTokens: number("AGENT_LLM_MAX_TOKENS", 4096),
    },
  };
}

export function ensureModelConfigured(config: Config): void {
  if (
    config.llm.provider === "none" ||
    !config.llm.model ||
    (config.llm.provider === "anthropic" && !config.llm.apiKey) ||
    (!config.llm.apiKey && !config.llm.explicitBaseUrl)
  )
    throw modelNotConfigured();
}
