import { For, Show, createEffect, createMemo, createResource, createSignal, on, type JSX } from "solid-js";
import { getJSON } from "../api";
import {
  act,
  agentRoute,
  cycleTheme,
  enableNotifications,
  go,
  paletteMode,
  paletteOpen,
  route,
  openDir,
  reopen,
  setComposer,
  setHelpOpen,
  setPaletteOpen,
  state,
} from "../store";
import { allMrs } from "../gitlab";
import { STATUS, alive, basename, cls, fuzzyWords } from "../util";
import { Bell, Bot, Undo, Code, Folder, Git, Inbox, Keyboard, Layers, Refresh, Search, Sparkles, Sun, Terminal } from "./icons";
import { Kbd, StatusDot } from "./ui";

interface Item {
  id: string;
  group: string;
  title: string;
  hint?: string;
  icon: JSX.Element;
  keywords?: string;
  run: (mod: boolean) => void;
}

type Dirs = { dirs: [string, boolean][]; recent: string[]; home: string };

export const [dirsVersion, setDirsVersion] = createSignal(0);

function short(p: string, home: string) {
  return p.startsWith(home) ? "~" + p.slice(home.length) : p;
}

export function Palette() {
  const [query, setQuery] = createSignal("");
  const [sel, setSel] = createSignal(0);
  let input!: HTMLInputElement;
  let list!: HTMLDivElement;
  const [dirs] = createResource(
    () => (paletteOpen() ? dirsVersion() + 1 : false),
    () => getJSON<Dirs>("/api/dirs"),
  );

  createEffect(
    on(paletteOpen, (o) => {
      if (o) {
        setQuery("");
        setSel(0);
        queueMicrotask(() => input?.focus());
      }
    }),
  );
  createEffect(on(query, () => setSel(0)));

  const close = () => setPaletteOpen(false);
  const here = () => {
    const r = route();
    return r.view === "workspace" ? state.workspaces.find((w) => w.name === r.name) : undefined;
  };

  const dirItems = createMemo<Item[]>(() => {
    const d = dirs.latest;
    if (!d) return [];
    const seen = new Set<string>();
    const ordered = [...d.recent.map((p) => [p, true] as [string, boolean]), ...d.dirs].filter(([p]) => !seen.has(p) && (seen.add(p), true));
    return ordered.map(([p, repo]) => ({
      id: `dir:${p}`,
      group: "Open a workspace",
      title: basename(p),
      hint: short(p, d.home),
      keywords: p,
      icon: repo ? <Git size={15} /> : <Folder size={15} />,
      run: (mod) => {
        close();
        if (mod) return setComposer({ path: p });
        void openDir(p);
      },
    }));
  });

  const items = createMemo<Item[]>(() => {
    if (paletteMode() === "open") return dirItems();
    const ws = here();
    const commands: Item[] = [
      { id: "new-agent", group: "Actions", title: "New agent…", hint: "Start Claude with a task", icon: <Sparkles size={15} />, keywords: "claude start task prompt", run: () => (close(), setComposer({ path: ws?.path ?? null })) },
      ...(ws
        ? [
            { id: "new-term", group: "Actions", title: `New terminal in ${ws.name}`, icon: <Terminal size={15} />, keywords: "shell", run: () => (close(), act({ type: "new_window", workspace: ws.name, kind: "shell" }, { quiet: true }).then((r) => r.ok && r.window && go({ view: "workspace", name: ws.name, window: r.window }))) },
            { id: "new-edit", group: "Actions", title: `New editor in ${ws.name}`, icon: <Code size={15} />, keywords: "nvim vim", run: () => (close(), act({ type: "new_window", workspace: ws.name, kind: "nvim" }, { quiet: true }).then((r) => r.ok && r.window && go({ view: "workspace", name: ws.name, window: r.window }))) },
          ]
        : []),
      { id: "go-inbox", group: "Go to", title: "Inbox", icon: <Inbox size={15} />, run: () => (close(), go({ view: "inbox" })) },
      { id: "go-agents", group: "Go to", title: "Agents", icon: <Bot size={15} />, run: () => (close(), go({ view: "agents" })) },
      ...(state.gitlab.enabled ? [{ id: "go-gitlab", group: "Go to", title: "GitLab", icon: <Git size={15} />, run: () => (close(), go({ view: "gitlab", key: null })) }] : []),
      ...state.workspaces.map((w) => ({
        id: `ws:${w.name}`,
        group: "Workspaces",
        title: w.name,
        hint: w.git?.branch ?? w.short_path,
        keywords: w.path,
        icon: <Layers size={15} />,
        run: () => (close(), go({ view: "workspace", name: w.name, window: null })),
      })),
      ...state.agents
        .filter((a) => alive(a.status))
        .sort((a, b) => STATUS[a.status].rank - STATUS[b.status].rank)
        .map((a) => ({
          id: `agent:${a.key}`,
          group: "Agents",
          title: a.title,
          hint: `${a.name} · ${STATUS[a.status].label}`,
          keywords: `${a.name} ${a.branch ?? ""}`,
          icon: <StatusDot status={a.status} />,
          run: () => (close(), go(agentRoute(a))),
        })),
      ...allMrs().map((m) => ({
        id: `mr:${m.key}`,
        group: "GitLab",
        title: `!${m.iid} ${m.title}`,
        hint: m.project.split("/").pop(),
        keywords: `${m.project} ${m.branch} ${m.author}`,
        icon: <Git size={15} />,
        run: () => (close(), go({ view: "gitlab", key: m.key })),
      })),
      ...state.gitlab.issues.map((i) => ({
        id: `issue:${i.key}`,
        group: "GitLab",
        title: `#${i.iid} ${i.title}`,
        hint: i.project.split("/").pop(),
        keywords: `${i.project} ${i.labels.join(" ")}`,
        icon: <Git size={15} />,
        run: () => (close(), go({ view: "gitlab", key: i.key })),
      })),
      ...state.closed.map((c) => ({
        id: `closed:${c.id}`,
        group: "Recently closed",
        title: `Reopen ${c.name}`,
        hint: c.short_path,
        keywords: c.path,
        icon: <Undo size={15} />,
        run: () => (close(), void reopen(c.id)),
      })),
      { id: "theme", group: "Settings", title: "Change theme", hint: "System, dark, light", icon: <Sun size={15} />, run: () => cycleTheme() },
      { id: "notify", group: "Settings", title: "Turn on notifications", icon: <Bell size={15} />, run: () => (close(), enableNotifications()) },
      { id: "rescan", group: "Settings", title: "Scan for new repos", hint: "After you clone one", icon: <Refresh size={15} />, run: () => (close(), act({ type: "rescan" }).then(() => setTimeout(() => setDirsVersion((v) => v + 1), 1500))) },
      { id: "help", group: "Settings", title: "Keyboard shortcuts", icon: <Keyboard size={15} />, run: () => (close(), setHelpOpen(true)) },
    ];
    // Directories only join the list when you search: then you can open any repo from here.
    return query().trim() ? [...commands, ...dirItems()] : commands;
  });

  const results = createMemo(() => {
    const q = query().trim();
    let out: Item[];
    if (!q) out = items().slice(0, 60);
    else {
      const scored: [number, Item][] = [];
      for (const it of items()) {
        const s = fuzzyWords(q, `${it.title} ${it.keywords ?? ""}`);
        if (s !== null) scored.push([s + (it.group === "Open a workspace" ? -2 : 0), it]);
      }
      // Groups stay together, the group with the best match first.
      const best = new Map<string, number>();
      for (const [sc, it] of scored) best.set(it.group, Math.max(best.get(it.group) ?? -Infinity, sc));
      scored.sort((a, b) => best.get(b[1].group)! - best.get(a[1].group)! || a[1].group.localeCompare(b[1].group) || b[0] - a[0]);
      out = scored.slice(0, 60).map(([, it]) => it);
    }
    // A typed path opens as it is, even when the scan does not know it.
    if (/^[~/.]/.test(q)) {
      out.unshift({
        id: "path",
        group: "Open a workspace",
        title: `Open ${q}`,
        hint: "Ctrl+Enter starts an agent there",
        icon: <Folder size={15} />,
        run: (mod) => {
          close();
          if (mod) return setComposer({ path: q });
          void openDir(q);
        },
      });
    }
    return out;
  });

  createEffect(() => {
    const el = list?.querySelector(`[data-index="${sel()}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: "nearest" });
  });

  const onKey = (e: KeyboardEvent) => {
    const n = results().length;
    if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) {
      e.preventDefault();
      setSel((s) => (s + 1) % Math.max(n, 1));
    } else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) {
      e.preventDefault();
      setSel((s) => (s - 1 + n) % Math.max(n, 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      results()[sel()]?.run(e.ctrlKey || e.metaKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
    e.stopPropagation();
  };

  return (
    <Show when={paletteOpen()}>
      <div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 px-4 pt-[12vh] backdrop-blur-[2px] animate-fade-in" onMouseDown={(e) => e.target === e.currentTarget && close()}>
        <div class="flex max-h-[70vh] w-full max-w-xl flex-col overflow-hidden rounded-2xl border border-line-strong bg-raised shadow-float animate-pop-in" role="dialog" aria-label="Command palette">
          <div class="flex items-center gap-3 border-b border-line px-4">
            <Search size={16} class="text-fg-3" />
            <input
              ref={input}
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
              onKeyDown={onKey}
              placeholder={paletteMode() === "open" ? "Search repos and folders, or type a path…" : "Search workspaces, agents and actions…"}
              class="h-12 min-w-0 flex-1 bg-transparent text-[15px] text-fg outline-none placeholder:text-fg-3"
              spellcheck={false}
              autocomplete="off"
            />
            <Kbd>esc</Kbd>
          </div>
          <div ref={list} class="min-h-0 flex-1 overflow-y-auto p-1.5">
            <Show when={results().length} fallback={<div class="px-3 py-10 text-center text-fg-3">{dirs.loading && paletteMode() === "open" ? "Loading folders…" : "No matches"}</div>}>
              <For each={results()}>
                {(it, i) => (
                  <>
                    <Show when={i() === 0 || results()[i() - 1].group !== it.group}>
                      <div class="px-3 pb-1 pt-2.5 text-[11px] font-semibold uppercase tracking-[0.06em] text-fg-3">{it.group}</div>
                    </Show>
                    <button
                      data-index={i()}
                      class={cls("flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left", sel() === i() ? "bg-hover text-fg" : "text-fg-2")}
                      onMouseMove={() => setSel(i())}
                      onClick={(e) => it.run(e.ctrlKey || e.metaKey)}
                    >
                      <span class="grid w-4 shrink-0 place-items-center text-fg-3">{it.icon}</span>
                      <span class="truncate text-[13.5px] text-fg">{it.title}</span>
                      <Show when={it.hint}>
                        <span class="ml-auto shrink-0 truncate pl-3 text-[12px] text-fg-3">{it.hint}</span>
                      </Show>
                    </button>
                  </>
                )}
              </For>
            </Show>
          </div>
          <div class="flex items-center gap-4 border-t border-line px-4 py-2 text-[11.5px] text-fg-3">
            <span><Kbd>↑</Kbd> <Kbd>↓</Kbd> move</span>
            <span><Kbd>↵</Kbd> open</span>
            <span><Kbd>Ctrl ↵</Kbd> agent in folder</span>
          </div>
        </div>
      </div>
    </Show>
  );
}
