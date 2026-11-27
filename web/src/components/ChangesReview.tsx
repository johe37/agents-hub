import { For, Show, createEffect, createMemo, createResource, createSignal } from "solid-js";
import { getJSON, type Agent } from "../api";
import { language } from "../highlight";
import { act, agentRoute, diffView, go, setDiffView, setModes, state, type DiffTarget } from "../store";
import { alive, cls } from "../util";
import { parseUnified, type DiffFile } from "./Diff";
import { FileDiff, Refresh, Sparkles, X } from "./icons";
import { highlightFile, linesLabel, split, toggleSplit, workspaceAgent } from "./MrReview";
import { ReviewDiff } from "./ReviewDiff";
import { Button, IconButton, Spinner } from "./ui";

/** A question waiting for the Claude that is starting in the workspace. */
const [pending, setPending] = createSignal<{ workspace: string; text: string } | null>(null);

/** Type text in the agent's input, unsent, and show its terminal so you can go on writing. */
function typeTo(a: Agent, text: string) {
  void act({ type: "type", agent: a.key, text }, { quiet: true });
  if (a.window) setModes((m) => ({ ...m, [a.window!]: "terminal" }));
  setDiffView(null);
  go(agentRoute(a));
}

function ask(d: DiffTarget, text: string) {
  const own = d.agent ? state.agents.find((a) => a.key === d.agent && a.reachable && alive(a.status)) : undefined;
  const a = own ?? workspaceAgent(d.workspace);
  if (a) return typeTo(a, text);
  if (!state.workspaces.some((w) => w.name === d.workspace)) return;
  setPending({ workspace: d.workspace, text });
  void act({ type: "new_window", workspace: d.workspace, kind: "agent" }, { key: `new:agent` }).then((r) => {
    if (!r.ok) setPending(null);
    else if (r.window) go({ view: "workspace", name: d.workspace, window: r.window });
  });
}

function askPrompt(d: DiffTarget, f: DiffFile, lines: string | null): string {
  const c = d.commit;
  const where = c ? `in commit ${c.short} "${c.title}"` : "in the uncommitted changes";
  const cmd = c ? `git show ${c.short} -- ${f.name}` : `git diff HEAD -- ${f.name}`;
  return `About ${f.name}${lines ? `, ${lines},` : ""} ${where} (see \`${cmd}\`):`;
}

/** Hands a question to a Claude that is starting: it shows up in state once its SessionStart hook reports. */
export function AskKeeper() {
  createEffect(() => {
    const p = pending();
    const a = p && workspaceAgent(p.workspace);
    if (p && a) {
      setPending(null);
      typeTo(a, p.text);
    }
  });
  return null;
}

type Sel = { file: string; a: number; b: number };

function FileSection(p: { f: DiffFile; sel: Sel | null; select: (a: number, b: number) => void; clear: () => void; ask: (whole: boolean) => void }) {
  const lang = language(p.f.name);
  // shortcut: highlights every file up front, unlike MrReview's lazy FileBox; fine for a branch's worth of changes.
  const html = createMemo(() => (lang && p.f.lines.length <= 4000 ? highlightFile(p.f.lines, lang) : undefined));
  const sel = (): [number, number] | null => (p.sel ? [p.sel.a, p.sel.b] : null);
  const label = () => {
    const s = p.sel;
    return s ? linesLabel(p.f.lines.slice(s.a, s.b + 1).filter((l) => l.type !== "hunk")) : "";
  };
  return (
    <section class="overflow-hidden rounded-xl border border-line bg-panel">
      <header class="sticky top-0 z-[1] flex items-center gap-2 border-b border-line bg-raised/95 px-3 py-2 backdrop-blur">
        <span class="truncate font-mono text-sm text-fg">{p.f.name}</span>
        <span class="ml-auto font-mono text-xs tabular-nums">
          <span class="text-add">+{p.f.added}</span> <span class="text-del">−{p.f.removed}</span>
        </span>
        <button class="grid size-6 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-accent" onClick={() => p.ask(true)} title="Ask Claude about this file" aria-label="Ask Claude about this file">
          <Sparkles size={13} />
        </button>
      </header>
      <Show when={!p.f.binary} fallback={<div class="px-3 py-3 text-fg-3">Binary file</div>}>
        <ReviewDiff
          lines={p.f.lines}
          html={html()}
          split={split()}
          sel={sel()}
          onPick={(i, extend) => {
            const s = sel();
            p.select(extend && s ? s[0] : i, i);
          }}
          onSelect={(a, b) => p.select(a, b)}
          has={(i) => sel()?.[1] === i}
          after={() => (
            <div class="flex flex-wrap items-center gap-2 font-sans animate-pop-in">
              <span class="text-sm text-fg-3">{label()}</span>
              <Button size="sm" variant="primary" onClick={() => p.ask(false)} kbd="a" title="Type these lines in Claude's input, unsent">
                <Sparkles size={13} /> Ask Claude
              </Button>
              <Button size="sm" variant="ghost" onClick={p.clear} title="Clear the selection (Esc)">
                <X size={13} />
              </Button>
            </div>
          )}
        />
      </Show>
    </section>
  );
}

