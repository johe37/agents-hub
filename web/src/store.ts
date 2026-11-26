import { batch, createEffect, createMemo, createRoot, createSignal } from "solid-js";
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
    if (r.view !== "workspace" || !r.window || lastWindows[r.name] === r.window) return;
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

function show() {
  if (raw) setState(reconcile(visible(raw), { key: "key", merge: false }));
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
      raw = msg.data as State;
      prune(raw);
      notifyChanges(raw);
      show();
      setLoaded(true);
    } else if (msg.type === "notice") {
      toast(msg.text, msg.error);
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
export const [composer, setComposer] = createSignal<{ path: string | null; prompt?: string; chat?: boolean } | null>(null);
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
      else if (here.window === w.id) go({ view: "workspace", name: ws.name, window: rest[0].id }, true);
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
/** Text to put in an agent's reply box, to edit before sending. */
export const [draft, setDraft] = createSignal<{ agent: string; text: string } | null>(null);
export const [confirmBox, setConfirmBox] = createSignal<{
  title: string;
  body?: string;
  confirm: string;
  danger?: boolean;
  run: () => void;
} | null>(null);
export const [diffView, setDiffView] = createSignal<{ workspace: string; path: string; file: string | null; title: string; commit?: Commit } | null>(null);

export function openPalette(mode: PaletteMode = "all") {
  setPaletteMode(mode);
  setPaletteOpen(true);
}

export function ask(c: NonNullable<ReturnType<typeof confirmBox>>) {
  setConfirmBox(c);
}
