import { For, Match, Show, Switch, createEffect, createMemo, createResource, createSignal, lazy, onCleanup, onMount, untrack } from "solid-js";
import { getJSON, type Agent, type Commit, type Win, type Workspace } from "../api";
import { act, actHiding, ask, closeWorkspace as closeWs, draft, forgetLastWindow, go, isBusy, lastWindow, moveWindow, now, setDiffView, state, stops, store } from "../store";
import { pipelineTone } from "../gitlab";
import { age, cls, worst } from "../util";
import { Conversation } from "./Conversation";
import { ArrowDown, ArrowUp, Bot, Check, Chevron, Done, More, Branch, Chat, External, FileDiff, Layers, Maximize, Minimize, PanelRight, Pipeline, Plus, Rename, Terminal as TermIcon, Trash, X } from "./icons";
import { Badge, Button, Edge, Empty, IconButton, MenuButton, Spinner, StatusDot, StatusPill, kindIcon } from "./ui";
import { focus, showNav, showPanel, toggleFocus, togglePanel } from "../layout";
import { agentView, lastAgentView, setLastAgentView, tabsMode } from "../settings";

const Terminal = lazy(() => import("./Terminal").then((m) => ({ default: m.Terminal })));

const FILE_TONE: Record<string, string> = { M: "text-waiting", A: "text-add", D: "text-del", R: "text-working", "?": "text-add", U: "text-danger" };

function fileCode(code: string) {
  const c = code === "??" ? "?" : (code.replace(".", "").trim()[0] ?? "M");
  return { letter: c === "?" ? "U" : c, tone: FILE_TONE[c] ?? "text-fg-2", title: code === "??" ? "Untracked" : code };
}

