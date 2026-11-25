import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import type { Agent } from "../api";
import { act, agentRoute, ask, go, isBusy, loaded, newChat, now, setComposer, state, store } from "../store";
import { STATUS, age, alive, cls, fuzzyWords, modelName, money } from "../util";
import { AgentCard } from "./AgentCard";
import { Conversation } from "./Conversation";
import { Bot, Branch, Chat, Check, Chevron, Done, Play, Search, Sparkles, Trash } from "./icons";
import { Badge, Button, Empty, Kbd, StatusDot, StatusPill } from "./ui";

const GROUPS: { title: string; test: (a: Agent) => boolean; collapsed?: boolean }[] = [
  { title: "Working", test: (a) => a.status === "working" },
  { title: "Idle", test: (a) => a.status === "idle" || a.status === "unknown" },
  { title: "Closed", test: (a) => !alive(a.status), collapsed: true },
];

export function Agents() {
  const [q, setQ] = createSignal("");
  const filtered = createMemo(() => {
    const query = q().trim();
    const list = query ? state.agents.filter((a) => fuzzyWords(query, `${a.title} ${a.name} ${a.branch ?? ""} ${a.short_path}`) !== null) : state.agents;
    return [...list].sort((a, b) => STATUS[a.status].rank - STATUS[b.status].rank || b.timestamp - a.timestamp);
  });
  // Oldest first: the agent that has waited longest gets answered first.
  const waiting = createMemo(() => filtered().filter((a) => a.status === "waiting").sort((a, b) => a.timestamp - b.timestamp));
  const working = () => store.live().filter((a) => a.status === "working").length;
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
  let cards!: HTMLDivElement;
  createEffect(() => {
    const el = cards?.children[focus()] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  });

  const summary = () => {
    const n = store.waiting().length;
    if (n) return `${n} agent${n > 1 ? "s" : ""} need${n > 1 ? "" : "s"} you.`;
    const w = working();
    return `Nothing needs you.${w ? ` ${w} agent${w > 1 ? "s" : ""} at work.` : ""}`;
  };

  return (
    <div class="mx-auto w-full max-w-4xl px-4 pb-24 pt-6 sm:px-8">
      <header class="mb-6 flex flex-wrap items-end gap-3">
        <div>
          <h1 class="text-[22px] font-semibold tracking-[-0.02em] text-fg">Agents</h1>
          <p class="mt-0.5 text-fg-2">
            <Show when={loaded()} fallback="Connecting to your agents…">
              {summary()}
            </Show>
          </p>
        </div>
        <div class="ml-auto flex w-full items-center gap-2 sm:w-auto">
          <label class="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-lg border border-line-strong bg-raised px-3 focus-within:border-accent/60 sm:w-64 sm:flex-none">
            <Search size={14} class="text-fg-3" />
            <input
              value={q()}
              onInput={(e) => setQ(e.currentTarget.value)}
              onKeyDown={(e) => e.key === "Escape" && (setQ(""), e.currentTarget.blur())}
              placeholder="Filter by task, repo or branch"
              class="min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-fg-3"
              data-filter
            />
          </label>
          <Button variant="secondary" onClick={newChat} kbd="⇧C">
            <Chat size={15} /> New chat
          </Button>
          <Button variant="primary" onClick={() => setComposer({ path: null })} kbd="N">
            <Sparkles size={15} /> New agent
          </Button>
        </div>
      </header>

      <Show when={loaded()} fallback={<div class="flex flex-col gap-3">{[0, 1].map(() => <div class="skeleton h-44" />)}</div>}>
        <Show when={state.agents.length} fallback={<Empty icon={<Bot size={22} />} title="No agents yet">Start one with N or from a workspace.</Empty>}>
          <section class="mb-8">
            <GroupTitle title="Needs you" count={waiting().length} />
            <Show
              when={waiting().length}
              fallback={
                <Empty icon={<Check size={20} />} title="All clear" class="rounded-2xl border border-dashed border-line py-8">
                  Agents that ask for permission or wait for your reply show up here. Answer them without leaving this page.
                </Empty>
              }
            >
              <div ref={cards} class="flex flex-col gap-3">
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
          </section>

          <For each={GROUPS}>
            {(g) => {
              const list = () => filtered().filter(g.test);
              const [open, setOpen] = createSignal(!g.collapsed);
              return (
                <Show when={list().length}>
                  <section class="mb-6">
                    <GroupTitle title={g.title} count={list().length} open={open()} onToggle={g.collapsed ? () => setOpen((o) => !o) : undefined} />
                    <Show when={open() || q().trim()}>
                      <div class="overflow-hidden rounded-2xl border border-line bg-panel">
                        <For each={list()}>{(a) => <AgentLine agent={a} />}</For>
                      </div>
                    </Show>
                  </section>
                </Show>
              );
            }}
          </For>
        </Show>
      </Show>
    </div>
  );
}

function GroupTitle(p: { title: string; count: number; open?: boolean; onToggle?: () => void }) {
  const label = (
    <>
      {p.title} <span class="ml-1 tabular-nums">{p.count}</span>
    </>
  );
  return (
    <h2 class="mb-2 px-1 text-[12px] font-semibold uppercase tracking-[0.06em] text-fg-3">
      <Show when={p.onToggle} fallback={label}>
        <button class="inline-flex items-center gap-1 uppercase hover:text-fg-2" onClick={() => p.onToggle?.()} aria-expanded={p.open}>
          <Chevron size={12} class={cls("transition-transform", p.open && "rotate-90")} />
          {label}
        </button>
      </Show>
    </h2>
  );
}

