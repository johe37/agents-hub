import { For, Show } from "solid-js";
import { cls } from "../util";

export interface DiffLine {
  type: "add" | "del" | "ctx" | "hunk";
  text: string;
  old?: number;
  new?: number;
  /** Old and new line counters at this line, as GitLab's line codes count them. */
  pos?: [number, number];
}

export interface DiffFile {
  name: string;
  lines: DiffLine[];
  added: number;
  removed: number;
  binary: boolean;
}

/** The lines of a diff's hunks: everything from the first `@@` on. */
export function parseHunks(text: string): { lines: DiffLine[]; added: number; removed: number } {
  const out = { lines: [] as DiffLine[], added: 0, removed: 0 };
  let o = 0;
  let n = 0;
  for (const line of text.split("\n")) {
    if (line.startsWith("@@")) {
      const m = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)/.exec(line);
      o = m ? +m[1] : 0;
      n = m ? +m[2] : 0;
      out.lines.push({ type: "hunk", text: m ? m[3].trim() : line });
    } else if (!out.lines.length) {
      continue;
    } else if (line.startsWith("+")) {
      out.lines.push({ type: "add", text: line.slice(1), new: n, pos: [o, n++] });
      out.added++;
    } else if (line.startsWith("-")) {
      out.lines.push({ type: "del", text: line.slice(1), old: o, pos: [o++, n] });
      out.removed++;
    } else if (line.startsWith(" ")) {
      out.lines.push({ type: "ctx", text: line.slice(1), old: o, new: n, pos: [o++, n++] });
    }
  }
  return out;
}

export function parseUnified(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let f: DiffFile | null = null;
  let body: string[] = [];
  const close = () => {
    if (f) Object.assign(f, parseHunks(body.join("\n")));
    body = [];
  };
  for (const line of text.split("\n")) {
    if (line.startsWith("diff --git")) {
      close();
      const m = / b\/(.*)$/.exec(line);
      f = { name: m ? m[1] : line, lines: [], added: 0, removed: 0, binary: false };
      files.push(f);
      continue;
    }
    if (!f) continue;
    if (!body.length && line.startsWith("+++ ")) {
      const name = line.slice(4).replace(/^b\//, "");
      if (name !== "/dev/null") f.name = name;
    } else if (!body.length && line.startsWith("Binary files")) {
      f.binary = true;
    } else if (body.length || line.startsWith("@@")) {
      body.push(line);
    }
  }
  close();
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

/** pick makes the gutter clickable: a click selects a line, Shift+click extends the selection to it. */
export function DiffLines(p: { lines: DiffLine[]; numbers?: boolean; class?: string; sel?: [number, number] | null; pick?: (i: number, extend: boolean) => void }) {
  const picked = (i: number) => !!p.sel && i >= p.sel[0] && i <= p.sel[1];
  const gutter = (i: number) => (p.pick ? { onClick: (e: MouseEvent) => p.pick!(i, e.shiftKey), class: "cursor-pointer hover:text-fg" } : { class: "" });
  return (
    <div class={cls("overflow-x-auto font-mono text-sm leading-[1.6]", p.class)}>
      <table class="w-full border-collapse">
        <tbody>
          <For each={p.lines}>
            {(l, i) => (
              <Show
                when={l.type !== "hunk"}
                fallback={
                  <tr class="bg-working-soft/60 text-fg-3">
                    <td colSpan={p.numbers === false ? 2 : 4} class="px-3 py-0.5 text-xs">
                      ⋯ {l.text}
                    </td>
                  </tr>
                }
              >
                <tr class={picked(i()) ? "bg-accent-soft shadow-[inset_3px_0_0_var(--accent)]" : cls(l.type === "add" && "bg-add-soft", l.type === "del" && "bg-del-soft")}>
                  <Show when={p.numbers !== false}>
                    <td onClick={gutter(i()).onClick} class={cls("w-10 select-none px-2 text-right align-top text-fg-3/70", gutter(i()).class)}>{l.old ?? ""}</td>
                    <td onClick={gutter(i()).onClick} class={cls("w-10 select-none px-2 text-right align-top text-fg-3/70", gutter(i()).class)}>{l.new ?? ""}</td>
                  </Show>
                  <td onClick={gutter(i()).onClick} class={cls("w-5 select-none text-center align-top", l.type === "add" ? "text-add" : l.type === "del" ? "text-del" : "text-fg-3/50", gutter(i()).class)}>
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
