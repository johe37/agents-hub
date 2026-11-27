import { batch, createEffect, createMemo, createRoot, createSignal, untrack } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { post, wsURL, type Action, type ActionResult, type Agent, type Commit, type State, type Win, type Workspace } from "./api";
import { apply, choiceLabel, isChoice, resolve } from "./themes";
import { notifyFinished, notifyInFocus, notifySound, notifyWaiting } from "./settings";
import { STATUS, alive } from "./util";
import { voiceTurn } from "./voice";

const EMPTY: State = {
  workspaces: [],
  agents: [],
  usage: null,
  agent: { command: "claude", name: "claude", label: "Claude", claude: true, installed: true, known: [] },
  gitlab: { enabled: false, loading: false, errors: [], reviews: [], authored: [], assigned: [], issues: [], todos: [] },
  closed: [],
  chat_dir: "~/chats",
  web_theme: readStored("config-theme") ?? "jugglr",
  home: "",
  warnings: [],
  version: "",
  commit: "",
};

export const [state, setState] = createStore<State>(structuredClone(EMPTY));
export const [connection, setConnection] = createSignal<"connecting" | "live" | "offline">("connecting");
export const [loaded, setLoaded] = createSignal(false);

/** A clock for "3m ago" labels; one tick for the whole page. */
export const [now, setNow] = createSignal(Date.now());
setInterval(() => setNow(Date.now()), 1000);

// ---- Routing: the URL always says what is on screen, so Back and reload work.

export type Route =
  | { view: "agents" }
  | { view: "gitlab"; key: string | null }
  | { view: "workspace"; name: string; window: string | null }
  | { view: "agent"; key: string }
  | { view: "open"; path: string };

function parse(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").map(decodeURIComponent);
  switch (parts[0]) {
    case "agents":
      return { view: "agents" };
    case "gitlab":
      return { view: "gitlab", key: parts.slice(1).join("/") || null };
    case "w":
      if (parts[1]) return { view: "workspace", name: parts[1], window: parts[2] || null };
      break;
    case "a":
      if (parts[1]) return { view: "agent", key: parts.slice(1).join("/") };
      break;
    case "open":
      if (parts[1]) return { view: "open", path: parts.slice(1).join("/") };
      break;
  }
  return { view: "agents" };
}

export function href(r: Route): string {
  switch (r.view) {
    case "agents":
      return "#/agents";
    case "gitlab":
      return r.key ? `#/gitlab/${encodeURIComponent(r.key)}` : "#/gitlab";
    case "workspace":
      return `#/w/${encodeURIComponent(r.name)}${r.window ? "/" + encodeURIComponent(r.window) : ""}`;
    case "agent":
      return `#/a/${encodeURIComponent(r.key)}`;
    case "open":
      return `#/open/${encodeURIComponent(r.path)}`;
  }
}

export const [route, setRoute] = createSignal<Route>(parse(location.hash));
window.addEventListener("hashchange", () => setRoute(parse(location.hash)));

/** The window last shown in each workspace, so coming back opens it again. */
const lastWindows: Record<string, string> = (() => {
  try {
    return JSON.parse(readStored("last-windows") ?? "{}");
  } catch {
    return {};
  }
})();
export const lastWindow = (workspace: string): string | undefined => lastWindows[workspace];
export function forgetLastWindow(workspace: string) {
  if (!(workspace in lastWindows)) return;
  delete lastWindows[workspace];
  writeStored("last-windows", JSON.stringify(lastWindows));
}
createRoot(() =>
  createEffect(() => {
    const r = route();
    if (r.view !== "workspace" || !r.window || isDiff(r.window) || lastWindows[r.name] === r.window) return;
    lastWindows[r.name] = r.window;
    writeStored("last-windows", JSON.stringify(lastWindows));
  }),
);

export function go(r: Route, replace = false) {
  const h = href(r);
  if (location.hash === h) return;
  if (replace) history.replaceState(null, "", h);
  else location.hash = h;
  setRoute(parse(h));
}

/** Where an agent lives: its window in a workspace, or its own page. */
export function agentRoute(a: Agent): Route {
  if (a.workspace && a.window) return { view: "workspace", name: a.workspace, window: a.window };
  return { view: "agent", key: a.key };
}

// ---- Toasts

