import { For, Match, Show, Switch, createEffect, createMemo, createSignal, lazy, on, onCleanup, onMount, untrack } from "solid-js";
import { type Agent, type Win, type Workspace } from "../api";
import { act, ask, readStored, writeStored, closeWindow as closeWin, closeWorkspace as closeWs, draft, forgetLastWindow, go, hasClaude, isBusy, lastWindow, moveWindow, NO_CLAUDE, setDiffView, state, store, toast } from "../store";
import { pipelineTone } from "../gitlab";
import { cls, worst } from "../util";
import { Commits } from "./Commits";
import { Conversation } from "./Conversation";
import { ArrowDown, ArrowUp, Bot, Check, Columns, Done, More, Branch, Chat, External, FileDiff, Layers, Maximize, Minimize, PanelRight, Pipeline, Plus, Rename, Terminal as TermIcon, Trash, X } from "./icons";
import { Badge, Button, Edge, Empty, IconButton, MenuButton, Spinner, StatusDot, StatusPill, kindIcon, SectionTitle, Ago, Resizer } from "./ui";
import { focus, panelWidth, resizable, showNav, showPanel, toggleFocus, togglePanel } from "../layout";
import { agentView, lastAgentView, setLastAgentView, tabsMode } from "../settings";

const Terminal = lazy(() => import("./Terminal").then((m) => ({ default: m.Terminal })));

const FILE_TONE: Record<string, string> = { M: "text-waiting", A: "text-add", D: "text-del", R: "text-working", "?": "text-add", U: "text-danger" };

function fileCode(code: string) {
  const c = code === "??" ? "?" : (code.replace(".", "").trim()[0] ?? "M");
  return { letter: c === "?" ? "U" : c, tone: FILE_TONE[c] ?? "text-fg-2", title: code === "??" ? "Untracked" : code };
}

/** Chat or terminal for each agent window you switched; the others follow agentView. */
const [modes, setModes] = createSignal<Record<string, "chat" | "terminal">>({});

/** The window shown beside the current one, per workspace. */
const [splits, setSplits] = createSignal<Record<string, string>>(JSON.parse(readStored("splits") ?? "{}"));
function setSplit(workspace: string, win: string | null) {
  const s = { ...splits() };
  if (win) s[workspace] = win;
  else delete s[workspace];
  setSplits(s);
  writeStored("splits", JSON.stringify(s));
}
const splitWidth = resizable("split-width", 560, 320, 1600);