/** A diff to read and ask Claude about. It fills its parent: a workspace's diff tab, or the modal. */
export function DiffPane(p: { target: DiffTarget; onClose: () => void }) {
  const commit = () => p.target.commit ?? null;
  const [diff, { refetch }] = createResource(
    () => p.target,
    (d) => {
      const q = (d.commit ? `&commit=${d.commit.sha}` : "") + (d.file ? `&file=${encodeURIComponent(d.file)}` : "");
      return getJSON<{ diff: string }>(`/api/diff?path=${encodeURIComponent(d.path)}${q}`);
    },
  );
  const files = createMemo(() => parseUnified(diff.latest?.diff ?? ""));
  // A refresh keeps the old diff on screen, so the scroll position stays.
  const [refreshing, setRefreshing] = createSignal(false);
  const refresh = () => {
    setRefreshing(true);
    void Promise.resolve(refetch()).finally(() => setRefreshing(false));
  };

  const [sel, setSel] = createSignal<Sel | null>(null);
  createEffect(() => (p.target, setSel(null)));
  const askAbout = (f: DiffFile, whole: boolean) => {
    const s = sel();
    const lines = !whole && s?.file === f.name ? linesLabel(f.lines.slice(s.a, s.b + 1).filter((l) => l.type !== "hunk")) : null;
    setSel(null);
    ask(p.target, askPrompt(p.target, f, lines));
  };
  const onKey = (e: KeyboardEvent) => {
    const s = sel();
    if (!s || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "Escape") setSel(null);
    else if (e.key === "a") {
      const f = files().find((x) => x.name === s.file);
      if (f) askAbout(f, false);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const title = () => {
    const d = p.target;
    const c = d.commit;
    return d.file ? (c ? `${d.file} in ${c.short}` : d.file) : c ? `${c.short} ${c.title}` : d.title;
  };

  return (
    <div class="flex h-full min-h-0 flex-col bg-bg outline-none" tabIndex={-1} onKeyDown={onKey} ref={(el) => queueMicrotask(() => el.focus())}>
      <header class="flex flex-wrap items-center gap-2 border-b border-line px-5 py-3">
        <FileDiff size={16} class="text-fg-3" />
        <h2 class="min-w-0 flex-1 truncate font-mono text-base text-fg">{title()}</h2>
        <div class="flex rounded-lg bg-hover p-0.5 text-sm" role="group" title="Layout">
          <button class={cls("rounded-md px-2 py-1", !split() ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => split() && toggleSplit()}>
            Unified
          </button>
          <button class={cls("rounded-md px-2 py-1", split() ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => !split() && toggleSplit()}>
            Side by side
          </button>
        </div>
        <IconButton label="Refresh" onClick={refresh}>
          <Show when={diff.loading} fallback={<Refresh size={15} />}>
            <Spinner size={13} />
          </Show>
        </IconButton>
        <IconButton label="Close (Esc)" onClick={p.onClose}>
          <X size={16} />
        </IconButton>
      </header>
      <div class="min-h-0 flex-1 overflow-y-auto p-5">
        <Show when={refreshing() || !diff.loading} fallback={<div class="flex flex-col gap-2">{[0, 1, 2, 3].map(() => <div class="skeleton h-6" />)}</div>}>
          <Show when={!diff.error} fallback={<div class="text-danger">{String(diff.error)}</div>}>
            <Show when={files().length} fallback={<div class="px-4 py-10 text-center text-fg-3">{commit() ? "This commit changes no files" : "No changes against HEAD"}</div>}>
              <p class="mb-3 text-sm text-fg-3">Select lines, or use ✦ on a file, to ask Claude about them. The question is typed in Claude's input; you finish and send it.</p>
              <div class="flex flex-col gap-4">
                <For each={files()}>
                  {(f) => (
                    <FileSection
                      f={f}
                      sel={sel()?.file === f.name ? sel() : null}
                      select={(a, b) => setSel({ file: f.name, a: Math.min(a, b), b: Math.max(a, b) })}
                      clear={() => setSel(null)}
                      ask={(whole) => askAbout(f, whole)}
                    />
                  )}
                </For>
              </div>
            </Show>
          </Show>
        </Show>
      </div>
    </div>
  );
}

/** The diff of an agent that has no workspace, over everything. */
export function ChangesReview() {
  const close = () => setDiffView(null);
  return (
    <Show when={diffView()}>
      {(d) => (
        <div
          class="fixed inset-0 z-50 flex justify-end bg-black/30 backdrop-blur-[2px] animate-fade-in"
          onMouseDown={(e) => e.target === e.currentTarget && close()}
          onKeyDown={(e) => e.key === "Escape" && !e.defaultPrevented && close()}
        >
          <div class={cls("h-full w-full border-l border-line-strong shadow-float animate-slide-up", split() ? "max-w-7xl" : "max-w-4xl")}>
            <DiffPane target={d()} onClose={close} />
          </div>
        </div>
      )}
    </Show>
  );
}
