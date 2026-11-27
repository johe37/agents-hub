import { For, Show, batch, createEffect, createMemo, createSignal, lazy, onCleanup, onMount, untrack } from "solid-js";
import { type Win, type Workspace } from "../api";
import { act, ask, backTo, closeWindow as closeWin, closeWorkspace as closeWs, draft, forgetLastWindow, go, hasAgent, isBusy, lastWindow, moveWindow, noAgent, openDiff, closeDiff, pinDiff, diffId, diffLabel, diffTabs, isDiff, modes, setModes, state, store, toast } from "../store";
import { pipelineTone } from "../gitlab";
import { cls, worst } from "../util";
import { DiffPane } from "./ChangesReview";
import { Commits } from "./Commits";
import { Conversation } from "./Conversation";
import { ArrowDown, ArrowUp, Bot, Check, Done, More, Branch, Chat, External, FileDiff, Layers, Maximize, Minimize, Minus, PanelRight, Pipeline, Plus, Refresh, Rename, Terminal as TermIcon, Trash, Undo, X } from "./icons";
import { Badge, Button, Edge, Empty, IconButton, MenuButton, Spinner, StatusDot, StatusPill, kindIcon, SectionTitle, Resizer } from "./ui";
import { focus, panelWidth, showNav, showPanel, toggleFocus, togglePanel } from "../layout";
import { agentView, lastAgentView, setLastAgentView, tabsMode } from "../settings";

const Terminal = lazy(() => import("./Terminal").then((m) => ({ default: m.Terminal })));

const FILE_TONE: Record<string, string> = { M: "text-waiting", A: "text-add", D: "text-del", R: "text-working", "?": "text-add", U: "text-danger" };

/** The letter of one column of git's XY code: index for staged, worktree for unstaged. */
function fileCode(code: string, side: "staged" | "unstaged") {
  const c = code === "??" ? "?" : code[side === "staged" ? 0 : 1];
  return { letter: c === "?" ? "U" : c, tone: FILE_TONE[c] ?? "text-fg-2", title: code === "??" ? "Untracked" : code };
}

const isStaged = (code: string) => code[0] !== "." && code[0] !== "?";
const isUnstaged = (code: string) => code === "??" || code[1] !== ".";
const baseName = (file: string) => file.replace(/\/$/, "").split("/").pop()!;

/** Stage, unstage or discard files, asking first before a discard. */
function changeFiles(ws: Workspace, type: "stage" | "unstage" | "discard", files: [string, string][]) {
  const names = files.map(([, f]) => f);
  const run = () => void act({ type, workspace: ws.name, files: names }, { key: `git:${ws.name}` });
  if (type !== "discard") return run();
  const untracked = files.filter(([c]) => c === "??").length;
  const one = files.length === 1;
  ask({
    title: one ? (untracked ? `Delete ${baseName(names[0])}?` : `Discard the changes in ${baseName(names[0])}?`) : `Discard ${files.length} changes?`,
    body: untracked
      ? `${one ? "The file is untracked: it is" : `${untracked} untracked ${untracked === 1 ? "file is" : "files are"}`} deleted for good. This can not be undone.`
      : "Staged changes stay. This can not be undone.",
    confirm: untracked && one ? "Delete" : "Discard",
    danger: true,
    run,
  });
}

