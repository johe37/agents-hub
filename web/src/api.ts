export type Status = "waiting" | "working" | "idle" | "unknown" | "ended" | "stale";

export interface Git {
  branch: string | null;
  commit: string | null;
  dirty: number;
  ahead: number;
  behind: number;
  added: number;
  removed: number;
  files: [string, string][];
}

export interface Note {
  id: number;
  author: string;
  body: string;
}

export interface Thread {
  key: string;
  id: string;
  place: string;
  path: string | null;
  line: number | null;
  /** The line is a removed one, in the old file. */
  old_line: boolean;
  notes: Note[];
  prompt: string;
}

export interface Job {
  key: number;
  id: number;
  name: string;
  stage: string;
  status: string;
  duration: number | null;
  url: string;
}

export interface PipelineRun {
  id: number;
  status: string;
  sha: string;
  /** The title of its commit. */
  title: string;
  user: string;
  created: number;
  duration: number | null;
  url: string;
  stages: { name: string; status: string }[];
}

export interface Mr {
  key: string;
  project_id: number;
  iid: number;
  title: string;
  url: string;
  state: string;
  pipeline: string | null;
  pipeline_url: string | null;
  merge_status: string | null;
  draft: boolean;
  author: string;
  project: string;
  branch: string;
  updated: number;
  description: string;
  approved: boolean;
  approved_by: string[];
  threads: Thread[];
  jobs: Job[];
  review_prompt: string;
  workspace: string | null;
}

export interface FileChange {
  old_path: string;
  new_path: string;
  /** Hunks only; empty for a binary file. */
  diff: string;
  new_file: boolean;
  deleted: boolean;
  renamed: boolean;
  too_large: boolean;
}

export interface Commit {
  sha: string;
  short: string;
  title: string;
  message: string;
  author: string;
  created: number;
}

export interface MrChanges {
  version?: { base: string; start: string; head: string };
  commits?: Commit[];
  files: FileChange[];
  /** The head you last reviewed, when it is not the head now. */
  reviewed?: string | null;
}

export interface DraftNote {
  id: number;
  body: string;
  path: string | null;
  line: number | null;
  old_line: boolean;
}

export interface Issue {
  key: string;
  iid: number;
  title: string;
  url: string;
  project: string;
  labels: string[];
  milestone: string | null;
  author: string;
  updated: number;
  description: string;
  prompt: string;
}

export type WindowKind = "agent" | "shell" | "editor" | "process";

export interface Win {
  key: string;
  id: string;
  index: number;
  name: string;
  pane: string;
  command: string;
  kind: WindowKind;
  /** The command that runs in a shell now, if any. */
  running: string | null;
  agent: string | null;
}

export interface Workspace {
  key: string;
  name: string;
  path: string;
  short_path: string;
  git: Git | null;
  mr: Mr | null;
  windows: Win[];
}

export interface PendingTool {
  name: string;
  input: Record<string, unknown>;
}

export interface Agent {
  key: string;
  id: string | null;
  title: string;
  name: string;
  status: Status;
  workspace: string | null;
  window: string | null;
  path: string;
  short_path: string;
  branch: string | null;
  dirty: number;
  timestamp: number;
  message: string | null;
  notification: string | null;
  permission_mode: string | null;
  pending_tool: PendingTool | null;
  needs_permission: boolean;
  tool: string | null;
  prompt: string | null;
  reply: string | null;
  model: string | null;
  turns: number;
  cost: number | null;
  background: boolean;
  reachable: boolean;
  adoptable: boolean;
  activity: number;
}

export interface ClosedWorkspace {
  key: string;
  id: string;
  name: string;
  path: string;
  short_path: string;
  closed_at: number;
  windows: { kind: "agent" | "editor" | "shell"; name: string; resume: boolean }[];
}

export interface UsageWindow {
  used: number;
  resets_at: number;
}

