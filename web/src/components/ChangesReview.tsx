import { For, Show, createEffect, createMemo, createResource, createSignal } from "solid-js";
import { getJSON, type Commit } from "../api";
import { language } from "../highlight";
import { act, diffView, go, setDiffView, setDraft } from "../store";
import { cls } from "../util";
import { parseUnified, type DiffFile } from "./Diff";
import { Chat, FileDiff, Trash, X } from "./icons";
import { highlightFile, linesLabel, load, save, split, toggleSplit, workspaceAgent } from "./MrReview";
import { ReviewDiff } from "./ReviewDiff";
import { Button, IconButton } from "./ui";

/** A comment on lines a..b of a file in the uncommitted changes (commit null) or in one commit. */
interface Note {
  id: number;
  commit: Commit | null;
  file: string;
  a: number;
  b: number;
  where: string;
  text: string;
}

/** a and b of a comment on the whole file. */
const FILE = -1;

function restore<T>(key: string, fallback: T): T {
  try {
    return JSON.parse(load(key) ?? "") ?? fallback;
  } catch {
    return fallback;
  }
}

/** Your unsent comments per checkout path: they outlive the modal and a reload until you send them. */
const [notes, setNotes] = createSignal<Record<string, Note[]>>(restore("review-notes", {}));
/** A review waiting for the Claude that is starting in the workspace. */
const [pending, setPending] = createSignal<{ workspace: string; text: string } | null>(restore("review-pending", null));

export function reviewPrompt(list: Note[]): string {
  const items = list.map((n, k) => {
    const src = n.commit ? `in commit ${n.commit.short} "${n.commit.title}"` : "in the uncommitted changes";
    return `(${k + 1}) ${n.file}, ${n.where} ${src}: ${n.text.trim()}`;
  });
  return `I reviewed the code and left these comments, please address each one: ${items.join(" ")}`;
}

function toAgent(workspace: string, text: string) {
  const a = workspaceAgent(workspace);
  if (a) {
    setDraft({ agent: a.key, text });
    go({ view: "workspace", name: workspace, window: a.window });
    return;
  }
  setPending({ workspace, text });
  void act({ type: "new_window", workspace, kind: "claude" }, { key: `new:claude` }).then((r) => {
    if (!r.ok) setPending(null);
    else if (r.window) go({ view: "workspace", name: workspace, window: r.window });
  });
}

type Form = { file: string; a: number; b: number; text: string };

/** The comments at one place, and the comment form when it is open there. */
function Thread(p: { notes: Note[]; form: Form | null; setText: (t: string) => void; onDelete: (id: number) => void; onAdd: () => void; onCancel: () => void }) {
  return (
    <div class="flex flex-col gap-2">
      <For each={p.notes}>
        {(n) => (
          <div class="flex items-start gap-2 rounded-lg border border-line bg-panel px-3 py-2 font-sans text-[13px]">
            <div class="min-w-0 flex-1">
              <div class="text-[11.5px] text-fg-3">{n.where}</div>
              <div class="whitespace-pre-wrap text-fg">{n.text}</div>
            </div>
            <IconButton label="Delete comment" onClick={() => p.onDelete(n.id)}>
              <Trash size={13} />
            </IconButton>
          </div>
        )}
      </For>
      <Show when={p.form}>
        <div class="flex flex-col gap-2">
          <textarea
            class="min-h-[4.5rem] w-full resize-y rounded-lg border border-line-strong bg-raised px-3 py-2 font-sans text-[13px] text-fg outline-none focus:border-accent/60"
            placeholder="Comment for Claude…  (Ctrl+Enter to add)"
            value={p.form?.text ?? ""}
            ref={(el) => queueMicrotask(() => el.focus())}
            onInput={(e) => p.setText(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") (e.stopPropagation(), p.onCancel());
              else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) (e.preventDefault(), p.onAdd());
            }}
          />
          <div class="flex justify-end gap-2">
            <Button size="sm" onClick={p.onCancel}>
              Cancel
            </Button>
            <Button size="sm" variant="primary" disabled={!p.form?.text.trim()} onClick={p.onAdd}>
              Add comment
            </Button>
          </div>
        </div>
      </Show>
    </div>
  );
}

