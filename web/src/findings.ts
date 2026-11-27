import { createEffect, createSignal, on } from "solid-js";
import { getJSON, type Agent, type ConversationItem, type Mr } from "./api";

/** The language of the fenced block Claude ends a review with: a JSON list of findings. */
export const FINDINGS_LANG = "review-findings";

export type Severity = "high" | "medium" | "low";

export interface Finding {
  id: string;
  file: string;
  /** In the new version, or in the old one when old is set: a removed line. Null for the whole file. */
  line: number | null;
  old: boolean;
  severity: Severity;
  body: string;
}

export type Decision = "kept" | "dismissed";

interface Saved {
  findings: Finding[];
  decisions: Record<string, Decision>;
  /** The MR's head when Claude reported them: line numbers may be off on a newer head. */
  head: string | null;
  /** The ids of the last block read, so a cleared block does not come back. */
  seen?: string;
}

const BLOCK = new RegExp("```" + FINDINGS_LANG + "[^\\n]*\\n([\\s\\S]*?)```", "g");

function hash(s: string): string {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(36);
}

const severity = (s: unknown): Severity => (s === "high" || s === "low" ? s : "medium");

/** The findings in the last block of text, or null when it has none or it is not valid JSON. */
export function parseFindings(text: string): Finding[] | null {
  const blocks = [...text.matchAll(BLOCK)];
  if (!blocks.length) return null;
  let raw: unknown;
  try {
    raw = JSON.parse(blocks[blocks.length - 1][1]);
  } catch {
    return null;
  }
  if (!Array.isArray(raw)) return null;
  const out: Finding[] = [];
  for (const r of raw as Record<string, unknown>[]) {
    if (!r || typeof r.file !== "string" || typeof r.body !== "string" || !r.body.trim()) continue;
    const line = typeof r.line === "number" && r.line > 0 ? Math.floor(r.line) : null;
    const f = { file: r.file.replace(/^\.?\//, ""), line, old: r.old === true, severity: severity(r.severity), body: r.body.trim() };
    out.push({ ...f, id: hash(`${f.file}:${f.line}:${f.old}:${f.body}`) });
  }
  return out;
}

const RANK: Record<Severity, number> = { high: 0, medium: 1, low: 2 };
export const worst = (fs: Finding[]): Severity | null => (fs.length ? fs.reduce((a, f) => (RANK[f.severity] < RANK[a] ? f.severity : a), "low" as Severity) : null);

const storeKey = (mr: string) => `findings:${mr}`;

function load(mr: string): Saved {
  try {
    const v = localStorage.getItem(storeKey(mr));
    if (v) return JSON.parse(v) as Saved;
  } catch {
    /* ignore */
  }
  return { findings: [], decisions: {}, head: null };
}

function save(mr: string, s: Saved) {
  try {
    if (s.findings.length || s.seen) localStorage.setItem(storeKey(mr), JSON.stringify(s));
    else localStorage.removeItem(storeKey(mr));
  } catch {
    /* ignore */
  }
}

/**
 * Claude's findings for the MR, from the last findings block in the conversation of agent.
 * They are kept in this browser with what you decided about each, so they outlive the agent.
 */
export function createFindings(mr: () => Mr, agent: () => Agent | undefined, head: () => string | null | undefined) {
  const [saved, setSaved] = createSignal<Saved>(load(mr().key));
  createEffect(on(() => mr().key, (k) => setSaved(load(k)), { defer: true }));

  const update = (fn: (s: Saved) => Saved) => {
    const next = fn(saved());
    setSaved(next);
    save(mr().key, next);
  };

  // Read the conversation once each turn ends: a block written mid-turn may still grow.
  createEffect(
    on(
      () => {
        const a = agent();
        return a && a.status !== "working" ? `${a.key}:${a.turns}:${a.status}` : null;
      },
      async (k) => {
        const a = agent();
        if (!k || !a) return;
        const key = mr().key;
        let items: ConversationItem[];
        try {
          items = (await getJSON<{ items: ConversationItem[] }>(`/api/conversation?agent=${encodeURIComponent(a.key)}`)).items;
        } catch {
          return;
        }
        if (mr().key !== key) return;
        for (let i = items.length - 1; i >= 0; i--) {
          const it = items[i];
          if (it.kind !== "assistant") continue;
          const found = parseFindings(it.text);
          if (!found) continue;
          const ids = found.map((f) => f.id).join();
          if (ids === (saved().seen ?? saved().findings.map((f) => f.id).join())) return;
          update((s) => ({
            seen: ids,
            findings: found,
            decisions: Object.fromEntries(Object.entries(s.decisions).filter(([id]) => found.some((f) => f.id === id))),
            head: head() ?? null,
          }));
          return;
        }
      },
    ),
  );

  const decide = (id: string, d: Decision | null) =>
    update((s) => {
      const decisions = { ...s.decisions };
      if (d) decisions[id] = d;
      else delete decisions[id];
      return { ...s, decisions };
    });

  return {
    all: () => saved().findings,
    /** The ones you have not kept or thrown away yet. */
    open: () => saved().findings.filter((f) => !saved().decisions[f.id]),
    decision: (id: string) => saved().decisions[id],
    decide,
    /** True when the MR moved on since Claude reported them. */
    stale: () => {
      const h = head();
      return !!(saved().head && h && saved().head !== h);
    },
    clear: () => update((s) => ({ findings: [], decisions: {}, head: null, seen: s.seen ?? s.findings.map((f) => f.id).join() })),
  };
}

export type Findings = ReturnType<typeof createFindings>;
