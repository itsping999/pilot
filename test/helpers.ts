import { createServer, type ServerResponse } from "node:http";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { loadConfig, type Config } from "../src/config.js";
import { Store } from "../src/store.js";
import { CapabilityRegistry } from "../src/capabilities.js";
import { PiFactory } from "../src/pi.js";
import { AgentService } from "../src/service.js";
import { createHttpServer } from "../src/http.js";
import type { RunEvent } from "../src/contracts.js";

export const SERVICE_TOKEN = "test-service-token";
interface ModelRequest {
  messages: {
    role: string;
    content: unknown;
    tool_calls?: { function: { name: string } }[];
  }[];
  tools?: { function: { name: string } }[];
}

export function text(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value))
    return value
      .map((part) => (typeof part?.text === "string" ? part.text : ""))
      .join("");
  return "";
}

async function requestBody(
  request: AsyncIterable<Buffer | string>,
): Promise<Record<string, unknown>> {
  const parts: Buffer[] = [];
  for await (const part of request) parts.push(Buffer.from(part));
  return JSON.parse(Buffer.concat(parts).toString("utf8")) as Record<
    string,
    unknown
  >;
}

async function listen(
  server: ReturnType<typeof createServer>,
): Promise<string> {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("Missing address");
  return `http://127.0.0.1:${address.port}`;
}

async function close(server: ReturnType<typeof createServer>) {
  server.closeAllConnections();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}

function completion(
  response: ServerResponse,
  value: string | { name: string; args: Record<string, unknown> },
  slow = false,
) {
  response.writeHead(200, { "Content-Type": "text/event-stream" });
  const id = randomUUID();
  const chunk = (delta: unknown, finish: string | null = null) => {
    if (!response.destroyed)
      response.write(
        `data: ${JSON.stringify({ id, object: "chat.completion.chunk", created: 0, model: "fake-agent", choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`,
      );
  };
  const done = () => {
    chunk({}, typeof value === "string" ? "stop" : "tool_calls");
    response.end("data: [DONE]\n\n");
  };
  chunk({ role: "assistant" });
  if (typeof value !== "string") {
    chunk({
      tool_calls: [
        {
          index: 0,
          id: `call_${id}`,
          type: "function",
          function: { name: value.name, arguments: JSON.stringify(value.args) },
        },
      ],
    });
    done();
    return;
  }
  if (!slow) {
    chunk({ content: value });
    done();
    return;
  }
  let count = 0;
  const timer = setInterval(() => {
    chunk({ content: `${value}:${++count} ` });
    if (count === 30) {
      clearInterval(timer);
      done();
    }
  }, 30);
  response.on("close", () => clearInterval(timer));
}

export interface FixtureOptions {
  mcp?: boolean;
  mcpFailure?: boolean;
  stdio?: boolean;
  anthropic?: boolean;
  configured?: boolean;
  overrides?: Partial<Config>;
}

