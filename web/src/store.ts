import { createMemo, createRoot, createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { post, wsURL, type Action, type ActionResult, type Agent, type State } from "./api";
import { STATUS, alive } from "./util";
import { voiceTurn } from "./voice";

const EMPTY: State = {
  workspaces: [],
  agents: [],
  usage: null,
  gitlab: { enabled: false, loading: false, errors: [], reviews: [], authored: [], assigned: [], issues: [] },
  chat_dir: "~/chats",
  home: "",
  warnings: [],
};

export const [state, setState] = createStore<State>(structuredClone(EMPTY));
export const [connection, setConnection] = createSignal<"connecting" | "live" | "offline">("connecting");
export const [loaded, setLoaded] = createSignal(false);

/** A clock for "3m ago" labels; one tick for the whole page. */
export const [now, setNow] = createSignal(Date.now());
setInterval(() => setNow(Date.now()), 1000);

// ---- Routing: the URL always says what is on screen, so Back and reload work.

export type Route =
  | { view: "inbox" }
  | { view: "agents" }
  | { view: "gitlab" }
  | { view: "workspace"; name: string; window: string | null }
  | { view: "agent"; key: string };

function parse(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").map(decodeURIComponent);
  switch (parts[0]) {
    case "agents":
      return { view: "agents" };
    case "gitlab":
      return { view: "gitlab" };
    case "w":
      if (parts[1]) return { view: "workspace", name: parts[1], window: parts[2] || null };
      break;
    case "a":
      if (parts[1]) return { view: "agent", key: parts.slice(1).join("/") };
      break;
  }
  return { view: "inbox" };
}

export function href(r: Route): string {
  switch (r.view) {
    case "inbox":
      return "#/inbox";
    case "agents":
      return "#/agents";
    case "gitlab":
      return "#/gitlab";
    case "workspace":
      return `#/w/${encodeURIComponent(r.name)}${r.window ? "/" + encodeURIComponent(r.window) : ""}`;
    case "agent":
      return `#/a/${encodeURIComponent(r.key)}`;
  }
}

export const [route, setRoute] = createSignal<Route>(parse(location.hash));
window.addEventListener("hashchange", () => setRoute(parse(location.hash)));

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
  setTimeout(() => dismiss(id), error ? 8000 : 3500);
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
      notifyChanges(msg.data as State);
      setState(reconcile(msg.data as State, { key: "key", merge: false }));
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
    if (notifyPermission() !== "granted" || (document.hasFocus() && document.visibilityState === "visible")) continue;
    const body = ask ? (a.message ?? "Waiting for you") : (a.reply?.slice(0, 180) ?? "Finished its turn");
    const n = new Notification(`${ask ? "⏳" : "✓"} ${a.name}: ${a.title}`, { body, tag: a.key, silent: !ask });
    n.onclick = () => {
      window.focus();
      go(ask ? { view: "inbox" } : agentRoute(a));
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
  /** Agents that finished a turn recently, for "ready for review" in the inbox. */
  const finished = createMemo(() =>
    state.agents
      .filter((a) => a.status === "idle" && a.turns > 0 && a.timestamp > 0)
      .sort((a, b) => b.timestamp - a.timestamp),
  );
  return { agentsByKey, waiting, live, byWorkspace, finished };
});

export function sortAgents(list: Agent[]): Agent[] {
  return [...list].sort((a, b) => STATUS[a.status].rank - STATUS[b.status].rank || b.timestamp - a.timestamp);
}

// ---- Theme

export type ThemePref = "system" | "dark" | "light";
const media = window.matchMedia("(prefers-color-scheme: dark)");
function readPref(): ThemePref {
  try {
    const v = localStorage.getItem("theme");
    if (v === "dark" || v === "light") return v;
  } catch {
    /* storage can be blocked */
  }
  return "system";
}
export const [themePref, setThemePref] = createSignal<ThemePref>(readPref());
export const [systemDark, setSystemDark] = createSignal(media.matches);
media.addEventListener("change", (e) => setSystemDark(e.matches));
export const theme = createRoot(() => createMemo(() => (themePref() === "system" ? (systemDark() ? "dark" : "light") : themePref())));
export function cycleTheme() {
  const order: ThemePref[] = ["system", "dark", "light"];
  const next = order[(order.indexOf(themePref()) + 1) % order.length];
  setThemePref(next);
  try {
    localStorage.setItem("theme", next);
  } catch {
    /* ignore */
  }
  toast(`Theme: ${next}`);
}

// ---- Overlays

export const [paletteOpen, setPaletteOpen] = createSignal(false);
export const [paletteMode, setPaletteMode] = createSignal<"all" | "open" | "agent">("all");
export const [helpOpen, setHelpOpen] = createSignal(false);
export const [composer, setComposer] = createSignal<{ path: string | null } | null>(null);
export const [confirmBox, setConfirmBox] = createSignal<{
  title: string;
  body?: string;
  confirm: string;
  danger?: boolean;
  run: () => void;
} | null>(null);
export const [diffView, setDiffView] = createSignal<{ path: string; file: string | null; title: string } | null>(null);

export function openPalette(mode: "all" | "open" | "agent" = "all") {
  setPaletteMode(mode);
  setPaletteOpen(true);
}

export function ask(c: NonNullable<ReturnType<typeof confirmBox>>) {
  setConfirmBox(c);
}
