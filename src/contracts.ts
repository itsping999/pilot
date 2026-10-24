export interface SessionRecord {
  id: string;
  tenant_id: string;
  user_id: string;
  title: string;
  pi_file: string | null;
  pi_ready: number;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export type RunStatus =
  | "running"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted";
export interface RunRecord {
  id: string;
  session_id: string;
  status: RunStatus;
  started_at: string;
  finished_at: string | null;
  error_code: string | null;
}

export interface PublicMessage {
  id: string;
  session_id: string;
  run_id: string;
  role: "user" | "assistant";
  content: string;
  created_at: string;
}

export interface RunEvent {
  type: "run_started" | "token" | "tool_start" | "tool_end" | "error" | "done";
  sequence: number;
  session_id: string;
  run_id: string;
  content?: string;
  tool?: string;
  tool_call_id?: string;
  is_error?: boolean;
  status?: RunStatus;
  code?: string;
}

export interface MessageInput {
  message: string;
  skill?: string;
}

export function publicSession(session: SessionRecord) {
  return {
    id: session.id,
    title: session.title,
    created_at: session.created_at,
    updated_at: session.updated_at,
  };
}
