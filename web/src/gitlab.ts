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
function toClaude(workspace: string, path: string, prompt: string) {
  const agents = state.agents.filter((a) => a.workspace === workspace && a.window && a.reachable && (a.status === "idle" || a.status === "waiting"));
  const a = agents.find((x) => x.status === "idle") ?? agents[0];
  if (a?.window) {
    go({ view: "workspace", name: workspace, window: a.window });
    setDraft({ agent: a.key, text: prompt });
  } else {
    setComposer({ path, prompt });
  }
}

/** The MR's code in a workspace: the one that has it, or a new checkout. */
async function checkout(m: Mr) {
  const r = await act({ type: "checkout", mr: m.key }, { key: `checkout:${m.key}` });
  return r.ok && r.workspace && r.path ? { workspace: r.workspace, path: r.path } : null;
}

/** Open the MR's code with a window of kind; Claude gets the review prompt (for a review) to edit first. */
export async function openMr(m: Mr, kind: Kind, prompt?: string) {
  const ws = await checkout(m);
  if (!ws) return;
  if (kind === "claude") {
    const p = prompt ?? (isReview(m) ? m.review_prompt : "");
    if (p) return toClaude(ws.workspace, ws.path, p);
  }
  const r = await act({ type: "new_window", workspace: ws.workspace, kind }, { quiet: true });
  if (r.ok && r.window) go({ view: "workspace", name: ws.workspace, window: r.window });
}

export async function startIssue(i: Issue) {
  const r = await act({ type: "find_clone", project: i.project }, { key: `issue:${i.key}`, quiet: true });
  if (r.ok && r.path) setComposer({ path: r.path, prompt: i.prompt });
}

export function goWorkspace(m: Mr) {
  if (m.workspace) go({ view: "workspace", name: m.workspace, window: null });
}