export interface Toast {
  id: number;
  text: string;
  error: boolean;
  action?: { label: string; run: () => void };
}
export const [toasts, setToasts] = createSignal<Toast[]>([]);
let toastId = 0;

export function toast(text: string, error = false, action?: Toast["action"]) {
  const id = ++toastId;
  setToasts((t) => [...t.slice(-3), { id, text, error, action }]);
  setTimeout(() => dismiss(id), error || action ? 8000 : 3500);
}
export function dismiss(id: number) {
  setToasts((t) => t.filter((x) => x.id !== id));
}

// ---- Actions

/** Run an action and report the result; keyed so the same button cannot fire twice at once. */
export const [busy, setBusy] = createSignal<Set<string>>(new Set());
/** The step a long action is at, such as a checkout that clones first, by its busy key. pct is null when unknown. */
export const [progress, setProgress] = createSignal<Record<string, { step: string; pct: number | null; seen: string[] }>>({});

export function isBusy(key: string) {
  return busy().has(key);
}

export async function act(action: Action, opts: { key?: string; quiet?: boolean } = {}): Promise<ActionResult> {
  const key = opts.key ?? JSON.stringify(action);
  if (busy().has(key)) return { ok: false };
  setBusy((b) => new Set(b).add(key));
  try {
    const r = await post(action);
    if (!r.ok) toast(r.error ?? "Something went wrong", true);
    else if (r.message && !opts.quiet) toast(r.message);
    return r;
  } finally {
    setBusy((b) => {
      const n = new Set(b);
      n.delete(key);
      return n;
    });
    if (key in progress()) setProgress(({ [key]: _, ...rest }) => rest);
  }
}

// ---- Live state

/** What a close removes, hidden at once. An entry stays until the hub's state no longer has it, or the close fails. */
const hidden = { workspaces: new Map<string, { done: boolean }>(), windows: new Map<string, { done: boolean }>() };
let raw: State | null = null;

/** Window order per workspace after a move, shown at once. An entry stays until the hub's state has that order, or the move fails. */
const orders = new Map<string, { ids: string[]; done: boolean }>();

function ordered(w: Workspace): Workspace {
  const o = orders.get(w.name);
  if (!o) return w;
  const rank = (id: string) => {
    const i = o.ids.indexOf(id);
    return i < 0 ? o.ids.length : i;
  };
  return { ...w, windows: [...w.windows].sort((a, b) => rank(a.id) - rank(b.id)) };
}

function visible(s: State): State {
  if (!hidden.workspaces.size && !hidden.windows.size && !orders.size) return s;
  const gone = (ws: string | null, win: string | null) => (ws !== null && hidden.workspaces.has(ws)) || (win !== null && hidden.windows.has(win));
  return {
    ...s,
    workspaces: s.workspaces
      .filter((w) => !hidden.workspaces.has(w.name))
      .map((w) => (w.windows.some((x) => hidden.windows.has(x.id)) ? { ...w, windows: w.windows.filter((x) => !hidden.windows.has(x.id)) } : w))
      .map(ordered),
    agents: s.agents.filter((a) => !(alive(a.status) && gone(a.workspace, a.window))),
  };
}

function prune(s: State) {
  for (const [name, h] of hidden.workspaces) if (h.done && !s.workspaces.some((w) => w.name === name)) hidden.workspaces.delete(name);
  for (const [id, h] of hidden.windows) if (h.done && !s.workspaces.some((w) => w.windows.some((x) => x.id === id))) hidden.windows.delete(id);
  for (const [name, o] of orders) {
    const ids = s.workspaces.find((w) => w.name === name)?.windows.map((x) => x.id) ?? [];
    if (o.done && ids.join() === o.ids.filter((id) => ids.includes(id)).join()) orders.delete(name);
  }
}

const setPart = setState as unknown as (key: keyof State, value: unknown) => void;

/** Only the fields named change; a close or a move hides or reorders workspaces and agents. */
function show(keys: (keyof State)[] = ["workspaces", "agents"]) {
  if (!raw) return;
  const v = visible(raw);
  batch(() => {
    for (const k of keys) setPart(k, reconcile(v[k] as object, { key: "key", merge: false }));
  });
}

