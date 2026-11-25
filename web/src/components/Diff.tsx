import { For, Show, createMemo } from "solid-js";
import { cls } from "../util";

export interface DiffLine {
  type: "add" | "del" | "ctx" | "hunk";
  text: string;
  old?: number;
  new?: number;
}

export interface DiffFile {
  name: string;
  lines: DiffLine[];
  added: number;
  removed: number;
  binary: boolean;
}

export function parseUnified(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let f: DiffFile | null = null;
  let o = 0;
  let n = 0;
  for (const line of text.split("\n")) {
    if (line.startsWith("diff --git")) {
      const m = / b\/(.*)$/.exec(line);
      f = { name: m ? m[1] : line, lines: [], added: 0, removed: 0, binary: false };
      files.push(f);
      continue;
    }
    if (!f) continue;
    if (line.startsWith("+++ ")) {
      const name = line.slice(4).replace(/^b\//, "");
      if (name !== "/dev/null") f.name = name;
      continue;
    }
    if (line.startsWith("--- ") || line.startsWith("index ") || line.startsWith("new file") || line.startsWith("deleted file") || line.startsWith("similarity") || line.startsWith("rename ") || line.startsWith("old mode") || line.startsWith("new mode")) continue;
    if (line.startsWith("Binary files")) {
      f.binary = true;
      continue;
    }
    if (line.startsWith("@@")) {
      const m = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)/.exec(line);
      o = m ? +m[1] : 0;
      n = m ? +m[2] : 0;
      f.lines.push({ type: "hunk", text: m ? m[3].trim() : line });
      continue;
    }
    if (line.startsWith("+")) {
      f.lines.push({ type: "add", text: line.slice(1), new: n++ });
      f.added++;
    } else if (line.startsWith("-")) {
      f.lines.push({ type: "del", text: line.slice(1), old: o++ });
      f.removed++;
    } else if (line.startsWith(" ")) {
      f.lines.push({ type: "ctx", text: line.slice(1), old: o++, new: n++ });
    }
  }
  return files;
}

/** Line diff of two texts (LCS), for edits Claude asks to make. */
export function lineDiff(a: string, b: string, startLine = 1): DiffLine[] {
  const x = a.split("\n");
  const y = b.split("\n");
  if (x.length * y.length > 2_000_000) {
    return [...x.map((t, i) => ({ type: "del" as const, text: t, old: startLine + i })), ...y.map((t, i) => ({ type: "add" as const, text: t, new: startLine + i }))];
  }
  const m = x.length;
  const n = y.length;
  const dp = Array.from({ length: m + 1 }, () => new Uint32Array(n + 1));
  for (let i = m - 1; i >= 0; i--) for (let j = n - 1; j >= 0; j--) dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < m || j < n) {
    if (i < m && j < n && x[i] === y[j]) {
      out.push({ type: "ctx", text: x[i], old: startLine + i, new: startLine + j });
      i++;
      j++;
    } else if (i < m && (j >= n || dp[i + 1][j] >= dp[i][j + 1])) {
      out.push({ type: "del", text: x[i], old: startLine + i });
      i++;
    } else {
      out.push({ type: "add", text: y[j], new: startLine + j });
      j++;
    }
  }
  return out;
}

export function DiffLines(p: { lines: DiffLine[]; numbers?: boolean; class?: string }) {
  return (
    <div class={cls("overflow-x-auto font-mono text-[12px] leading-[1.6]", p.class)}>
      <table class="w-full border-collapse">
        <tbody>
          <For each={p.lines}>
            {(l) => (
              <Show
                when={l.type !== "hunk"}
                fallback={
                  <tr class="bg-working-soft/60 text-fg-3">
                    <td colSpan={p.numbers === false ? 2 : 4} class="px-3 py-0.5 text-[11px]">
                      ⋯ {l.text}
                    </td>
                  </tr>
                }
              >
                <tr class={cls(l.type === "add" && "bg-add-soft", l.type === "del" && "bg-del-soft")}>
                  <Show when={p.numbers !== false}>
                    <td class="w-10 select-none px-2 text-right align-top text-fg-3/70">{l.old ?? ""}</td>
                    <td class="w-10 select-none px-2 text-right align-top text-fg-3/70">{l.new ?? ""}</td>
                  </Show>
                  <td class={cls("w-5 select-none text-center align-top", l.type === "add" ? "text-add" : l.type === "del" ? "text-del" : "text-fg-3/50")}>
                    {l.type === "add" ? "+" : l.type === "del" ? "−" : ""}
                  </td>
                  <td class={cls("whitespace-pre pr-4 align-top", l.type === "add" ? "text-add" : l.type === "del" ? "text-del" : "text-fg-2")}>{l.text || " "}</td>
                </tr>
              </Show>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
}

export function UnifiedDiff(p: { text: string }) {
  const files = createMemo(() => parseUnified(p.text));
  return (
    <Show when={files().length} fallback={<div class="px-4 py-10 text-center text-fg-3">No changes against HEAD</div>}>
      <div class="flex flex-col gap-4">
        <For each={files()}>
          {(f) => (
            <section class="overflow-hidden rounded-xl border border-line bg-panel">
              <header class="sticky top-0 z-[1] flex items-center gap-2 border-b border-line bg-raised/95 px-3 py-2 backdrop-blur">
                <span class="truncate font-mono text-[12px] text-fg">{f.name}</span>
                <span class="ml-auto font-mono text-[11.5px] tabular-nums">
                  <span class="text-add">+{f.added}</span> <span class="text-del">−{f.removed}</span>
                </span>
              </header>
              <Show when={!f.binary} fallback={<div class="px-3 py-3 text-fg-3">Binary file</div>}>
                <DiffLines lines={f.lines} />
              </Show>
            </section>
          )}
        </For>
      </div>
    </Show>
  );
}
