import { onCleanup, onMount } from "solid-js";

const KONAMI = ["ArrowUp", "ArrowUp", "ArrowDown", "ArrowDown", "ArrowLeft", "ArrowRight", "ArrowLeft", "ArrowRight", "b", "a"];
const JUGGLE = [..."juggle"];
const COLORS = ["accent", "working", "waiting", "add", "del", "idle"];

const calm = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Whether the keys typed so far end with seq. */
function endsWith(keys: string[], seq: string[]): boolean {
  return keys.length >= seq.length && seq.every((k, i) => keys[keys.length - seq.length + i] === k);
}

/** Juggling balls rain down, bounce on the bottom of the window and fade out. */
function rain() {
  const c = document.createElement("canvas");
  c.setAttribute("aria-hidden", "true");
  c.style.cssText = "position:fixed;inset:0;width:100vw;height:100vh;pointer-events:none;z-index:70";
  document.body.append(c);
  const dpr = devicePixelRatio || 1;
  const w = innerWidth;
  const h = innerHeight;
  c.width = w * dpr;
  c.height = h * dpr;
  const g = c.getContext("2d")!;
  g.scale(dpr, dpr);
  const css = getComputedStyle(document.documentElement);
  const colors = COLORS.map((n) => css.getPropertyValue(`--color-${n}`).trim() || "#e5484d");
  const balls = Array.from({ length: 48 }, (_, i) => ({
    x: Math.random() * w,
    y: -Math.random() * h - 20,
    vx: (Math.random() - 0.5) * 4,
    vy: 0,
    r: 7 + Math.random() * 7,
    color: colors[i % colors.length],
  }));
  const start = performance.now();
  const frame = (now: number) => {
    const t = (now - start) / 1000;
    if (t > 5) return c.remove();
    g.clearRect(0, 0, w, h);
    g.globalAlpha = t > 4 ? 5 - t : 1;
    for (const b of balls) {
      b.vy += 0.45;
      b.x += b.vx;
      b.y += b.vy;
      if (b.y + b.r > h) (b.y = h - b.r), (b.vy *= -0.72);
      if (b.x < b.r || b.x > w - b.r) b.vx *= -1;
      g.fillStyle = b.color;
      g.beginPath();
      g.arc(b.x, b.y, b.r, 0, Math.PI * 2);
      g.fill();
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

/** The agent cards on screen hop one after another, like balls tossed in turn. */
function hop() {
  document.querySelectorAll<HTMLElement>("[data-agent-card]").forEach((el, i) =>
    el.animate(
      [{ transform: "translateY(0)" }, { transform: "translateY(-18px) rotate(-1deg)", offset: 0.4 }, { transform: "translateY(0)" }],
      { duration: 520, delay: i * 90, easing: "cubic-bezier(.3,.7,.4,1)" },
    ),
  );
}

export function Eggs() {
  let keys: string[] = [];
  const onKey = (e: KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if ((e.target as HTMLElement).closest?.("input,textarea,select,[contenteditable],.xterm")) return;
    keys = [...keys, e.key.length === 1 ? e.key.toLowerCase() : e.key].slice(-KONAMI.length);
    const egg = endsWith(keys, KONAMI) ? rain : endsWith(keys, JUGGLE) ? hop : null;
    if (!egg) return;
    keys = [];
    if (!calm()) egg();
  };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));
  return null;
}