/** Run an action that closes a workspace or window, and hide it before the hub answers. `before` runs with the hiding, e.g. to leave the page. */
export async function actHiding(
  action: Action,
  what: { workspace?: string; window?: string },
  opts: { key?: string; quiet?: boolean; before?: () => void } = {},
): Promise<ActionResult> {
  const entries: [Map<string, { done: boolean }>, string, { done: boolean }][] = [];
  if (what.workspace) entries.push([hidden.workspaces, what.workspace, { done: false }]);
  if (what.window) entries.push([hidden.windows, what.window, { done: false }]);
  batch(() => {
    for (const [m, k, h] of entries) m.set(k, h);
    opts.before?.();
    show();
  });
  const r = await act(action, opts);
  for (const [m, k, h] of entries) {
    if (r.ok) h.done = true;
    else if (m.get(k) === h) m.delete(k);
  }
  if (raw) prune(raw);
  show();
  return r;
}

/** Put a window right before another one, or last when before is null, and show the new order before the hub answers. */
export async function moveWindow(workspace: string, window: string, before: string | null): Promise<void> {
  const ids = state.workspaces.find((w) => w.name === workspace)?.windows.map((x) => x.id);
  if (!ids || before === window) return;
  const want = ids.filter((id) => id !== window);
  const at = before === null ? want.length : want.indexOf(before);
  if (at < 0) return;
  want.splice(at, 0, window);
  if (want.join() === ids.join()) return;
  const o = { ids: want, done: false };
  orders.set(workspace, o);
  show();
  const r = await act({ type: "move_window", window, before }, { quiet: true });
  if (orders.get(workspace) !== o) return;
  if (!r.ok) orders.delete(workspace);
  else {
    o.done = true;
    // A move made elsewhere at the same time would otherwise keep this order on screen.
    setTimeout(() => orders.get(workspace) === o && (orders.delete(workspace), show()), 5000);
  }
  if (raw) prune(raw);
  show();
}

let socket: WebSocket | null = null;
let retry = 0;
const seen = new Map<string, string>();

function connect() {
  setConnection(retry === 0 ? "connecting" : "offline");
  const ws = new WebSocket(wsURL("/api/events"));
  socket = ws;
  ws.onopen = () => {
    retry = 0;
    setConnection("live");
  };
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data as string);
    if (msg.type === "state" && msg.data) {
      // The hub sends only the fields that changed since its last message on this socket.
      const data = msg.data as Partial<State>;
      raw = { ...(raw ?? EMPTY), ...data };
      prune(raw);
      if (data.agents) notifyChanges(raw);
      show(Object.keys(data) as (keyof State)[]);
      setLoaded(true);
    } else if (msg.type === "notice") {
      toast(msg.text, msg.error);
    } else if (msg.type === "progress" && busy().has(msg.key)) {
      setProgress((p) => {
        const seen = p[msg.key]?.seen ?? [];
        return { ...p, [msg.key]: { step: msg.step, pct: msg.pct ?? null, seen: seen.includes(msg.step) ? seen : [...seen, msg.step] } };
      });
    }
  };
  ws.onclose = () => {
    if (socket !== ws) return;
    setConnection("offline");
    retry++;
    setTimeout(connect, Math.min(5000, 300 * 2 ** Math.min(retry, 4)));
  };
}
connect();

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && socket?.readyState === WebSocket.OPEN) socket.send("refresh");
});

// ---- Browser notifications for agents that start waiting for you

export const [notifyPermission, setNotifyPermission] = createSignal<NotificationPermission>(
  typeof Notification === "undefined" ? "denied" : Notification.permission,
);

export async function enableNotifications() {
  if (typeof Notification === "undefined") return toast("This browser has no notifications", true);
  const p = await Notification.requestPermission();
  setNotifyPermission(p);
  if (p === "granted") toast("Notifications on: you hear from agents that need you");
  else toast("Notifications are blocked in the browser settings", true);
}

function notifyChanges(next: State) {
  const first = seen.size === 0 && !loaded();
  for (const a of next.agents) {
    const before = seen.get(a.key);
    seen.set(a.key, a.status);
    if (first || before === a.status) continue;
    const ask = a.status === "waiting";
    const finished = a.status === "idle" && before === "working";
    if (!ask && !finished) continue;
    voiceTurn(a);
    if (notifyPermission() !== "granted" || !(ask ? notifyWaiting() : notifyFinished())) continue;
    if (!notifyInFocus() && document.hasFocus() && document.visibilityState === "visible") continue;
    const body = ask ? (a.message ?? "Waiting for you") : (a.reply?.slice(0, 180) ?? "Finished its turn");
    const n = new Notification(`${ask ? "⏳" : "✓"} ${a.name}: ${a.title}`, { body, tag: a.key, silent: !ask || !notifySound() });
    n.onclick = () => {
      window.focus();
      go(ask ? { view: "agents" } : agentRoute(a));
      n.close();
    };
  }
}

