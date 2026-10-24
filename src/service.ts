import type { Principal } from "./auth.js";
import { existsSync } from "node:fs";
import type { Config } from "./config.js";
import type {
  MessageInput,
  RunEvent,
  RunRecord,
  RunStatus,
} from "./contracts.js";
import { publicSession } from "./contracts.js";
import { ApiError, publicError, badRequest } from "./errors.js";
import type { AgentRuntime, RuntimeFactory, RuntimeEvent } from "./pi.js";
import { CapabilityRegistry, type CapabilitySnapshot } from "./capabilities.js";
import { Store } from "./store.js";

interface CachedRuntime {
  runtime: AgentRuntime;
  touched: number;
  snapshot: CapabilitySnapshot;
}
export interface ActiveRun {
  record: RunRecord;
  owner: Principal;
  controller: AbortController;
  listeners: Set<(event: RunEvent) => void>;
  completion: Promise<RunRecord>;
  runtime?: AgentRuntime;
  failureCode?: string;
  snapshot: CapabilitySnapshot;
}

export class AgentService {
  private runtimes = new Map<string, CachedRuntime>();
  private runs = new Map<string, ActiveRun>();
  private busy = new Map<string, string>();
  private creating = 0;
  private shuttingDown = false;
  private sweep: ReturnType<typeof setInterval>;
  private readonly unsubscribeCapabilities: () => void;
  private readonly closingRuntimes = new Set<Promise<void>>();
  constructor(
    readonly config: Config,
    readonly store: Store,
    readonly factory: RuntimeFactory,
    readonly capabilities: CapabilityRegistry,
  ) {
    this.unsubscribeCapabilities = capabilities.onChange(() => {
      for (const [id, cached] of this.runtimes) {
        if (!this.busy.has(id) && cached.snapshot !== capabilities.snapshot) {
          this.runtimes.delete(id);
          void this.closeRuntime(cached).catch(() => undefined);
        }
      }
    });
    this.sweep = setInterval(
      () => {
        void this.releaseIdle().catch(() => undefined);
      },
      Math.min(config.idleTimeoutMs, 60000),
    );
    this.sweep.unref();
  }

  get skills() {
    return this.capabilities.snapshot.skills;
  }
  get mcp() {
    return this.capabilities.snapshot.mcp;
  }

  private closeRuntime(cached: CachedRuntime) {
    const completion = cached.runtime
      .close()
      .finally(() => this.capabilities.release(cached.snapshot));
    this.closingRuntimes.add(completion);
    void completion
      .finally(() => this.closingRuntimes.delete(completion))
      .catch(() => undefined);
    return completion;
  }

  createSession(owner: Principal, title: string) {
    return publicSession(this.store.createSession(owner, title));
  }
  listSessions(owner: Principal) {
    return this.store.listSessions(owner).map((entry) => ({
      ...publicSession(entry),
      active_run_id: this.busy.get(entry.id) ?? null,
    }));
  }
  getSession(owner: Principal, id: string) {
    return {
      ...publicSession(this.store.session(owner, id)),
      active_run_id: this.busy.get(id) ?? null,
    };
  }
  messages(owner: Principal, id: string, limit: number) {
    return this.store.messages(owner, id, limit);
  }
  getRun(owner: Principal, id: string, runId: string) {
    return {
      ...this.store.run(owner, id, runId),
      reply: this.store.reply(owner, id, runId),
    };
  }
  events(owner: Principal, id: string, runId: string, after: number) {
    return this.store.events(owner, id, runId, after);
  }

  async deleteSession(owner: Principal, id: string): Promise<void> {
    this.store.session(owner, id);
    if (this.busy.has(id))
      throw new ApiError(409, "session_busy", "请先取消当前执行。");
    this.store.deleteSession(owner, id);
    const cached = this.runtimes.get(id);
    this.runtimes.delete(id);
    if (cached) await this.closeRuntime(cached);
  }