export async function fixture(options: FixtureOptions = {}) {
  const root = await mkdtemp(join(tmpdir(), "pi-agent-test-"));
  const skillRoot = join(root, "skills");
  const skillFile = join(skillRoot, "test-flow", "SKILL.md");
  await mkdir(join(skillRoot, "test-flow"), { recursive: true });
  await writeFile(
    skillFile,
    "---\nname: test-flow\ndescription: Use this skill for the integration workflow.\n---\n# Workflow\nSKILL_MARKER: read the skill, call an authorized MCP echo tool, and report the real result.\n",
  );
  await writeFile(
    join(root, "AGENTS.md"),
    "GLOBAL_INJECTION_SENTINEL: this file must not be discovered by the service.",
  );
  const mcpCalls: {
    user: string;
    tenant: string;
    session: string;
    authorization: string;
    args: unknown;
  }[] = [];
  let mcpRequests = 0;
  const mcpServer = createServer((request, response) => {
    if (request.method === "GET") {
      response.writeHead(405).end();
      return;
    }
    if (request.method === "DELETE") {
      response.writeHead(200).end();
      return;
    }
    void requestBody(request)
      .then((body) => {
        mcpRequests++;
        if (options.mcpFailure) {
          response.writeHead(401).end("PRIVATE_MCP_SENTINEL");
          return;
        }
        if (body.id === undefined) {
          response.writeHead(202).end();
          return;
        }
        let result: unknown;
        const params = body.params as Record<string, unknown> | undefined;
        if (body.method === "initialize")
          result = {
            protocolVersion: params?.protocolVersion,
            capabilities: { tools: {} },
            serverInfo: { name: "fixture", version: "1.0.0" },
          };
        else if (body.method === "tools/list")
          result = {
            tools: [
              {
                name: "echo",
                description:
                  "Return a value together with the authenticated owner.",
                inputSchema: {
                  type: "object",
                  properties: { value: { type: "string" } },
                  required: ["value"],
                  additionalProperties: false,
                },
              },
            ],
          };
        else if (body.method === "tools/call") {
          const user = String(request.headers["x-user-id"]),
            tenant = String(request.headers["x-tenant-id"]),
            session = String(request.headers["x-session-id"]);
          mcpCalls.push({
            user,
            tenant,
            session,
            authorization: String(request.headers.authorization),
            args: params?.arguments,
          });
          result = {
            content: [
              {
                type: "text",
                text: JSON.stringify({
                  user,
                  tenant,
                  session,
                  value: (params?.arguments as { value?: string })?.value,
                }),
              },
            ],
          };
        } else if (body.method === "ping") result = {};
        else result = {};
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end(JSON.stringify({ jsonrpc: "2.0", id: body.id, result }));
      })
      .catch(() => response.writeHead(500).end());
  });
  const mcpUrl = await listen(mcpServer);
  const modelRequests: ModelRequest[] = [];
  const modelServer = createServer((request, response) => {
    void requestBody(request)
      .then((raw) => {
        const body = raw as unknown as ModelRequest;
        modelRequests.push(body);
        if (options.anthropic) {
          if (!request.url?.startsWith("/v1/messages")) {
            response.writeHead(404).end();
            return;
          }
          response.writeHead(200, { "Content-Type": "text/event-stream" });
          const event = (type: string, data: unknown) =>
            response.write(`event: ${type}\ndata: ${JSON.stringify(data)}\n\n`);
          event("message_start", {
            type: "message_start",
            message: {
              id: "msg_fixture",
              type: "message",
              role: "assistant",
              content: [],
              model: "fake-agent",
              stop_reason: null,
              stop_sequence: null,
              usage: { input_tokens: 1, output_tokens: 0 },
            },
          });
          event("content_block_start", {
            type: "content_block_start",
            index: 0,
            content_block: { type: "text", text: "" },
          });
          event("content_block_delta", {
            type: "content_block_delta",
            index: 0,
            delta: { type: "text_delta", text: "anthropic-compatible-ok" },
          });
          event("content_block_stop", { type: "content_block_stop", index: 0 });
          event("message_delta", {
            type: "message_delta",
            delta: { stop_reason: "end_turn", stop_sequence: null },
            usage: { output_tokens: 1 },
          });
          event("message_stop", { type: "message_stop" });
          response.end();
          return;
        }
        const users = body.messages
          .filter((message) => message.role === "user")
          .map((message) => text(message.content));
        const latest = users.at(-1) ?? "";
        if (latest.includes("provider-error")) {
          response.writeHead(401, { "Content-Type": "application/json" }).end(
            JSON.stringify({
              error: {
                message: "PRIVATE_PROVIDER_SENTINEL",
                type: "invalid_request_error",
              },
            }),
          );
          return;
        }
        const last = body.messages.at(-1);
        if (latest.includes("auto-skill")) {
          if (last?.role === "tool") {
            if (text(last.content).includes("SKILL_MARKER")) {
              const tool = body.tools?.find((entry) =>
                entry.function.name.startsWith("mcp__"),
              );
              if (tool) {
                completion(response, {
                  name: tool.function.name,
                  args: { value: "from-skill" },
                });
                return;
              }
            }
            completion(response, `tool-result:${text(last.content)}`);
            return;
          }
          const location =
            body.messages
              .map((message) => text(message.content))
              .join("\n")
              .match(/<location>([^<]+)<\/location>/)?.[1] ?? skillFile;
          completion(response, { name: "read", args: { path: location } });
          return;
        }
        if (latest.startsWith("read-path:")) {
          if (last?.role === "tool") {
            completion(response, `read-result:${text(last.content)}`);
            return;
          }
          completion(response, {
            name: "read",
            args: { path: latest.slice("read-path:".length) },
          });
          return;
        }
        if (latest.includes("use-mcp")) {
          if (last?.role === "tool") {
            completion(response, `tool-result:${text(last.content)}`);
            return;
          }
          const tool = body.tools?.find((entry) =>
            entry.function.name.startsWith("mcp__"),
          );
          if (tool) {
            completion(response, {
              name: tool.function.name,
              args: { value: latest },
            });
            return;
          }
        }
        completion(
          response,
          `visible-users:${users.join("|")}`,
          latest.includes("slow"),
        );
      })
      .catch(() => response.writeHead(500).end());
  });
  const modelUrl = await listen(modelServer);
  const configFile = join(root, "mcp.json");
  const serverConfig = options.stdio
    ? {
        command: process.execPath,
        args: [new URL("./fixtures/mcp-stdio.js", import.meta.url).pathname],
        env: {
          USER_ID: { context: "user_id" },
          TENANT_ID: { context: "tenant_id" },
          SESSION_ID: { context: "session_id" },
        },
        allow: "*",
      }
    : {
        url: `${mcpUrl}/mcp`,
        allow: "*",
        headers: {
          "X-User-Id": { context: "user_id" },
          "X-Tenant-Id": { context: "tenant_id" },
          "X-Session-Id": { context: "session_id" },
          Authorization: { context: "user_id", prefix: "Bearer test-token-" },
        },
      };
  await writeFile(
    configFile,
    JSON.stringify({
      mcpServers: options.mcp || options.stdio ? { demo: serverConfig } : {},
    }),
  );
  const config = loadConfig({
    AGENT_ADDR: "127.0.0.1:0",
    AGENT_API_TOKEN: SERVICE_TOKEN,
    AGENT_DATA_DIR: join(root, "data"),
    AGENT_SKILLS_DIR: skillRoot,
    AGENT_MCP_CONFIG_PATH: configFile,
    AGENT_LLM_PROVIDER:
      options.configured === false
        ? "none"
        : options.anthropic
          ? "anthropic"
          : "custom",
    AGENT_LLM_BASE_URL: options.anthropic ? modelUrl : `${modelUrl}/v1`,
    AGENT_LLM_MODEL: "fake-agent",
    ...(options.anthropic ? { AGENT_LLM_API_KEY: "fixture-model-key" } : {}),
  });
  Object.assign(config, options.overrides);
  let http: ReturnType<typeof createHttpServer>;
  let service: AgentService;
  let baseUrl = "";
  async function start() {
    const store = new Store(config.dataDir);
    const capabilities = new CapabilityRegistry(config, {});
    service = new AgentService(
      config,
      store,
      new PiFactory(config),
      capabilities,
    );
    http = createHttpServer(service);
    baseUrl = await listen(http.server);
  }
  await start();
  return {
    root,
    skillFile,
    config,
    serverConfig,
    modelRequests,
    mcpCalls,
    get mcpRequests() {
      return mcpRequests;
    },
    get service() {
      return service;
    },
    get url() {
      return baseUrl;
    },
    async restart() {
      await http.close();
      await start();
    },
    async cleanup() {
      await http.close();
      await close(modelServer);
      await close(mcpServer);
      await rm(root, { recursive: true, force: true });
    },
    async request(
      path: string,
      user = "alice",
      method = "GET",
      payload?: unknown,
      tenant = "default",
    ) {
      return fetch(baseUrl + path, {
        method,
        headers: {
          Authorization: `Bearer ${SERVICE_TOKEN}`,
          "X-User-Id": user,
          "X-Tenant-Id": tenant,
          "Content-Type": "application/json",
        },
        body: payload === undefined ? undefined : JSON.stringify(payload),
      });
    },
  };
}

export async function json(response: Response) {
  const value = (await response.json()) as {
    code: string;
    message: string;
    data: Record<string, unknown>;
  };
  return value;
}

export function events(value: string): RunEvent[] {
  return value
    .split("\n")
    .filter((line) => line.startsWith("data: "))
    .map((line) => JSON.parse(line.slice(6)) as RunEvent);
}

export async function waitFor(test: () => boolean, timeout = 10000) {
  const start = Date.now();
  while (!test()) {
    if (Date.now() - start > timeout) throw new Error("Condition timed out");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}