/** ~/x for paths under home, in any text such as a command line. */
export function tilde(text: string): string {
  return state.home ? text.split(state.home + "/").join("~/") : text;
}

// ---- Derived data

export const store = createRoot(() => {
  const agentsByKey = createMemo(() => new Map(state.agents.map((a) => [a.key, a])));
  const waiting = createMemo(() => state.agents.filter((a) => a.status === "waiting"));
  const live = createMemo(() => state.agents.filter((a) => alive(a.status)));
  const byWorkspace = createMemo(() => {
    const m = new Map<string, Agent[]>();
    for (const a of state.agents) {
      if (!a.workspace || !alive(a.status)) continue;
      const l = m.get(a.workspace) ?? [];
      l.push(a);
      m.set(a.workspace, l);
    }
    return m;
  });
  return { agentsByKey, waiting, live, byWorkspace };
});

export function sortAgents(list: Agent[]): Agent[] {
  return [...list].sort((a, b) => STATUS[a.status].rank - STATUS[b.status].rank || b.timestamp - a.timestamp);
}

// ---- Theme

export function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null; // storage can be blocked
  }
}
export function writeStored(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* ignore */
  }
}

/** Before themes, "theme" held "dark" or "light" for the two Jugglr palettes. */
function readChoice(): string | null {
  const v = readStored("theme");
  if (v === "dark" || v === "light") return `jugglr-${v}`;
  return isChoice(v) ? v : null;
}

const media = window.matchMedia("(prefers-color-scheme: dark)");
const [systemDark, setSystemDark] = createSignal(media.matches);
media.addEventListener("change", (e) => setSystemDark(e.matches));

/** This browser's pick; null follows web_theme in config.toml. */
export const [themeChoice, setThemeChoice] = createSignal<string | null>(readChoice());
/** The theme under the cursor in the theme picker, shown before you pick it. */
export const [themePreview, setThemePreview] = createSignal<string | null>(null);
export const configTheme = () => (isChoice(state.web_theme) ? state.web_theme : "jugglr");
export const theme = createRoot(() => {
  const t = createMemo(() => resolve(themePreview() ?? themeChoice() ?? configTheme(), systemDark()));
  createEffect(() => apply(t()));
  createEffect(() => writeStored("config-theme", state.web_theme));
  return t;
});

export function pickTheme(choice: string | null) {
  setThemeChoice(choice);
  writeStored("theme", choice);
  toast(`Theme: ${choiceLabel(choice ?? configTheme())}${choice ? "" : " (from config)"}`);
}

/** Go to the workspace for path, or to the page that opens one with what you pick. */
export async function openDir(path: string) {
  const r = await act({ type: "open_workspace", path }, { key: `open:${path}`, quiet: true });
  if (!r.ok) return;
  if (r.workspace) go({ view: "workspace", name: r.workspace, window: null });
  else go({ view: "open", path: r.path ?? path });
}

/** What closing the window would stop that Undo cannot bring back, if anything. */
export function stops(w: Win): string | null {
  if (w.kind === "agent") {
    const a = w.agent ? store.agentsByKey().get(w.agent) : undefined;
    return a?.status === "working" ? `“${a.title}” is working` : null;
  }
  if (w.kind === "editor") return `${w.command} in ${w.name} may have unsaved changes`;
  if (w.kind === "process") return `${w.command} runs in ${w.name}`;
  return w.running ? `${w.running} runs in ${w.name}` : null;
}