  startRun(owner: Principal, id: string, input: MessageInput): ActiveRun {
    this.store.session(owner, id);
    this.factory.ensureConfigured();
    if (this.shuttingDown)
      throw new ApiError(503, "service_unavailable", "服务正在关闭。");
    if (
      !input.message.trim() ||
      input.message.length > this.config.messageLimit
    )
      throw badRequest("消息为空或超过长度限制。");
    if (input.skill !== undefined) this.skills.validate(input.skill);
    // Missing secret references fail before any message/run persistence.
    this.mcp.entries(owner, id);
    if (this.busy.has(id))
      throw new ApiError(
        409,
        "session_busy",
        "当前会话正在执行，请等待或取消后重试。",
      );
    if (this.busy.size >= this.config.maxActiveSessions)
      throw new ApiError(503, "capacity_limited", "当前执行数量已达到上限。");
    const snapshot = this.capabilities.acquire();
    let record: RunRecord;
    try {
      record = this.store.startRun(owner, id, input.message);
    } catch (error) {
      this.capabilities.release(snapshot);
      throw error;
    }
    const run: ActiveRun = {
      record,
      owner,
      controller: new AbortController(),
      listeners: new Set(),
      completion: Promise.resolve(record),
      snapshot,
    };
    this.busy.set(id, record.id);
    this.runs.set(record.id, run);
    this.emit(run, { type: "run_started", status: "running" });
    run.completion = this.execute(run, input);
    return run;
  }

  subscribe(
    owner: Principal,
    id: string,
    runId: string,
    listener: (event: RunEvent) => void,
  ): () => void {
    this.store.run(owner, id, runId);
    const run = this.runs.get(runId);
    run?.listeners.add(listener);
    return () => run?.listeners.delete(listener);
  }

  cancel(owner: Principal, id: string, runId: string): RunRecord {
    const stored = this.store.run(owner, id, runId);
    const run = this.runs.get(runId);
    if (stored.status === "running" && run) {
      run.controller.abort();
      if (run.runtime) void run.runtime.abort().catch(() => undefined);
    }
    return this.store.run(owner, id, runId);
  }

  private emit(
    run: ActiveRun,
    event: Omit<RunEvent, "session_id" | "run_id" | "sequence">,
  ) {
    const saved = this.store.event(run.record.id, event);
    for (const listener of run.listeners) {
      try {
        listener(saved);
      } catch {
        run.listeners.delete(listener);
      }
    }
  }

  private async runtime(run: ActiveRun): Promise<AgentRuntime> {
    const cached = this.runtimes.get(run.record.session_id);
    if (cached && cached.snapshot === run.snapshot) {
      cached.touched = Date.now();
      return cached.runtime;
    }
    let evicted: CachedRuntime | undefined;
    if (cached) {
      this.runtimes.delete(run.record.session_id);
      evicted = cached;
    }
    if (this.runtimes.size + this.creating >= this.config.maxActiveSessions) {
      const idle = [...this.runtimes.entries()]
        .filter(([id]) => !this.busy.has(id))
        .sort((a, b) => a[1].touched - b[1].touched)[0];
      if (!idle)
        throw new ApiError(503, "capacity_limited", "当前会话数量已达到上限。");
      this.runtimes.delete(idle[0]);
      evicted = idle[1];
    }
    this.creating++;
    try {
      if (evicted) await this.closeRuntime(evicted);
      const record = this.store.session(run.owner, run.record.session_id);
      const runtime = await this.factory.create(
        run.owner,
        record,
        run.snapshot,
      );
      this.store.setPiFile(run.owner, record.id, runtime.file);
      this.runtimes.set(record.id, {
        runtime,
        touched: Date.now(),
        snapshot: this.capabilities.acquire(run.snapshot),
      });
      return runtime;
    } finally {
      this.creating--;
    }
  }

