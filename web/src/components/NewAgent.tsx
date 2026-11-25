import { For, Show, createEffect, createMemo, createResource, createSignal, on } from "solid-js";
import { getJSON } from "../api";
import { act, composer, go, isBusy, setComposer, state } from "../store";
import { basename, cls, fuzzyWords } from "../util";
import { Folder, Git, Sparkles } from "./icons";
import { Button, Kbd } from "./ui";

type Dirs = { dirs: [string, boolean][]; recent: string[]; home: string };

const LAST = "new-agent-path";

/** Start Claude on a task: pick where, say what. The prompt goes in as the first message. */
export function NewAgent() {
  const [path, setPath] = createSignal("");
  const [prompt, setPrompt] = createSignal("");
  const [picking, setPicking] = createSignal(false);
  /** What is typed in the place field; empty until you type, so the list starts with the usual places. */
  const [typed, setTyped] = createSignal("");
  const [sel, setSel] = createSignal(0);
  const [dirs] = createResource(() => !!composer(), () => getJSON<Dirs>("/api/dirs"));
  let promptEl!: HTMLTextAreaElement;
  let pathEl!: HTMLInputElement;

  const short = (p: string) => {
    const home = dirs.latest?.home;
    return home && p.startsWith(home) ? "~" + p.slice(home.length) : p;
  };

  createEffect(
    on(composer, (c) => {
      if (!c) return;
      let start = c.path;
      try {
        start ??= localStorage.getItem(LAST);
      } catch {
        /* ignore */
      }
      start ??= state.workspaces[0]?.path ?? null;
      setPath(start ?? "");
      setPrompt(c.prompt ?? "");
      setTyped("");
      setPicking(false);
      // With a place filled in, the task is what is left to write.
      queueMicrotask(() => (start ? promptEl : pathEl)?.focus());
    }),
  );

  const options = createMemo(() => {
    const d = dirs.latest;
    if (!d) return [];
    const seen = new Set<string>();
    const all = [...state.workspaces.map((w) => [w.path, true] as [string, boolean]), ...d.recent.map((p) => [p, true] as [string, boolean]), ...d.dirs].filter(
      ([p]) => !seen.has(p) && (seen.add(p), true),
    );
    const q = typed().trim();
    if (!q) return all.slice(0, 8);
    return all
      .map(([p, repo]) => [fuzzyWords(q.replace(/^~\//, ""), p), p, repo] as const)
      .filter(([s]) => s !== null)
      .sort((a, b) => b[0]! - a[0]!)
      .slice(0, 8)
      .map(([, p, repo]) => [p, repo] as [string, boolean]);
  });

  const close = () => setComposer(null);
  const start = async () => {
    const p = path().trim();
    if (!p) return pathEl.focus();
    const r = await act({ type: "new_agent", path: p, prompt: prompt().trim() || undefined }, { key: "new-agent" });
    if (!r.ok) return;
    try {
      localStorage.setItem(LAST, p);
    } catch {
      /* ignore */
    }
    close();
    if (r.workspace && r.window) go({ view: "workspace", name: r.workspace, window: r.window });
  };

  const choose = (p: string) => {
    setPath(p);
    setTyped("");
    setPicking(false);
    promptEl.focus();
  };

  return (
    <Show when={composer()}>
      <div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 px-4 pt-[10vh] backdrop-blur-[2px] animate-fade-in" onMouseDown={(e) => e.target === e.currentTarget && close()}>
        <div
          class="w-full max-w-2xl overflow-hidden rounded-2xl border border-line-strong bg-raised shadow-float animate-pop-in"
          role="dialog"
          aria-label="New agent"
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Escape") close();
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              void start();
            }
          }}
        >
          <div class="flex items-center gap-2 px-5 pt-4">
            <div class="grid size-7 place-items-center rounded-lg bg-accent-soft text-accent">
              <Sparkles size={15} />
            </div>
            <h2 class="text-[15px] font-semibold text-fg">New agent</h2>
          </div>

          <div class="relative px-5 pt-4">
            <label class="mb-1.5 block text-[12px] font-medium text-fg-2">Where</label>
            <div class="flex h-9 items-center gap-2 rounded-lg border border-line-strong bg-bg px-3 focus-within:border-accent/60">
              <Folder size={14} class="text-fg-3" />
              <input
                ref={pathEl}
                value={short(path())}
                onFocus={(e) => {
                  setPicking(true);
                  e.currentTarget.select();
                }}
                onBlur={() => setTimeout(() => setPicking(false), 120)}
                onInput={(e) => {
                  setPath(e.currentTarget.value);
                  setTyped(e.currentTarget.value);
                  setSel(0);
                  setPicking(true);
                }}
                onKeyDown={(e) => {
                  if (!picking()) return;
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    setSel((s) => Math.min(s + 1, options().length - 1));
                  } else if (e.key === "ArrowUp") {
                    e.preventDefault();
                    setSel((s) => Math.max(s - 1, 0));
                  } else if ((e.key === "Enter" || e.key === "Tab") && !e.ctrlKey && options()[sel()]) {
                    e.preventDefault();
                    choose(options()[sel()][0]);
                  }
                }}
                placeholder="Search a repo, or type a path"
                class="min-w-0 flex-1 bg-transparent font-mono text-[12.5px] text-fg outline-none placeholder:font-sans placeholder:text-fg-3"
                spellcheck={false}
                autocomplete="off"
              />
            </div>
            <Show when={picking() && options().length}>
              <div class="absolute inset-x-5 top-full z-10 mt-1 overflow-hidden rounded-xl border border-line-strong bg-raised p-1 shadow-float animate-pop-in">
                <For each={options()}>
                  {([p, repo], i) => (
                    <button
                      class={cls("flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left", sel() === i() ? "bg-hover" : "")}
                      onMouseMove={() => setSel(i())}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        choose(p);
                      }}
                    >
                      <span class="text-fg-3">{repo ? <Git size={14} /> : <Folder size={14} />}</span>
                      <span class="text-[13px] font-medium text-fg">{basename(p)}</span>
                      <span class="ml-auto truncate pl-3 font-mono text-[11.5px] text-fg-3">{short(p)}</span>
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </div>

          <div class="px-5 pt-4">
            <label class="mb-1.5 block text-[12px] font-medium text-fg-2">Task</label>
            <textarea
              ref={promptEl}
              value={prompt()}
              onFocus={() => setPicking(false)}
              onInput={(e) => setPrompt(e.currentTarget.value)}
              rows={6}
              placeholder="Describe what Claude should do. Leave it empty to start Claude without a prompt."
              class="w-full resize-y rounded-xl border border-line-strong bg-bg px-3.5 py-3 text-[14px] leading-relaxed text-fg outline-none placeholder:text-fg-3 focus:border-accent/60"
            />
          </div>

          <div class="flex items-center gap-3 px-5 pb-4 pt-3">
            <span class="text-[12px] text-fg-3">
              The workspace opens if it is not open yet. <Kbd>{"Ctrl ↵"}</Kbd> starts.
            </span>
            <div class="ml-auto flex gap-2">
              <Button variant="ghost" onClick={close}>
                Cancel
              </Button>
              <Button variant="primary" onClick={start} loading={isBusy("new-agent")}>
                <Sparkles size={14} /> {prompt().trim() ? "Start agent" : "Open Claude"}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
}