function FileSection(p: { f: DiffFile; notes: Note[]; form: Form | null; open: (a: number, b: number) => void; setText: (t: string) => void; onDelete: (id: number) => void; onAdd: () => void; onCancel: () => void }) {
  const lang = language(p.f.name);
  // shortcut: highlights every file up front, unlike MrReview's lazy FileBox; fine for a branch's worth of changes.
  const html = createMemo(() => (lang && p.f.lines.length <= 4000 ? highlightFile(p.f.lines, lang) : undefined));
  const sel = (): [number, number] | null => (p.form && p.form.a !== FILE ? [p.form.a, p.form.b] : null);
  const at = (i: number) => p.notes.filter((n) => n.b === i);
  // ReviewDiff builds after(i) once: the props must stay getters, so the form state is passed as a test.
  const thread = (i: number, here: () => boolean) => (
    <Thread notes={at(i)} form={here() ? p.form : null} setText={p.setText} onDelete={p.onDelete} onAdd={p.onAdd} onCancel={p.onCancel} />
  );
  return (
    <section class="overflow-hidden rounded-xl border border-line bg-panel">
      <header class="sticky top-0 z-[1] flex items-center gap-2 border-b border-line bg-raised/95 px-3 py-2 backdrop-blur">
        <span class="truncate font-mono text-[12px] text-fg">{p.f.name}</span>
        <span class="ml-auto font-mono text-[11.5px] tabular-nums">
          <span class="text-add">+{p.f.added}</span> <span class="text-del">−{p.f.removed}</span>
        </span>
        <button class="grid size-6 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-accent" onClick={() => p.open(FILE, FILE)} title="Comment on this file" aria-label="Comment on this file">
          <Chat size={13} />
        </button>
      </header>
      <Show when={at(FILE).length || p.form?.a === FILE}>
        <div class="border-b border-line bg-bg px-3 py-2">{thread(FILE, () => p.form?.a === FILE)}</div>
      </Show>
      <Show when={!p.f.binary} fallback={<div class="px-3 py-3 text-fg-3">Binary file</div>}>
        <ReviewDiff
          lines={p.f.lines}
          html={html()}
          split={split()}
          sel={sel()}
          onPick={(i, extend) => {
            const s = sel();
            p.open(extend && s ? s[0] : i, i);
          }}
          onSelect={(a, b) => p.open(a, b)}
          has={(i) => sel()?.[1] === i || at(i).length > 0}
          after={(i) => thread(i, () => sel()?.[1] === i)}
        />
      </Show>
    </section>
  );
}

