import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import type { Agent } from "../api";
import { act, agentRoute, go, loaded, now, setComposer, sortAgents, state, store } from "../store";
import { age, cls } from "../util";
import { AgentCard } from "./AgentCard";
import { Check, Chevron, Sparkles } from "./icons";
import { Badge, Button, Empty, Kbd, StatusDot } from "./ui";

function Section(p: { title: string; count?: number; children: any; action?: any }) {
  return (
    <section class="mt-8 first:mt-0">
      <div class="mb-2.5 flex items-center gap-2 px-1">
        <h2 class="text-[12px] font-semibold uppercase tracking-[0.06em] text-fg-3">{p.title}</h2>
        <Show when={p.count}>
          <span class="text-[12px] tabular-nums text-fg-3">{p.count}</span>
        </Show>
        <div class="ml-auto">{p.action}</div>
      </div>
      {p.children}
    </section>
  );
}

function AgentRow(p: { agent: Agent; detail: string | null }) {
  return (
    <button
      onClick={() => go(agentRoute(p.agent))}
      class="group flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors hover:bg-hover"
    >
      <StatusDot status={p.agent.status} />
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline gap-2">
          <span class="truncate font-medium text-fg">{p.agent.title}</span>
          <span class="shrink-0 text-[12px] text-fg-3">{p.agent.name}</span>
        </div>
        <Show when={p.detail}>
          <div class="truncate text-[12.5px] text-fg-2">{p.detail}</div>
        </Show>
      </div>
      <span class="shrink-0 text-[12px] tabular-nums text-fg-3">{age(p.agent.timestamp, now())}</span>
      <Chevron size={14} class="shrink-0 text-fg-3 opacity-0 transition-opacity group-hover:opacity-100" />
    </button>
  );
}

export function Inbox() {
  const waiting = createMemo(() => sortAgents(store.waiting()).sort((a, b) => a.timestamp - b.timestamp));
  const working = createMemo(() => state.agents.filter((a) => a.status === "working"));
  const finished = createMemo(() => store.finished().slice(0, 8));
  const [focus, setFocus] = createSignal(0);
  const replies = new Map<string, HTMLTextAreaElement>();

  createEffect(() => {
    if (focus() >= waiting().length) setFocus(Math.max(0, waiting().length - 1));
  });

  const current = () => waiting()[focus()];
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target as HTMLElement;
    if (t.closest("input,textarea,[contenteditable],.xterm")) return;
    const a = current();
    switch (e.key) {
      case "j":
      case "ArrowDown":
        setFocus((f) => Math.min(f + 1, waiting().length - 1));
        break;
      case "k":
      case "ArrowUp":
        setFocus((f) => Math.max(f - 1, 0));
        break;
      case "a":
        if (a?.needs_permission) act({ type: "approve", agent: a.key }, { key: `approve:${a.key}` });
        break;
      case "d":
        if (a?.needs_permission) act({ type: "deny", agent: a.key }, { key: `deny:${a.key}` });
        break;
      case "m":
        if (a && !a.needs_permission) act({ type: "mark_done", agent: a.key });
        break;
      case "r":
        if (a) replies.get(a.key)?.focus();
        break;
      case "Enter":
      case "o":
        if (a) go(agentRoute(a));
        break;
      default:
        return;
    }
    e.preventDefault();
  };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));

  // Keep the focused card in view as j/k moves.
  let list!: HTMLDivElement;
  createEffect(() => {
    const el = list?.children[focus()] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  });

  return (
    <div class="mx-auto w-full max-w-3xl px-4 pb-24 pt-6 sm:px-8">
      <header class="mb-6 flex items-end gap-3">
        <div>
          <h1 class="text-[22px] font-semibold tracking-[-0.02em] text-fg">Inbox</h1>
          <p class="mt-0.5 text-fg-2">
            <Show when={loaded()} fallback="Connecting to your agents…">
              {waiting().length === 0
                ? `Nothing needs you. ${working().length ? `${working().length} agent${working().length > 1 ? "s" : ""} at work.` : ""}`
                : `${waiting().length} agent${waiting().length > 1 ? "s" : ""} need${waiting().length > 1 ? "" : "s"} you.`}
            </Show>
          </p>
        </div>
        <Button variant="primary" class="ml-auto" onClick={() => setComposer({ path: null })} kbd="N">
          <Sparkles size={15} /> New agent
        </Button>
      </header>

      <Show when={loaded()} fallback={<div class="flex flex-col gap-3">{[0, 1].map(() => <div class="skeleton h-44" />)}</div>}>
        <Show
          when={waiting().length}
          fallback={
            <Empty icon={<Check size={22} />} title="All clear" class="rounded-2xl border border-dashed border-line py-12">
              Agents that ask for permission or wait for your reply show up here. Answer them without leaving this page.
            </Empty>
          }
        >
          <div ref={list} class="flex flex-col gap-3">
            <For each={waiting()}>
              {(a, i) => <AgentCard agent={a} focused={focus() === i()} onFocus={() => setFocus(i())} replyRef={(el) => replies.set(a.key, el)} />}
            </For>
          </div>
          <div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 px-1 text-[12px] text-fg-3">
            <span><Kbd>j</Kbd> <Kbd>k</Kbd> move</span>
            <span><Kbd>a</Kbd> approve</span>
            <span><Kbd>d</Kbd> deny</span>
            <span><Kbd>r</Kbd> reply</span>
            <span><Kbd>m</Kbd> done</span>
            <span><Kbd>↵</Kbd> open</span>
          </div>
        </Show>

        <Show when={working().length}>
          <Section title="Working" count={working().length}>
            <div class="rounded-2xl border border-line bg-panel p-1">
              <For each={working()}>{(a) => <AgentRow agent={a} detail={a.tool ?? a.prompt} />}</For>
            </div>
          </Section>
        </Show>

        <Show when={finished().length}>
          <Section title="Ready for review" count={finished().length}>
            <div class="rounded-2xl border border-line bg-panel p-1">
              <For each={finished()}>
                {(a) => (
                  <div class="flex items-center">
                    <div class="min-w-0 flex-1">
                      <AgentRow agent={a} detail={a.reply?.split("\n").find((l) => l.trim()) ?? null} />
                    </div>
                    <Show when={a.dirty > 0}>
                      <Badge class={cls("mr-3 shrink-0")} title="Changed files in its checkout">
                        {a.dirty} changed
                      </Badge>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Section>
        </Show>
      </Show>
    </div>
  );
}
