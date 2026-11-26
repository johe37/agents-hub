import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import { cls } from "../util";
import { Chat } from "./icons";
import type { DiffLine } from "./Diff";

type Row = { hunk: number } | { l?: number; r?: number };

/** Side by side: context on both sides, a run of removed lines next to the added lines after it. */
function pairRows(lines: DiffLine[]): Row[] {
  const rows: Row[] = [];
  let i = 0;
  while (i < lines.length) {
    const t = lines[i].type;
    if (t === "hunk") rows.push({ hunk: i++ });
    else if (t === "ctx") rows.push({ l: i, r: i++ });
    else {
      const dels: number[] = [];
      const adds: number[] = [];
      while (i < lines.length && lines[i].type === "del") dels.push(i++);
      while (i < lines.length && lines[i].type === "add") adds.push(i++);
      for (let k = 0; k < Math.max(dels.length, adds.length); k++) rows.push({ l: dels[k], r: adds[k] });
    }
  }
  return rows;
}

function scrollParent(el: HTMLElement): HTMLElement {
  for (let e = el.parentElement; e; e = e.parentElement) {
    const o = getComputedStyle(e).overflowY;
    if ((o === "auto" || o === "scroll") && e.scrollHeight > e.clientHeight) return e;
  }
  return document.scrollingElement as HTMLElement;
}

const tint = (l: DiffLine | undefined) => (l?.type === "add" ? "bg-add-soft" : l?.type === "del" ? "bg-del-soft" : "");

export interface ReviewDiffProps {
  lines: DiffLine[];
  /** Highlighted HTML per line, when the language is known. */
  html?: (string | undefined)[];
  split: boolean;
  /** The selected lines, first and last index. */
  sel: [number, number] | null;
  /** A click on line i: select it, or extend the selection to it with Shift. */
  onPick: (i: number, extend: boolean) => void;
  /** A finished drag from line a to line b. */
  onSelect: (a: number, b: number) => void;
  /** Whether anything goes under line i. */
  has: (i: number) => boolean;
  /** What goes under line i: threads, drafts, the comment form. Built once while has(i) holds. */
  after: (i: number) => JSX.Element;
}

