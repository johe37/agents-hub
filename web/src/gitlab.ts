import { createSignal } from "solid-js";
import type { Issue, Mr, Todo } from "./api";
import { act, go, setComposer, setDraft, state } from "./store";

export type Kind = "claude" | "shell";

export const allMrs = (): Mr[] => {
  const g = state.gitlab;
  const seen = new Set<string>();
  return [...g.reviews, ...g.authored, ...g.assigned, ...state.workspaces.flatMap((w) => (w.mr ? [w.mr] : []))].filter((m) => !seen.has(m.key) && (seen.add(m.key), true));
};

export const isReview = (m: Mr) => state.gitlab.reviews.some((r) => r.key === m.key);

export function pipelineTone(p: string | null): "add" | "danger" | "waiting" | "neutral" {
  if (p === "success") return "add";
  if (p === "failed") return "danger";
  if (p === "canceled" || p === "skipped" || p === "manual") return "neutral";
  return "waiting";
}

/** Give prompt to a free Claude in the workspace, to edit before sending; else start a new one with it. */
function toClaude(workspace: string | null, path: string, prompt: string) {
  const agents = state.agents.filter((a) => workspace && a.workspace === workspace && a.window && a.reachable && (a.status === "idle" || a.status === "waiting"));
  const a = agents.find((x) => x.status === "idle") ?? agents[0];
  if (workspace && a?.window) {
    go({ view: "workspace", name: workspace, window: a.window });
    setDraft({ agent: a.key, text: prompt });
  } else {
    setComposer({ path, prompt });
  }
}

/** Open the MR's code with a window of kind; Claude gets the review prompt (for a review) to edit first. */
export async function openMr(m: Mr, kind: Kind, prompt?: string) {
  const p = kind === "claude" ? (prompt ?? (isReview(m) ? m.review_prompt : "")) : "";
  // With a prompt to edit, only check the code out: the workspace opens with the agent you start.
  const r = await act({ type: "checkout", mr: m.key, kind: p ? undefined : kind }, { key: `checkout:${m.key}` });
  if (!r.ok || !r.path) return;
  if (p) return toClaude(r.workspace ?? null, r.path, p);
  if (r.workspace) go({ view: "workspace", name: r.workspace, window: r.window ?? null });
}

export async function startIssue(i: Issue) {
  const r = await act({ type: "find_clone", project: i.project }, { key: `issue:${i.key}`, quiet: true });
  if (r.ok && r.path) setComposer({ path: r.path, prompt: i.prompt });
}

export function goWorkspace(m: Mr) {
  if (m.workspace) go({ view: "workspace", name: m.workspace, window: null });
}

export type Tab = "todo" | "review" | "mine" | "issues";

export interface Entry {
  key: string;
  tab: Tab;
  /** The group in its tab: what you need to do about it. */
  group: string;
  mr?: Mr;
  issue?: Issue;
  todo?: Todo;
}

const running = (p: string | null) => p === "running" || p === "pending" || p === "created" || p === "preparing" || p === "waiting_for_resource";

export const TODO_GROUPS = ["Mentions", "Reviews", "Failed", "Assigned", "Other"];

function todoGroup(t: Todo): string {
  switch (t.action) {
    case "mentioned":
    case "directly_addressed":
      return "Mentions";
    case "review_requested":
    case "approval_required":
      return "Reviews";
    case "build_failed":
    case "unmergeable":
    case "merge_train_removed":
      return "Failed";
    case "assigned":
      return "Assigned";
    default:
      return "Other";
  }
}

export const isMention = (t: Todo) => t.action === "mentioned" || t.action === "directly_addressed";

// Done here but maybe not yet gone from the state: hidden at once.
const [doneTodos, setDoneTodos] = createSignal<number[]>([]);

export const todos = (): Todo[] => state.gitlab.todos.filter((t) => !doneTodos().includes(t.id));

/** Mark one To-Do done, or all of them without t. */
export async function markDone(t?: Todo) {
  const ids = t ? [t.id] : todos().map((x) => x.id);
  setDoneTodos((d) => [...d, ...ids]);
  const r = await act({ type: "todo_done", id: t?.id }, { key: t ? `todo:${t.id}` : "todo:all" });
  if (!r.ok) setDoneTodos((d) => d.filter((id) => !ids.includes(id)));
}

export const REVIEW_GROUPS = ["In progress", "New", "Approved"];
export const MINE_GROUPS = ["Needs you", "Waiting", "Ready to merge", "Drafts"];

function reviewGroup(m: Mr): string {
  if (m.approved && !m.workspace) return "Approved";
  return m.workspace ? "In progress" : "New";
}

function mineGroup(m: Mr): string {
  if (m.pipeline === "failed" || m.threads.length) return "Needs you";
  if (m.draft) return "Drafts";
  if (m.merge_status === "mergeable" && !running(m.pipeline)) return "Ready to merge";
  return "Waiting";
}

/** Everything in the GitLab view, by tab and group in display order, newest first in a group. */
export function entries(): Entry[] {
  const g = state.gitlab;
  const byTime = <T extends { updated: number }>(a: T, b: T) => b.updated - a.updated;
  const order = (groups: string[]) => (a: Entry, b: Entry) => groups.indexOf(a.group) - groups.indexOf(b.group);
  const reviews: Entry[] = [...g.reviews].sort(byTime).map((mr) => ({ key: mr.key, tab: "review", group: reviewGroup(mr), mr }));
  const mineMrs = [...g.authored, ...g.assigned.filter((m) => !g.authored.some((a) => a.key === m.key))].sort(byTime);
  const mine: Entry[] = mineMrs.map((mr) => ({ key: mr.key, tab: "mine", group: mineGroup(mr), mr }));
  const projects = new Set(g.issues.map((i) => i.project));
  const issues: Entry[] = [...g.issues].sort(byTime).map((issue) => ({
    key: issue.key,
    tab: "issues",
    group: projects.size > 1 ? issue.project.split("/").pop()! : "Assigned to you",
    issue,
  }));
  const issueGroups = [...new Set(issues.map((e) => e.group))];
  const mrs = allMrs();
  const todo: Entry[] = todos().map((t) => ({
    key: t.key,
    tab: "todo",
    group: todoGroup(t),
    todo: t,
    mr: t.target_type === "MergeRequest" ? mrs.find((m) => m.key === t.target) : undefined,
    issue: t.target_type === "Issue" ? g.issues.find((i) => i.key === t.target) : undefined,
  }));
  return [...todo.sort(order(TODO_GROUPS)), ...reviews.sort(order(REVIEW_GROUPS)), ...mine.sort(order(MINE_GROUPS)), ...issues.sort(order(issueGroups))];
}

/** What needs you on GitLab: your pending To-Dos. */
export const attention = (): number => todos().length;
