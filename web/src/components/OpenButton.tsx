import { For, Show, type JSX } from "solid-js";
import { agentLabel, hasAgent, isBusy, progress, state } from "../store";
import { cls } from "../util";
import { Bot, Check, ChevronDown, Layers, Terminal } from "./icons";
import { Button, MenuButton, Spinner } from "./ui";

export type Kind = "agent" | "shell";

export interface Choice {
  kind: Kind;
  title: () => string;
  body: () => string;
  key: string;
  icon: (size: number) => JSX.Element;
}

const ALL_CHOICES: Choice[] = [
  { kind: "agent", title: agentLabel, body: () => `${state.agent.claude ? "Claude Code" : agentLabel()} in this directory`, key: "c", icon: (s) => <Bot size={s} /> },
  { kind: "shell", title: () => "Terminal", body: () => "A shell: run, test or start your editor", key: "t", icon: (s) => <Terminal size={s} /> },
];

/** What a new workspace or window can start with. */
export const openChoices = () => ALL_CHOICES.filter((c) => c.kind !== "agent" || hasAgent());

/** The steps the hub reports for a checkout, in order. A step it skips, such as a clone that exists, never shows. */
const STEPS: [string, string][] = [
  ["clone", "Cloning the project"],
  ["checkout", "Checking out the code"],
  ["start", "Starting"],
];
const stepLabel = (step: string) => STEPS.find(([s]) => s === step)?.[1] ?? step;

/** Short text for where a long action is, such as "Cloning the project 42%", or null before the hub says. */
export function stepText(key: string): string | null {
  const s = progress()[key];
  if (!s) return null;
  return `${stepLabel(s.step)}${s.pct != null ? ` ${s.pct}%` : ""}`;
}

/** The steps of a long action: done ones checked, the current one spinning, with a bar when its share is known. */
export function Steps(p: { busyKey: string; fallback: string; class?: string }) {
  const s = () => progress()[p.busyKey];
  const rows = () => {
    const cur = s();
    if (!cur) return [];
    const at = STEPS.findIndex(([x]) => x === cur.step);
    const later = at < 0 ? [] : STEPS.slice(at + 1).map(([x]) => x);
    return [...cur.seen.map((x) => ({ step: x, state: x === cur.step ? "now" : "done" })), ...later.map((x) => ({ step: x, state: "todo" }))];
  };
  return (
    <Show
      when={s()}
      fallback={
        <span class={cls("flex items-center gap-2", p.class)}>
          <Spinner size={14} /> {p.fallback}
        </span>
      }
    >
      {(cur) => (
        <ol class={cls("flex flex-col gap-1.5 text-left", p.class)}>
          <For each={rows()}>
            {(r) => (
              <li class={cls("flex items-center gap-2", r.state === "todo" ? "text-fg-3" : "text-fg-2", r.state === "now" && "text-fg")}>
                <span class="grid size-4 place-items-center">
                  <Show when={r.state === "now"} fallback={r.state === "done" ? <Check size={13} class="text-idle" /> : <span class="size-1.5 rounded-full bg-line-strong" />}>
                    <Spinner size={13} />
                  </Show>
                </span>
                <span class="flex-1">{stepLabel(r.step)}</span>
                <Show when={r.state === "now" && cur().pct != null}>
                  <span class="tabular-nums text-fg-3">{cur().pct}%</span>
                </Show>
              </li>
            )}
          </For>
          <Show when={cur().pct != null}>
            <div class="ml-6 h-1 overflow-hidden rounded-full bg-hover">
              <div class="h-full rounded-full bg-accent transition-[width] duration-300" style={{ width: `${cur().pct}%` }} />
            </div>
          </Show>
        </ol>
      )}
    </Show>
  );
}

/**
 * Open something in a workspace: go to its workspace when one is open, else pick what it starts with.
 * The arrow next to "Go to" adds a window to the open workspace.
 */
export function OpenButton(p: { workspace?: string | null; busyKey: string; open: (k: Kind) => void; go: () => void; size?: "sm" | "md" }) {
  const busy = () => isBusy(p.busyKey);
  const items = () =>
    openChoices().map((c) => ({
      label: p.workspace ? `New ${c.kind === "agent" ? agentLabel() : "terminal"} in ${p.workspace}` : c.title(),
      icon: c.icon(14),
      hint: c.key,
      run: () => p.open(c.kind),
    }));
  return (
    <Show
      when={!busy()}
      fallback={
        <Button size={p.size ?? "sm"} variant="secondary" disabled class="!opacity-100">
          <Spinner size={13} /> {stepText(p.busyKey) ?? "Opening…"}
        </Button>
      }
    >
      <Show
        when={p.workspace}
        fallback={<MenuButton label="Open in a workspace (c, t)" size={p.size ?? "sm"} variant="primary" icon={<Layers size={13} />} text={<>Open <ChevronDown size={12} class="-mr-0.5 opacity-80" /></>} items={items()} />}
      >
        {(ws) => (
          <span class="inline-flex">
            <Button size={p.size ?? "sm"} variant="primary" class="rounded-r-none" onClick={p.go} title={`Go to the workspace ${ws()}`}>
              <Layers size={13} /> Go to {ws()}
            </Button>
            <MenuButton label="New window in the workspace (c, t)" size={p.size ?? "sm"} variant="primary" class="rounded-l-none border-l border-accent-fg/20 !px-1.5" icon={<ChevronDown size={12} />} text="" items={items()} />
          </span>
        )}
      </Show>
    </Show>
  );
}
