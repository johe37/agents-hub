import { For, Show, createEffect, createRoot, createSignal, onCleanup, onMount } from "solid-js";
import type { Agent, Status, Win, Workspace } from "../api";
import { getJSON } from "../api";
import { agentRoute, go, now, state, theme } from "../store";
import { STATUS, age, alive, cls, worst } from "../util";

interface Sat {
  key: string;
  label: string;
  win: Win | null;
  agent: Agent | null;
  orbit: 0 | 1;
  slot: number;
  of: number;
  x: number;
  y: number;
}

interface Node {
  key: string;
  name: string;
  ws: Workspace | null;
  status: Status | null;
  sats: Sat[];
  x: number;
  y: number;
  tx: number;
  ty: number;
  r: number;
  phase: [number, number];
  born: number;
}

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  max: number;
  color: string;
  size: number;
}

interface Beam {
  sat: string;
  node: string;
  t: number;
  color: string;
  label: string;
}

interface Label {
  node: string;
  text: string;
  life: number;
  dy: number;
  color: string;
}

interface Ring {
  x: number;
  y: number;
  t: number;
  color: string;
}

export interface FeedEntry {
  id: string;
  ts: number;
  source: "claude" | "shell" | "status";
  where: string;
  text: string;
  status?: Status;
  route: Parameters<typeof go>[0] | null;
}

const ELSEWHERE = "~elsewhere";
const MAX_FEED = 60;

const [statusFeed, setStatusFeed] = createSignal<FeedEntry[]>([]);
const [commandFeed, setCommandFeed] = createSignal<FeedEntry[]>([]);
let feedId = 0;
const feed = () => [...commandFeed(), ...statusFeed()].sort((a, b) => b.ts - a.ts).slice(0, MAX_FEED);

interface LoggedCommand {
  time: number;
  source: "claude" | "shell";
  text: string;
  cwd?: string;
  agent?: string | null;
  workspace?: string | null;
  window?: string | null;
  window_name?: string;
  title?: string;
}

function fromLog(c: LoggedCommand, i: number): FeedEntry {
  const id = `${c.time}-${i}`;
  if (c.source === "shell") {
    return { id, ts: c.time, source: "shell", where: `${c.workspace} · ${c.window_name}`, text: c.text, route: c.workspace ? { view: "workspace", name: c.workspace, window: c.window ?? null } : null };
  }
  const agent = c.agent ? state.agents.find((a) => a.key === c.agent) : undefined;
  return { id, ts: c.time, source: "claude", where: [c.workspace ?? c.cwd?.replace(state.home, "~"), c.title].filter(Boolean).join(" · "), text: c.text, route: agent ? agentRoute(agent) : null };
}

