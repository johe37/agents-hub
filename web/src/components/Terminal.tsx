import { Show, createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { Terminal as XTerm } from "@xterm/xterm";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { wsURL } from "../api";
import { openPalette, theme } from "../store";
import { termFontSize } from "../settings";
import { Refresh } from "./icons";
import { Button, Spinner } from "./ui";

/** A live tmux window. Typing here is typing in the window itself. */
const MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/**
 * What macOS terminals send for their text-editing keys. xterm.js sends Option+arrow as a
 * modified arrow, which shells and Claude Code do not read as a word move.
 */
function macKey(e: KeyboardEvent): string | undefined {
  const only = (m: "alt" | "meta") => (m === "alt" ? e.altKey && !e.metaKey : e.metaKey && !e.altKey) && !e.ctrlKey && !e.shiftKey;
  // Option stays Meta on letters, but layouts such as Swedish type @ $ | [ ] { } \ with
  // Option on the other keys, and Meta would swallow them.
  if (e.altKey && !e.metaKey && !e.ctrlKey && e.key.length === 1 && !/^Key|^Space$/.test(e.code)) return e.key;
  if (only("alt")) return { ArrowLeft: "\x1bb", ArrowRight: "\x1bf" }[e.key];
  if (only("meta")) return { ArrowLeft: "\x01", ArrowRight: "\x05", Backspace: "\x15" }[e.key];
  return undefined;
}

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
      fontSize: termFontSize(),
      lineHeight: 1.2,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 0,
      theme: theme().term,
      macOptionIsMeta: true,
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    // tmux owns the mouse and copies a selection with OSC 52; this puts it on the clipboard.
    // tmux leaves the selection parameter empty, which the default provider ignores.
    term.loadAddon(
      new ClipboardAddon(undefined, {
        readText: () => "",
        writeText: (_, text) => navigator.clipboard.writeText(text),
      }),
    );
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
      const seq = MAC && e.type === "keydown" ? macKey(e) : undefined;
      if (seq) {
        e.preventDefault();
        if (socket?.readyState === WebSocket.OPEN) socket.send(new TextEncoder().encode(seq));
        return false;
      }
      return true;
    });
    // tmux scrolls on its side, one round trip per wheel report. Turn the wheel's pixel
    // deltas into whole lines and send them once per frame, so a trackpad does not flood
    // tmux and a notch scrolls as far as it would in a page.
    let wheelLines = 0;
    let wheelAt = { col: 1, row: 1 };
    let wheelFrame = 0;
    const flushWheel = () => {
      wheelFrame = 0;
      const n = Math.trunc(wheelLines);
      if (!n || socket?.readyState !== WebSocket.OPEN) return;
      wheelLines -= n;
      const report = `\x1b[<${n < 0 ? 64 : 65};${wheelAt.col};${wheelAt.row}M`;
      socket.send(new TextEncoder().encode(report.repeat(Math.min(Math.abs(n), term!.rows))));
    };
    term.attachCustomWheelEventHandler((e) => {
      if (!term || e.ctrlKey || term.modes.mouseTrackingMode === "none" || !e.deltaY) return true;
      const screen = host.querySelector(".xterm-screen")?.getBoundingClientRect();
      if (!screen?.height) return true;
      const cellW = screen.width / term.cols;
      const cellH = screen.height / term.rows;
      const lines = e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? e.deltaY / cellH : e.deltaMode === WheelEvent.DOM_DELTA_LINE ? e.deltaY : e.deltaY * term.rows;
      if (Math.sign(lines) !== Math.sign(wheelLines)) wheelLines = 0;
      wheelLines += lines;
      wheelAt = {
        col: Math.min(term.cols, Math.max(1, Math.floor((e.clientX - screen.left) / cellW) + 1)),
        row: Math.min(term.rows, Math.max(1, Math.floor((e.clientY - screen.top) / cellH) + 1)),
      };
      wheelFrame ||= requestAnimationFrame(flushWheel);
      e.preventDefault();
      return false;
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
    const t = theme().term;
    if (term) term.options.theme = t;
  });
  createEffect(
    on(
      termFontSize,
      (size) => {
        if (!term) return;
        term.options.fontSize = size;
        refit();
      },
      { defer: true },
    ),
  );
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
