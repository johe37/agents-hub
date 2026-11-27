import { For, Show, createMemo, createSignal, onCleanup } from "solid-js";
import type { Agent, UsageWindow } from "../api";
import { act, chatWorkspace, readStored, writeStored, closeChat, closeWindow, closeWorkspace, connection, go, href, now, isBusy, newChat, openPalette, reopen, route, setSettingsOpen, state, store } from "../store";
import { showFiveHour, showWeekly, tabsMode } from "../settings";
import { MOD, alive, cls, until, worst } from "../util";
import { Bot, Chat, ChevronDown, Cog, Folder, Git, PanelLeft, Plus, Search, X } from "./icons";
import { stored, toggleNav } from "../layout";
import { attention } from "../gitlab";
import { IconButton, Spinner, StatusDot, kindIcon, SectionTitle, Ago } from "./ui";

const [showClosed, setShowClosed] = stored("show-closed", false);
const toggleClosed = () => setShowClosed(!showClosed());
const [workspacesOpen, setWorkspacesOpen] = stored("nav-workspaces-open", true);
const [chatsOpen, setChatsOpen] = stored("nav-chats-open", true);

function storedSet(key: string) {
  const list = (() => {
    try {
      const v = JSON.parse(readStored(key) ?? "[]");
      return Array.isArray(v) ? (v as string[]) : [];
    } catch {
      return [];
    }
  })();
  const [get, set] = createSignal(new Set(list));
  const toggle = (name: string) => {
    const next = new Set(get());
    if (!next.delete(name)) next.add(name);
    set(next);
    writeStored(key, JSON.stringify([...next]));
  };
  return [get, toggle] as const;
}

// The tree starts collapsed next to tabs, and open when it is the only way to pick a window.
const [expanded, toggleExpanded] = storedSet("nav-expanded");
const [collapsed, toggleCollapsed] = storedSet("nav-collapsed");
const isExpanded = (name: string) => (tabsMode() === "tree" ? !collapsed().has(name) : expanded().has(name));
const toggleTree = (name: string) => (tabsMode() === "tree" ? toggleCollapsed(name) : toggleExpanded(name));

function NavItem(p: { to: Parameters<typeof href>[0]; icon: any; label: string; count?: number; tone?: "waiting"; active: boolean; onNavigate?: () => void }) {
  return (
    <a
      href={href(p.to)}
      onClick={() => p.onNavigate?.()}
      class={cls(
        "flex h-8 items-center gap-2.5 rounded-lg px-2.5 text-base font-medium transition-colors",
        p.active ? "bg-hover text-fg" : "text-fg-2 hover:bg-hover/70 hover:text-fg",
      )}
    >
      <span class={cls(p.active ? "text-fg" : "text-fg-3")}>{p.icon}</span>
      {p.label}
      <Show when={p.count}>
        <span
          class={cls(
            "ml-auto min-w-5 rounded-full px-1.5 text-center text-xs font-semibold tabular-nums leading-5",
            p.tone === "waiting" ? "bg-waiting text-waiting-fg" : "bg-hover text-fg-2",
          )}
        >
          {p.count}
        </span>
      </Show>
    </a>
  );
}

// A folded group still shows its worst status, so a waiting agent is not missed.
function GroupHeader(p: { title: string; count: number; open: boolean; onToggle: () => void; agents: Agent[]; addLabel: string; onAdd: () => void; class?: string }) {
  const waiting = () => p.agents.filter((a) => a.status === "waiting").length;
  return (
    <div class={cls("flex items-center gap-2 pb-1 pl-2.5 pr-1", p.class)}>
      <SectionTitle as="div" class="min-w-0 flex-1" count={p.open ? undefined : p.count} open={p.open} onToggle={p.onToggle}>
        {p.title}
      </SectionTitle>
      <Show when={!p.open}>
        <Show when={waiting()} fallback={<StatusDot status={worst(p.agents)} size={7} />}>
          <span class="rounded-full bg-waiting px-1.5 text-xs font-semibold leading-[18px] text-waiting-fg">{waiting()}</span>
        </Show>
      </Show>
      <button class="grid size-5 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg" onClick={p.onAdd} title={p.addLabel}>
        <Plus size={14} />
      </button>
    </div>
  );
}

