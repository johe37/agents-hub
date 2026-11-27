import { For, Match, Show, Switch, createMemo } from "solid-js";
import { DiffLines, lineDiff } from "./Diff";
import { tilde } from "../store";
import { cls } from "../util";
import { Check } from "./icons";

type Input = Record<string, unknown>;

const str = (v: unknown) => (typeof v === "string" ? v : v == null ? "" : JSON.stringify(v, null, 2));

function FilePath(p: { path: string }) {
  const parts = () => {
    const path = tilde(p.path);
    const i = path.lastIndexOf("/");
    return i < 0 ? ["", path] : [path.slice(0, i + 1), path.slice(i + 1)];
  };
  return (
    <div class="truncate font-mono text-sm" title={p.path}>
      <span class="text-fg-3">{parts()[0]}</span>
      <span class="text-fg">{parts()[1]}</span>
    </div>
  );
}

function Block(p: { children: string; class?: string; prompt?: string }) {
  return (
    <pre class={cls("max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-line bg-bg px-3 py-2 font-mono text-sm leading-relaxed text-fg", p.class)}>
      <Show when={p.prompt}>
        <span class="select-none text-accent">{p.prompt} </span>
      </Show>
      {p.children}
    </pre>
  );
}

/** What a tool call does, shown the way you would review it: a command, a diff, a file. */
export function ToolDetail(p: { name: string; input: Input; compact?: boolean }) {
  const edits = createMemo(() => {
    if (p.name === "Edit") return [{ old: str(p.input.old_string), new: str(p.input.new_string) }];
    if (p.name === "MultiEdit" && Array.isArray(p.input.edits)) return (p.input.edits as Input[]).map((e) => ({ old: str(e.old_string), new: str(e.new_string) }));
    return [];
  });
  return (
    <div class="flex min-w-0 flex-col gap-2">
      <Switch
        fallback={
          <Show when={Object.keys(p.input).length}>
            <Block>{JSON.stringify(p.input, null, 2)}</Block>
          </Show>
        }
      >
        <Match when={p.name === "TodoWrite" && Array.isArray(p.input.todos) && (p.input.todos as Input[])}>
          {(todos) => (
            <ul class="flex flex-col gap-1 text-sm">
              <For each={todos()}>
                {(t) => (
                  <li class="flex items-start gap-2">
                    <span
                      class={cls(
                        "mt-[3px] grid size-4 shrink-0 place-items-center rounded-full border",
                        t.status === "completed" ? "border-add bg-add-soft text-add" : t.status === "in_progress" ? "border-accent" : "border-line-strong",
                      )}
                    >
                      <Show when={t.status === "completed"} fallback={<Show when={t.status === "in_progress"}><span class="size-1.5 rounded-full bg-accent" /></Show>}>
                        <Check size={10} stroke-width={3} />
                      </Show>
                    </span>
                    <span class={t.status === "completed" ? "text-fg-3 line-through" : t.status === "in_progress" ? "text-fg" : "text-fg-2"}>
                      {str(t.status === "in_progress" ? (t.activeForm ?? t.content) : t.content)}
                    </span>
                  </li>
                )}
              </For>
            </ul>
          )}
        </Match>
        <Match when={p.name === "Bash" || p.name === "BashOutput"}>
          <Show when={p.input.description}>
            <div class="text-fg-2">{str(p.input.description)}</div>
          </Show>
          <Block prompt="$">{str(p.input.command)}</Block>
        </Match>
        <Match when={edits().length}>
          <FilePath path={str(p.input.file_path)} />
          <div class="overflow-hidden rounded-lg border border-line bg-bg">
            <For each={edits()}>
              {(e, i) => (
                <>
                  <Show when={i() > 0}>
                    <div class="border-t border-line" />
                  </Show>
                  <DiffLines lines={lineDiff(e.old, e.new)} numbers={false} class={p.compact ? "max-h-72" : "max-h-[28rem]"} />
                </>
              )}
            </For>
          </div>
        </Match>
        <Match when={p.name === "Write"}>
          <FilePath path={str(p.input.file_path)} />
          <div class="overflow-hidden rounded-lg border border-line bg-bg">
            <DiffLines lines={str(p.input.content).split("\n").map((t, i) => ({ type: "add" as const, text: t, new: i + 1 }))} class="max-h-80" />
          </div>
        </Match>
        <Match when={p.name === "Read" || p.name === "NotebookEdit"}>
          <FilePath path={str(p.input.file_path ?? p.input.notebook_path)} />
        </Match>
        <Match when={p.name === "Grep" || p.name === "Glob"}>
          <Block>{`${str(p.input.pattern)}${p.input.path ? `   in ${str(p.input.path)}` : ""}`}</Block>
        </Match>
        <Match when={p.name === "WebFetch" || p.name === "WebSearch"}>
          <Block>{str(p.input.url ?? p.input.query)}</Block>
          <Show when={p.input.prompt}>
            <div class="text-fg-2">{str(p.input.prompt)}</div>
          </Show>
        </Match>
        <Match when={p.name === "Task" || p.name === "Agent"}>
          <div class="font-medium text-fg">{str(p.input.description)}</div>
          <Block>{str(p.input.prompt)}</Block>
        </Match>
      </Switch>
    </div>
  );
}