function ChangeGroup(p: { ws: Workspace; side: "staged" | "unstaged"; files: [string, string][] }) {
  const [open, setOpen] = createSignal(true);
  const staged = () => p.side === "staged";
  const busy = () => isBusy(`git:${p.ws.name}`);
  const tool = "grid size-5 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg disabled:opacity-40";
  // Shown on hover with a mouse; a touch screen has no hover, so there they always show.
  const onHover = "md:opacity-0 md:group-hover:opacity-100 md:group-focus-within:opacity-100";
  const actions = (files: [string, string][]) => (
    <>
      <Show when={!staged()}>
        <button class={tool} disabled={busy()} title="Discard changes" aria-label="Discard changes" onClick={(e) => (e.stopPropagation(), changeFiles(p.ws, "discard", files))}>
          <Undo size={12} />
        </button>
      </Show>
      <button
        class={tool}
        disabled={busy()}
        title={staged() ? "Unstage" : "Stage"}
        aria-label={staged() ? "Unstage" : "Stage"}
        onClick={(e) => (e.stopPropagation(), changeFiles(p.ws, staged() ? "unstage" : "stage", files))}
      >
        {staged() ? <Minus size={12} /> : <Plus size={12} />}
      </button>
    </>
  );
  return (
    <div class="flex flex-col">
      <div class="group flex items-center gap-1 px-1 py-0.5">
        <SectionTitle class="flex-1" count={p.files.length} open={open()} onToggle={() => setOpen(!open())}>
          {staged() ? "Staged changes" : "Changes"}
        </SectionTitle>
        <div class={cls("flex items-center gap-0.5", onHover)}>{actions(p.files)}</div>
      </div>
      <Show when={open()}>
        <For each={p.files}>
          {([code, file]) => {
            const c = fileCode(code, p.side);
            // Git lists an untracked directory once, with a trailing slash.
            const dir = file.endsWith("/");
            const i = dir ? -1 : file.lastIndexOf("/");
            return (
              <div
                role="button"
                tabIndex={0}
                class="group flex min-w-0 cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-left hover:bg-hover"
                title={`${c.title}: ${file}`}
                onClick={() => openDiff({ workspace: p.ws.name, path: p.ws.path, file, title: file, stage: p.side })}
                onKeyDown={(e) => e.key === "Enter" && e.currentTarget.click()}
              >
                <span class={cls("w-3 shrink-0 text-center font-mono text-xs font-semibold", c.tone)}>{c.letter}</span>
                <span class="min-w-0 flex-1 truncate font-mono text-sm">
                  <span class="text-fg">{file.slice(i + 1)}</span>
                  <Show when={i > 0}>
                    <span class="ml-1.5 text-fg-3">{file.slice(0, i)}</span>
                  </Show>
                </span>
                <div class={cls("flex shrink-0 items-center gap-0.5", onHover)}>{actions([[code, file]])}</div>
              </div>
            );
          }}
        </For>
      </Show>
    </div>
  );
}