function Meter(p: { label: string; w: UsageWindow }) {
  const tone = () => (p.w.used >= 90 ? "var(--danger)" : p.w.used >= 70 ? "var(--waiting)" : "var(--fg-3)");
  return (
    <div class="flex flex-col gap-1" title={`${p.label}: ${p.w.used.toFixed(0)}% used, resets in ${until(p.w.resets_at, now())}`}>
      <div class="flex items-baseline justify-between text-xs text-fg-3">
        <span>{p.label}</span>
        <span class="tabular-nums">
          {p.w.used.toFixed(0)}% · {until(p.w.resets_at, now())}
        </span>
      </div>
      <div class="h-1 overflow-hidden rounded-full bg-hover">
        <div class="h-full rounded-full transition-[width] duration-500" style={{ width: `${Math.min(100, p.w.used)}%`, background: tone() }} />
      </div>
    </div>
  );
}

export function Sidebar(p: { onNavigate?: () => void }) {
  const r = route;
  const workspaces = createMemo(() => state.workspaces.filter((w) => w.path !== state.chat_dir).sort((a, b) => a.name.localeCompare(b.name)));
  const chats = createMemo(() => {
    const ws = chatWorkspace();
    return ws ? ws.windows.filter((w) => w.kind === "agent").map((win) => ({ ws, win })) : [];
  });
  // Closing the last chat closes the chats workspace too; its chats show under Chats instead.
  const closed = createMemo(() => state.closed.filter((c) => c.path !== state.chat_dir));
  const closedChats = createMemo(() =>
    state.agents.filter((a) => a.id && !a.workspace && !alive(a.status) && a.path === state.chat_dir),
  );
  const closedItems = createMemo(() =>
    [
      ...closed().map((c) => ({ kind: "workspace" as const, c, ts: c.closed_at })),
      ...closedChats().map((a) => ({ kind: "chat" as const, a, ts: a.timestamp })),
    ].sort((x, y) => y.ts - x.ts),
  );
  const workspaceAgents = createMemo(() => workspaces().flatMap((w) => store.byWorkspace().get(w.name) ?? []));
  const chatAgents = createMemo(() => chats().flatMap(({ win }) => store.agentsByKey().get(win.agent ?? "") ?? []));
  const resume = (a: Agent) => {
    p.onNavigate?.();
    void act({ type: "resume", agent: a.key }, { key: `resume:${a.key}` }).then((r) => r.ok && r.workspace && r.window && go({ view: "workspace", name: r.workspace, window: r.window }));
  };
  const active = (name: string) => {
    const x = r();
    return x.view === "workspace" && x.name === name;
  };
  // A new query string per click restarts the animation; the same URL would resume mid-loop.
  const [juggle, setJuggle] = createSignal(0);
  let juggleTimer: number | undefined;
  const startJuggle = () => {
    setJuggle((n) => n + 1);
    clearTimeout(juggleTimer);
    juggleTimer = window.setTimeout(() => setJuggle(0), 2400);
  };
  onCleanup(() => clearTimeout(juggleTimer));
  return (
    <div class="flex h-full w-full flex-col">
      <div class="flex items-center gap-2.5 px-4 pb-2 pt-4">
        <button type="button" onClick={startJuggle} class="flex items-center gap-2.5 rounded-lg" aria-label="Juggle">
          <img src={juggle() ? `/logo-juggling.webp?${juggle()}` : "/logo.png"} alt="" class="size-10" />
          <span class="text-lg font-semibold tracking-[-0.01em] text-fg">jugglr</span>
        </button>
        <span
          class={cls(
            "ml-auto flex h-5 items-center gap-1.5 rounded-full text-xs font-medium",
            connection() === "live" ? "px-1" : "px-2",
            connection() === "connecting" && "bg-waiting-soft text-waiting",
            connection() === "offline" && "bg-danger-soft text-danger",
          )}
          title={connection() === "live" ? "Live" : connection() === "connecting" ? "Connecting" : "Offline: reconnecting"}
        >
          <span class="relative flex size-2">
            <Show when={connection() !== "live"}>
              <span class="absolute inset-0 animate-ping rounded-full bg-current opacity-60" />
            </Show>
            <span class={cls("relative size-2 rounded-full", connection() === "live" ? "bg-idle" : "bg-current")} />
          </span>
          <Show when={connection() !== "live"}>{connection() === "connecting" ? "Connecting" : "Offline"}</Show>
        </span>
        <IconButton
          label="Settings (,)"
          onClick={() => {
            p.onNavigate?.();
            setSettingsOpen(true);
          }}
          class="size-7 max-md:-mr-2"
        >
          <Cog size={15} />
        </IconButton>
        <IconButton label="Hide the sidebar (|)" onClick={toggleNav} class="-mr-2 hidden size-7 md:inline-grid">
          <PanelLeft size={15} />
        </IconButton>
      </div>

      <div class="px-3 pb-2">
        <button
          onClick={() => openPalette()}
          class="flex h-8 w-full items-center gap-2 rounded-lg border border-line bg-raised px-2.5 text-base text-fg-3 shadow-soft transition-colors hover:border-line-strong hover:text-fg-2"
        >
          <Search size={14} />
          Search or run…
          <span class="ml-auto flex gap-0.5">
            <span class="kbd">{MOD}</span>
            <span class="kbd">k</span>
          </span>
        </button>
      </div>

      <nav class="flex flex-col gap-0.5 px-3 py-1">
        <NavItem
          to={{ view: "agents" }}
          icon={<Bot size={15} />}
          label="Agents"
          count={store.waiting().length || store.live().length}
          tone={store.waiting().length ? "waiting" : undefined}
          active={r().view === "agents" || r().view === "agent"}
          onNavigate={p.onNavigate}
        />
        <Show when={state.gitlab.enabled}>
          <NavItem
            to={{ view: "gitlab", key: null }}
            icon={<Git size={15} />}
            label="GitLab"
            count={attention()}
            tone="waiting"
            active={r().view === "gitlab"}
            onNavigate={p.onNavigate}
          />
        </Show>
      </nav>

      <div class="mt-4 min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        <GroupHeader
          title="Workspaces"
          count={workspaces().length}
          open={workspacesOpen()}
          onToggle={() => setWorkspacesOpen(!workspacesOpen())}
          agents={workspaceAgents()}
          addLabel="Open a workspace (O)"
          onAdd={() => openPalette("open")}
        />
        <Show when={workspacesOpen()}>
          <For each={workspaces()} fallback={<div class="px-2.5 py-2 text-sm text-fg-3">No workspaces open. Press O to open one.</div>}>
            {(w) => {
              const agents = () => store.byWorkspace().get(w.name) ?? [];
              const waiting = () => agents().filter((a) => a.status === "waiting").length;
              const tree = () => tabsMode() !== "tabs" && w.windows.length > (tabsMode() === "tree" ? 0 : 1);
              const open = () => isExpanded(w.name);
              return (
                <div>
                  <div
                    data-juggle
                    class={cls(
                      "group flex items-center gap-2.5 rounded-lg pl-2.5 pr-1 transition-colors",
                      active(w.name) ? "bg-hover" : "hover:bg-hover/70",
                    )}
                  >
                    <Show when={tree()} fallback={<span class="-ml-1.5 size-4 shrink-0" />}>
                      <button
                        class="-ml-1.5 grid size-4 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg"
                        onClick={() => toggleTree(w.name)}
                        aria-expanded={open()}
                        aria-label={`${open() ? "Collapse" : "Expand"} ${w.name}`}
                      >
                        <ChevronDown size={12} class={cls("transition-transform", !open() && "-rotate-90")} />
                      </button>
                    </Show>
                    <a
                      href={href({ view: "workspace", name: w.name, window: null })}
                      onClick={(e) => {
                        e.preventDefault();
                        p.onNavigate?.();
                        go({ view: "workspace", name: w.name, window: null });
                      }}
                      aria-current={active(w.name) ? "page" : undefined}
                      class="flex min-w-0 flex-1 items-center gap-2.5 py-1.5"
                    >
                      <StatusDot status={worst(agents())} size={7} />
                      <div class="min-w-0 flex-1">
                        <div class={cls("truncate text-base font-medium", active(w.name) ? "text-fg" : "text-fg-2 group-hover:text-fg")}>{w.name}</div>
                        <Show when={w.git?.branch}>
                          <div class="truncate text-xs text-fg-3">
                            {w.git!.branch}
                            <Show when={w.git!.dirty}>
                              <span class="text-waiting"> · {w.git!.dirty}</span>
                            </Show>
                          </div>
                        </Show>
                      </div>
                      <Show when={waiting()}>
                        <span class="rounded-full bg-waiting px-1.5 text-xs font-semibold leading-[18px] text-waiting-fg group-hover:hidden group-focus-within:hidden">{waiting()}</span>
                      </Show>
                    </a>
                    <button
                      class="hidden size-5 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-danger group-hover:grid group-focus-within:grid"
                      onClick={() => closeWorkspace(w)}
                      aria-label={`Close workspace ${w.name}`}
                      title="Close workspace"
                    >
                      <X size={12} />
                    </button>
                  </div>
                  <Show when={tree() && open()}>
                    <div class="mb-1 ml-[13px] flex flex-col gap-px border-l border-line pl-1.5">
                      <For each={w.windows}>
                        {(win) => {
                          const a = () => (win.agent ? store.agentsByKey().get(win.agent) : undefined);
                          const on = () => {
                            const x = r();
                            return x.view === "workspace" && x.name === w.name && x.window === win.id;
                          };
                          return (
                            <div
                              class={cls(
                                "group/win flex h-7 items-center rounded-md pr-1 text-sm transition-colors",
                                on() ? "bg-hover text-fg" : "text-fg-2 hover:bg-hover/70 hover:text-fg",
                              )}
                            >
                              <a
                                href={href({ view: "workspace", name: w.name, window: win.id })}
                                onClick={(e) => {
                                  e.preventDefault();
                                  p.onNavigate?.();
                                  go({ view: "workspace", name: w.name, window: win.id });
                                }}
                                onAuxClick={(e) => {
                                  if (e.button !== 1) return;
                                  e.preventDefault();
                                  closeWindow(w, win);
                                }}
                                title={win.command}
                                aria-current={on() ? "page" : undefined}
                                class="flex h-full min-w-0 flex-1 items-center gap-2 pl-2 pr-1"
                              >
                                <span class={cls("shrink-0", on() ? "text-accent" : "text-fg-3")}>{kindIcon(win, 13)}</span>
                                <span class="min-w-0 flex-1 truncate">{a()?.title ?? win.running ?? win.name}</span>
                                <Show when={a()}>
                                  <span class="group-hover/win:hidden group-focus-within/win:hidden">
                                    <StatusDot status={a()!.status} size={6} />
                                  </span>
                                </Show>
                              </a>
                              <button
                                class="hidden size-5 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-danger group-hover/win:grid group-focus-within/win:grid"
                                onClick={() => closeWindow(w, win)}
                                aria-label={`Close window ${a()?.title ?? win.running ?? win.name}`}
                                title="Close window"
                              >
                                <X size={12} />
                              </button>
                            </div>
                          );
                        }}
                      </For>
                    </div>
                  </Show>
                </div>
              );
            }}
          </For>

        </Show>

        <GroupHeader
          title="Chats"
          class="mt-4"
          count={chats().length}
          open={chatsOpen()}
          onToggle={() => setChatsOpen(!chatsOpen())}
          agents={chatAgents()}
          addLabel="New chat (⇧C)"
          onAdd={newChat}
        />
        <Show when={chatsOpen()}>
          <For each={chats()} fallback={<div class="px-2.5 py-1.5 text-sm text-fg-3">No chats. Press ⇧C to start one.</div>}>
            {({ ws, win }) => {
              const a = () => (win.agent ? store.agentsByKey().get(win.agent) : undefined);
              const on = () => {
                const x = r();
                return x.view === "workspace" && x.name === ws.name && x.window === win.id;
              };
              return (
                <div class={cls("group flex items-center rounded-lg pr-1 transition-colors", on() ? "bg-hover" : "hover:bg-hover/70")}>
                  <a
                    href={href({ view: "workspace", name: ws.name, window: win.id })}
                    onClick={(e) => {
                      e.preventDefault();
                      p.onNavigate?.();
                      go({ view: "workspace", name: ws.name, window: win.id });
                    }}
                    aria-current={on() ? "page" : undefined}
                    class="flex min-w-0 flex-1 items-center gap-2.5 py-1.5 pl-2.5 pr-1"
                  >
                    <Chat size={13} class={cls("shrink-0", on() ? "text-accent" : "text-fg-3")} />
                    <div class="min-w-0 flex-1">
                      <div class={cls("truncate text-base", on() ? "text-fg" : "text-fg-2 group-hover:text-fg")}>{a()?.title ?? win.name}</div>
                      <Show when={a()}>
                        <div class="truncate text-xs text-fg-3"><Ago ts={a()!.timestamp} suffix /></div>
                      </Show>
                    </div>
                    <Show when={a()}>
                      <span class="group-hover:hidden group-focus-within:hidden">
                        <StatusDot status={a()!.status} size={7} />
                      </span>
                    </Show>
                  </a>
                  <button
                    class="hidden size-5 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-danger group-hover:grid group-focus-within:grid"
                    onClick={() => closeChat(ws, win)}
                    aria-label={`Close chat ${a()?.title ?? win.name}`}
                    title="Close chat"
                  >
                    <X size={12} />
                  </button>
                </div>
              );
            }}
          </For>
        </Show>

        <Show when={closedItems().length}>
          <div class="mt-4 pb-1 pl-2.5 pr-1">
            <SectionTitle as="div" count={closedItems().length} open={showClosed()} onToggle={toggleClosed}>
              Closed
            </SectionTitle>
          </div>
          <Show when={showClosed()}>
            <For each={closedItems()}>
              {(item) =>
                item.kind === "workspace" ? (
                  <div class="group flex items-center gap-1 rounded-lg hover:bg-hover/70">
                    <button
                      class="flex min-w-0 flex-1 items-center gap-2.5 px-2.5 py-1.5 text-left"
                      title={`Open ${item.c.short_path} again with ${item.c.windows.length} window${item.c.windows.length === 1 ? "" : "s"}${item.c.windows.some((w) => w.resume) ? "; its agents resume" : ""}`}
                      disabled={isBusy(`reopen:${item.c.id}`)}
                      onClick={() => {
                        p.onNavigate?.();
                        void reopen(item.c.id);
                      }}
                    >
                      <Show when={isBusy(`reopen:${item.c.id}`)} fallback={<Folder size={13} class="shrink-0 text-fg-3" />}>
                        <Spinner size={13} />
                      </Show>
                      <div class="min-w-0 flex-1">
                        <div class="truncate text-base text-fg-2 group-hover:text-fg">{item.c.name}</div>
                        <div class="truncate text-xs text-fg-3">
                          {item.c.windows.length} window{item.c.windows.length === 1 ? "" : "s"} · <Ago ts={item.c.closed_at} suffix />
                        </div>
                      </div>
                    </button>
                    <button
                      class="mr-1 grid size-5 shrink-0 place-items-center rounded text-fg-3 opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100"
                      onClick={() => act({ type: "forget_closed", id: item.c.id }, { quiet: true })}
                      aria-label={`Remove ${item.c.name} from the list`}
                      title="Remove from the list"
                    >
                      <X size={12} />
                    </button>
                  </div>
                ) : (
                  <button
                    class="group flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left hover:bg-hover/70"
                    title="Resume this chat"
                    disabled={isBusy(`resume:${item.a.key}`)}
                    onClick={() => resume(item.a)}
                  >
                    <Show when={isBusy(`resume:${item.a.key}`)} fallback={<Chat size={13} class="shrink-0 text-fg-3" />}>
                      <Spinner size={13} />
                    </Show>
                    <div class="min-w-0 flex-1">
                      <div class="truncate text-base text-fg-2 group-hover:text-fg">{item.a.title}</div>
                      <div class="truncate text-xs text-fg-3"><Ago ts={item.a.timestamp} suffix /></div>
                    </div>
                  </button>
                )
              }
            </For>
          </Show>
        </Show>
      </div>

      <Show when={(showFiveHour() && state.usage?.five_hour) || (showWeekly() && state.usage?.seven_day)}>
        <div class="flex flex-col gap-2 border-t border-line px-4 py-3">
          <Show when={showFiveHour() && state.usage!.five_hour}>
            <Meter label="5-hour limit" w={state.usage!.five_hour!} />
          </Show>
          <Show when={showWeekly() && state.usage!.seven_day}>
            <Meter label="Weekly limit" w={state.usage!.seven_day!} />
          </Show>
        </div>
      </Show>
    </div>
  );
}