/** Close the workspace; it goes to the recently closed ones, so Undo opens it again. Ask first only when something runs. */
export function closeWorkspace(w: Workspace) {
  const g = w.git;
  if (g?.base) {
    const fate = g.base_ahead ? `${g.branch} keeps its ${g.base_ahead} unmerged commit${g.base_ahead === 1 ? "" : "s"}.` : `${g.branch} is merged into ${g.base}, so the branch goes too.`;
    return ask({
      title: `Close “${w.name}” and remove its worktree?`,
      body: `${fate} Uncommitted changes stop it. Undo cannot bring the worktree back.`,
      confirm: "Close and remove",
      danger: true,
      run: async () => {
        const before = () => {
          const here = route();
          if (here.view === "workspace" && here.name === w.name) go({ view: "agents" });
        };
        await actHiding({ type: "close_workspace", workspace: w.name }, { workspace: w.name }, { before });
      },
    });
  }
  const running = w.windows.map(stops).filter((x): x is string => x !== null);
  if (!running.length) return closeNow(w);
  ask({
    title: `Close workspace “${w.name}”?`,
    body: `${running.join(". ")}. That stops, and Undo cannot start it again.`,
    confirm: "Close workspace",
    danger: true,
    run: () => closeNow(w),
  });
}

async function closeNow(w: Workspace) {
  const before = () => {
    const here = route();
    if (here.view === "workspace" && here.name === w.name) go({ view: "agents" });
  };
  const r = await actHiding({ type: "close_workspace", workspace: w.name }, { workspace: w.name }, { quiet: true, before });
  if (!r.ok) return;
  forgetLastWindow(w.name);
  const undo = () => {
    const c = state.closed.find((c) => c.path === w.path);
    if (c) reopen(c.id);
    else toast(`${w.name} is not among the recently closed`, true);
  };
  toast(r.message ?? `Closed ${w.name}`, false, { label: "Undo", run: undo });
}

export async function reopen(id: string) {
  const r = await act({ type: "reopen_workspace", id }, { key: `reopen:${id}` });
  if (r.ok && r.workspace) go({ view: "workspace", name: r.workspace, window: null });
}

// ---- Overlays

export const [paletteOpen, setPaletteOpen] = createSignal(false);
export type PaletteMode = "all" | "open" | "agent" | "theme";
export const [paletteMode, setPaletteMode] = createSignal<PaletteMode>("all");
export const [helpOpen, setHelpOpen] = createSignal(false);
export const [settingsOpen, setSettingsOpen] = createSignal(false);
/** Ctrl . arms the prefix: the next key acts as a shortcut, even in a terminal or an input.
 *  The value is the chord typed so far ("" at first, "g" after g); null when not armed. */
export const [prefix, setPrefix] = createSignal<string | null>(null);
type Composer = { path: string | null; prompt?: string; chat?: boolean };
const [composer, openComposer] = createSignal<Composer | null>(null);
export { composer };
export const hasAgent = () => state.agent.installed;
/** Claude, Codex, …: for buttons and hints. */
export const agentLabel = () => state.agent.label;
export const noAgent = () => `${state.agent.name} is not installed: there is no ${state.agent.name} in PATH`;
export function setComposer(c: Composer | null) {
  if (c && !hasAgent()) return void toast(noAgent(), true);
  openComposer(c);
}
export const newChat = () => setComposer({ path: state.chat_dir, chat: true });
/** The workspace of chat_dir, for questions that belong to no repo. */
export const chatWorkspace = () => state.workspaces.find((w) => w.path === state.chat_dir);

