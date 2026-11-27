import { For, Show, createMemo, createSignal, type Signal } from "solid-js";
import type { Agent, ConversationItem } from "../api";
import { openDiff, setDraft, tilde } from "../store";
import { DiffLines, lineDiff, parseHunks, type DiffLine } from "./Diff";
import { FileDiff, Sparkles } from "./icons";
import { linesLabel } from "./MrReview";
import { Button } from "./ui";

export type Tool = Extract<ConversationItem, { kind: "tool" }>;
type Input = Record<string, unknown>;

const EDITS = new Set(["Edit", "MultiEdit", "Write"]);
export const isEdit = (t: Tool) => EDITS.has(t.name);

const str = (v: unknown) => (typeof v === "string" ? v : "");

/** The page reloads the conversation many times a second while Claude works, so row state lives outside the rows. */
const kept = new Map<string, Signal<unknown>>();
export function keep<T>(key: string, init: T): Signal<T> {
  let s = kept.get(key);
  if (!s) kept.set(key, (s = createSignal<unknown>(init)));
  return s as Signal<T>;
}

export interface Change {
  file: string;
  lines: DiffLine[];
  added: number;
  removed: number;
  /** Line numbers are the file's. Without the applied patch they only count from the edit. */
  exact: boolean;
}

/** What an edit did to its file: the patch Claude Code applied, else the edit as Claude asked for it. */
export function change(t: Tool): Change | null {
  if (!isEdit(t) || t.result?.error) return null;
  const file = str(t.input.file_path);
  if (t.result?.patch) return { file, ...parseHunks(t.result.patch), exact: true };
  if (t.name === "Write") {
    const lines = str(t.input.content).split("\n").map((text, i) => ({ type: "add" as const, text, new: i + 1 }));
    return { file, lines, added: lines.length, removed: 0, exact: true };
  }
  const edits = t.name === "Edit" ? [t.input] : Array.isArray(t.input.edits) ? (t.input.edits as Input[]) : [];
  const lines: DiffLine[] = edits.flatMap((e, k) => [
    ...(k ? [{ type: "hunk" as const, text: "" }] : []),
    ...lineDiff(str(e.old_string), str(e.new_string)).map((l) => ({ type: l.type, text: l.text })),
  ]);
  return { file, lines, added: lines.filter((l) => l.type === "add").length, removed: lines.filter((l) => l.type === "del").length, exact: false };
}

function openFile(a: Agent, file: string) {
  openDiff({ workspace: a.workspace ?? a.name, path: a.path, file, title: tilde(file), agent: a.key });
}

function Stat(p: { added: number; removed: number }) {
  return (
    <span class="shrink-0 font-mono text-xs tabular-nums">
      <span class="text-add">+{p.added}</span> <span class="text-del">−{p.removed}</span>
    </span>
  );
}

function FileName(p: { file: string }) {
  const parts = () => {
    const path = tilde(p.file);
    const i = path.lastIndexOf("/");
    return [path.slice(0, i + 1), path.slice(i + 1)];
  };
  return (
    <span class="min-w-0 truncate font-mono text-sm">
      <span class="text-fg-3">{parts()[0]}</span>
      <span class="text-fg">{parts()[1]}</span>
    </span>
  );
}

/** One edit as a diff. Select lines in the gutter to ask Claude about them. */
export function EditView(p: { id: string; agent: Agent; change: Change }) {
  const sel = createMemo(() => keep<[number, number] | null>(`sel:${p.id}`, null));
  let anchor = 0;
  const pick = (i: number, extend: boolean) => {
    if (p.change.lines[i]?.type === "hunk") return;
    const [cur, set] = sel();
    const s = cur();
    if (extend && s) set([Math.min(anchor, i), Math.max(anchor, i)]);
    else if (s && s[0] === i && s[1] === i) set(null);
    else {
      anchor = i;
      set([i, i]);
    }
  };
  const picked = () => {
    const s = sel()[0]();
    return s ? p.change.lines.slice(s[0], s[1] + 1).filter((l) => l.type !== "hunk") : [];
  };
  const ask = () => {
    const lines = picked();
    const where = p.change.exact ? linesLabel(lines) : "these lines";
    const code = lines.map((l) => (l.type === "add" ? "+" : l.type === "del" ? "-" : " ") + l.text).join("\n");
    setDraft({ agent: p.agent.key, text: `In ${tilde(p.change.file)}, ${where} of your edit:\n\`\`\`diff\n${code}\n\`\`\`\n` });
    sel()[1](null);
  };
  return (
    <div class="flex min-w-0 flex-col gap-1.5">
      <button class="group/file flex min-w-0 items-center gap-1.5 self-start text-left" title="Show the file's changes against HEAD" onClick={() => openFile(p.agent, p.change.file)}>
        <FileName file={p.change.file} />
        <FileDiff size={12} class="shrink-0 text-fg-3 group-hover/file:text-fg" />
      </button>
      <div class="overflow-hidden rounded-lg border border-line bg-bg">
        <DiffLines lines={p.change.lines} numbers={p.change.exact} class="max-h-96" sel={sel()[0]()} pick={pick} />
      </div>
      <Show when={picked().length}>
        <div class="flex items-center gap-2 animate-fade-in">
          <Button size="sm" variant="primary" onClick={ask} title="Put the lines in the reply box">
            <Sparkles size={13} /> Ask Claude about {picked().length === 1 ? "this line" : `${picked().length} lines`}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => sel()[1](null)}>
            Cancel
          </Button>
          <span class="text-xs text-fg-3">Shift+click extends the selection</span>
        </div>
      </Show>
    </div>
  );
}

/** The files a turn changed, at the end of the turn. */
export function TurnChanges(p: { agent: Agent; changes: Change[] }) {
  const files = createMemo(() => {
    const m = new Map<string, { added: number; removed: number }>();
    for (const c of p.changes) {
      const s = m.get(c.file) ?? { added: 0, removed: 0 };
      s.added += c.added;
      s.removed += c.removed;
      m.set(c.file, s);
    }
    return [...m];
  });
  const total = () => files().reduce((t, [, s]) => ({ added: t.added + s.added, removed: t.removed + s.removed }), { added: 0, removed: 0 });
  return (
    <div class="rounded-xl border border-line bg-panel/60 px-2 py-2 animate-fade-in">
      <div class="mb-1 flex items-center gap-2 px-1.5 text-sm text-fg-2">
        <FileDiff size={13} class="text-fg-3" />
        <span>
          {files().length} {files().length === 1 ? "file" : "files"} changed
        </span>
        <Stat {...total()} />
      </div>
      <For each={files()}>
        {([file, s]) => (
          <button class="flex w-full min-w-0 items-center gap-2 rounded-md px-1.5 py-0.5 text-left hover:bg-hover" title={tilde(file)} onClick={() => openFile(p.agent, file)}>
            <FileName file={file} />
            <span class="ml-auto" />
            <Stat {...s} />
          </button>
        )}
      </For>
    </div>
  );
}
