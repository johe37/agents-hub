import { For, Show, createMemo, type JSX } from "solid-js";
import { cls } from "../util";
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

const tint = (l: DiffLine | undefined) => (l?.type === "add" ? "bg-add-soft" : l?.type === "del" ? "bg-del-soft" : "");

export interface ReviewDiffProps {
  lines: DiffLine[];
  /** Highlighted HTML per line, when the language is known. */
  html?: (string | undefined)[];
  split: boolean;
  /** The selected lines, first and last index. */
  sel: [number, number] | null;
  onPick: (i: number, extend: boolean) => void;
  /** Whether anything goes under line i. */
  has: (i: number) => boolean;
  /** What goes under line i: threads, drafts, the comment form. Built once while has(i) holds. */
  after: (i: number) => JSX.Element;
}

export function ReviewDiff(p: ReviewDiffProps) {
  const picked = (i: number | undefined) => i != null && !!p.sel && i >= p.sel[0] && i <= p.sel[1];
  const Code = (q: { i: number | undefined; class?: string }) => {
    const l = () => (q.i == null ? undefined : p.lines[q.i]);
    return (
      <td class={cls("pr-4 align-top", p.split ? "whitespace-pre-wrap break-all" : "whitespace-pre", q.class, !p.html?.[q.i ?? -1] && (l()?.type === "add" ? "text-add" : l()?.type === "del" ? "text-del" : "text-fg-2"))}>
        <Show when={p.html?.[q.i ?? -1]} fallback={l()?.text || " "}>
          {(h) => <span innerHTML={h() || " "} />}
        </Show>
      </td>
    );
  };
  const Num = (q: { i: number | undefined; n: number | undefined }) => (
    <td
      class={cls("w-10 select-none px-2 text-right align-top", q.i != null && "cursor-pointer hover:text-accent", picked(q.i) ? "text-accent" : "text-fg-3/70")}
      onClick={(e) => q.i != null && p.onPick(q.i, e.shiftKey)}
      title={q.i != null ? "Select the line; Shift+click selects up to it" : undefined}
    >
      {q.n ?? ""}
    </td>
  );
  const Under = (q: { at: number[] }) => (
    <For each={q.at}>
      {(i) => (
        <Show when={p.has(i)}>
          <tr>
            <td colSpan={4} class="border-y border-line bg-bg px-3 py-2 font-sans text-[13px] leading-normal">
              {p.after(i)}
            </td>
          </tr>
        </Show>
      )}
    </For>
  );
  const Hunk = (q: { i: number }) => (
    <tr class="bg-working-soft/60 text-fg-3">
      <td colSpan={4} class="px-3 py-0.5 text-[11px]">
        ⋯ {p.lines[q.i].text}
      </td>
    </tr>
  );
  const rows = createMemo(() => (p.split ? pairRows(p.lines) : []));

  return (
    <div class="overflow-x-auto font-mono text-[12px] leading-[1.6]">
      <table class={cls("w-full border-collapse", p.split && "table-fixed")}>
        <Show
          when={p.split}
          fallback={
            <tbody>
              <For each={p.lines}>
                {(l, i) => (
                  <Show when={l.type !== "hunk"} fallback={<Hunk i={i()} />}>
                    <tr class={cls(tint(l), picked(i()) && "outline outline-1 -outline-offset-1 outline-accent/50 !bg-accent-soft")}>
                      <Num i={i()} n={l.old} />
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
                      <Num i={r.l} n={r.l != null ? p.lines[r.l].old : undefined} />
                      <Code i={r.l} class={cls("border-r border-line", r.l == null ? "bg-hover/40" : tint(p.lines[r.l]), picked(r.l) && "!bg-accent-soft")} />
                      <Num i={r.r} n={r.r != null ? p.lines[r.r].new : undefined} />
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
