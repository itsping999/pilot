import { randomUUID } from "node:crypto";
import { mkdirSync, chmodSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import type { Principal } from "./auth.js";
import type {
  PublicMessage,
  RunEvent,
  RunRecord,
  RunStatus,
  SessionRecord,
} from "./contracts.js";
import { ApiError, notFound } from "./errors.js";

export class Store {
  readonly db: DatabaseSync;
  private readonly lock: DatabaseSync;
  constructor(readonly dataDir: string) {
    mkdirSync(dataDir, { recursive: true, mode: 0o700 });
    // A separate SQLite file provides a crash-safe process lock without blocking normal writes.
    this.lock = new DatabaseSync(join(dataDir, "runtime-lock.db"));
    try {
      this.lock.exec(
        "PRAGMA busy_timeout=0; PRAGMA locking_mode=EXCLUSIVE; BEGIN EXCLUSIVE; CREATE TABLE IF NOT EXISTS runtime_lock(id INTEGER)",
      );
    } catch {
      this.lock.close();
      throw new ApiError(
        503,
        "data_directory_in_use",
        "该数据目录已由另一个 Agent 进程使用。",
      );
    }
    const filename = join(dataDir, "agent.db");
    this.db = new DatabaseSync(filename);
    chmodSync(filename, 0o600);
    this.db.exec(`
      PRAGMA journal_mode = WAL;
      PRAGMA synchronous = NORMAL;
      PRAGMA foreign_keys = ON;
      PRAGMA busy_timeout = 5000;
      CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, user_id TEXT NOT NULL,
        title TEXT NOT NULL, pi_file TEXT, pi_ready INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL, deleted_at TEXT
      );
      CREATE INDEX IF NOT EXISTS sessions_owner ON sessions(tenant_id, user_id, updated_at);
      CREATE TABLE IF NOT EXISTS runs (
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id),
        status TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT, error_code TEXT
      );
      CREATE TABLE IF NOT EXISTS messages (
        seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
        session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT NOT NULL REFERENCES runs(id),
        role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id, seq);
      CREATE TABLE IF NOT EXISTS events (
        run_id TEXT NOT NULL REFERENCES runs(id), sequence INTEGER NOT NULL,
        json TEXT NOT NULL, PRIMARY KEY(run_id, sequence)
      );
    `);
    if (
      !this.db
        .prepare("PRAGMA table_info(sessions)")
        .all()
        .some((column) => column.name === "pi_ready")
    )
      this.db.exec(
        "ALTER TABLE sessions ADD COLUMN pi_ready INTEGER NOT NULL DEFAULT 0",
      );
    const interrupted = this.db
      .prepare("SELECT * FROM runs WHERE status = 'running'")
      .all() as unknown as RunRecord[];
    for (const run of interrupted) {
      this.finishRun(run.id, "interrupted", "service_restarted");
      this.event(run.id, {
        type: "error",
        code: "service_restarted",
        content: "服务重启，上一轮执行已中断。",
      });
      this.event(run.id, { type: "done", status: "interrupted" });
    }
  }

  createSession(owner: Principal, title: string): SessionRecord {
    const id = randomUUID(),
      now = new Date().toISOString();
    this.db
      .prepare(
        "INSERT INTO sessions(id,tenant_id,user_id,title,created_at,updated_at) VALUES(?,?,?,?,?,?)",
      )
      .run(id, owner.tenantId, owner.userId, title, now, now);
    return this.session(owner, id);
  }

  session(owner: Principal, id: string): SessionRecord {
    const record = this.db
      .prepare(
        "SELECT * FROM sessions WHERE id=? AND tenant_id=? AND user_id=? AND deleted_at IS NULL",
      )
      .get(id, owner.tenantId, owner.userId) as unknown as
      | SessionRecord
      | undefined;
    if (!record) throw notFound();
    return record;
  }

  listSessions(owner: Principal, limit = 100): SessionRecord[] {
    return this.db
      .prepare(
        "SELECT * FROM sessions WHERE tenant_id=? AND user_id=? AND deleted_at IS NULL ORDER BY updated_at DESC,id DESC LIMIT ?",
      )
      .all(owner.tenantId, owner.userId, limit) as unknown as SessionRecord[];
  }

  setPiFile(owner: Principal, id: string, filename: string): void {
    this.session(owner, id);
    this.db
      .prepare(
        "UPDATE sessions SET pi_file=? WHERE id=? AND tenant_id=? AND user_id=?",
      )
      .run(filename, id, owner.tenantId, owner.userId);
  }

  markPiReady(owner: Principal, id: string): void {
    this.db
      .prepare(
        "UPDATE sessions SET pi_ready=1 WHERE id=? AND tenant_id=? AND user_id=?",
      )
      .run(id, owner.tenantId, owner.userId);
  }

  deleteSession(owner: Principal, id: string): void {
    this.session(owner, id);
    this.db
      .prepare(
        "UPDATE sessions SET deleted_at=? WHERE id=? AND tenant_id=? AND user_id=?",
      )
      .run(new Date().toISOString(), id, owner.tenantId, owner.userId);
  }

  startRun(owner: Principal, sessionId: string, text: string): RunRecord {
    this.session(owner, sessionId);
    const id = randomUUID(),
      now = new Date().toISOString();
    this.db.exec("BEGIN IMMEDIATE");
    try {
      this.db
        .prepare(
          "INSERT INTO runs(id,session_id,status,started_at) VALUES(?,?,?,?)",
        )
        .run(id, sessionId, "running", now);
      this.db
        .prepare(
          "INSERT INTO messages(id,session_id,run_id,role,content,created_at) VALUES(?,?,?,?,?,?)",
        )
        .run(randomUUID(), sessionId, id, "user", text, now);
      this.db
        .prepare("UPDATE sessions SET updated_at=? WHERE id=?")
        .run(now, sessionId);
      this.db.exec("COMMIT");
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
    return this.run(owner, sessionId, id);
  }

  run(owner: Principal, sessionId: string, id: string): RunRecord {
    this.session(owner, sessionId);
    const record = this.db
      .prepare("SELECT * FROM runs WHERE id=? AND session_id=?")
      .get(id, sessionId) as unknown as RunRecord | undefined;
    if (!record) throw notFound();
    return record;
  }

  finishRun(id: string, status: RunStatus, code?: string): void {
    this.db
      .prepare(
        "UPDATE runs SET status=?,finished_at=?,error_code=? WHERE id=? AND status='running'",
      )
      .run(status, new Date().toISOString(), code ?? null, id);
  }

  assistant(run: RunRecord, text: string): void {
    const now = new Date().toISOString();
    this.db
      .prepare(
        "INSERT INTO messages(id,session_id,run_id,role,content,created_at) VALUES(?,?,?,?,?,?)",
      )
      .run(randomUUID(), run.session_id, run.id, "assistant", text, now);
    this.db
      .prepare("UPDATE sessions SET updated_at=? WHERE id=?")
      .run(now, run.session_id);
  }

  messages(owner: Principal, id: string, limit = 100): PublicMessage[] {
    this.session(owner, id);
    const rows = this.db
      .prepare(
        "SELECT id,session_id,run_id,role,content,created_at FROM messages WHERE session_id=? ORDER BY seq DESC LIMIT ?",
      )
      .all(id, limit) as unknown as PublicMessage[];
    return rows.reverse();
  }

  reply(owner: Principal, sessionId: string, id: string): string | null {
    this.run(owner, sessionId, id);
    const row = this.db
      .prepare(
        "SELECT content FROM messages WHERE session_id=? AND run_id=? AND role='assistant' ORDER BY seq DESC LIMIT 1",
      )
      .get(sessionId, id) as { content: string } | undefined;
    return row?.content ?? null;
  }

  event(
    id: string,
    data: Omit<RunEvent, "session_id" | "run_id" | "sequence">,
  ): RunEvent {
    const run = this.db
      .prepare("SELECT session_id FROM runs WHERE id=?")
      .get(id) as { session_id: string };
    const last = this.db
      .prepare(
        "SELECT COALESCE(MAX(sequence),0) AS sequence FROM events WHERE run_id=?",
      )
      .get(id) as { sequence: number };
    const event = {
      ...data,
      run_id: id,
      session_id: run.session_id,
      sequence: last.sequence + 1,
    };
    this.db
      .prepare("INSERT INTO events(run_id,sequence,json) VALUES(?,?,?)")
      .run(id, event.sequence, JSON.stringify(event));
    return event;
  }

  events(
    owner: Principal,
    sessionId: string,
    id: string,
    after = 0,
  ): RunEvent[] {
    this.run(owner, sessionId, id);
    return this.db
      .prepare(
        "SELECT json FROM events WHERE run_id=? AND sequence>? ORDER BY sequence",
      )
      .all(id, after)
      .map((row) => JSON.parse(row.json as string) as RunEvent);
  }

  close(): void {
    this.db.close();
    this.lock.close();
  }
}
