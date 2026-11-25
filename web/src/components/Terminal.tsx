import { Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { wsURL } from "../api";
import { openPalette, theme } from "../store";
import { Refresh } from "./icons";
import { Button, Spinner } from "./ui";

const DARK: ITheme = {
  background: "#0e0e11",
  foreground: "#e4e4ea",
  cursor: "#e07b53",
  cursorAccent: "#0e0e11",
  selectionBackground: "#e07b5340",
  black: "#1c1c21",
  red: "#ff7b72",
  green: "#6fd394",
  yellow: "#f0b44c",
  blue: "#7c93ff",
  magenta: "#d2a8ff",
  cyan: "#6ad7e5",
  white: "#d0d0d8",
  brightBlack: "#5b5b66",
  brightRed: "#ffa198",
  brightGreen: "#8ee6ad",
  brightYellow: "#f7cc7a",
  brightBlue: "#a5b4ff",
  brightMagenta: "#e2c5ff",
  brightCyan: "#9becf5",
  brightWhite: "#ffffff",
};

const LIGHT: ITheme = {
  background: "#fdfdfc",
  foreground: "#1b1a18",
  cursor: "#c15f3c",
  cursorAccent: "#fdfdfc",
  selectionBackground: "#c15f3c30",
  black: "#1b1a18",
  red: "#c4362c",
  green: "#1f7a46",
  yellow: "#9a6700",
  blue: "#3451d1",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#6e6a63",
  brightBlack: "#8e8a83",
  brightRed: "#d64545",
  brightGreen: "#2f855a",
  brightYellow: "#b7791f",
  brightBlue: "#4f6bed",
  brightMagenta: "#a475f9",
  brightCyan: "#3192aa",
  brightWhite: "#3a3833",
};

/** A live tmux window. Typing here is typing in the window itself. */
export function Terminal(p: { window: string; focus?: boolean }) {
  let host!: HTMLDivElement;
  const [phase, setPhase] = createSignal<"connecting" | "open" | "closed">("connecting");
  const [attempt, setAttempt] = createSignal(0);
  let term: XTerm | null = null;
  let fit: FitAddon | null = null;
  let socket: WebSocket | null = null;

  const sendSize = () => {
    if (!term || socket?.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify({ cols: term.cols, rows: term.rows }));
  };

  const refit = () => {
    if (!fit || !host.offsetWidth || !host.offsetHeight) return;
    try {
      fit.fit();
    } catch {
      /* not attached yet */
    }
  };

  let ro: ResizeObserver | undefined;
  onCleanup(() => ro?.disconnect());

  onMount(async () => {
    await document.fonts.load('13px "JetBrains Mono Variable"').catch(() => undefined);
    term = new XTerm({
      fontFamily: '"JetBrains Mono Variable", ui-monospace, Menlo, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 0,
      theme: theme() === "dark" ? DARK : LIGHT,
      macOptionIsMeta: true,
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((_, uri) => window.open(uri, "_blank", "noopener")));
    term.open(host);
    try {
      const { WebglAddon } = await import("@xterm/addon-webgl");
      const gl = new WebglAddon();
      gl.onContextLoss(() => gl.dispose());
      term.loadAddon(gl);
    } catch {
      /* The DOM renderer is the fallback. */
    }
    // The palette must stay reachable while the terminal has the keyboard.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type === "keydown" && e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "k") {
        openPalette();
        return false;
      }
      return true;
    });
    term.onData((d) => socket?.readyState === WebSocket.OPEN && socket.send(new TextEncoder().encode(d)));
    term.onBinary((d) => {
      if (socket?.readyState !== WebSocket.OPEN) return;
      const buf = new Uint8Array(d.length);
      for (let i = 0; i < d.length; i++) buf[i] = d.charCodeAt(i) & 0xff;
      socket.send(buf);
    });
    term.onResize(sendSize);
    refit();
    ro = new ResizeObserver(() => requestAnimationFrame(refit));
    ro.observe(host);
    connect();
  });

  function connect() {
    if (!term) return;
    socket?.close();
    setPhase("connecting");
    term.reset();
    const q = new URLSearchParams({ window: p.window, cols: String(term.cols), rows: String(term.rows) });
    const ws = new WebSocket(wsURL(`/api/term?${q}`));
    ws.binaryType = "arraybuffer";
    socket = ws;
    ws.onopen = () => {
      setPhase("open");
      sendSize();
      if (p.focus) term?.focus();
    };
    ws.onmessage = (ev) => {
      if (ev.data instanceof ArrayBuffer) term?.write(new Uint8Array(ev.data));
    };
    ws.onclose = () => {
      if (socket === ws) setPhase("closed");
    };
  }

  createEffect(on(() => p.window, () => term && connect(), { defer: true }));
  createEffect(on(attempt, () => term && connect(), { defer: true }));
  createEffect(() => {
    const t = theme();
    if (term) term.options.theme = t === "dark" ? DARK : LIGHT;
  });
  createEffect(() => {
    if (p.focus && phase() === "open") term?.focus();
  });

  onCleanup(() => {
    const s = socket;
    socket = null;
    s?.close();
    term?.dispose();
  });

  return (
    <div class="relative h-full min-h-0 w-full overflow-hidden bg-[var(--term-bg)]">
      <div ref={host} class="xterm-host absolute inset-0" />
      <Show when={phase() === "connecting"}>
        <div class="pointer-events-none absolute right-3 top-3 flex items-center gap-2 rounded-lg border border-line bg-panel/90 px-2.5 py-1.5 text-[12px] text-fg-2 backdrop-blur animate-fade-in">
          <Spinner size={12} /> Attaching…
        </div>
      </Show>
      <Show when={phase() === "closed"}>
        <div class="absolute inset-0 grid place-items-center bg-bg/70 backdrop-blur-[2px] animate-fade-in">
          <div class="flex flex-col items-center gap-3 text-center">
            <div class="text-fg-2">This terminal is detached. The window may have closed.</div>
            <Button size="sm" onClick={() => setAttempt((n) => n + 1)}>
              <Refresh size={14} /> Reconnect
            </Button>
          </div>
        </div>
      </Show>
    </div>
  );
}
