import { For, Match, Show, Switch, createEffect, createMemo, createSignal, lazy, onCleanup, onMount } from "solid-js";
import type { Agent, Win, Workspace } from "../api";
import { act, ask, draft, go, isBusy, now, setDiffView, state, store } from "../store";
import { pipelineTone } from "../gitlab";
import { age, cls, worst } from "../util";
import { Conversation } from "./Conversation";
import { ArrowDown, ArrowUp, Bot, Done, More, Branch, Chat, Code, External, FileDiff, Layers, Pipeline, Plus, Rename, Terminal as TermIcon, Trash, X } from "./icons";
import { Badge, Button, Empty, IconButton, MenuButton, Spinner, StatusDot, StatusPill } from "./ui";

const Terminal = lazy(() => import("./Terminal").then((m) => ({ default: m.Terminal })));

function kindIcon(w: Win, size = 14) {
  switch (w.kind) {
    case "agent":
      return <Bot size={size} />;
    case "editor":
      return <Code size={size} />;
    default:
      return <TermIcon size={size} />;
  }
}

const FILE_TONE: Record<string, string> = { M: "text-waiting", A: "text-add", D: "text-del", R: "text-working", "?": "text-add", U: "text-danger" };

function fileCode(code: string) {
  const c = code === "??" ? "?" : (code.replace(".", "").trim()[0] ?? "M");
  return { letter: c === "?" ? "U" : c, tone: FILE_TONE[c] ?? "text-fg-2", title: code === "??" ? "Untracked" : code };
}

const [panelOpen, setPanelOpen] = createSignal(localStorage.getItem("panel") !== "closed");
const togglePanel = () => {
  const v = !panelOpen();
  setPanelOpen(v);
  try {
    localStorage.setItem("panel", v ? "open" : "closed");
  } catch {
    /* ignore */
  }
};

/** Chat or terminal for each agent window; chat first, as it reads better. */
const [modes, setModes] = createSignal<Record<string, "chat" | "terminal">>({});

function SidePanel(p: { ws: Workspace; agents: Agent[] }) {
  const g = () => p.ws.git;
  // Commits to push, or a branch of its own that has no merge request yet.
  const canPush = () => {
    const b = g()?.branch;
    if (!b || !state.gitlab.enabled) return false;
    return g()!.ahead > 0 || (!p.ws.mr && b !== "main" && b !== "master");
  };
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
            <button class="ml-auto text-[12px] text-fg-2 hover:text-fg" onClick={() => setDiffView({ path: p.ws.path, file: null, title: `All changes in ${p.ws.name}` })}>
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
                      onClick={() => setDiffView({ path: p.ws.path, file, title: file })}
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
    </aside>
  );
}