function SidePanel(p: { ws: Workspace }) {
  const g = () => p.ws.git;
  // Commits to push, or a branch of its own that has no merge request yet.
  const canPush = () => {
    const b = g()?.branch;
    if (!b || !state.gitlab.enabled) return false;
    return g()!.ahead > 0 || (!p.ws.mr && b !== "main" && b !== "master");
  };
  const [message, setMessage] = createSignal("");
  const stagedFiles = () => g()?.files.filter(([code]) => isStaged(code)) ?? [];
  const unstagedFiles = () => g()?.files.filter(([code]) => isUnstaged(code)) ?? [];
  const staged = () => stagedFiles().length > 0;
  return (
    <aside class="relative flex h-full shrink-0 flex-col border-l border-line bg-panel" style={{ width: `${panelWidth.width()}px` }}>
      <Resizer size={panelWidth} side="left" label="Side panel width" />
      <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
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
              <button class="ml-auto text-sm text-fg-2 hover:text-fg" onClick={() => openDiff({ workspace: p.ws.name, path: p.ws.path, file: null, title: `All changes in ${p.ws.name}` })}>
                View diff
              </button>
            </Show>
            <Show when={g()}>
              <button
                class={cls("grid size-6 place-items-center rounded-md text-fg-3 hover:bg-hover hover:text-fg", !g()!.files.length && "ml-auto")}
                aria-label="Refresh changes"
                title="Refresh changes"
                disabled={isBusy(`refresh_git:${p.ws.name}`)}
                onClick={() => void act({ type: "refresh_git", workspace: p.ws.name }, { key: `refresh_git:${p.ws.name}`, quiet: true })}
              >
                <Show when={isBusy(`refresh_git:${p.ws.name}`)} fallback={<Refresh size={13} />}>
                  <Spinner size={12} />
                </Show>
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
              <div class="flex flex-col gap-1">
                <Show when={stagedFiles().length}>
                  <ChangeGroup ws={p.ws} side="staged" files={stagedFiles()} />
                </Show>
                <Show when={unstagedFiles().length}>
                  <ChangeGroup ws={p.ws} side="unstaged" files={unstagedFiles()} />
                </Show>
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
  const diffs = () => diffTabs()[p.name] ?? [];
  const diff = createMemo(() => diffs().find((d) => diffId(d) === p.window));
  const onDiff = () => !!diff();
  // The diff tab hides the window shown before it, so a terminal stays connected and keeps its screen.
  const shownId = createMemo<string | null>((prev) => current()?.id ?? (onDiff() ? prev : null), null);
  const shown = createMemo(() => ws()?.windows.find((w) => w.id === shownId()) ?? null);
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
  // The tab follows the pointer and the others slide aside; positions are measured once, at the start.
  type Drag = { id: string; from: number; ids: string[]; lefts: number[]; widths: number[]; gap: number; x: number; scroll: number; slot?: number };
  const [drag, setDrag] = createSignal<Drag | null>(null);
  const [dx, setDx] = createSignal(0);
  const [settling, setSettling] = createSignal(false);
  let press: { id: string; x: number; pointer: number; touch: boolean; timer?: number } | null = null;
  let dragged = false;
  // Pointer moves only store the position; one frame loop scrolls the row and moves the tab.
  let pointerX = 0;
  let frame = 0;

  /** Where the dragged tab lands, as an index among the other tabs. */
  const slot = createMemo(() => {
    const d = drag();
    if (!d) return -1;
    if (d.slot !== undefined) return d.slot;
    // A neighbour gives way once the dragged tab's leading edge passes its middle.
    const left = d.lefts[d.from] + dx();
    const right = left + d.widths[d.from];
    let t = d.from;
    d.ids.forEach((_, k) => {
      const mid = d.lefts[k] + d.widths[k] / 2;
      if (k < d.from && left < mid) t--;
      if (k > d.from && right > mid) t++;
    });
    return t;
  });
  const shiftOf = (id: string) => {
    const d = drag();
    if (!d) return 0;
    const k = d.ids.indexOf(id);
    if (k === d.from) return dx();
    const t = slot();
    const s = d.widths[d.from] + d.gap;
    if (k > d.from && k <= t) return -s;
    if (k < d.from && k >= t) return s;
    return 0;
  };

  const unlisten = () => {
    window.removeEventListener("pointermove", onDragMove);
    window.removeEventListener("pointerup", onDragUp);
    window.removeEventListener("pointercancel", onDragCancel);
    window.removeEventListener("touchmove", stopScroll);
    window.removeEventListener("keydown", onDragKey, true);
  };
  const endPress = () => {
    if (press?.timer) clearTimeout(press.timer);
    press = null;
    cancelAnimationFrame(frame);
    unlisten();
    setDrag(null);
    setSettling(false);
    setDx(0);
    // The click that follows the drop must not select the tab.
    setTimeout(() => (dragged = false));
  };
  const begin = () => {
    if (!press || !tabs) return;
    const row = tabs.getBoundingClientRect();
    const els = [...tabs.querySelectorAll<HTMLElement>("[data-win]")];
    const rects = els.map((el) => el.getBoundingClientRect());
    const lefts = rects.map((r) => r.left - row.left + tabs!.scrollLeft);
    const widths = rects.map((r) => r.width);
    const ids = els.map((el) => el.dataset.win!);
    const from = ids.indexOf(press.id);
    if (from < 0) return;
    dragged = true;
    const gap = ids.length > 1 ? lefts[1] - lefts[0] - widths[0] : 0;
    setDx(0);
    setDrag({ id: press.id, from, ids, lefts, widths, gap, x: press.x, scroll: tabs.scrollLeft });
    pointerX = press.x;
    frame = requestAnimationFrame(tick);
  };
  const EDGE = 48;
  const tick = () => {
    if (!drag() || settling() || !tabs) return;
    const r = tabs.getBoundingClientRect();
    // Faster the closer the pointer is to the edge, up to 16px a frame.
    const into = (d: number) => 16 * Math.min(1, Math.max(0, (EDGE - d) / EDGE));
    tabs.scrollLeft += into(r.right - pointerX) - into(pointerX - r.left);
    follow(pointerX);
    frame = requestAnimationFrame(tick);
  };
  const follow = (x: number) => {
    const d = drag()!;
    const end = d.lefts[d.lefts.length - 1] + d.widths[d.widths.length - 1];
    const raw = x - d.x + tabs!.scrollLeft - d.scroll;
    setDx(Math.min(Math.max(raw, -d.lefts[d.from]), end - d.widths[d.from] - d.lefts[d.from]));
  };
  /** Slide the tab into its place, then move the window there. */
  const drop = (keep: boolean) => {
    const d = drag();
    if (!d || settling()) return endPress();
    cancelAnimationFrame(frame);
    unlisten();
    const t = keep ? slot() : d.from;
    const left = t > d.from ? d.lefts[t] + d.widths[t] - d.widths[d.from] : t < d.from ? d.lefts[t] : d.lefts[d.from];
    const before = d.ids.filter((id) => id !== d.id)[t] ?? null;
    batch(() => {
      setDrag({ ...d, slot: t });
      setSettling(true);
      setDx(left - d.lefts[d.from]);
    });
    setTimeout(() => {
      if (drag()?.id !== d.id) return;
      batch(() => {
        if (t !== d.from) moveWindow(p.name, d.id, before);
        endPress();
      });
    }, 160);
  };
  const onDragMove = (e: PointerEvent) => {
    if (!press || e.pointerId !== press.pointer || settling()) return;
    pointerX = e.clientX;
    if (!drag()) {
      const moved = Math.abs(e.clientX - press.x);
      if (press.touch && moved > 8) endPress();
      else if (!press.touch && moved > 5) begin();
    }
  };
  const onDragUp = (e: PointerEvent) => {
    if (!press || e.pointerId !== press.pointer) return;
    if (drag()) drop(true);
    else endPress();
  };
  const onDragCancel = () => (drag() ? drop(false) : endPress());
  const stopScroll = (e: TouchEvent) => drag() && e.preventDefault();
  const onDragKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || !drag()) return;
    e.preventDefault();
    e.stopPropagation();
    drop(false);
  };
  const onTabDown = (e: PointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as Element).closest("button") || settling()) return;
    endPress();
    const touch = e.pointerType === "touch";
    press = { id, x: e.clientX, pointer: e.pointerId, touch };
    if (touch) press.timer = window.setTimeout(begin, 400);
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", onDragUp);
    window.addEventListener("pointercancel", onDragCancel);
    window.addEventListener("touchmove", stopScroll, { passive: false });
    window.addEventListener("keydown", onDragKey, true);
  };
  onCleanup(endPress);

  // Without a window in the URL, show the last one shown here, else the one that needs you, else the first agent, else the first window.
  // A window just opened is in the URL before the hub lists it: give it a moment to show up.
  // A window that was shown here and is gone has closed: there is nothing to wait for.
  const seen = new Set<string>();
  createEffect(() => {
    const w = ws();
    const c = current();
    if (c) seen.add(c.id);
    if (!w || c || onDiff()) return;
    const pickOne = () => {
      const back = backTo(w.name, p.window);
      if (back) return go({ view: "workspace", name: w.name, window: back }, true);
      const last = lastWindow(w.name);
      const pick =
        w.windows.find((x) => x.id === last) ??
        w.windows.find((x) => agentOf(x)?.status === "waiting") ??
        w.windows.find((x) => x.kind === "agent") ??
        w.windows[0];
      if (pick) go({ view: "workspace", name: w.name, window: pick.id }, true);
    };
    if (!p.window || isDiff(p.window) || seen.has(p.window)) return pickOne();
    const t = setTimeout(() => untrack(current) || pickOne(), 4000);
    onCleanup(() => clearTimeout(t));
  });

  const mode = () => {
    const w = shown();
    if (!w || w.kind !== "agent") return "terminal";
    const v = agentView();
    return modes()[w.id] ?? (v === "last" ? lastAgentView() : v);
  };
  const setMode = (m: "chat" | "terminal", picked = true) => {
    const w = shown();
    if (!w) return;
    setModes((s) => ({ ...s, [w.id]: m }));
    if (picked) setLastAgentView(m);
  };

  // A draft for this window's agent waits in the chat's reply box, so show the chat.
  createEffect(() => {
    const d = draft();
    if (d && agentOf(current())?.key === d.agent) setMode("chat", false);
  });

  const open = (kind: "agent" | "shell") =>
    act({ type: "new_window", workspace: p.name, kind }, { key: `new:${kind}`, quiet: true }).then((r) => {
      if (r.ok && r.window) {
        go({ view: "workspace", name: p.name, window: r.window });
      }
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
      icon={<Show when={isBusy("new:agent") || isBusy("new:shell")} fallback={<Plus size={15} />}><Spinner size={13} /></Show>}
      size="sm"
      class={cls_}
      items={[
        ...(hasAgent() ? [{ label: "Agent", icon: <Bot size={14} />, hint: "c", run: () => open("agent") }] : []),
        { label: "Terminal", icon: <TermIcon size={14} />, hint: "t", run: () => open("shell") },
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
    else if (k === "[" && wins.length) go({ view: "workspace", name: p.name, window: wins[((i < 0 ? wins.length : i) - 1 + wins.length) % wins.length].id });
    else if (k === "c" && hasAgent()) open("agent");
    else if (k === "c") toast(noAgent(), true);
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
              <Show when={!onDiff() && shown()?.kind === "agent" && agentOf(shown())}>
                {(a) => (
                  <div class="mr-1 flex items-center gap-2">
                    <StatusPill status={a().status} title={[a().status === "working" ? (a().tool ?? "Thinking…") : a().status === "waiting" ? a().message : null, a().model].filter(Boolean).join(" · ")} />
                    <Show when={a().status === "waiting" && !a().needs_permission}>
                      <Button size="sm" variant="ghost" class="shrink-0" onClick={() => act({ type: "mark_done", agent: a().key })} title="Stop showing it as waiting (m)">
                        <Done size={13} /> Done
                      </Button>
                    </Show>
                    <div class="flex rounded-lg bg-hover p-0.5 text-sm">
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

          <nav class={cls("flex items-center gap-1.5 border-b border-line bg-panel/60 px-2 py-0.5 sm:px-3", tabsHidden() && "md:hidden")}>
            <div
              ref={tabs}
              class="flex min-w-0 items-center gap-1.5 overflow-x-auto overflow-y-hidden py-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              role="tablist"
              onWheel={wheelTabs}
            >
              <For each={w().windows}>
                {(win, i) => {
                  const a = () => agentOf(win);
                  const active = () => win.id === p.window;
                  const lifted = () => drag()?.id === win.id;
                  return (
                    <div
                      role="tab"
                      aria-selected={active()}
                      data-win={win.id}
                      class={cls(
                        "group relative flex h-8 shrink-0 cursor-pointer select-none items-center gap-2 rounded-lg border pl-2.5 pr-1.5 text-base",
                        active() || lifted()
                          ? cls("border-line-strong bg-raised text-fg", lifted() ? "z-10 cursor-grabbing shadow-lg" : "shadow-soft")
                          : "border-line bg-raised/40 text-fg-2 hover:border-line-strong hover:bg-raised/80 hover:text-fg",
                        !drag() ? "transition-colors" : cls("will-change-transform", !lifted() && "transition-transform duration-150 ease-out"),
                        lifted() && settling() && "transition-transform duration-150 ease-out",
                      )}
                      style={{ transform: drag() ? `translateX(${shiftOf(win.id)}px)` : undefined }}
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
                        class={cls(
                          "grid size-5 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg group-hover:opacity-100",
                          active() ? "opacity-60" : "opacity-0",
                        )}
                        onClick={(e) => {
                          e.stopPropagation();
                          closeWindow(win);
                        }}
                        aria-label="Close window"
                      >
                        <X size={12} />
                      </button>
                      <Show when={active()}>
                        <span class="absolute inset-x-3 -bottom-px h-[2px] rounded-full bg-accent" />
                      </Show>
                    </div>
                  );
                }}
              </For>
              <For each={diffs()}>
                {(d) => {
                  const id = diffId(d);
                  const active = () => p.window === id;
                  return (
                    <div
                      role="tab"
                      aria-selected={active()}
                      class={cls(
                        "group relative flex h-8 shrink-0 cursor-pointer select-none items-center gap-2 rounded-lg border border-dashed pl-2.5 pr-1.5 text-base transition-colors",
                        active() ? "border-line-strong bg-raised text-fg shadow-soft" : "border-line bg-raised/40 text-fg-2 hover:border-line-strong hover:bg-raised/80 hover:text-fg",
                      )}
                      onClick={() => go({ view: "workspace", name: p.name, window: id })}
                      onDblClick={() => pinDiff(p.name, id)}
                      onAuxClick={(e) => e.button === 1 && closeDiff(p.name, id)}
                      title={d.commit ? `${d.commit.short} ${d.commit.title}` : d.title}
                    >
                      <FileDiff size={14} class={active() ? "text-accent" : "text-fg-3"} />
                      <span class={cls("max-w-[16rem] truncate", d.preview && "italic")}>{diffLabel(d)}</span>
                      <button
                        class={cls(
                          "grid size-5 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg group-hover:opacity-100",
                          active() ? "opacity-60" : "opacity-0",
                        )}
                        onClick={(e) => {
                          e.stopPropagation();
                          closeDiff(p.name, id);
                        }}
                        aria-label="Close diff"
                      >
                        <X size={12} />
                      </button>
                      <Show when={active()}>
                        <span class="absolute inset-x-3 -bottom-px h-[2px] rounded-full bg-accent" />
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
              <Show when={diff()} keyed>
                {(d) => (
                  <div
                    class="min-h-0 flex-1"
                    onKeyDown={(e) => {
                      if (e.key !== "Escape" || e.defaultPrevented || (e.target as HTMLElement).closest("input,textarea")) return;
                      e.preventDefault();
                      go({ view: "workspace", name: p.name, window: lastWindow(p.name) ?? null });
                    }}
                  >
                    <DiffPane target={d} onClose={() => closeDiff(p.name, diffId(d))} />
                  </div>
                )}
              </Show>
              <Show when={shown()} fallback={<Show when={!onDiff()}><div class="grid flex-1 place-items-center"><Spinner size={18} /></div></Show>}>
                {(win) => (
                  <div class={cls("flex min-h-0 flex-1 flex-col", onDiff() && "hidden")}>
                    <div class="min-h-0 flex-1">
                      <Show when={mode() === "chat" && agentOf(win())} fallback={<Terminal window={win().id} focus={!onDiff()} />}>
                        {(a) => <Conversation agent={a()} />}
                      </Show>
                    </div>
                  </div>
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
                <SidePanel ws={w()} />
              </div>
            </Show>
          </div>
        </div>
      )}
    </Show>
  );
}