function SidePanel(p: { ws: Workspace; agents: Agent[] }) {
  const g = () => p.ws.git;
  // Commits to push, or a branch of its own that has no merge request yet.
  const canPush = () => {
    const b = g()?.branch;
    if (!b || !state.gitlab.enabled) return false;
    return g()!.ahead > 0 || (!p.ws.mr && b !== "main" && b !== "master");
  };
  const [message, setMessage] = createSignal("");
  // Index column of git's XY code: something is staged.
  const staged = () => !!g()?.files.some(([code]) => code[0] !== "." && code[0] !== "?");
  return (
    <aside class="relative flex h-full shrink-0 flex-col border-l border-line bg-panel" style={{ width: `${panelWidth.width()}px` }}>
      <Resizer size={panelWidth} side="left" label="Side panel width" />
      <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <Show when={p.agents.length}>
          <section class="border-b border-line p-3">
            <SectionTitle class="mb-2 px-1">Agents</SectionTitle>
            <For each={p.agents}>
              {(a) => (
                <button
                  class="flex w-full items-start gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-hover"
                  onClick={() => a.window && go({ view: "workspace", name: p.ws.name, window: a.window })}
                >
                  <StatusDot status={a.status} class="mt-[6px]" />
                  <div class="min-w-0 flex-1">
                    <div class="truncate text-base font-medium text-fg">{a.title}</div>
                    <div class="truncate text-sm text-fg-3">{a.status === "working" ? (a.tool ?? "Thinking…") : a.status === "waiting" ? (a.message ?? "Waiting for you") : <Ago ts={a.timestamp} suffix />}</div>
                  </div>
                </button>
              )}
            </For>
          </section>
        </Show>

        <Show when={p.ws.mr}>
          {(mr) => (
            <section class="border-b border-line p-3">
              <div class="mb-2 flex items-center px-1">
                <SectionTitle>Merge request</SectionTitle>
                <a href={mr().url} target="_blank" rel="noopener" class="ml-auto text-fg-3 hover:text-fg" title="Open in GitLab">
                  <External size={12} />
                </a>
              </div>
              <button class="block w-full rounded-lg px-2 py-1.5 text-left hover:bg-hover" onClick={() => go({ view: "gitlab", key: mr().key })} title="Threads, pipeline and description">
                <div class="flex items-center gap-1.5 text-base font-medium text-fg">
                  <span class="text-fg-3">!{mr().iid}</span>
                  <span class="truncate">{mr().title}</span>
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-1.5">
                  <Show when={mr().draft}>
                    <Badge>Draft</Badge>
                  </Show>
                  <Show when={mr().pipeline}>
                    <Badge tone={pipelineTone(mr().pipeline)}>
                      <Pipeline size={11} /> {mr().pipeline}
                    </Badge>
                  </Show>
                  <Show when={mr().threads.length}>
                    <Badge tone="waiting">{mr().threads.length} open threads</Badge>
                  </Show>
                </div>
              </button>
            </section>
          )}
        </Show>

        <section class="flex shrink-0 flex-col p-3">
          <div class="mb-2 flex items-center gap-2 px-1">
            <SectionTitle>Changes</SectionTitle>
            <Show when={g()}>
              <span class="font-mono text-xs tabular-nums">
                <span class="text-add">+{g()!.added}</span> <span class="text-del">−{g()!.removed}</span>
              </span>
            </Show>
            <Show when={g()?.files.length}>
              <button class="ml-auto text-sm text-fg-2 hover:text-fg" onClick={() => setDiffView({ workspace: p.ws.name, path: p.ws.path, file: null, title: `All changes in ${p.ws.name}` })}>
                Review all
              </button>
            </Show>
          </div>
          <Show when={canPush()}>
            <Button
              size="sm"
              class="mb-2 w-full"
              loading={isBusy(`publish:${p.ws.name}`)}
              onClick={() =>
                ask({
                  title: `Push ${g()!.branch}?`,
                  body: p.ws.mr ? `It updates !${p.ws.mr.iid}.` : "It also opens a draft merge request.",
                  confirm: "Push",
                  run: () => act({ type: "publish", workspace: p.ws.name }, { key: `publish:${p.ws.name}` }),
                })
              }
            >
              <ArrowUp size={13} /> {p.ws.mr ? `Push ${g()!.ahead} commit${g()!.ahead === 1 ? "" : "s"}` : "Push and open draft MR"}
            </Button>
          </Show>
          <Show when={g()?.files.length}>
            <form
              class="mb-2 flex gap-1.5"
              onSubmit={(e) => {
                e.preventDefault();
                if (!message().trim()) return;
                void act({ type: "commit", workspace: p.ws.name, message: message() }, { key: `commit:${p.ws.name}` }).then((r) => r.ok && setMessage(""));
              }}
            >
              <input
                value={message()}
                onInput={(e) => setMessage(e.currentTarget.value)}
                placeholder="Commit message"
                aria-label="Commit message"
                class="h-7 min-w-0 flex-1 rounded-md border border-line-strong bg-raised px-2 text-sm outline-none placeholder:text-fg-3 focus:border-accent/60"
              />
              <Button size="sm" type="submit" disabled={!message().trim()} loading={isBusy(`commit:${p.ws.name}`)} title={staged() ? "Commit the staged changes" : "Commit all changes"}>
                <Check size={13} /> {staged() ? "Commit staged" : "Commit all"}
              </Button>
            </form>
          </Show>
          <Show when={g()} fallback={<div class="px-1 text-sm text-fg-3">Not a git repository</div>}>
            <Show when={g()!.files.length} fallback={<div class="px-1 text-sm text-fg-3">Working tree clean</div>}>
              <div class="flex flex-col">
                <For each={g()!.files}>
                  {([code, file]) => {
                    const c = fileCode(code);
                    // Git lists an untracked directory once, with a trailing slash.
                    const dir = file.endsWith("/");
                    const i = dir ? -1 : file.lastIndexOf("/");
                    return (
                      <button
                        class="group flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-left hover:bg-hover"
                        title={`${c.title}: ${file}`}
                        onClick={() => setDiffView({ workspace: p.ws.name, path: p.ws.path, file, title: file })}
                      >
                        <span class={cls("w-3 shrink-0 text-center font-mono text-xs font-semibold", c.tone)}>{c.letter}</span>
                        <span class="min-w-0 truncate font-mono text-sm">
                          <span class="text-fg">{file.slice(i + 1)}</span>
                          <Show when={i > 0}>
                            <span class="ml-1.5 text-fg-3">{file.slice(0, i)}</span>
                          </Show>
                        </span>
                        <FileDiff size={13} class="ml-auto shrink-0 text-fg-3 opacity-0 group-hover:opacity-100" />
                      </button>
                    );
                  }}
                </For>
              </div>
            </Show>
          </Show>
        </section>

        <Show when={g()}>
          <Commits ws={p.ws} />
        </Show>
      </div>
    </aside>
  );
}

