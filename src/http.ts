import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { randomUUID } from "node:crypto";
import { authenticate } from "./auth.js";
import type { AgentService } from "./service.js";
import type { RunEvent } from "./contracts.js";
import { ApiError, badRequest, publicError } from "./errors.js";

const success = (response: ServerResponse, data: unknown, status = 200) => {
  response.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  response.end(JSON.stringify({ code: "ok", message: "ok", data }));
};

export async function body(
  request: IncomingMessage,
  limit: number,
): Promise<Record<string, unknown>> {
  const contentType = request.headers["content-type"];
  if (
    contentType !== undefined &&
    !contentType.toLowerCase().startsWith("application/json")
  )
    throw new ApiError(
      415,
      "unsupported_media_type",
      "请求需要使用 application/json。",
    );
  const buffers: Buffer[] = [];
  let bytes = 0;
  for await (const chunk of request.iterator({ destroyOnReturn: false })) {
    bytes += chunk.length;
    if (bytes > limit) {
      request.resume();
      throw new ApiError(413, "payload_too_large", "请求内容超过大小限制。");
    }
    buffers.push(Buffer.from(chunk));
  }
  let parsed: unknown;
  try {
    parsed = buffers.length
      ? JSON.parse(Buffer.concat(buffers).toString("utf8"))
      : {};
  } catch {
    throw badRequest("请求 JSON 无效。");
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed))
    throw badRequest("请求内容需要为 JSON 对象。");
  return parsed as Record<string, unknown>;
}

function fields(input: Record<string, unknown>, names: string[]) {
  if (Object.keys(input).some((key) => !names.includes(key)))
    throw badRequest("请求包含不支持的字段。");
}

function integer(value: string | null, fallback: number, max: number): number {
  if (value === null) return fallback;
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)))
    throw badRequest("查询参数无效。");
  return Math.min(max, Math.max(fallback === 0 ? 0 : 1, Number(value)));
}