let lastFetch = 0;
let fetchTimer = 0;
/** The server log has every command, also the ones that start and end between two state updates. */
function fetchCommands() {
  clearTimeout(fetchTimer);
  const wait = Math.max(0, lastFetch + 3000 - Date.now());
  fetchTimer = window.setTimeout(async () => {
    lastFetch = Date.now();
    try {
      const list = await getJSON<LoggedCommand[]>(`/api/commands?limit=${MAX_FEED}`);
      setCommandFeed(list.map(fromLog));
    } catch {
      // The next state update tries again.
    }
  }, wait);
}

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function rgba(hex: string, a: number): string {
  if (!/^#[0-9a-f]{6}$/i.test(hex)) return hex;
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

function shortTool(tool: string): string {
  const i = tool.indexOf(": ");
  if (i < 0) return tool;
  const name = tool.slice(0, i);
  let arg = tool.slice(i + 2);
  if (/^(Read|Edit|Write|MultiEdit|NotebookEdit)$/.test(name)) arg = arg.split("/").pop() ?? arg;
  if (arg.length > 34) arg = arg.slice(0, 33) + "…";
  return `${name} ${arg}`;
}

function agentFor(w: Workspace, win: Win): Agent | null {
  return state.agents.find((a) => (win.agent && a.key === win.agent) || (a.workspace === w.name && a.window === win.id)) ?? null;
}

function buildNodes(prev: Map<string, Node>, t: number): Node[] {
  const nodes: Node[] = [];
  for (const w of state.workspaces) {
    const sats: Sat[] = w.windows.map((win) => {
      const agent = win.kind === "agent" ? agentFor(w, win) : null;
      return { key: `${w.name}/${win.id}`, label: win.name, win, agent, orbit: agent ? 0 : 1, slot: 0, of: 0, x: 0, y: 0 };
    });
    nodes.push(node(prev, t, w.name, w.name, w, sats));
  }
  const loose = state.agents.filter((a) => alive(a.status) && !a.workspace);
  if (loose.length) {
    const sats: Sat[] = loose.map((a) => ({ key: `${ELSEWHERE}/${a.key}`, label: a.title || a.name, win: null, agent: a, orbit: 0, slot: 0, of: 0, x: 0, y: 0 }));
    nodes.push(node(prev, t, ELSEWHERE, "elsewhere", null, sats));
  }
  for (const n of nodes) {
    for (const o of [0, 1] as const) {
      const ring = n.sats.filter((s) => s.orbit === o);
      ring.forEach((s, i) => ((s.slot = i), (s.of = ring.length)));
    }
  }
  // Busy workspaces sit closest to the hub.
  const weight = (n: Node) => (n.status ? STATUS[n.status].rank : 9);
  nodes.sort((a, b) => weight(a) - weight(b) || a.name.localeCompare(b.name));
  return nodes;
}

function node(prev: Map<string, Node>, t: number, key: string, name: string, ws: Workspace | null, sats: Sat[]): Node {
  const old = prev.get(key);
  const status = worst(sats.flatMap((s) => (s.agent ? [s.agent] : [])));
  for (const s of sats) {
    const p = old?.sats.find((o) => o.key === s.key);
    if (p) ((s.x = p.x), (s.y = p.y));
  }
  return {
    key,
    name,
    ws,
    status,
    sats,
    x: old?.x ?? 0,
    y: old?.y ?? 0,
    tx: 0,
    ty: 0,
    r: old?.r ?? 0,
    phase: old?.phase ?? [Math.random() * Math.PI * 2, Math.random() * Math.PI * 2],
    born: old?.born ?? t,
  };
}

function layout(nodes: Node[], w: number, h: number) {
  const cx = w / 2;
  const cy = h / 2;
  const n = nodes.length;
  const span = Math.min(w, h) / 2 - 24;
  // A sunflower spiral keeps any number of workspaces evenly spaced and leaves the centre for the hub.
  const step = span / Math.sqrt(n + 1.2);
  const r = Math.max(18, Math.min(64, step * 0.42));
  const aspect = w / h;
  nodes.forEach((node, i) => {
    const d = step * Math.sqrt(i + 1.2);
    const a = i * 2.399963 - Math.PI / 2;
    node.tx = cx + Math.cos(a) * d * Math.min(1.5, Math.max(1, aspect * 0.85));
    node.ty = cy + Math.sin(a) * d;
    node.r = r;
    if (!node.x && !node.y) ((node.x = cx), (node.y = cy));
  });
}

function satAt(prev: Map<string, Node>, key: string): Sat | undefined {
  for (const n of prev.values()) {
    const s = n.sats.find((x) => x.key === key);
    if (s) return s;
  }
}

function where(a: Agent): string {
  return `${a.workspace ?? a.short_path} · ${a.title || a.name}`;
}

function firstLine(text: string | null): string | null {
  const line = text?.split("\n").find((l) => l.trim())?.trim();
  if (!line) return null;
  return line.length > 140 ? line.slice(0, 139) + "…" : line;
}

function statusText(a: Agent): string {
  if (a.status === "waiting") return a.message ?? "Needs you";
  if (a.status === "idle") return `Done: ${firstLine(a.reply) ?? "finished its turn"}`;
  return STATUS[a.status].label;
}

function log(e: Omit<FeedEntry, "id" | "ts">) {
  setStatusFeed((f) => [{ ...e, id: `s${++feedId}`, ts: Date.now() / 1000 }, ...f].slice(0, MAX_FEED));
}

type Signal = { kind: "claude" | "shell"; sat: string; node: string; label: string } | { kind: "status"; sat: string; color: string };
const listeners = new Set<(e: Signal) => void>();
const emit = (e: Signal) => listeners.forEach((l) => l(e));

// Runs for the whole session, so status changes from before the view opened are in the feed.
createRoot(() => {
  const seen = { tool: new Map<string, string | null>(), status: new Map<string, Status>(), running: new Map<string, string | null>() };
  let primed = false;
  createEffect(() => {
    for (const a of state.agents) {
      const prevStatus = seen.status.get(a.key);
      const sat = a.workspace && a.window ? `${a.workspace}/${a.window}` : `${ELSEWHERE}/${a.key}`;
      if (primed && a.tool && a.tool !== seen.tool.get(a.key) && alive(a.status)) {
        emit({ kind: "claude", sat, node: a.workspace ?? ELSEWHERE, label: shortTool(a.tool) });
      }
      if (primed && prevStatus && prevStatus !== a.status && (a.status === "waiting" || a.status === "idle" || a.status === "ended")) {
        if (a.status !== "ended") emit({ kind: "status", sat, color: STATUS[a.status].color });
        log({ source: "status", where: where(a), text: statusText(a), status: a.status, route: agentRoute(a) });
      }
      seen.tool.set(a.key, a.tool);
      seen.status.set(a.key, a.status);
    }
    for (const w of state.workspaces) {
      for (const win of w.windows) {
        if (win.kind === "agent") continue;
        const k = `${w.name}/${win.id}`;
        if (primed && win.running && win.running !== seen.running.get(k)) {
          emit({ kind: "shell", sat: k, node: w.name, label: win.running.slice(0, 36) });
        }
        seen.running.set(k, win.running);
      }
    }
    if (state.workspaces.length || state.agents.length) primed = true;
    if (listeners.size) fetchCommands();
  });
});

/** "working" from "var(--working)", resolved against the theme. */
const resolveColor = (c: string, v: Record<string, string>) => (c.startsWith("var(--") ? (v[c.slice(6, -1)] ?? v.fg) : c);

/** Futuristic live map of every workspace, its windows and its agents, with a feed of what they run. */
export function Mission() {
  let canvas!: HTMLCanvasElement;
  let wrap!: HTMLDivElement;
  const nodes = new Map<string, Node>();
  const particles: Particle[] = [];
  const beams: Beam[] = [];
  const labels: Label[] = [];
  const rings: Ring[] = [];
  const stars = Array.from({ length: 180 }, () => ({ x: Math.random(), y: Math.random(), z: Math.random(), p: Math.random() * 6 }));
  const [hover, setHover] = createSignal<{ x: number; y: number; node: Node; sat: Sat | null } | null>(null);
  let size = { w: 0, h: 0 };
  let mouse = { x: -1, y: -1 };

  createEffect(() => {
    const next = buildNodes(nodes, performance.now() / 1000);
    nodes.clear();
    for (const n of next) nodes.set(n.key, n);
    layout(next, size.w, size.h);
  });

  const onSignal = (e: Signal) => {
    const v = theme().vars;
    if (e.kind === "status") {
      const s = satAt(nodes, e.sat);
      if (s) rings.push({ x: s.x, y: s.y, t: 0, color: e.color });
      return;
    }
    beams.push({ sat: e.sat, node: e.node, t: 0, color: e.kind === "claude" ? v.working : v.accent, label: e.label });
  };
  listeners.add(onSignal);
  fetchCommands();
  onCleanup(() => listeners.delete(onSignal));

  const pick = (x: number, y: number): { node: Node; sat: Sat | null } | null => {
    for (const n of nodes.values()) {
      for (const s of n.sats) if (Math.hypot(s.x - x, s.y - y) < 11) return { node: n, sat: s };
    }
    for (const n of nodes.values()) if (Math.hypot(n.x - x, n.y - y) < n.r * 0.55) return { node: n, sat: null };
    return null;
  };

  const open = (n: Node, s: Sat | null) => {
    if (s?.agent && !s.win) return go(agentRoute(s.agent));
    if (n.ws) go({ view: "workspace", name: n.ws.name, window: s?.win?.id ?? null });
  };

  onMount(() => {
    const g = canvas.getContext("2d")!;
    const ro = new ResizeObserver(() => {
      const r = wrap.getBoundingClientRect();
      size = { w: r.width, h: r.height };
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(r.width * dpr);
      canvas.height = Math.round(r.height * dpr);
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      layout([...nodes.values()], size.w, size.h);
    });
    ro.observe(wrap);

    let last = performance.now();
    let raf = 0;
    let packetClock = 0;
    const font = getComputedStyle(document.documentElement).getPropertyValue("--font-mono") || "monospace";

    const frame = (ms: number) => {
      raf = requestAnimationFrame(frame);
      const slow = reducedMotion() ? 0.15 : 1;
      const dt = Math.min(0.05, (ms - last) / 1000) * slow;
      last = ms;
      const t = ms / 1000;
      const v = theme().vars;
      const dark = theme().mode === "dark";
      const { w, h } = size;
      if (!w || !h) return;
      const cx = w / 2;
      const cy = h / 2;
      const span = Math.min(w, h) / 2;

      g.globalCompositeOperation = "source-over";
      g.fillStyle = v.bg;
      g.fillRect(0, 0, w, h);
      const vignette = g.createRadialGradient(cx, cy, span * 0.1, cx, cy, Math.max(w, h) * 0.75);
      vignette.addColorStop(0, rgba(v.accent, dark ? 0.07 : 0.05));
      vignette.addColorStop(1, rgba(v.bg, 0));
      g.fillStyle = vignette;
      g.fillRect(0, 0, w, h);

      for (const s of stars) {
        const tw = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * (0.6 + s.z) + s.p));
        g.globalAlpha = (dark ? 0.55 : 0.3) * tw * (0.3 + s.z * 0.7);
        g.fillStyle = v.fg;
        const sx = ((s.x * w + t * 3 * s.z) % w + w) % w;
        g.fillRect(sx, s.y * h, s.z > 0.85 ? 1.6 : 1, s.z > 0.85 ? 1.6 : 1);
      }
      g.globalAlpha = 1;

      // Polar grid and a radar sweep around the hub.
      g.strokeStyle = rgba(v.accent, dark ? 0.09 : 0.12);
      g.lineWidth = 1;
      for (let i = 1; i <= 4; i++) {
        g.beginPath();
        g.arc(cx, cy, (span * i) / 4, 0, Math.PI * 2);
        g.stroke();
      }
      for (let i = 0; i < 12; i++) {
        const a = (i / 12) * Math.PI * 2;
        g.beginPath();
        g.moveTo(cx + Math.cos(a) * span * 0.12, cy + Math.sin(a) * span * 0.12);
        g.lineTo(cx + Math.cos(a) * span, cy + Math.sin(a) * span);
        g.stroke();
      }
      const sweep = (t * 0.6 * slow) % (Math.PI * 2);
      if ("createConicGradient" in g) {
        const cg = g.createConicGradient(sweep - 0.9, cx, cy);
        cg.addColorStop(0, rgba(v.accent, 0));
        cg.addColorStop(0.143, rgba(v.accent, dark ? 0.16 : 0.12));
        cg.addColorStop(0.1431, rgba(v.accent, 0));
        cg.addColorStop(1, rgba(v.accent, 0));
        g.fillStyle = cg;
        g.beginPath();
        g.arc(cx, cy, span, 0, Math.PI * 2);
        g.fill();
      }

      const list = [...nodes.values()];
      for (const n of list) {
        n.x += (n.tx - n.x) * Math.min(1, dt * 3);
        n.y += (n.ty - n.y) * Math.min(1, dt * 3);
        const working = n.status === "working" || n.status === "polling";
        n.phase[0] += dt * (working ? 0.9 : n.status === "waiting" ? 0.05 : 0.18);
        n.phase[1] -= dt * 0.07;
        const orbits = [n.r * 0.95, n.r * 1.35];
        for (const s of n.sats) {
          const a = n.phase[s.orbit] + (s.slot / Math.max(1, s.of)) * Math.PI * 2;
          const ox = n.x + Math.cos(a) * orbits[s.orbit];
          const oy = n.y + Math.sin(a) * orbits[s.orbit] * 0.62;
          if (!s.x && !s.y) ((s.x = ox), (s.y = oy));
          s.x += (ox - s.x) * Math.min(1, dt * 8);
          s.y += (oy - s.y) * Math.min(1, dt * 8);
        }
      }

      // Links from the hub, with packets streaming in from busy workspaces.
      packetClock += dt;
      for (const n of list) {
        const col = n.status ? resolveColor(STATUS[n.status].color, v) : v["fg-3"];
        g.strokeStyle = rgba(col, n.status === "waiting" ? 0.35 + 0.25 * Math.sin(t * 4) : 0.14);
        g.setLineDash(n.status === "waiting" ? [4, 6] : []);
        g.lineDashOffset = -t * 20;
        g.beginPath();
        g.moveTo(cx, cy);
        g.lineTo(n.x, n.y);
        g.stroke();
        g.setLineDash([]);
        if ((n.status === "working" || n.status === "polling") && packetClock > 0.25 && Math.random() < 0.35) {
          beams.push({ sat: "", node: n.key, t: 0, color: v.working, label: "" });
        }
      }
      if (packetClock > 0.25) packetClock = 0;

      g.globalCompositeOperation = dark ? "lighter" : "source-over";

      // Hub.
      const hubR = Math.max(16, span * 0.07);
      const pulse = 1 + 0.06 * Math.sin(t * 2);
      const hg = g.createRadialGradient(cx, cy, 0, cx, cy, hubR * 2.4 * pulse);
      hg.addColorStop(0, rgba(v.accent, 0.55));
      hg.addColorStop(0.4, rgba(v.accent, 0.18));
      hg.addColorStop(1, rgba(v.accent, 0));
      g.fillStyle = hg;
      g.beginPath();
      g.arc(cx, cy, hubR * 2.4 * pulse, 0, Math.PI * 2);
      g.fill();
      g.strokeStyle = rgba(v.accent, 0.8);
      g.lineWidth = 1.5;
      for (let i = 0; i < 3; i++) {
        g.beginPath();
        const a0 = t * (i % 2 ? -0.8 : 0.5) + i * 2;
        g.arc(cx, cy, hubR * (0.7 + i * 0.22), a0, a0 + Math.PI * (0.9 + i * 0.2));
        g.stroke();
      }

      for (const n of list) {
        const col = n.status ? resolveColor(STATUS[n.status].color, v) : v["fg-3"];
        const appear = Math.min(1, (t - n.born) * 2);
        const glow = g.createRadialGradient(n.x, n.y, 0, n.x, n.y, n.r * 0.9);
        glow.addColorStop(0, rgba(col, (n.status ? 0.35 : 0.12) * appear));
        glow.addColorStop(1, rgba(col, 0));
        g.fillStyle = glow;
        g.beginPath();
        g.arc(n.x, n.y, n.r * 0.9, 0, Math.PI * 2);
        g.fill();

        g.strokeStyle = rgba(col, 0.18);
        g.lineWidth = 1;
        for (const o of [0.95, 1.35]) {
          if (!n.sats.some((s) => s.orbit === (o < 1 ? 0 : 1))) continue;
          g.beginPath();
          g.ellipse(n.x, n.y, n.r * o, n.r * o * 0.62, 0, 0, Math.PI * 2);
          g.stroke();
        }

        // A hexagonal core.
        const cr = n.r * 0.32;
        g.strokeStyle = rgba(col, 0.9 * appear);
        g.fillStyle = rgba(col, n.status ? 0.22 : 0.08);
        g.lineWidth = 1.5;
        g.beginPath();
        for (let i = 0; i < 6; i++) {
          const a = (i / 6) * Math.PI * 2 + t * 0.2;
          g[i ? "lineTo" : "moveTo"](n.x + Math.cos(a) * cr, n.y + Math.sin(a) * cr);
        }
        g.closePath();
        g.fill();
        g.stroke();

        for (const s of n.sats) {
          const st = s.agent?.status;
          if (s.agent && st) {
            const c = resolveColor(STATUS[st].color, v);
            const sr = st === "waiting" ? 5.5 + Math.sin(t * 6) * 1.2 : 4.5;
            const sg = g.createRadialGradient(s.x, s.y, 0, s.x, s.y, sr * 3.2);
            sg.addColorStop(0, rgba(c, alive(st) ? 0.7 : 0.2));
            sg.addColorStop(1, rgba(c, 0));
            g.fillStyle = sg;
            g.beginPath();
            g.arc(s.x, s.y, sr * 3.2, 0, Math.PI * 2);
            g.fill();
            g.fillStyle = alive(st) ? c : rgba(c, 0.5);
            g.beginPath();
            g.arc(s.x, s.y, sr, 0, Math.PI * 2);
            g.fill();
            if (st === "waiting") {
              const k = (t * 0.8) % 1;
              g.strokeStyle = rgba(c, 1 - k);
              g.lineWidth = 1.5;
              g.beginPath();
              g.arc(s.x, s.y, sr + k * 18, 0, Math.PI * 2);
              g.stroke();
            }
            if ((st === "working" || st === "polling") && particles.length < 700 && Math.random() < dt * 40) {
              particles.push({ x: s.x, y: s.y, vx: (Math.random() - 0.5) * 14, vy: (Math.random() - 0.5) * 14, life: 0, max: 0.6 + Math.random() * 0.8, color: c, size: 1 + Math.random() * 1.5 });
            }
          } else {
            const c = s.win?.running ? v.accent : v["fg-3"];
            g.fillStyle = rgba(c, s.win?.running ? 0.95 : 0.55);
            g.fillRect(s.x - 2.5, s.y - 2.5, 5, 5);
            if (s.win?.running) {
              g.strokeStyle = rgba(c, 0.9);
              g.lineWidth = 1.2;
              g.beginPath();
              g.arc(s.x, s.y, 6, t * 5, t * 5 + Math.PI * 1.2);
              g.stroke();
            }
          }
        }
      }

      for (let i = particles.length - 1; i >= 0; i--) {
        const p = particles[i];
        p.life += dt;
        if (p.life > p.max) {
          particles.splice(i, 1);
          continue;
        }
        p.x += p.vx * dt;
        p.y += p.vy * dt;
        g.globalAlpha = (1 - p.life / p.max) * 0.8;
        g.fillStyle = p.color;
        g.fillRect(p.x - p.size / 2, p.y - p.size / 2, p.size, p.size);
      }
      g.globalAlpha = 1;

      for (let i = beams.length - 1; i >= 0; i--) {
        const b = beams[i];
        const n = nodes.get(b.node);
        const from = b.sat ? n?.sats.find((s) => s.key === b.sat) : undefined;
        if (!n || (b.sat && !from)) {
          beams.splice(i, 1);
          continue;
        }
        b.t += dt * (b.sat ? 1.4 : 0.9);
        const [ax, ay, bx, by] = b.sat ? [from!.x, from!.y, n.x, n.y] : [n.x, n.y, cx, cy];
        const k = Math.min(1, b.t);
        const e = 1 - (1 - k) * (1 - k);
        const x = ax + (bx - ax) * e;
        const y = ay + (by - ay) * e;
        const tail = Math.max(0, e - 0.25);
        const lg = g.createLinearGradient(ax + (bx - ax) * tail, ay + (by - ay) * tail, x, y);
        lg.addColorStop(0, rgba(b.color, 0));
        lg.addColorStop(1, rgba(b.color, b.sat ? 0.95 : 0.5));
        g.strokeStyle = lg;
        g.lineWidth = b.sat ? 2 : 1.4;
        g.beginPath();
        g.moveTo(ax + (bx - ax) * tail, ay + (by - ay) * tail);
        g.lineTo(x, y);
        g.stroke();
        g.fillStyle = b.color;
        g.beginPath();
        g.arc(x, y, b.sat ? 2.4 : 1.6, 0, Math.PI * 2);
        g.fill();
        if (b.t >= 1) {
          beams.splice(i, 1);
          if (b.sat) {
            rings.push({ x: n.x, y: n.y, t: 0, color: b.color });
            labels.push({ node: n.key, text: b.label, life: 0, dy: 0, color: b.color });
            for (let j = 0; j < 14; j++) {
              const a = Math.random() * Math.PI * 2;
              const sp = 20 + Math.random() * 40;
              particles.push({ x: n.x, y: n.y, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp, life: 0, max: 0.5 + Math.random() * 0.4, color: b.color, size: 1.5 });
            }
          }
        }
      }

      for (let i = rings.length - 1; i >= 0; i--) {
        const r = rings[i];
        r.t += dt;
        if (r.t > 1) {
          rings.splice(i, 1);
          continue;
        }
        const c = resolveColor(r.color, v);
        g.strokeStyle = rgba(c, 1 - r.t);
        g.lineWidth = 2 * (1 - r.t) + 0.5;
        g.beginPath();
        g.arc(r.x, r.y, 6 + r.t * 40, 0, Math.PI * 2);
        g.stroke();
      }

      g.globalCompositeOperation = "source-over";
      g.textAlign = "center";
      g.textBaseline = "top";
      for (const n of list) {
        g.font = `600 11px ${font}`;
        g.fillStyle = rgba(v.fg, n.status ? 0.95 : 0.5);
        g.fillText(n.name.toUpperCase(), n.x, n.y + n.r * 0.95 + 8);
        const branch = n.ws?.git?.branch;
        if (branch && n.r > 26) {
          g.font = `400 10px ${font}`;
          g.fillStyle = rgba(v["fg-3"], 0.9);
          g.fillText(branch.length > 24 ? branch.slice(0, 23) + "…" : branch, n.x, n.y + n.r * 0.95 + 22);
        }
      }

      g.textBaseline = "middle";
      g.font = `500 10.5px ${font}`;
      const stack = new Map<string, number>();
      for (let i = labels.length - 1; i >= 0; i--) {
        const l = labels[i];
        l.life += dt;
        const n = nodes.get(l.node);
        if (!n || l.life > 3.2) {
          labels.splice(i, 1);
          continue;
        }
        const level = stack.get(l.node) ?? 0;
        stack.set(l.node, level + 1);
        const y = n.y - n.r * 0.62 - 14 - level * 15 - l.life * 6;
        const a = Math.min(1, l.life * 4) * Math.min(1, (3.2 - l.life) * 1.5);
        const tw = g.measureText(l.text).width + 12;
        g.globalAlpha = a;
        g.fillStyle = rgba(v.bg, 0.8);
        g.fillRect(n.x - tw / 2, y - 7, tw, 14);
        g.strokeStyle = rgba(l.color, 0.6);
        g.lineWidth = 1;
        g.strokeRect(n.x - tw / 2 + 0.5, y - 6.5, tw - 1, 13);
        g.fillStyle = l.color;
        g.fillText(l.text, n.x, y + 0.5);
      }
      g.globalAlpha = 1;

      const hit = mouse.x >= 0 ? pick(mouse.x, mouse.y) : null;
      canvas.style.cursor = hit ? "pointer" : "default";
      if (hit) {
        const x = hit.sat ? hit.sat.x : hit.node.x;
        const y = hit.sat ? hit.sat.y : hit.node.y;
        const rr = hit.sat ? 10 : hit.node.r * 0.45;
        g.strokeStyle = v.fg;
        g.lineWidth = 1;
        const l = rr * 0.5;
        for (const [sx, sy] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) {
          g.beginPath();
          g.moveTo(x + sx * rr, y + sy * (rr - l));
          g.lineTo(x + sx * rr, y + sy * rr);
          g.lineTo(x + sx * (rr - l), y + sy * rr);
          g.stroke();
        }
      }
      const prev = hover();
      if (hit?.node !== prev?.node || hit?.sat !== prev?.sat) setHover(hit ? { x: mouse.x, y: mouse.y, ...hit } : null);
    };
    raf = requestAnimationFrame(frame);
    onCleanup(() => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    });
  });

  const onMove = (e: MouseEvent) => {
    const r = canvas.getBoundingClientRect();
    mouse = { x: e.clientX - r.left, y: e.clientY - r.top };
    const h = hover();
    if (h) setHover({ ...h, x: mouse.x, y: mouse.y });
  };

  const counts = () => {
    const c: Partial<Record<Status, number>> = {};
    for (const a of state.agents) if (alive(a.status)) c[a.status] = (c[a.status] ?? 0) + 1;
    return c;
  };
  const clock = () => new Date(now()).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });

  return (
    <div class="relative flex h-full min-h-0 bg-bg font-mono">
      <div ref={wrap} class="relative min-w-0 flex-1">
        <canvas
          ref={canvas}
          class="absolute inset-0 size-full"
          onMouseMove={onMove}
          onMouseLeave={() => ((mouse = { x: -1, y: -1 }), setHover(null))}
          onClick={(e) => {
            const r = canvas.getBoundingClientRect();
            const hit = pick(e.clientX - r.left, e.clientY - r.top);
            if (hit) open(hit.node, hit.sat);
          }}
        />
        <div class="pointer-events-none absolute left-4 top-3 select-none">
          <div class="text-xs tracking-[0.3em] text-accent">JUGGLR // MISSION CONTROL</div>
          <div class="mt-1 text-xl tabular-nums text-fg">{clock()}</div>
          <div class="mt-2 flex flex-wrap gap-3 text-xs">
            <For each={["waiting", "working", "idle"] as Status[]}>
              {(s) => (
                <span class="flex items-center gap-1.5" style={{ color: STATUS[s].color }}>
                  <span class="size-1.5 rounded-full" style={{ background: STATUS[s].color }} />
                  {(counts()[s] ?? 0) + (s === "working" ? (counts().polling ?? 0) : 0)} {STATUS[s].label.toUpperCase()}
                </span>
              )}
            </For>
            <span class="text-fg-3">{state.workspaces.length} WORKSPACES</span>
          </div>
        </div>
        <div class="pointer-events-none absolute bottom-3 left-4 flex select-none flex-wrap gap-x-4 gap-y-1 text-xs text-fg-3">
          <span>● agent</span>
          <span>■ terminal</span>
          <span class="text-accent">◔ command running</span>
          <span>click to open</span>
        </div>
        <Show when={hover()}>
          {(h) => (
            <div
              class="pointer-events-none absolute z-10 max-w-72 rounded-md border border-line-strong bg-raised/95 px-2.5 py-1.5 text-xs shadow-float"
              style={{ left: `${Math.min(h().x + 14, size.w - 290)}px`, top: `${h().y + 14}px` }}
            >
              <Show
                when={h().sat}
                fallback={
                  <>
                    <div class="font-semibold text-fg">{h().node.name}</div>
                    <div class="text-fg-3">{h().node.ws?.short_path}</div>
                    <div class="text-fg-2">
                      {h().node.sats.length} windows
                      <Show when={h().node.ws?.git?.dirty}>{(d) => <> · {d()} changed</>}</Show>
                    </div>
                  </>
                }
              >
                {(s) => (
                  <>
                    <div class="font-semibold text-fg">{s().agent?.title || s().label}</div>
                    <Show when={s().agent}>
                      {(a) => (
                        <>
                          <div style={{ color: STATUS[a().status].color }}>
                            {STATUS[a().status].label} · {age(a().timestamp, now())}
                          </div>
                          <Show when={a().tool}>{(t) => <div class="truncate text-fg-2">{t()}</div>}</Show>
                          <Show when={a().status === "waiting" && a().message}>{(m) => <div class="text-fg-2">{m()}</div>}</Show>
                        </>
                      )}
                    </Show>
                    <Show when={!s().agent}>
                      <div class="text-fg-2">{s().win?.running ?? s().win?.command}</div>
                    </Show>
                  </>
                )}
              </Show>
            </div>
          )}
        </Show>
      </div>
      <Feed />
    </div>
  );
}

