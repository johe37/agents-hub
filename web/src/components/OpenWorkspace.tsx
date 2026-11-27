import { For, onCleanup, onMount } from "solid-js";
import { act, go, isBusy, setComposer, state } from "../store";
import { basename } from "../util";
import { Bot, Folder, Sparkles, Terminal } from "./icons";
import { Spinner } from "./ui";

type Kind = "claude" | "shell";

const CHOICES: { kind: Kind; title: string; body: string; key: string; icon: () => any }[] = [
  { kind: "claude", title: "Claude", body: "Claude Code in this directory", key: "c", icon: () => <Bot size={20} /> },
  { kind: "shell", title: "Terminal", body: "A shell: run, test or start your editor", key: "t", icon: () => <Terminal size={20} /> },
];

/** A directory without a workspace: the workspace opens with what you pick here. */
export function OpenWorkspace(p: { path: string }) {
  const short = () => (state.home && p.path.startsWith(state.home) ? "~" + p.path.slice(state.home.length) : p.path);
  const busy = (k: Kind) => isBusy(`open:${p.path}:${k}`);
  const start = async (kind: Kind) => {
    const r = await act({ type: "open_workspace", path: p.path, kind }, { key: `open:${p.path}:${kind}` });
    if (r.ok && r.workspace) go({ view: "workspace", name: r.workspace, window: r.window ?? null }, true);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    if ((e.target as HTMLElement).closest("input,textarea,[contenteditable]")) return;
    const c = CHOICES.find((x) => x.key.toLowerCase() === e.key);
    if (!c) return;
    e.preventDefault();
    void start(c.kind);
  };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));

  return (
    <div class="mx-auto flex h-full w-full max-w-2xl flex-col justify-center px-4 pb-24 pt-10 sm:px-8">
      <div class="flex items-center gap-3">
        <span class="grid size-10 place-items-center rounded-xl border border-line bg-raised text-fg-2">
          <Folder size={18} />
        </span>
        <div class="min-w-0">
          <h1 class="truncate text-xl font-semibold tracking-[-0.02em] text-fg">{basename(p.path)}</h1>
          <p class="truncate text-sm text-fg-3">{short()}</p>
        </div>
      </div>
      <p class="mt-6 text-fg-2">What do you want to start in this workspace?</p>
      <div class="mt-3 grid gap-3 sm:grid-cols-2">
        <For each={CHOICES}>
          {(c) => (
            <button
              class="group relative flex flex-col items-start gap-2 rounded-2xl border border-line bg-panel p-4 text-left transition-[border-color,box-shadow] hover:border-accent/50 hover:shadow-soft disabled:opacity-60"
              disabled={CHOICES.some((x) => busy(x.kind))}
              onClick={() => start(c.kind)}
            >
              <span class="text-fg-2 group-hover:text-accent">{busy(c.kind) ? <Spinner size={20} /> : c.icon()}</span>
              <span class="font-medium text-fg">{c.title}</span>
              <span class="text-sm text-fg-3">{c.body}</span>
              <span class="kbd absolute right-3 top-3">{c.key}</span>
            </button>
          )}
        </For>
      </div>
      <button class="mt-4 inline-flex items-center gap-1.5 self-start text-base text-fg-2 hover:text-fg" onClick={() => setComposer({ path: p.path })}>
        <Sparkles size={14} /> Or start Claude with a task <span class="kbd">n</span>
      </button>
    </div>
  );
}
