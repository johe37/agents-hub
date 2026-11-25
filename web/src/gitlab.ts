import type { Issue, Mr } from "./api";
import { act, go, setComposer, setDraft, state } from "./store";

export type Kind = "claude" | "shell" | "nvim";

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

export type Tab = "review" | "mine" | "issues";

export interface Entry {
  key: string;
  tab: Tab;
  /** The group in its tab: what you need to do about it. */
  group: string;
  mr?: Mr;
  issue?: Issue;
}

const running = (p: string | null) => p === "running" || p === "pending" || p === "created" || p === "preparing" || p === "waiting_for_resource";

export const REVIEW_GROUPS = ["New", "In progress", "Approved"];
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
  return [...reviews.sort(order(REVIEW_GROUPS)), ...mine.sort(order(MINE_GROUPS)), ...issues.sort(order(issueGroups))];
}

/** What needs you on GitLab: reviews not started, and your MRs with a failed pipeline or open threads. */
export function attention(): number {
  return entries().filter((e) => (e.tab === "review" && e.group === "New") || (e.tab === "mine" && e.group === "Needs you")).length;
}