export function createHttpServer(service: AgentService) {
  const streaming = new Set<ServerResponse>();
  const server = createServer((request, response) => {
    response.setHeader("X-Request-Id", randomUUID());
    response.setHeader("X-Content-Type-Options", "nosniff");
    void handle(request, response).catch((error: unknown) => {
      const safe = publicError(error);
      if (response.headersSent) {
        response.destroy();
        return;
      }
      response.writeHead(safe.status, {
        "Content-Type": "application/json; charset=utf-8",
        "Cache-Control": "no-store",
      });
      response.end(JSON.stringify({ code: safe.code, message: safe.message }));
    });
  });
  server.requestTimeout = 30000;
  server.headersTimeout = 15000;

  function stream(
    response: ServerResponse,
    owner: ReturnType<typeof authenticate>,
    id: string,
    runId: string,
    after: number,
  ) {
    service.getRun(owner, id, runId);
    response.writeHead(200, {
      "Content-Type": "text/event-stream; charset=utf-8",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    });
    response.flushHeaders();
    streaming.add(response);
    let sequence = after;
    let cleanup = () => {};
    const send = (event: RunEvent) => {
      if (response.destroyed || event.sequence <= sequence) return;
      sequence = event.sequence;
      response.write(
        `id: ${event.run_id}:${event.sequence}\nevent: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`,
      );
      if (response.writableLength > 1024 * 1024) response.destroy();
      if (event.type === "done") {
        response.end();
        cleanup();
      }
    };
    const unsubscribe = service.subscribe(owner, id, runId, send);
    const heartbeat = setInterval(() => {
      if (!response.destroyed) response.write(": heartbeat\n\n");
    }, 15000);
    heartbeat.unref();
    cleanup = () => {
      unsubscribe();
      clearInterval(heartbeat);
      streaming.delete(response);
    };
    response.on("close", cleanup);
    for (const event of service.events(owner, id, runId, after)) send(event);
    if (
      service.getRun(owner, id, runId).status !== "running" &&
      !response.writableEnded
    ) {
      response.end();
      cleanup();
    }
  }

  async function handle(request: IncomingMessage, response: ServerResponse) {
    const url = new URL(request.url ?? "/", "http://agent.internal");
    const method = request.method ?? "GET";
    if (method === "GET" && url.pathname === "/health") {
      success(response, { status: "ok", service: "pilot", version: "0.1.0" });
      return;
    }
    let path: string[];
    try {
      path = url.pathname.split("/").filter(Boolean).map(decodeURIComponent);
    } catch {
      throw badRequest("请求路径无效。");
    }
    if (path[0] !== "api" || path[1] !== "v1")
      throw new ApiError(404, "not_found", "接口不存在。");
    const owner = authenticate(request.headers, service.config);
    const base = path.slice(2);
    if (method === "GET" && base.join("/") === "skills") {
      success(response, { skills: service.skills.catalogue() });
      return;
    }
    if (method === "GET" && base.join("/") === "mcp/servers") {
      success(response, { servers: service.mcp.catalogue(owner) });
      return;
    }
    if (method === "GET" && base.join("/") === "health/detail") {
      let configured = true;
      try {
        service.factory.ensureConfigured();
      } catch {
        configured = false;
      }
      success(response, {
        status: "ok",
        model_configured: configured,
        skills: service.skills.catalogue().length,
        mcp_servers: service.mcp.catalogue(owner).length,
      });
      return;
    }
    if (base[0] !== "sessions")
      throw new ApiError(404, "not_found", "接口不存在。");
    if (base.length === 1) {
      if (method === "GET") {
        success(response, { sessions: service.listSessions(owner) });
        return;
      }
      if (method === "POST") {
        const input = await body(request, service.config.bodyLimit);
        fields(input, ["title"]);
        if (
          input.title !== undefined &&
          (typeof input.title !== "string" || input.title.length > 200)
        )
          throw badRequest("会话标题无效。");
        success(
          response,
          service.createSession(
            owner,
            typeof input.title === "string"
              ? input.title.trim() || "新会话"
              : "新会话",
          ),
          201,
        );
        return;
      }
    }
    const id = base[1];
    if (!id) throw new ApiError(404, "not_found", "接口不存在。");
    // Check ownership before parsing subordinate resource identifiers or request data.
    service.getSession(owner, id);
    if (base.length === 2) {
      if (method === "GET") {
        success(response, service.getSession(owner, id));
        return;
      }
      if (method === "DELETE") {
        await service.deleteSession(owner, id);
        success(response, { id, status: "deleted" });
        return;
      }
    }
    if (base[2] === "messages") {
      if (base.length === 3 && method === "GET") {
        success(response, {
          messages: service.messages(
            owner,
            id,
            integer(url.searchParams.get("limit"), 100, 1000),
          ),
        });
        return;
      }
      if (
        method === "POST" &&
        (base.length === 3 || (base.length === 4 && base[3] === "stream"))
      ) {
        const input = await body(request, service.config.bodyLimit);
        fields(input, ["message", "skill"]);
        if (
          typeof input.message !== "string" ||
          (input.skill !== undefined && typeof input.skill !== "string")
        )
          throw badRequest("消息或技能参数无效。");
        const run = service.startRun(owner, id, {
          message: input.message,
          skill: input.skill as string | undefined,
        });
        if (base[3] === "stream") stream(response, owner, id, run.record.id, 0);
        else success(response, { run: run.record }, 202);
        return;
      }
    }
    if (base[2] === "runs" && base[3]) {
      const runId = base[3];
      if (base.length === 4 && method === "GET") {
        success(response, service.getRun(owner, id, runId));
        return;
      }
      if (base.length === 5 && base[4] === "events" && method === "GET") {
        stream(
          response,
          owner,
          id,
          runId,
          integer(url.searchParams.get("after"), 0, Number.MAX_SAFE_INTEGER),
        );
        return;
      }
      if (base.length === 5 && base[4] === "cancel" && method === "POST") {
        const input = await body(request, service.config.bodyLimit);
        fields(input, []);
        success(response, service.cancel(owner, id, runId));
        return;
      }
    }
    throw new ApiError(404, "not_found", "接口不存在。");
  }

  return {
    server,
    async close() {
      const closed = new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
      server.closeIdleConnections();
      await service.close();
      for (const response of streaming) response.end();
      server.closeAllConnections();
      await closed;
    },
  };
}