/** Chat or terminal for each agent window you switched; the others follow agentView. */
const [modes, setModes] = createSignal<Record<string, "chat" | "terminal">>({});

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
  // Refetched when HEAD moves: a commit, checkout or pull.
  const [commits] = createResource(
    () => g() && ([p.ws.path, g()!.commit] as const),
    ([path]) => getJSON<Commit[]>(`/api/diff?path=${encodeURIComponent(path)}&commits=1`),
  );
  return (
    <aside class="flex h-full w-[300px] shrink-0 flex-col overflow-y-auto border-l border-line bg-panel">
      <Show when={p.agents.length}>
        <section class="border-b border-line p-3">
          <h3 class="mb-2 px-1 text-[11.5px] font-semibold uppercase tracking-[0.06em] text-fg-3">Agents</h3>
          <For each={p.agents}>
            {(a) => (
              <button
                class="flex w-full items-start gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-hover"
                onClick={() => a.window && go({ view: "workspace", name: p.ws.name, window: a.window })}
              >
                <StatusDot status={a.status} class="mt-[6px]" />
                <div class="min-w-0 flex-1">
                  <div class="truncate text-[13px] font-medium text-fg">{a.title}</div>
                  <div class="truncate text-[12px] text-fg-3">{a.status === "working" ? (a.tool ?? "Thinking…") : a.status === "waiting" ? (a.message ?? "Waiting for you") : `${age(a.timestamp, now())} ago`}</div>
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
              <h3 class="text-[11.5px] font-semibold uppercase tracking-[0.06em] text-fg-3">Merge request</h3>
              <a href={mr().url} target="_blank" rel="noopener" class="ml-auto text-fg-3 hover:text-fg" title="Open in GitLab">
                <External size={12} />
              </a>
            </div>
            <button class="block w-full rounded-lg px-2 py-1.5 text-left hover:bg-hover" onClick={() => go({ view: "gitlab", key: mr().key })} title="Threads, pipeline and description">
              <div class="flex items-center gap-1.5 text-[13px] font-medium text-fg">
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

      <section class="flex min-h-0 flex-1 flex-col p-3">
        <div class="mb-2 flex items-center gap-2 px-1">
          <h3 class="text-[11.5px] font-semibold uppercase tracking-[0.06em] text-fg-3">Changes</h3>
          <Show when={g()}>
            <span class="font-mono text-[11.5px] tabular-nums">
              <span class="text-add">+{g()!.added}</span> <span class="text-del">−{g()!.removed}</span>
            </span>
          </Show>
          <Show when={g()?.files.length}>
            <button class="ml-auto text-[12px] text-fg-2 hover:text-fg" onClick={() => setDiffView({ workspace: p.ws.name, path: p.ws.path, file: null, title: `All changes in ${p.ws.name}` })}>
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
              class="h-7 min-w-0 flex-1 rounded-md border border-line-strong bg-raised px-2 text-[12.5px] outline-none placeholder:text-fg-3 focus:border-accent/60"
            />
            <Button size="sm" type="submit" disabled={!message().trim()} loading={isBusy(`commit:${p.ws.name}`)} title={staged() ? "Commit the staged changes" : "Commit all changes"}>
              <Check size={13} /> {staged() ? "Commit staged" : "Commit all"}
            </Button>
          </form>
        </Show>
        <Show when={g()} fallback={<div class="px-1 text-[12.5px] text-fg-3">Not a git repository</div>}>
          <Show when={g()!.files.length} fallback={<div class="px-1 text-[12.5px] text-fg-3">Working tree clean</div>}>
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
                      <span class={cls("w-3 shrink-0 text-center font-mono text-[11px] font-semibold", c.tone)}>{c.letter}</span>
                      <span class="min-w-0 truncate font-mono text-[12px]">
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

      <Show when={commits()?.length}>
        <section class="border-t border-line p-3">
          <h3 class="mb-2 px-1 text-[11.5px] font-semibold uppercase tracking-[0.06em] text-fg-3">Commits</h3>
          <div class="flex flex-col">
            <For each={commits()}>
              {(c) => {
                const [open, setOpen] = createSignal(false);
                const show = (file: string | null) => setDiffView({ workspace: p.ws.name, path: p.ws.path, file, title: c.title, commit: c });
                return (
                  <div>
                    <div class="group flex min-w-0 items-start gap-1 rounded-md px-1 py-1 hover:bg-hover">
                      <button class="mt-0.5 grid size-4 shrink-0 place-items-center text-fg-3 hover:text-fg" aria-label={open() ? "Hide files" : "Show files"} aria-expanded={open()} onClick={() => setOpen(!open())}>
                        <Chevron size={12} class={cls("transition-transform", open() && "rotate-90")} />
                      </button>
                      <button class="min-w-0 flex-1 text-left" title={c.message ? `${c.title}\n\n${c.message}` : c.title} onClick={() => show(null)}>
                        <div class="truncate text-[12.5px] text-fg">{c.title}</div>
                        <div class="truncate text-[11px] text-fg-3">
                          <span class="font-mono">{c.short}</span> · {c.author} · {age(c.created, now())} ago
                        </div>
                      </button>
                      <FileDiff size={13} class="mt-1 shrink-0 text-fg-3 opacity-0 group-hover:opacity-100" />
                    </div>
                    <Show when={open()}>
                      <div class="mb-1 ml-5 flex flex-col">
                        <For each={c.files ?? []}>
                          {([code, file]) => {
                            const f = fileCode(code);
                            const i = file.lastIndexOf("/");
                            return (
                              <button class="flex min-w-0 items-center gap-2 rounded-md px-1.5 py-0.5 text-left hover:bg-hover" title={file} onClick={() => show(file)}>
                                <span class={cls("w-3 shrink-0 text-center font-mono text-[11px] font-semibold", f.tone)}>{f.letter}</span>
                                <span class="min-w-0 truncate font-mono text-[12px]">
                                  <span class="text-fg">{file.slice(i + 1)}</span>
                                  <Show when={i > 0}>
                                    <span class="ml-1.5 text-fg-3">{file.slice(0, i)}</span>
                                  </Show>
                                </span>
                              </button>
                            );
                          }}
                        </For>
                      </div>
                    </Show>
                  </div>
                );
              }}
            </For>
          </div>
        </section>
      </Show>
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

  const mode = () => {
    const w = current();
    if (!w || w.kind !== "agent") return "terminal";
    const v = agentView();
    return modes()[w.id] ?? (v === "last" ? lastAgentView() : v);
  };
  const setMode = (m: "chat" | "terminal", picked = true) => {
    const w = current();
    if (!w) return;
    setModes((s) => ({ ...s, [w.id]: m }));
    if (picked) setLastAgentView(m);
  };
  // A draft for this window's agent waits in the chat's reply box, so show the chat.
  createEffect(() => {
    const d = draft();
    if (d && agentOf(current())?.key === d.agent) setMode("chat", false);
  });

  const open = (kind: "claude" | "shell") =>
    act({ type: "new_window", workspace: p.name, kind }, { key: `new:${kind}`, quiet: true }).then((r) => {
      if (r.ok && r.window) {
        go({ view: "workspace", name: p.name, window: r.window });
      }
    });

  const closeWindow = (w: Win) => {
    const a = agentOf(w);
    const run = () => {
      const rest = ws()?.windows.filter((x) => x.id !== w.id) ?? [];
      // tmux closes the session with its last window.
      const what = rest.length ? { window: w.id } : { window: w.id, workspace: p.name };
      const before = () => {
        if (!rest.length) go({ view: "agents" });
        else if (p.window === w.id) go({ view: "workspace", name: p.name, window: rest[0].id }, true);
      };
      actHiding({ type: "close_window", window: w.id }, what, { quiet: true, before });
    };
    const stopped = stops(w);
    if (!stopped) return run();
    ask({
      title: a ? `Close agent “${a.title}”?` : `Close ${w.name}?`,
      body: a ? "It stops in the middle of its turn. You can resume the session later from the agent list." : `${stopped}. It stops.`,
      confirm: "Close",
      danger: true,
      run,
    });
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
        { label: "Agent", icon: <Bot size={14} />, hint: "C", run: () => open("claude") },
        { label: "Terminal", icon: <TermIcon size={14} />, hint: "T", run: () => open("shell") },
      ]}
    />
  );

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
    else if (k === "c") open("claude");
    else if (k === "t") open("shell");
    else if (k === "v" && current()?.kind === "agent") setMode(mode() === "chat" ? "terminal" : "chat");
    else if (k === "\\") togglePanel();
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
                  <h1 class="group flex min-w-0 items-center gap-1 truncate text-[16px] font-semibold tracking-[-0.01em] text-fg" onDblClick={() => setRenaming(true)}>
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
                  class="w-48 rounded-md border border-accent/50 bg-raised px-2 py-0.5 text-[15px] font-semibold outline-none"
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Enter") rename(e.currentTarget.value);
                    if (e.key === "Escape") setRenaming(false);
                  }}
                  onBlur={(e) => rename(e.currentTarget.value)}
                />
              </Show>
              <span class="hidden truncate text-[12.5px] text-fg-3 md:inline">{w().short_path}</span>
            </div>
            <Show when={w().git}>
              {(g) => (
                <div class="flex items-center gap-1.5 text-[12px] text-fg-2">
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
                  const d = () => drag();
                  const markerBefore = () => d() && !inPlace(d()!) && d()!.before === win.id;
                  const markerAfter = () => d() && !inPlace(d()!) && d()!.before === null && i() === w().windows.length - 1;
                  return (
                    <div
                      role="tab"
                      aria-selected={active()}
                      data-win={win.id}
                      class={cls(
                        "group relative flex h-10 shrink-0 cursor-pointer select-none items-center gap-2 rounded-t-lg px-3 text-[13px] transition-colors",
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
                      <Show when={active()}>
                        <span class="absolute inset-x-2 bottom-0 h-[2px] rounded-full bg-accent" />
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
                {(win) => (
                  <>
                    <Show when={win().kind === "agent" && agentOf(win())}>
                      {(a) => (
                        <div class="flex items-center gap-2 border-b border-line px-4 py-1.5 sm:px-5">
                          <StatusPill status={a().status} />
                          <span class="min-w-0 truncate text-[12.5px] text-fg-2">
                            <Switch fallback={a().model ?? ""}>
                              <Match when={a().status === "working"}>{a().tool ?? "Thinking…"}</Match>
                              <Match when={a().status === "waiting"}>{a().message}</Match>
                            </Switch>
                          </span>
                          <Show when={a().status === "waiting" && !a().needs_permission}>
                            <Button size="sm" variant="ghost" class="ml-auto shrink-0" onClick={() => act({ type: "mark_done", agent: a().key })} title="Stop showing it as waiting (m)">
                              <Done size={13} /> Done
                            </Button>
                          </Show>
                          <div class={cls("flex rounded-lg bg-hover p-0.5 text-[12px]", !(a().status === "waiting" && !a().needs_permission) && "ml-auto")}>
                            <button class={cls("flex items-center gap-1 rounded-md px-2 py-1", mode() === "chat" ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => setMode("chat")} title="Chat (v)">
                              <Chat size={12} /> Chat
                            </button>
                            <button class={cls("flex items-center gap-1 rounded-md px-2 py-1", mode() === "terminal" ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => setMode("terminal")} title="Terminal (v)">
                              <TermIcon size={12} /> Terminal
                            </button>
                          </div>
                        </div>
                      )}
                    </Show>
                    <div class="min-h-0 flex-1">
                      <Show when={mode() === "chat" && agentOf(win())} fallback={<Terminal window={win().id} focus />}>
                        {(a) => <Conversation agent={a()} />}
                      </Show>
                    </div>
                  </>
                )}
              </Show>
            </main>
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