export interface State {
  workspaces: Workspace[];
  agents: Agent[];
  usage: { five_hour: UsageWindow | null; seven_day: UsageWindow | null } | null;
  gitlab: {
    enabled: boolean;
    loading: boolean;
    errors: [string, string][];
    reviews: Mr[];
    authored: Mr[];
    assigned: Mr[];
    issues: Issue[];
  };
  closed: ClosedWorkspace[];
  chat_dir: string;
  /** The theme from config.toml, used when this browser has not picked one. */
  web_theme: string;
  home: string;
  warnings: string[];
  version: string;
  commit: string;
}

export type ConversationItem =
  | { kind: "user"; text: string; ts: string }
  | { kind: "assistant"; text: string; ts: string }
  | {
      kind: "tool";
      id: string;
      name: string;
      summary: string | null;
      input: Record<string, unknown>;
      result?: { text: string; error: boolean };
      ts: string;
    };

export type Action =
  | { type: "approve"; agent: string }
  | { type: "deny"; agent: string }
  | { type: "reply"; agent: string; text: string }
  | { type: "interrupt"; agent: string }
  | { type: "mark_done"; agent: string }
  | { type: "resume"; agent: string }
  | { type: "close_agent"; agent: string }
  | { type: "adopt"; agent: string }
  | { type: "new_window"; workspace: string; kind: "claude" | "shell" | "nvim" }
  | { type: "new_agent"; path: string; prompt?: string }
  | { type: "open_workspace"; path: string; kind?: "claude" | "shell" | "nvim" }
  | { type: "close_workspace"; workspace: string }
  | { type: "close_window"; window: string }
  | { type: "move_window"; window: string; before: string | null }
  | { type: "rename_workspace"; workspace: string; name: string }
  | { type: "rescan" }
  | { type: "checkout"; mr: string; kind?: "claude" | "shell" | "nvim" }
  | { type: "find_clone"; project: string }
  | { type: "thread_reply"; mr: string; thread: string; text: string; resolve: boolean }
  | { type: "publish"; workspace: string }
  | { type: "gitlab_refresh" }
  | { type: "mr_note"; mr: string; body: string; position?: Record<string, unknown>; draft: boolean }
  | { type: "draft_edit"; mr: string; id: number; body: string }
  | { type: "draft_delete"; mr: string; id: number }
  | { type: "finish_review"; mr: string; summary: string; approve: boolean; head?: string }
  | { type: "mr_approve"; mr: string; approve: boolean; head?: string }
  | { type: "retry_job"; mr: string; job: number }
  | { type: "retry_pipeline"; mr: string; pipeline: number }
  | { type: "cancel_pipeline"; mr: string; pipeline: number }
  | { type: "run_pipeline"; mr: string }
  | { type: "reopen_workspace"; id: string }
  | { type: "forget_closed"; id: string };

export interface ActionResult {
  ok: boolean;
  message?: string | null;
  error?: string;
  workspace?: string | null;
  window?: string | null;
  path?: string | null;
}

export async function post(action: Action): Promise<ActionResult> {
  try {
    const r = await fetch("/api/action", {
      method: "POST",
      headers: { "content-type": "application/json", "x-jugglr": "1" },
      body: JSON.stringify(action),
    });
    if (r.status === 401) return { ok: false, error: "The page lost its key. Open the link from the terminal again." };
    return (await r.json()) as ActionResult;
  } catch (e) {
    return { ok: false, error: `jugglr does not answer: ${(e as Error).message}` };
  }
}

export async function getJSON<T>(url: string): Promise<T> {
  const r = await fetch(url);
  if (!r.ok) {
    const text = await r.text();
    let error: string | undefined;
    try {
      error = (JSON.parse(text) as { error?: string }).error;
    } catch {}
    throw new Error(error ?? `${r.status} ${text}`);
  }
  return (await r.json()) as T;
}

export function wsURL(path: string): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${path}`;
}