  private async execute(
    run: ActiveRun,
    input: MessageInput,
  ): Promise<RunRecord> {
    let unsubscribe: (() => void) | undefined;
    let turns = 0,
      tools = 0,
      finalText = "",
      finalReason = "";
    const timer = setTimeout(() => {
      run.failureCode = "run_timeout";
      run.controller.abort();
    }, this.config.runTimeoutMs);
    timer.unref();
    let status: RunStatus = "completed",
      code: string | undefined;
    try {
      const runtime = await this.runtime(run);
      run.runtime = runtime;
      if (run.controller.signal.aborted)
        throw new ApiError(409, "run_cancelled", "执行已取消。");
      const abortForLimit = () => {
        run.failureCode = "run_limit_exceeded";
        run.controller.abort();
      };
      unsubscribe = runtime.subscribe((event: RuntimeEvent) => {
        if (event.type === "context_persisted") {
          this.store.markPiReady(run.owner, run.record.session_id);
          return;
        }
        if (run.controller.signal.aborted) return;
        if (event.type === "turn_start") {
          if (++turns > this.config.maxTurns) abortForLimit();
        } else if (event.type === "tool_start") {
          if (++tools > this.config.maxToolCalls) {
            abortForLimit();
            return;
          }
          this.emit(run, {
            type: "tool_start",
            tool: event.tool,
            tool_call_id: event.toolCallId,
          });
        } else if (event.type === "tool_end")
          this.emit(run, {
            type: "tool_end",
            tool: event.tool,
            tool_call_id: event.toolCallId,
            is_error: event.isError,
          });
        else if (event.type === "token") this.emit(run, event);
        else if (event.type === "assistant_final") {
          finalText = event.content;
          finalReason = event.stopReason;
        }
      });
      await runtime.prompt(input.message, input.skill, run.controller.signal);
      if (run.controller.signal.aborted)
        throw new ApiError(409, "run_cancelled", "执行已取消。");
      if (!finalText || finalReason === "error" || finalReason === "aborted")
        throw new ApiError(
          502,
          "model_provider_error",
          "模型服务暂时无法完成请求。",
        );
      this.store.assistant(run.record, finalText);
    } catch (error) {
      if (run.controller.signal.aborted && !run.failureCode)
        status = "cancelled";
      else {
        status = "failed";
        code =
          run.failureCode ??
          (error instanceof ApiError ? error.code : "model_provider_error");
      }
      if (status === "failed")
        this.emit(run, {
          type: "error",
          code,
          content:
            code === "run_timeout"
              ? "执行超时，请重试。"
              : code === "run_limit_exceeded"
                ? "执行达到允许的步骤上限。"
                : error instanceof ApiError
                  ? publicError(error).message
                  : "模型或工具服务暂时无法完成请求。",
        });
    } finally {
      clearTimeout(timer);
      unsubscribe?.();
      if (run.runtime && existsSync(run.runtime.file))
        this.store.markPiReady(run.owner, run.record.session_id);
      if (
        code === "mcp_unavailable" ||
        run.snapshot !== this.capabilities.snapshot
      ) {
        const failed = this.runtimes.get(run.record.session_id);
        this.runtimes.delete(run.record.session_id);
        if (failed) await this.closeRuntime(failed).catch(() => undefined);
      }
      this.store.finishRun(run.record.id, status, code);
      this.busy.delete(run.record.session_id);
      const cached = this.runtimes.get(run.record.session_id);
      if (cached) cached.touched = Date.now();
      this.emit(run, { type: "done", status });
      this.runs.delete(run.record.id);
      this.capabilities.release(run.snapshot);
    }
    return this.store.run(run.owner, run.record.session_id, run.record.id);
  }

  private async releaseIdle() {
    const stale = [...this.runtimes.entries()].filter(
      ([id, entry]) =>
        !this.busy.has(id) &&
        Date.now() - entry.touched >= this.config.idleTimeoutMs,
    );
    for (const [id, entry] of stale) {
      if (
        this.busy.has(id) ||
        this.runtimes.get(id) !== entry ||
        Date.now() - entry.touched < this.config.idleTimeoutMs
      )
        continue;
      this.runtimes.delete(id);
      await this.closeRuntime(entry);
    }
  }

  async close(): Promise<void> {
    this.shuttingDown = true;
    this.unsubscribeCapabilities();
    clearInterval(this.sweep);
    const runs = [...this.runs.values()];
    for (const run of runs) {
      run.controller.abort();
      if (run.runtime) void run.runtime.abort().catch(() => undefined);
    }
    await Promise.allSettled(runs.map((run) => run.completion));
    await Promise.allSettled(
      [...this.runtimes.values()].map((entry) => this.closeRuntime(entry)),
    );
    await Promise.allSettled([...this.closingRuntimes]);
    this.capabilities.close();
    this.runtimes.clear();
    this.store.close();
  }
}