export function WorkspaceView(p: { name: string; window: string | null }) {
  const ws = createMemo(() => state.workspaces.find((w) => w.name === p.name));
  const agents = createMemo(() => store.byWorkspace().get(p.name) ?? []);
  const current = createMemo(() => ws()?.windows.find((w) => w.id === p.window) ?? null);
  const agentOf = (w: Win | null) => (w?.agent ? store.agentsByKey().get(w.agent) : undefined);

  // Without a window in the URL, show the one that needs you, else the first agent, else the first window.
  createEffect(() => {
    const w = ws();
    if (!w || current()) return;
    const pick =
      w.windows.find((x) => agentOf(x)?.status === "waiting") ?? w.windows.find((x) => x.kind === "agent") ?? w.windows[0];
    if (pick) go({ view: "workspace", name: w.name, window: pick.id }, true);
  });

  const mode = () => {
    const w = current();
    if (!w || w.kind !== "agent") return "terminal";
    return modes()[w.id] ?? "terminal";
  };
  const setMode = (m: "chat" | "terminal") => {
    const w = current();
    if (w) setModes((s) => ({ ...s, [w.id]: m }));
  };
  // A draft for this window's agent waits in the chat's reply box, so show the chat.
  createEffect(() => {
    const d = draft();
    if (d && agentOf(current())?.key === d.agent) setMode("chat");
  });

  const open = (kind: "claude" | "shell" | "nvim") =>
    act({ type: "new_window", workspace: p.name, kind }, { key: `new:${kind}`, quiet: true }).then((r) => {
      if (r.ok && r.window) {
        go({ view: "workspace", name: p.name, window: r.window });
      }
    });

  const closeWindow = (w: Win) => {
    const a = agentOf(w);
    const run = () =>
      act({ type: "close_window", window: w.id }, { quiet: true }).then((r) => {
        if (r.ok && p.window === w.id) {
          const rest = ws()?.windows.filter((x) => x.id !== w.id) ?? [];
          if (rest[0]) go({ view: "workspace", name: p.name, window: rest[0].id }, true);
        }
      });
    if (w.kind === "shell") return run();
    ask({
      title: a ? `Close agent “${a.title}”?` : `Close ${w.name}?`,
      body: a ? "This ends its Claude session. You can resume it later from the agent list." : `The window runs ${w.command}. It stops.`,
      confirm: "Close",
      danger: true,
      run,
    });
  };

  const closeWorkspace = () => {
    const w = ws();
    if (!w) return;
    ask({
      title: `Close workspace “${w.name}”?`,
      body: `All ${w.windows.length} window${w.windows.length === 1 ? "" : "s"} close, agents included. Files stay as they are.`,
      confirm: "Close workspace",
      danger: true,
      run: () => act({ type: "close_workspace", workspace: w.name }).then((r) => r.ok && go({ view: "inbox" })),
    });
  };

  const [renaming, setRenaming] = createSignal(false);
  const rename = (name: string) => {
    setRenaming(false);
    if (!name.trim() || name === p.name) return;
    act({ type: "rename_workspace", workspace: p.name, name }).then((r) => r.ok && r.workspace && go({ view: "workspace", name: r.workspace, window: p.window }, true));
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey) return;
    const t = e.target as HTMLElement;
    if (t.closest("input,textarea,[contenteditable],.xterm")) return;
    const wins = ws()?.windows ?? [];
    const i = wins.findIndex((w) => w.id === p.window);
    const k = e.altKey ? e.code.replace(/^Digit/, "") : e.key;
    if (e.altKey && /^[1-9]$/.test(k)) {
      const w = wins[+k - 1];
      if (w) go({ view: "workspace", name: p.name, window: w.id });
    } else if (e.altKey) return;
    else if (k === "]" && wins.length) go({ view: "workspace", name: p.name, window: wins[(i + 1) % wins.length].id });
    else if (k === "[" && wins.length) go({ view: "workspace", name: p.name, window: wins[(i - 1 + wins.length) % wins.length].id });
    else if (k === "c") open("claude");
    else if (k === "t") open("shell");
    else if (k === "e") open("nvim");
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
          <Button class="mt-4" onClick={() => go({ view: "inbox" })}>
            Back to the inbox
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
              <MenuButton
                label="Workspace actions"
                icon={<More size={16} />}
                align="right"
                items={[
                  { label: "Rename", icon: <Rename size={14} />, run: () => setRenaming(true) },
                  { label: "Close workspace", icon: <Trash size={14} />, danger: true, run: closeWorkspace },
                ]}
              />
              <IconButton label="Toggle side panel (\)" active={panelOpen()} onClick={togglePanel} class="hidden lg:inline-grid">
                <Layers size={15} />
              </IconButton>
            </div>
          </header>

          <nav class="flex items-center gap-1 overflow-x-auto border-b border-line bg-panel/60 px-2 sm:px-3" role="tablist">
            <For each={w().windows}>
              {(win, i) => {
                const a = () => agentOf(win);
                const active = () => win.id === p.window;
                return (
                  <div
                    role="tab"
                    aria-selected={active()}
                    class={cls(
                      "group relative flex h-10 shrink-0 cursor-pointer items-center gap-2 rounded-t-lg px-3 text-[13px] transition-colors",
                      active() ? "text-fg" : "text-fg-2 hover:text-fg",
                    )}
                    onClick={() => go({ view: "workspace", name: p.name, window: win.id })}
                    onAuxClick={(e) => e.button === 1 && closeWindow(win)}
                    title={`${win.command} · Alt+${i() + 1}`}
                  >
                    <span class={cls("text-fg-3", active() && "text-accent")}>{kindIcon(win)}</span>
                    <span class="max-w-[16rem] truncate">{a()?.title ?? win.name}</span>
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
                      <span class="absolute inset-x-2 -bottom-px h-[2px] rounded-full bg-accent" />
                    </Show>
                  </div>
                );
              }}
            </For>
            <MenuButton
              label="New window (c, t, e)"
              icon={<Show when={isBusy("new:claude") || isBusy("new:shell") || isBusy("new:nvim")} fallback={<Plus size={15} />}><Spinner size={13} /></Show>}
              size="sm"
              class="ml-0.5"
              items={[
                { label: "Agent", icon: <Bot size={14} />, hint: "C", run: () => open("claude") },
                { label: "Terminal", icon: <TermIcon size={14} />, hint: "T", run: () => open("shell") },
                { label: "Editor", icon: <Code size={14} />, hint: "E", run: () => open("nvim") },
              ]}
            />
          </nav>

          <div class="flex min-h-0 flex-1">
            <main class="flex min-w-0 flex-1 flex-col">
              <Show when={current()}>
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
            <Show when={panelOpen()}>
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