export function WorkspaceView(p: { name: string; window: string | null }) {
  const ws = createMemo(() => state.workspaces.find((w) => w.name === p.name));
  const agents = createMemo(() => store.byWorkspace().get(p.name) ?? []);
  const current = createMemo(() => ws()?.windows.find((w) => w.id === p.window) ?? null);
  const agentOf = (w: Win | null) => (w?.agent ? store.agentsByKey().get(w.agent) : undefined);

  let tabs: HTMLDivElement | undefined;
  // A plain mouse wheel only scrolls vertically; the tab row only scrolls sideways.
  const wheelTabs = (e: WheelEvent) => {
    if (!tabs || e.deltaX || tabs.scrollWidth <= tabs.clientWidth) return;
    e.preventDefault();
    tabs.scrollLeft += e.deltaY;
  };
  createEffect(() => {
    p.window;
    ws()?.windows.length;
    requestAnimationFrame(() =>
      tabs?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" }),
    );
  });

  // Drag a tab to move its window. A mouse drags once it moves a little.
  // A finger must hold the tab still first, because a finger that moves scrolls the tab row.
  const [drag, setDrag] = createSignal<{ id: string; before: string | null } | null>(null);
  let press: { id: string; x: number; pointer: number; touch: boolean; timer?: number } | null = null;
  let dragged = false;

  const dropBefore = (x: number): string | null => {
    for (const el of tabs?.querySelectorAll<HTMLElement>("[data-win]") ?? []) {
      const r = el.getBoundingClientRect();
      if (x < r.left + r.width / 2) return el.dataset.win!;
    }
    return null;
  };
  /** Dropping here leaves the window where it is. */
  const inPlace = (d: { id: string; before: string | null }) => {
    const wins = ws()?.windows ?? [];
    const i = wins.findIndex((x) => x.id === d.id);
    return d.before === d.id || d.before === (wins[i + 1]?.id ?? null);
  };

  const endPress = () => {
    if (press?.timer) clearTimeout(press.timer);
    press = null;
    setDrag(null);
    window.removeEventListener("pointermove", onDragMove);
    window.removeEventListener("pointerup", onDragUp);
    window.removeEventListener("pointercancel", endPress);
    window.removeEventListener("touchmove", stopScroll);
    window.removeEventListener("keydown", onDragKey, true);
    // The click that follows the drop must not select the tab.
    setTimeout(() => (dragged = false));
  };
  const begin = () => {
    if (!press) return;
    dragged = true;
    setDrag({ id: press.id, before: press.id });
  };
  const onDragMove = (e: PointerEvent) => {
    if (!press || e.pointerId !== press.pointer) return;
    if (!drag()) {
      const moved = Math.abs(e.clientX - press.x);
      if (press.touch && moved > 8) endPress();
      else if (!press.touch && moved > 5) begin();
      if (!drag()) return;
    }
    setDrag({ id: press.id, before: dropBefore(e.clientX) });
    const r = tabs!.getBoundingClientRect();
    if (e.clientX < r.left + 24) tabs!.scrollLeft -= 8;
    else if (e.clientX > r.right - 24) tabs!.scrollLeft += 8;
  };
  const onDragUp = (e: PointerEvent) => {
    if (!press || e.pointerId !== press.pointer) return;
    const d = drag();
    endPress();
    if (d && !inPlace(d)) moveWindow(p.name, d.id, d.before);
  };
  const stopScroll = (e: TouchEvent) => drag() && e.preventDefault();
  const onDragKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || !drag()) return;
    e.preventDefault();
    e.stopPropagation();
    endPress();
  };
  const onTabDown = (e: PointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as Element).closest("button")) return;
    endPress();
    const touch = e.pointerType === "touch";
    press = { id, x: e.clientX, pointer: e.pointerId, touch };
    if (touch) press.timer = window.setTimeout(begin, 400);
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", onDragUp);
    window.addEventListener("pointercancel", endPress);
    window.addEventListener("touchmove", stopScroll, { passive: false });
    window.addEventListener("keydown", onDragKey, true);
  };
  onCleanup(endPress);

  // Without a window in the URL, show the last one shown here, else the one that needs you, else the first agent, else the first window.
  // A window just opened is in the URL before the hub lists it: give it a moment to show up.
  createEffect(() => {
    const w = ws();
    if (!w || current()) return;
    const pickOne = () => {
      const last = lastWindow(w.name);
      const pick =
        w.windows.find((x) => x.id === last) ??
        w.windows.find((x) => agentOf(x)?.status === "waiting") ??
        w.windows.find((x) => x.kind === "agent") ??
        w.windows[0];
      if (pick) go({ view: "workspace", name: w.name, window: pick.id }, true);
    };
    if (!p.window) return pickOne();
    const t = setTimeout(() => untrack(current) || pickOne(), 4000);
    onCleanup(() => clearTimeout(t));
  });

  const modeOf = (w: Win | null) => {
    if (!w || w.kind !== "agent") return "terminal";
    const v = agentView();
    return modes()[w.id] ?? (v === "last" ? lastAgentView() : v);
  };
  const setModeOf = (w: Win | null, m: "chat" | "terminal", picked = true) => {
    if (!w) return;
    setModes((s) => ({ ...s, [w.id]: m }));
    if (picked) setLastAgentView(m);
  };
  const mode = () => modeOf(current());
  const setMode = (m: "chat" | "terminal", picked = true) => setModeOf(current(), m, picked);

  const second = createMemo(() => {
    const id = splits()[p.name];
    return id && id !== p.window ? (ws()?.windows.find((w) => w.id === id) ?? null) : null;
  });
  // Going to the window beside swaps the two, so both stay in view.
  createEffect(
    on([() => p.name, () => p.window], ([name, win], prev) => {
      if (prev && prev[0] === name && prev[1] && win && win === untrack(() => splits()[name])) setSplit(name, prev[1]);
    }),
  );
  const beside = (id: string) => setSplit(p.name, id);
  const toggleSplit = () => {
    if (second()) return setSplit(p.name, null);
    const wins = ws()?.windows ?? [];
    const i = wins.findIndex((w) => w.id === p.window);
    if (wins.length > 1) beside(wins[(i + 1) % wins.length].id);
    else openBeside("shell");
  };

  // A draft for an agent waits in the chat's reply box, so show the chat.
  createEffect(() => {
    const d = draft();
    if (!d) return;
    for (const w of [current(), second()]) if (agentOf(w)?.key === d.agent) setModeOf(w, "chat", false);
  });

  const open = (kind: "claude" | "shell") =>
    act({ type: "new_window", workspace: p.name, kind }, { key: `new:${kind}`, quiet: true }).then((r) => {
      if (r.ok && r.window) {
        go({ view: "workspace", name: p.name, window: r.window });
      }
    });

  const openBeside = (kind: "claude" | "shell") =>
    act({ type: "new_window", workspace: p.name, kind }, { key: `new:${kind}`, quiet: true }).then((r) => {
      if (r.ok && r.window) beside(r.window);
    });

  const closeWindow = (win: Win) => {
    const w = ws();
    if (w) closeWin(w, win);
  };

  // With the tree only, the sidebar lists the windows. Without the sidebar (hidden, or a phone) the tabs come back.
  const tabsHidden = () => tabsMode() === "tree" && showNav();
  const newWindowMenu = (cls_: string) => (
    <MenuButton
      label="New window (c, t)"
      icon={<Show when={isBusy("new:claude") || isBusy("new:shell")} fallback={<Plus size={15} />}><Spinner size={13} /></Show>}
      size="sm"
      class={cls_}
      items={[
        ...(hasClaude() ? [{ label: "Agent", icon: <Bot size={14} />, hint: "c", run: () => open("claude") }] : []),
        { label: "Terminal", icon: <TermIcon size={14} />, hint: "t", run: () => open("shell") },
        { label: "Terminal beside", icon: <Columns size={14} />, run: () => openBeside("shell") },
        ...(hasClaude() ? [{ label: "Agent beside", icon: <Columns size={14} />, run: () => openBeside("claude") }] : []),
      ]}
    />
  );

  /** One window's content. The window beside has a bar of its own, to swap it in or close it. */
  const Pane = (q: { win: Win; primary: boolean }) => {
    const a = () => (q.win.kind === "agent" ? agentOf(q.win) : undefined);
    const m = () => modeOf(q.win);
    return (
      <>
        <Show when={a() || !q.primary}>
          <div class={cls("flex min-h-[41px] items-center gap-2 border-b border-line py-1.5", q.primary ? "px-4 sm:px-5" : "bg-panel/60 px-3")}>
            <Show when={!q.primary}>
              <span class="text-fg-3">{kindIcon(q.win)}</span>
              <span class="max-w-[12rem] shrink-0 truncate text-sm font-medium text-fg">{a()?.title ?? q.win.running ?? q.win.name}</span>
            </Show>
            <Show when={a()}>
              {(ag) => (
                <>
                  <StatusPill status={ag().status} />
                  <span class="min-w-0 truncate text-sm text-fg-2">
                    <Switch fallback={ag().model ?? ""}>
                      <Match when={ag().status === "working"}>{ag().tool ?? "Thinking…"}</Match>
                      <Match when={ag().status === "waiting"}>{ag().message}</Match>
                    </Switch>
                  </span>
                </>
              )}
            </Show>
            <div class="ml-auto flex shrink-0 items-center gap-1">
              <Show when={a() && a()!.status === "waiting" && !a()!.needs_permission}>
                <Button size="sm" variant="ghost" class="shrink-0" onClick={() => act({ type: "mark_done", agent: a()!.key })} title="Stop showing it as waiting (m)">
                  <Done size={13} /> Done
                </Button>
              </Show>
              <Show when={a()}>
                <div class="flex rounded-lg bg-hover p-0.5 text-sm">
                  <button class={cls("flex items-center gap-1 rounded-md px-2 py-1", m() === "chat" ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => setModeOf(q.win, "chat")} title={q.primary ? "Chat (v)" : "Chat"}>
                    <Chat size={12} /> Chat
                  </button>
                  <button class={cls("flex items-center gap-1 rounded-md px-2 py-1", m() === "terminal" ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => setModeOf(q.win, "terminal")} title={q.primary ? "Terminal (v)" : "Terminal"}>
                    <TermIcon size={12} /> Terminal
                  </button>
                </div>
              </Show>
              <Show when={!q.primary}>
                <IconButton label="Swap with the main window" onClick={() => go({ view: "workspace", name: p.name, window: q.win.id })}>
                  <Columns size={14} />
                </IconButton>
                <IconButton label="Close the split (s)" onClick={() => setSplit(p.name, null)}>
                  <X size={14} />
                </IconButton>
              </Show>
            </div>
          </div>
        </Show>
        <div class="min-h-0 flex-1">
          <Show when={m() === "chat" && a()} fallback={<Terminal window={q.win.id} focus={q.primary} />}>
            {(ag) => <Conversation agent={ag()} />}
          </Show>
        </div>
      </>
    );
  };

  const closeWorkspace = () => {
    const w = ws();
    if (w) closeWs(w);
  };

  const [renaming, setRenaming] = createSignal(false);
  const rename = (name: string) => {
    setRenaming(false);
    if (!name.trim() || name === p.name) return;
    act({ type: "rename_workspace", workspace: p.name, name }).then((r) => r.ok && r.workspace && (forgetLastWindow(p.name), go({ view: "workspace", name: r.workspace, window: p.window }, true)));
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey) return;
    const t = e.target as HTMLElement;
    if (t.closest("input,textarea,[contenteditable],.xterm")) return;
    const wins = ws()?.windows ?? [];
    const i = wins.findIndex((w) => w.id === p.window);
    const k = e.altKey ? e.code.replace(/^Digit/, "") : e.key;
    if (e.altKey && e.shiftKey && (k === "ArrowLeft" || k === "ArrowRight")) {
      const j = k === "ArrowLeft" ? i - 1 : i + 1;
      if (i >= 0 && j >= 0 && j < wins.length) moveWindow(p.name, wins[i].id, k === "ArrowLeft" ? wins[j].id : (wins[j + 1]?.id ?? null));
    } else if (e.altKey && /^[1-9]$/.test(k)) {
      const w = wins[+k - 1];
      if (w) go({ view: "workspace", name: p.name, window: w.id });
    } else if (e.altKey) return;
    else if (k === "]" && wins.length) go({ view: "workspace", name: p.name, window: wins[(i + 1) % wins.length].id });
    else if (k === "[" && wins.length) go({ view: "workspace", name: p.name, window: wins[(i - 1 + wins.length) % wins.length].id });
    else if (k === "c" && hasClaude()) open("claude");
    else if (k === "c") toast(NO_CLAUDE, true);
    else if (k === "t") open("shell");
    else if (k === "v" && current()?.kind === "agent") setMode(mode() === "chat" ? "terminal" : "chat");
    else if (k === "\\") togglePanel();
    else if (k === "s") toggleSplit();
    else if (k === "m") {
      const a = agentOf(current());
      if (!a || a.status !== "waiting" || a.needs_permission) return;
      act({ type: "mark_done", agent: a.key });
    }
    else return;
    e.preventDefault();
  };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));

  return (
    <Show
      when={ws()}
      fallback={
        <Empty icon={<Layers size={22} />} title="This workspace is closed" class="h-full">
          <Button class="mt-4" onClick={() => go({ view: "agents" })}>
            Back to agents
          </Button>
        </Empty>
      }
    >
      {(w) => (
        <div class="flex h-full min-h-0 flex-col">
          <header class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line px-4 py-2.5 sm:px-5">
            <div class="flex min-w-0 items-center gap-2">
              <StatusDot status={worst(agents())} size={9} />
              <Show
                when={renaming()}
                fallback={
                  <h1 class="group flex min-w-0 items-center gap-1 truncate text-lg font-semibold tracking-[-0.01em] text-fg" onDblClick={() => setRenaming(true)}>
                    {w().name}
                    <button class="opacity-0 transition-opacity group-hover:opacity-100" onClick={() => setRenaming(true)} aria-label="Rename">
                      <Rename size={13} class="text-fg-3" />
                    </button>
                  </h1>
                }
              >
                <input
                  value={w().name}
                  ref={(el) => queueMicrotask(() => el.select())}
                  class="w-48 rounded-md border border-accent/50 bg-raised px-2 py-0.5 text-lg font-semibold outline-none"
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Enter") rename(e.currentTarget.value);
                    if (e.key === "Escape") setRenaming(false);
                  }}
                  onBlur={(e) => rename(e.currentTarget.value)}
                />
              </Show>
              <span class="hidden truncate text-sm text-fg-3 md:inline">{w().short_path}</span>
            </div>
            <Show when={w().git}>
              {(g) => (
                <div class="flex items-center gap-1.5 text-sm text-fg-2">
                  <Badge>
                    <Branch size={12} /> {g().branch ?? "detached"}
                  </Badge>
                  <Show when={g().dirty}>
                    <Badge tone="waiting" title="Changed files">
                      {g().dirty} changed
                    </Badge>
                  </Show>
                  <Show when={g().ahead}>
                    <Badge title="Commits to push">
                      <ArrowUp size={11} />
                      {g().ahead}
                    </Badge>
                  </Show>
                  <Show when={g().behind}>
                    <Badge title="Commits to pull">
                      <ArrowDown size={11} />
                      {g().behind}
                    </Badge>
                  </Show>
                </div>
              )}
            </Show>
            <div class="ml-auto flex items-center gap-1">
              <Show when={tabsHidden()}>{newWindowMenu("hidden md:inline-grid")}</Show>
              <MenuButton
                label="Workspace actions"
                icon={<More size={16} />}
                align="right"
                items={[
                  { label: "Rename", icon: <Rename size={14} />, run: () => setRenaming(true) },
                  { label: "Close workspace", icon: <Trash size={14} />, danger: true, run: closeWorkspace },
                ]}
              />
              <IconButton label={focus() ? "Leave focus mode (Z)" : "Focus mode (Z)"} active={focus()} onClick={toggleFocus}>
                <Show when={focus()} fallback={<Maximize size={15} />}>
                  <Minimize size={15} />
                </Show>
              </IconButton>
              <IconButton label={showPanel() ? "Hide the side panel (\\)" : "Show the side panel (\\)"} active={showPanel()} onClick={togglePanel} class="hidden lg:inline-grid">
                <PanelRight size={15} />
              </IconButton>
            </div>
          </header>

          <nav class={cls("flex items-center gap-1 border-b border-line bg-panel/60 px-2 sm:px-3", tabsHidden() && "md:hidden")}>
            <div
              ref={tabs}
              class="flex min-w-0 items-center gap-1 overflow-x-auto overflow-y-hidden [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              role="tablist"
              onWheel={wheelTabs}
            >
              <For each={w().windows}>
                {(win, i) => {
                  const a = () => agentOf(win);
                  const active = () => win.id === p.window;
                  const isBeside = () => second()?.id === win.id;
                  const d = () => drag();
                  const markerBefore = () => d() && !inPlace(d()!) && d()!.before === win.id;
                  const markerAfter = () => d() && !inPlace(d()!) && d()!.before === null && i() === w().windows.length - 1;
                  return (
                    <div
                      role="tab"
                      aria-selected={active()}
                      data-win={win.id}
                      class={cls(
                        "group relative flex h-10 shrink-0 cursor-pointer select-none items-center gap-2 rounded-t-lg px-3 text-base transition-colors",
                        active() ? "text-fg" : "text-fg-2 hover:text-fg",
                        d()?.id === win.id && "cursor-grabbing opacity-50",
                      )}
                      onPointerDown={(e) => onTabDown(e, win.id)}
                      onContextMenu={(e) => drag() && e.preventDefault()}
                      onClick={() => dragged || go({ view: "workspace", name: p.name, window: win.id })}
                      onAuxClick={(e) => e.button === 1 && closeWindow(win)}
                      title={`${win.command} · Alt+${i() + 1}`}
                    >
                      <span class={cls("text-fg-3", active() && "text-accent")}>{kindIcon(win)}</span>
                      <span class="max-w-[16rem] truncate">{a()?.title ?? win.running ?? win.name}</span>
                      <Show when={a()}>
                        <StatusDot status={a()!.status} size={7} />
                      </Show>
                      <Show when={!active() && !isBeside()}>
                        <button
                          class="-mr-1 hidden size-5 place-items-center rounded text-fg-3 opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100 lg:grid"
                          onClick={(e) => {
                            e.stopPropagation();
                            beside(win.id);
                          }}
                          aria-label="Show beside"
                          title="Show beside the current window"
                        >
                          <Columns size={12} />
                        </button>
                      </Show>
                      <button
                        class="-mr-1 grid size-5 place-items-center rounded text-fg-3 opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100"
                        onClick={(e) => {
                          e.stopPropagation();
                          closeWindow(win);
                        }}
                        aria-label="Close window"
                      >
                        <X size={12} />
                      </button>
                      <Show when={active() || isBeside()}>
                        <span class={cls("absolute inset-x-2 bottom-0 h-[2px] rounded-full", active() ? "bg-accent" : "bg-accent/40")} />
                      </Show>
                      <Show when={markerBefore() || markerAfter()}>
                        <span class={cls("absolute inset-y-2 w-[2px] rounded-full bg-accent", markerBefore() ? "-left-[3px]" : "-right-[3px]")} />
                      </Show>
                    </div>
                  );
                }}
              </For>
            </div>
            {newWindowMenu("ml-0.5 shrink-0")}
          </nav>

          <div class="flex min-h-0 flex-1">
            <main class="flex min-w-0 flex-1 flex-col">
              <Show when={current()} fallback={<div class="grid flex-1 place-items-center"><Spinner size={18} /></div>}>
                {(win) => <Pane win={win()} primary />}
              </Show>
            </main>
            <Show when={second()}>
              {(win) => (
                <section class="relative hidden min-w-0 shrink-0 flex-col border-l border-line lg:flex" style={{ width: `${splitWidth.width()}px`, "max-width": "70%" }}>
                  <Resizer size={splitWidth} side="left" label="Width of the window beside" />
                  <Pane win={win()} primary={false} />
                </section>
              )}
            </Show>
            <Show
              when={showPanel()}
              fallback={
                <div class="hidden lg:flex">
                  <Edge side="right" label={focus() ? "Leave focus mode (Z)" : "Show the side panel (\\)"} onClick={togglePanel} />
                </div>
              }
            >
              <div class="hidden lg:block">
                <SidePanel ws={w()} agents={agents()} />
              </div>
            </Show>
          </div>
        </div>
      )}
    </Show>
  );
}