export function ReviewDiff(p: ReviewDiffProps) {
  // While a drag runs, only the highlight follows the pointer: the selection, and with it the
  // Ask Claude / Comment bar, comes when the button is let go, as in GitLab.
  const [live, setLive] = createSignal<[number, number] | null>(null);
  const range = () => live() ?? p.sel;
  const picked = (i: number | undefined) => {
    const r = range();
    return i != null && !!r && i >= r[0] && i <= r[1];
  };
  const [hover, setHover] = createSignal<number>();

  /** Line i, or the last line before it in the same hunk as line from. */
  const clamp = (from: number, i: number) => {
    const step = i < from ? -1 : 1;
    let b = from;
    while (b !== i && p.lines[b + step] && p.lines[b + step].type !== "hunk") b += step;
    return b;
  };

  // The line under the pointer is looked up at the start cell's x, so the drag keeps to its side
  // and tracks fast moves, a pointer outside the gutter and auto-scroll near the edges.
  const down = (e: PointerEvent, start: number) => {
    if (e.button !== 0) return;
    e.preventDefault();
    if (e.shiftKey) return p.onPick(start, true);
    const cell = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = cell.left + cell.width / 2;
    let y = e.clientY;
    let end = start;
    const scroller = scrollParent(e.currentTarget as HTMLElement);
    const track = () => {
      const td = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-i]");
      if (!td?.dataset.i) return;
      end = clamp(start, Number(td.dataset.i));
      setLive([Math.min(start, end), Math.max(start, end)]);
    };
    let frame = 0;
    const tick = () => {
      const r = scroller === document.scrollingElement ? { top: 0, bottom: window.innerHeight } : scroller.getBoundingClientRect();
      const edge = 48;
      const v = y < r.top + edge ? -(r.top + edge - y) : y > r.bottom - edge ? y - (r.bottom - edge) : 0;
      if (v) {
        scroller.scrollTop += Math.max(-24, Math.min(24, v / 2));
        track();
      }
      frame = requestAnimationFrame(tick);
    };
    const move = (ev: PointerEvent) => {
      y = ev.clientY;
      track();
    };
    const stop = () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", esc, true);
      document.documentElement.classList.remove("dragging-lines");
      setLive(null);
    };
    const up = () => {
      stop();
      if (end === start) p.onPick(start, false);
      else p.onSelect(start, end);
    };
    const cancel = () => stop();
    const esc = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      stop();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", esc, true);
    document.documentElement.classList.add("dragging-lines");
    setLive([start, start]);
    frame = requestAnimationFrame(tick);
  };
  const Code = (q: { i: number | undefined; class?: string }) => {
    const l = () => (q.i == null ? undefined : p.lines[q.i]);
    return (
      <td
        data-i={q.i}
        onMouseEnter={() => setHover(q.i)}
        class={cls("pr-4 align-top", p.split ? "whitespace-pre-wrap break-all" : "whitespace-pre", q.class, !p.html?.[q.i ?? -1] && (l()?.type === "add" ? "text-add" : l()?.type === "del" ? "text-del" : "text-fg-2"))}
      >
        <Show when={p.html?.[q.i ?? -1]} fallback={l()?.text || " "}>
          {(h) => <span innerHTML={h() || " "} />}
        </Show>
      </td>
    );
  };
  const Num = (q: { i: number | undefined; n: number | undefined; button?: boolean }) => (
    <td
      data-i={q.i}
      class={cls("relative w-10 touch-none select-none px-2 text-right align-top", q.i != null && "cursor-ns-resize hover:text-accent", picked(q.i) ? "bg-accent-soft text-accent" : "text-fg-3/70")}
      onPointerDown={(e) => q.i != null && down(e, q.i)}
      onMouseEnter={() => setHover(q.i)}
      title={q.i != null ? "Click or drag to select lines, then ask Claude or comment" : undefined}
    >
      <Show when={q.button && q.i != null && hover() === q.i && !live()}>
        <span class="pointer-events-none absolute left-0.5 top-[1px] grid h-[17px] w-[17px] place-items-center rounded bg-accent text-white shadow-sm">
          <Chat size={11} />
        </span>
      </Show>
      {q.n ?? ""}
    </td>
  );
  const Under = (q: { at: number[] }) => (
    <For each={q.at}>
      {(i) => (
        <Show when={p.has(i)}>
          <tr>
            <td colSpan={4} class="border-y border-line bg-bg px-3 py-2 font-sans text-base leading-normal">
              {p.after(i)}
            </td>
          </tr>
        </Show>
      )}
    </For>
  );
  const Hunk = (q: { i: number }) => (
    <tr class="bg-working-soft/60 text-fg-3">
      <td colSpan={4} class="px-3 py-0.5 text-xs">
        ⋯ {p.lines[q.i].text}
      </td>
    </tr>
  );
  const rows = createMemo(() => (p.split ? pairRows(p.lines) : []));

  return (
    <div class="overflow-x-auto font-mono text-sm leading-[1.6]" onMouseLeave={() => setHover(undefined)}>
      <table class={cls("w-full border-collapse", p.split && "table-fixed")}>
        <Show
          when={p.split}
          fallback={
            <tbody>
              <For each={p.lines}>
                {(l, i) => (
                  <Show when={l.type !== "hunk"} fallback={<Hunk i={i()} />}>
                    <tr class={cls(tint(l), picked(i()) && "outline outline-1 -outline-offset-1 outline-accent/50 !bg-accent-soft")}>
                      <Num i={i()} n={l.old} button />
                      <Num i={i()} n={l.new} />
                      <td class={cls("w-5 select-none text-center align-top", l.type === "add" ? "text-add" : l.type === "del" ? "text-del" : "text-fg-3/50")}>
                        {l.type === "add" ? "+" : l.type === "del" ? "−" : ""}
                      </td>
                      <Code i={i()} />
                    </tr>
                    <Under at={[i()]} />
                  </Show>
                )}
              </For>
            </tbody>
          }
        >
          <colgroup>
            <col class="w-10" />
            <col />
            <col class="w-10" />
            <col />
          </colgroup>
          <tbody>
            <For each={rows()}>
              {(r) =>
                "hunk" in r ? (
                  <Hunk i={r.hunk} />
                ) : (
                  <>
                    <tr>
                      <Num i={r.l} n={r.l != null ? p.lines[r.l].old : undefined} button />
                      <Code i={r.l} class={cls("border-r border-line", r.l == null ? "bg-hover/40" : tint(p.lines[r.l]), picked(r.l) && "!bg-accent-soft")} />
                      <Num i={r.r} n={r.r != null ? p.lines[r.r].new : undefined} button />
                      <Code i={r.r} class={cls(r.r == null ? "bg-hover/40" : tint(p.lines[r.r]), picked(r.r) && "!bg-accent-soft")} />
                    </tr>
                    <Under at={r.l === r.r ? [r.l!] : [r.l, r.r].filter((x): x is number => x != null)} />
                  </>
                )
              }
            </For>
          </tbody>
        </Show>
      </table>
    </div>
  );
}