export function ChangesReview() {
  createEffect(() => save("review-notes", JSON.stringify(notes())));
  createEffect(() => save("review-pending", JSON.stringify(pending())));
  // A new Claude shows up in state once its SessionStart hook reports; then the review goes to its reply box.
  createEffect(() => {
    const p = pending();
    const a = p && workspaceAgent(p.workspace);
    if (p && a) {
      setPending(null);
      setDraft({ agent: a.key, text: p.text });
    }
  });

  const commit = () => diffView()?.commit ?? null;
  const [diff] = createResource(diffView, (d) => {
      const q = (d.commit ? `&commit=${d.commit.sha}` : "") + (d.file ? `&file=${encodeURIComponent(d.file)}` : "");
      return getJSON<{ diff: string }>(`/api/diff?path=${encodeURIComponent(d.path)}${q}`);
  });
  const files = createMemo(() => parseUnified(diff.latest?.diff ?? ""));

  const path = () => diffView()?.path ?? "";
  const mine = () => notes()[path()] ?? [];
  const setMine = (f: (l: Note[]) => Note[]) => setNotes((all) => ({ ...all, [path()]: f(all[path()] ?? []) }));
  const here = (file: string) => mine().filter((n) => n.file === file && (n.commit?.sha ?? "") === (commit()?.sha ?? ""));

  const [form, setForm] = createSignal<Form | null>(null);
  createEffect(() => (diffView(), setForm(null)));
  const open = (f: DiffFile, a: number, b: number) => setForm((o) => ({ file: f.name, a: Math.min(a, b), b: Math.max(a, b), text: o?.text ?? "" }));
  const add = (f: DiffFile) => {
    const fm = form();
    if (!fm || !fm.text.trim()) return;
    const where = fm.a === FILE ? "the whole file" : linesLabel(f.lines.slice(fm.a, fm.b + 1).filter((l) => l.type !== "hunk"));
    setMine((l) => [...l, { id: Date.now(), commit: commit(), file: f.name, a: fm.a, b: fm.b, where, text: fm.text }]);
    setForm(null);
  };

  const close = () => setDiffView(null);
  const send = () => {
    const d = diffView();
    if (!d || !mine().length) return;
    const text = reviewPrompt(mine());
    setMine(() => []);
    close();
    toAgent(d.workspace, text);
  };
  const title = () => {
    const d = diffView();
    const c = d?.commit;
    return d?.file ? (c ? `${d.file} in ${c.short}` : d.file) : c ? `${c.short} ${c.title}` : d?.title;
  };

  return (
    <Show when={diffView()}>
      {(d) => (
        <div
          class="fixed inset-0 z-50 flex justify-end bg-black/30 backdrop-blur-[2px] animate-fade-in"
          onMouseDown={(e) => e.target === e.currentTarget && close()}
          onKeyDown={(e) => e.key === "Escape" && close()}
          tabIndex={-1}
          ref={(el) => queueMicrotask(() => el.focus())}
        >
          <div class={cls("flex h-full w-full flex-col border-l border-line-strong bg-bg shadow-float animate-slide-up", split() ? "max-w-7xl" : "max-w-4xl")}>
            <header class="flex flex-wrap items-center gap-2 border-b border-line px-5 py-3">
              <FileDiff size={16} class="text-fg-3" />
              <h2 class="min-w-0 flex-1 truncate font-mono text-[13px] text-fg">{title()}</h2>
              <div class="flex rounded-lg bg-hover p-0.5 text-[12px]" role="group" title="Layout">
                <button class={cls("rounded-md px-2 py-1", !split() ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => split() && toggleSplit()}>
                  Unified
                </button>
                <button class={cls("rounded-md px-2 py-1", split() ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => !split() && toggleSplit()}>
                  Side by side
                </button>
              </div>
              <Show when={mine().length}>
                <Button size="sm" variant="primary" onClick={send} title="Put your comments in the workspace Claude's reply box">
                  <Chat size={13} /> Send review ({mine().length})
                </Button>
              </Show>
              <IconButton label="Close (Esc)" onClick={close}>
                <X size={16} />
              </IconButton>
            </header>
            <div class="min-h-0 flex-1 overflow-y-auto p-5">
              <Show when={!diff.loading} fallback={<div class="flex flex-col gap-2">{[0, 1, 2, 3].map(() => <div class="skeleton h-6" />)}</div>}>
                <Show when={!diff.error} fallback={<div class="text-danger">{String(diff.error)}</div>}>
                  <Show when={files().length} fallback={<div class="px-4 py-10 text-center text-fg-3">{commit() ? "This commit changes no files" : "No changes against HEAD"}</div>}>
                    <p class="mb-3 text-[12px] text-fg-3">Click or drag line numbers to comment. Comments collect until you send them as one review to {d().workspace}'s Claude.</p>
                    <div class="flex flex-col gap-4">
                      <For each={files()}>
                        {(f) => (
                          <FileSection
                            f={f}
                            notes={here(f.name)}
                            form={form()?.file === f.name ? form() : null}
                            open={(a, b) => open(f, a, b)}
                            setText={(t) => setForm((o) => o && { ...o, text: t })}
                            onDelete={(id) => setMine((l) => l.filter((x) => x.id !== id))}
                            onAdd={() => add(f)}
                            onCancel={() => setForm(null)}
                          />
                        )}
                      </For>
                    </div>
                  </Show>
                </Show>
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