function Feed() {
  const icon = (e: FeedEntry) => (e.source === "claude" ? "◆" : e.source === "shell" ? "$" : "●");
  const color = (e: FeedEntry) => (e.source === "claude" ? "var(--working)" : e.source === "shell" ? "var(--accent)" : STATUS[e.status ?? "idle"].color);
  return (
    <aside class="hidden w-80 shrink-0 flex-col border-l border-line bg-panel/60 lg:flex">
      <div class="flex items-center justify-between border-b border-line px-3 py-2 text-xs tracking-[0.2em] text-fg-3">
        <span>TELEMETRY</span>
        <span class="flex items-center gap-1.5">
          <span class="size-1.5 animate-pulse-dot rounded-full bg-working" />
          LIVE
        </span>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto">
        <Show when={feed().length} fallback={<div class="px-3 py-4 text-xs text-fg-3">Waiting for commands from Claude or a terminal…</div>}>
          <For each={feed()}>
            {(e) => (
              <button
                class={cls("block w-full border-b border-line/50 px-3 py-1.5 text-left text-xs animate-fade-in", e.route && "hover:bg-hover")}
                onClick={() => e.route && go(e.route)}
              >
                <div class="flex items-center gap-2 text-fg-3">
                  <span style={{ color: color(e) }}>{icon(e)}</span>
                  <span class="truncate">{e.where}</span>
                  <span class="ml-auto shrink-0 tabular-nums">{age(e.ts, now())}</span>
                </div>
                <div class="mt-0.5 break-all text-fg-2" style={{ color: e.source === "status" ? color(e) : undefined }}>
                  {e.text}
                </div>
              </button>
            )}
          </For>
        </Show>
      </div>
    </aside>
  );
}