/** Close a chat's window. Ask first only when Claude works. */
export function closeChat(ws: Workspace, w: Win) {
  const run = () => {
    // tmux closes the session with its last window.
    const what = ws.windows.length > 1 ? { window: w.id } : { window: w.id, workspace: ws.name };
    const before = () => {
      const here = route();
      if (here.view === "workspace" && here.name === ws.name && here.window === w.id) go({ view: "agents" });
    };
    void actHiding({ type: "close_window", window: w.id }, what, { quiet: true, before });
  };
  const stopped = stops(w);
  if (!stopped) return run();
  const a = w.agent ? store.agentsByKey().get(w.agent) : undefined;
  ask({
    title: `Close chat “${a?.title ?? w.name}”?`,
    body: "It stops in the middle of its turn. You can resume the session later from the agent list.",
    confirm: "Close",
    danger: true,
    run,
  });
}
export function closeWindow(ws: Workspace, w: Win) {
  const run = () => {
    const rest = ws.windows.filter((x) => x.id !== w.id);
    // tmux closes the session with its last window.
    const what = rest.length ? { window: w.id } : { window: w.id, workspace: ws.name };
    const before = () => {
      const here = route();
      if (here.view !== "workspace" || here.name !== ws.name) return;
      if (!rest.length) go({ view: "agents" });
      else if (here.window === w.id) go({ view: "workspace", name: ws.name, window: backTo(ws.name, w.id) ?? rest[0].id }, true);
    };
    void actHiding({ type: "close_window", window: w.id }, what, { quiet: true, before });
  };
  const stopped = stops(w);
  if (!stopped) return run();
  const a = w.agent ? store.agentsByKey().get(w.agent) : undefined;
  ask({
    title: a ? `Close agent “${a.title}”?` : `Close ${w.name}?`,
    body: a ? "It stops in the middle of its turn. You can resume the session later from the agent list." : `${stopped}. It stops.`,
    confirm: "Close",
    danger: true,
    run,
  });
}
/** Prompts sent from the page that the transcript does not show yet, per agent. */
export type Sent = { id: number; text: string; at: number };
export const [sent, setSent] = createSignal<Record<string, Sent[]>>({});
let sentId = 0;
/** Same as the server, which sends one paragraph. */
export const oneLine = (t: string) => t.trim().split(/\s+/).join(" ");
export function markSent(agent: string, text: string) {
  const s = { id: ++sentId, text: oneLine(text), at: Date.now() };
  setSent((m) => ({ ...m, [agent]: [...(m[agent] ?? []), s] }));
  // A prompt the transcript never shows as typed, such as a slash command, must not wait forever.
  setTimeout(() => forgetSent(agent, [s.id]), 20_000);
  return s.id;
}
export function forgetSent(agent: string, ids: number[]) {
  setSent((m) => {
    const left = (m[agent] ?? []).filter((s) => !ids.includes(s.id));
    if (left.length === (m[agent] ?? []).length) return m;
    return { ...m, [agent]: left };
  });
}
/** Text to put in an agent's reply box, to edit before sending. */
export const [draft, setDraft] = createSignal<{ agent: string; text: string } | null>(null);
export const [confirmBox, setConfirmBox] = createSignal<{
  title: string;
  body?: string;
  confirm: string;
  danger?: boolean;
  /** Options to pick one of before confirming; the first is chosen at the start. */
  choices?: string[];
  /** A text field; run gets its text, and confirming needs some. note says what the text leads to. */
  input?: { placeholder: string; note?: (text: string) => string };
  run: (choice?: string) => void;
} | null>(null);
/** Chat or terminal for each agent window you switched; the others follow agentView. */
export const [modes, setModes] = createSignal<Record<string, "chat" | "terminal">>({});

/** agent is the Claude to ask about the diff, when it came from one; else the workspace's Claude. */
/** A preview tab is replaced by the next preview opened in its workspace, until you pin it. */
/** stage limits an uncommitted diff to one side of the index; without it the diff is against HEAD. */
export type DiffTarget = { workspace: string; path: string; file: string | null; title: string; commit?: Commit; agent?: string; preview?: boolean; stage?: "staged" | "unstaged" };
/** A diff for an agent without a workspace: it opens over everything. */
export const [diffView, setDiffView] = createSignal<DiffTarget | null>(null);

/** A diff tab's window id in the URL. tmux window ids start with @, so it never clashes. */
export const diffId = (d: DiffTarget) => `diff:${d.commit?.sha ?? d.stage ?? "HEAD"}${d.file ? ":" + d.file : ""}`;
export const isDiff = (window: string | null) => !!window?.startsWith("diff:");
/** The diff tabs of each workspace, after its windows. */
export const [diffTabs, setDiffTabs] = createSignal<Record<string, DiffTarget[]>>(
  (() => {
    try {
      const t: Record<string, DiffTarget | DiffTarget[]> = JSON.parse(readStored("diff-tabs") ?? "{}");
      // Older versions kept one diff per workspace.
      return Object.fromEntries(Object.entries(t).map(([k, v]) => [k, Array.isArray(v) ? v : [v]]));
    } catch {
      return {};
    }
  })(),
);
createRoot(() => createEffect(() => writeStored("diff-tabs", JSON.stringify(diffTabs()))));

export function openDiff(d: DiffTarget, preview = false) {
  if (!state.workspaces.some((w) => w.name === d.workspace)) return setDiffView(d);
  const id = diffId(d);
  setDiffTabs((t) => {
    const list = t[d.workspace] ?? [];
    const at = list.findIndex((x) => diffId(x) === id);
    if (at >= 0) return { ...t, [d.workspace]: list.map((x, i) => (i === at ? { ...d, preview: (preview && x.preview) || undefined } : x)) };
    const tab = { ...d, preview: preview || undefined };
    const old = preview ? list.findIndex((x) => x.preview) : -1;
    return { ...t, [d.workspace]: old < 0 ? [...list, tab] : list.map((x, i) => (i === old ? tab : x)) };
  });
  go({ view: "workspace", name: d.workspace, window: id });
}