function AgentLine(p: { agent: Agent }) {
  const a = () => p.agent;
  const detail = () =>
    a().status === "working" ? a().tool : a().status === "waiting" ? a().message : (a().reply?.split("\n").find((l) => l.trim()) ?? a().prompt);
  return (
    <div class="group flex items-center gap-3 border-b border-line px-4 py-3 last:border-b-0 hover:bg-hover/60">
      <StatusDot status={a().status} />
      <button class="min-w-0 flex-1 text-left" onClick={() => go(agentRoute(a()))}>
        <div class="flex items-baseline gap-2">
          <span class="truncate font-medium text-fg">{a().title}</span>
          <Show when={a().background}>
            <Badge>background</Badge>
          </Show>
          <Show when={!a().workspace && alive(a().status) && !a().background}>
            <Badge title="Runs outside jugglr">outside</Badge>
          </Show>
          <Show when={a().status === "idle" && a().dirty > 0}>
            <Badge title="Changed files in its checkout">{a().dirty} changed</Badge>
          </Show>
        </div>
        <div class="mt-0.5 flex min-w-0 items-center gap-2 text-[12.5px] text-fg-3">
          <span class="shrink-0 text-fg-2">{a().name}</span>
          <Show when={a().branch}>
            <span class="inline-flex min-w-0 max-w-[40%] items-center gap-1">
              <Branch size={11} class="shrink-0" />
              <span class="truncate">{a().branch}</span>
            </span>
          </Show>
          <Show when={detail()}>
            <span class="truncate">· {detail()}</span>
          </Show>
        </div>
      </button>
      <div class="hidden shrink-0 items-center gap-3 text-[12px] tabular-nums text-fg-3 md:flex">
        <Show when={modelName(a().model)}>
          <span>{modelName(a().model)}</span>
        </Show>
        <Show when={money(a().cost)}>
          <span>{money(a().cost)}</span>
        </Show>
        <span class="w-8 text-right">{age(a().timestamp, now())}</span>
      </div>
      <Show when={!alive(a().status) && a().id}>
        <Button size="sm" variant="ghost" loading={isBusy(`resume:${a().key}`)} onClick={() => act({ type: "resume", agent: a().key }, { key: `resume:${a().key}` }).then((r) => r.ok && r.workspace && r.window && go({ view: "workspace", name: r.workspace, window: r.window }))}>
          <Play size={12} /> Resume
        </Button>
      </Show>
    </div>
  );
}

/** An agent without a hub window: outside the hub, in the background, or closed. */
export function AgentPage(p: { agentKey: string }) {
  const a = createMemo(() => state.agents.find((x) => x.key === p.agentKey));
  return (
    <Show when={a()} fallback={<Empty icon={<Bot size={22} />} title="This agent is gone" class="h-full">It closed, or its record expired.</Empty>}>
      {(ag) => {
        // An agent adopted into or resumed in the hub moves to its window.
        const moved = () => ag().workspace && ag().window;
        return (
          <div class="flex h-full min-h-0 flex-col">
            <header class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-line px-4 py-3 sm:px-5">
              <div class="min-w-0">
                <div class="flex items-center gap-2">
                  <StatusPill status={ag().status} />
                  <h1 class="truncate text-[16px] font-semibold tracking-[-0.01em] text-fg">{ag().title}</h1>
                </div>
                <div class="mt-0.5 flex items-center gap-2 text-[12.5px] text-fg-3">
                  <span>{ag().short_path}</span>
                  <Show when={ag().branch}>
                    <span class="inline-flex items-center gap-1">
                      <Branch size={11} /> {ag().branch}
                    </span>
                  </Show>
                  <Show when={modelName(ag().model)}>
                    <span>· {modelName(ag().model)}</span>
                  </Show>
                </div>
              </div>
              <div class="ml-auto flex items-center gap-2">
                <Show when={moved()}>
                  <Button size="sm" variant="primary" onClick={() => go(agentRoute(ag()))}>
                    Open in workspace
                  </Button>
                </Show>
                <Show when={ag().status === "waiting" && !ag().needs_permission}>
                  <Button size="sm" variant="ghost" onClick={() => act({ type: "mark_done", agent: ag().key })} title="Stop showing it as waiting">
                    <Done size={13} /> Done
                  </Button>
                </Show>
                <Show when={ag().adoptable}>
                  <Button
                    size="sm"
                    variant="primary"
                    title="Stop it where it runs and continue it in a hub workspace"
                    onClick={() =>
                      ask({
                        title: `Adopt “${ag().title}”?`,
                        body: `It stops where it runs now and continues in a jugglr workspace for ${ag().short_path}.${ag().status === "working" || ag().status === "waiting" ? " Its current turn gets cut off." : ""}`,
                        confirm: "Adopt",
                        run: () => act({ type: "adopt", agent: ag().key }),
                      })
                    }
                  >
                    Adopt into hub
                  </Button>
                </Show>
                <Show when={!alive(ag().status) && ag().id}>
                  <Button size="sm" variant="primary" loading={isBusy(`resume:${ag().key}`)} onClick={() => act({ type: "resume", agent: ag().key }, { key: `resume:${ag().key}` }).then((r) => r.ok && r.workspace && r.window && go({ view: "workspace", name: r.workspace, window: r.window }))}>
                    <Play size={12} /> Resume
                  </Button>
                </Show>
                <Show when={ag().background && alive(ag().status)}>
                  <Button size="sm" variant="danger" onClick={() => ask({ title: `Stop “${ag().title}”?`, body: "This ends its background Claude session.", confirm: "Stop", danger: true, run: () => act({ type: "close_agent", agent: ag().key }) })}>
                    <Trash size={13} /> Stop
                  </Button>
                </Show>
              </div>
            </header>
            <div class={cls("min-h-0 flex-1")}>
              <Conversation agent={ag()} />
            </div>
          </div>
        );
      }}
    </Show>
  );
}
