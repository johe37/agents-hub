import type { Agent, Status } from "./api";

export const STATUS: Record<Status, { label: string; color: string; soft: string; rank: number }> = {
  waiting: { label: "Needs you", color: "var(--waiting)", soft: "var(--waiting-soft)", rank: 0 },
  working: { label: "Working", color: "var(--working)", soft: "var(--working-soft)", rank: 1 },
  idle: { label: "Done", color: "var(--idle)", soft: "var(--idle-soft)", rank: 2 },
  unknown: { label: "Running", color: "var(--fg-3)", soft: "var(--hover)", rank: 3 },
  ended: { label: "Ended", color: "var(--ended)", soft: "var(--hover)", rank: 4 },
  stale: { label: "Gone", color: "var(--ended)", soft: "var(--hover)", rank: 5 },
};

export const alive = (s: Status) => s !== "ended" && s !== "stale";

export function worst(agents: Agent[]): Status | null {
  let best: Status | null = null;
  for (const a of agents) if (alive(a.status) && (best === null || STATUS[a.status].rank < STATUS[best].rank)) best = a.status;
  return best;
}

/** "12s", "4m", "3h", "2d" since a unix time in seconds. */
export function age(ts: number, now: number): string {
  if (!ts) return "";
  const s = Math.max(0, Math.floor(now / 1000 - ts));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export function until(ts: number, now: number): string {
  const s = Math.max(0, Math.floor(ts - now / 1000));
  if (s < 3600) return `${Math.ceil(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
  return `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;
}

/** Subsequence match with a score that favours word starts and runs. Returns null when it does not match. */
export function fuzzy(query: string, text: string): { score: number; hits: number[] } | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return { score: 0, hits: [] };
  const t = text.toLowerCase();
  const hits: number[] = [];
  let score = 0;
  let ti = 0;
  let prev = -2;
  for (const ch of q) {
    const i = t.indexOf(ch, ti);
    if (i < 0) return null;
    hits.push(i);
    const start = i === 0 || "/-_. ".includes(t[i - 1]);
    score += 1 + (i === prev + 1 ? 3 : 0) + (start ? 4 : 0);
    prev = i;
    ti = i + 1;
  }
  // Prefer matches near the end of a path: the repo name, not its parents.
  score -= (t.length - prev) * 0.02;
  return { score, hits };
}

/** Space-separated words must all match, in any order. */
export function fuzzyWords(query: string, text: string): number | null {
  let total = 0;
  for (const w of query.trim().split(/\s+/).filter(Boolean)) {
    const m = fuzzy(w, text);
    if (!m) return null;
    total += m.score;
  }
  return total;
}

export function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

export function cls(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
export const MOD = isMac ? "⌘" : "Ctrl";

/** Short model name: claude-opus-5-5 → Opus 5.5. */
export function modelName(m: string | null): string | null {
  if (!m) return null;
  const r = /claude-([a-z]+)-(\d+)(?:-(\d+))?/.exec(m);
  if (!r) return m;
  return `${r[1][0].toUpperCase()}${r[1].slice(1)} ${r[2]}${r[3] && r[3].length <= 2 ? "." + r[3] : ""}`;
}

export function money(n: number | null): string | null {
  if (n == null) return null;
  return n < 1 ? `$${n.toFixed(2)}` : `$${n.toFixed(n < 10 ? 2 : 0)}`;
}

/** The line of a tool input worth showing: the command, the file, the pattern. */
export function toolLine(input: Record<string, unknown>): string | null {
  for (const k of ["command", "file_path", "pattern", "query", "url", "skill", "description", "prompt", "path"]) {
    const v = input[k];
    if (typeof v === "string" && v.trim()) return v.trim();
  }
  return null;
}