export function pinDiff(workspace: string, id: string) {
  setDiffTabs((t) => {
    const list = t[workspace] ?? [];
    if (!list.some((x) => x.preview && diffId(x) === id)) return t;
    return { ...t, [workspace]: list.map((x) => (diffId(x) === id ? { ...x, preview: undefined } : x)) };
  });
}

export function diffLabel(d: DiffTarget) {
  const name = d.file ? d.file.replace(/\/$/, "").split("/").pop()! : null;
  if (d.commit) return name ? `${name} @ ${d.commit.short}` : d.commit.short;
  if (d.stage === "staged") return `${name ?? "All changes"} (staged)`;
  return name ?? "All changes";
}

/** The diff tab and file each editor window was opened from, to go back to when the window closes. */
type From = { diff: string; file: string; ts: number };
const [openedFrom, setOpenedFrom] = createSignal<Record<string, From>>(
  (() => {
    try {
      return JSON.parse(readStored("opened-from") ?? "{}");
    } catch {
      return {};
    }
  })(),
);
createRoot(() => {
  createEffect(() => writeStored("opened-from", JSON.stringify(openedFrom())));
  // tmux reuses window ids after a restart, so a closed window's entry must go. A new window is not listed
  // at once: give it time first.
  createEffect(() => {
    if (!loaded()) return;
    const live = new Set(state.workspaces.flatMap((w) => w.windows.map((x) => x.id)));
    const old = Date.now() - 30_000;
    const all = untrack(openedFrom);
    const keep = Object.entries(all).filter(([w, f]) => live.has(w) || f.ts > old);
    if (keep.length < Object.keys(all).length) setOpenedFrom(Object.fromEntries(keep));
  });
});
export const returnTo = (window: string, diff: string, file: string) => setOpenedFrom((m) => ({ ...m, [window]: { diff, file, ts: Date.now() } }));
/** The diff tab that window was opened from, while that tab is still open. */
export function backTo(workspace: string, window: string | null | undefined): string | undefined {
  const id = window ? openedFrom()[window]?.diff : undefined;
  return id && diffTabs()[workspace]?.some((d) => diffId(d) === id) ? id : undefined;
}
/** The open windows of workspace that edit file from the diff tab diff. */
export function editorsOf(workspace: string, diff: string, file: string): Win[] {
  const from = openedFrom();
  const ws = state.workspaces.find((w) => w.name === workspace);
  return ws?.windows.filter((x) => from[x.id]?.diff === diff && from[x.id]?.file === file) ?? [];
}

/** Close a diff tab. Closing the one shown shows the diff next to it, else the window you came from. */
export function closeDiff(workspace: string, id: string) {
  const list = diffTabs()[workspace] ?? [];
  const at = list.findIndex((x) => diffId(x) === id);
  if (at < 0) return;
  const rest = list.filter((_, i) => i !== at);
  setDiffTabs(({ [workspace]: _, ...others }) => (rest.length ? { ...others, [workspace]: rest } : others));
  const r = route();
  if (r.view !== "workspace" || r.name !== workspace || r.window !== id) return;
  const next = rest[Math.min(at, rest.length - 1)];
  go({ view: "workspace", name: workspace, window: next ? diffId(next) : (lastWindow(workspace) ?? null) }, true);
}

export function openPalette(mode: PaletteMode = "all") {
  setPaletteMode(mode);
  setPaletteOpen(true);
}

export function ask(c: NonNullable<ReturnType<typeof confirmBox>>) {
  setConfirmBox(c);
}

/** The clipboard API needs a secure page; the page is often plain http on the LAN. */
export async function copy(text: string, what: string) {
  try {
    if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(text);
    else {
      const t = document.createElement("textarea");
      t.value = text;
      t.style.position = "fixed";
      t.style.opacity = "0";
      document.body.append(t);
      t.select();
      const ok = document.execCommand("copy");
      t.remove();
      if (!ok) throw new Error();
    }
    toast(`Copied ${what}`);
  } catch {
    toast(`Could not copy ${what}`, true);
  }
}
