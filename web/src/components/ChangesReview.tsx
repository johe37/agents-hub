import { For, Show, createEffect, createMemo, createResource, createSignal } from "solid-js";
import { getJSON, type Commit } from "../api";
import { act, diffView, go, setDiffView, setDraft } from "../store";
import { parseUnified, type DiffFile } from "./Diff";
import { Chat, FileDiff, Trash, X } from "./icons";
import { linesLabel, workspaceAgent } from "./MrReview";
import { ReviewDiff } from "./ReviewDiff";
import { Button, IconButton } from "./ui";

/** A comment on lines a..b of a file in the uncommitted changes (commit "") or in one commit. */
interface Note {
  id: number;
  commit: Commit | null;
  file: string;
  a: number;
  b: number;
  where: string;
  text: string;
}

/** Your unsent comments per checkout path: they outlive the modal until you send them. */
const [notes, setNotes] = createSignal<Record<string, Note[]>>({});
/** A review waiting for the Claude that is starting in the workspace. */
const [pending, setPending] = createSignal<{ workspace: string; text: string } | null>(null);
let seq = 0;

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

export function ChangesReview() {
  // A new Claude shows up in state once its SessionStart hook reports; then the review goes to its reply box.
  createEffect(() => {
    const p = pending();
    const a = p && workspaceAgent(p.workspace);
    if (p && a) {
      setPending(null);
      setDraft({ agent: a.key, text: p.text });
    }
  });

  const [commit, setCommit] = createSignal<Commit | null>(null);
  createEffect(() => diffView() && setCommit(null));
  const [commits] = createResource(
    () => diffView()?.path,
    (path) => getJSON<Commit[]>(`/api/diff?path=${encodeURIComponent(path)}&commits=1`),
  );
  const [diff] = createResource(
    () => diffView() && ([diffView()!, commit()] as const),
    ([d, c]) => {
      const q = c ? `&commit=${c.sha}` : d.file ? `&file=${encodeURIComponent(d.file)}` : "";
      return getJSON<{ diff: string }>(`/api/diff?path=${encodeURIComponent(d.path)}${q}`);
    },
  );
  const files = createMemo(() => parseUnified(diff.latest?.diff ?? ""));

  const path = () => diffView()?.path ?? "";
  const mine = () => notes()[path()] ?? [];
  const setMine = (f: (l: Note[]) => Note[]) => setNotes((all) => ({ ...all, [path()]: f(all[path()] ?? []) }));
  const here = (file: string) => mine().filter((n) => n.file === file && (n.commit?.sha ?? "") === (commit()?.sha ?? ""));

  const [form, setForm] = createSignal<{ file: string; a: number; b: number; text: string } | null>(null);
  createEffect(() => (diffView(), commit(), setForm(null)));
  const open = (f: DiffFile, a: number, b: number) => setForm((o) => ({ file: f.name, a: Math.min(a, b), b: Math.max(a, b), text: o?.text ?? "" }));
  const add = (f: DiffFile) => {
    const fm = form();
    if (!fm || !fm.text.trim()) return;
    const where = linesLabel(f.lines.slice(fm.a, fm.b + 1).filter((l) => l.type !== "hunk"));
    setMine((l) => [...l, { id: ++seq, commit: commit(), file: f.name, a: fm.a, b: fm.b, where, text: fm.text }]);
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
    const c = commit();
    return c ? `${c.short} ${c.title}` : diffView()?.title;
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
          <div class="flex h-full w-full max-w-4xl flex-col border-l border-line-strong bg-bg shadow-float animate-slide-up">
            <header class="flex flex-wrap items-center gap-2 border-b border-line px-5 py-3">
              <FileDiff size={16} class="text-fg-3" />
              <h2 class="min-w-0 flex-1 truncate font-mono text-[13px] text-fg">{title()}</h2>
              <select
                class="max-w-[16rem] truncate rounded-md border border-line bg-raised px-2 py-1 text-[12px] text-fg"
                title="Show the uncommitted changes or one commit"
                value={commit()?.sha ?? ""}
                onChange={(e) => setCommit(commits()?.find((c) => c.sha === e.currentTarget.value) ?? null)}
              >
                <option value="">Uncommitted changes</option>
                <For each={commits() ?? []}>{(c) => <option value={c.sha}>{`${c.short} ${c.title}`}</option>}</For>
              </select>
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
                        {(f) => {
                          const sel = (): [number, number] | null => {
                            const fm = form();
                            return fm && fm.file === f.name ? [fm.a, fm.b] : null;
                          };
                          return (
                            <section class="overflow-hidden rounded-xl border border-line bg-panel">
                              <header class="sticky top-0 z-[1] flex items-center gap-2 border-b border-line bg-raised/95 px-3 py-2 backdrop-blur">
                                <span class="truncate font-mono text-[12px] text-fg">{f.name}</span>
                                <span class="ml-auto font-mono text-[11.5px] tabular-nums">
                                  <span class="text-add">+{f.added}</span> <span class="text-del">−{f.removed}</span>
                                </span>
                              </header>
                              <Show when={!f.binary} fallback={<div class="px-3 py-3 text-fg-3">Binary file</div>}>
                                <ReviewDiff
                                  lines={f.lines}
                                  split={false}
                                  sel={sel()}
                                  onPick={(i, extend) => {
                                    const s = sel();
                                    open(f, extend && s ? s[0] : i, i);
                                  }}
                                  onSelect={(a, b) => open(f, a, b)}
                                  has={(i) => sel()?.[1] === i || here(f.name).some((n) => n.b === i)}
                                  after={(i) => (
                                    <div class="flex flex-col gap-2">
                                      <For each={here(f.name).filter((n) => n.b === i)}>
                                        {(n) => (
                                          <div class="flex items-start gap-2 rounded-lg border border-line bg-panel px-3 py-2">
                                            <div class="min-w-0 flex-1">
                                              <div class="text-[11.5px] text-fg-3">{n.where}</div>
                                              <div class="whitespace-pre-wrap text-fg">{n.text}</div>
                                            </div>
                                            <IconButton label="Delete comment" onClick={() => setMine((l) => l.filter((x) => x.id !== n.id))}>
                                              <Trash size={13} />
                                            </IconButton>
                                          </div>
                                        )}
                                      </For>
                                      <Show when={sel()?.[1] === i}>
                                        <div class="flex flex-col gap-2">
                                          <textarea
                                            class="min-h-[4.5rem] w-full resize-y rounded-lg border border-line-strong bg-raised px-3 py-2 text-fg outline-none focus:border-accent/60"
                                            placeholder="Comment for Claude…  (Ctrl+Enter to add)"
                                            value={form()?.text ?? ""}
                                            ref={(el) => queueMicrotask(() => el.focus())}
                                            onInput={(e) => setForm((o) => o && { ...o, text: e.currentTarget.value })}
                                            onKeyDown={(e) => {
                                              if (e.key === "Escape") (e.stopPropagation(), setForm(null));
                                              else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) (e.preventDefault(), add(f));
                                            }}
                                          />
                                          <div class="flex justify-end gap-2">
                                            <Button size="sm" onClick={() => setForm(null)}>
                                              Cancel
                                            </Button>
                                            <Button size="sm" variant="primary" disabled={!form()?.text.trim()} onClick={() => add(f)}>
                                              Add comment
                                            </Button>
                                          </div>
                                        </div>
                                      </Show>
                                    </div>
                                  )}
                                />
                              </Show>
                            </section>
                          );
                        }}
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
