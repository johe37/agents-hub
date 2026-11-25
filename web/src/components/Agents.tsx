import { For, Show, createMemo, createSignal } from "solid-js";
import type { Agent } from "../api";
import { act, agentRoute, ask, go, isBusy, now, state } from "../store";
import { STATUS, age, alive, cls, fuzzyWords, modelName, money } from "../util";
import { Conversation } from "./Conversation";
import { Bot, Branch, Play, Search, Trash } from "./icons";
import { Badge, Button, Empty, StatusDot, StatusPill } from "./ui";

const GROUPS: { title: string; test: (a: Agent) => boolean }[] = [
  { title: "Needs you", test: (a) => a.status === "waiting" },
  { title: "Working", test: (a) => a.status === "working" },
  { title: "Idle", test: (a) => a.status === "idle" || a.status === "unknown" },
  { title: "Closed", test: (a) => !alive(a.status) },
];

export function Agents() {
  const [q, setQ] = createSignal("");
  const filtered = createMemo(() => {
    const query = q().trim();
    const list = query ? state.agents.filter((a) => fuzzyWords(query, `${a.title} ${a.name} ${a.branch ?? ""} ${a.short_path}`) !== null) : state.agents;
    return [...list].sort((a, b) => STATUS[a.status].rank - STATUS[b.status].rank || b.timestamp - a.timestamp);
  });
  return (
    <div class="mx-auto w-full max-w-5xl px-4 pb-24 pt-6 sm:px-8">
      <header class="mb-5 flex flex-wrap items-end gap-3">
        <div>
          <h1 class="text-[22px] font-semibold tracking-[-0.02em] text-fg">Agents</h1>
          <p class="mt-0.5 text-fg-2">Every Claude session on this machine, in the hub or outside it.</p>
        </div>
        <label class="ml-auto flex h-9 w-full items-center gap-2 rounded-lg border border-line-strong bg-raised px-3 focus-within:border-accent/60 sm:w-72">
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
      </header>
      <Show when={state.agents.length} fallback={<Empty icon={<Bot size={22} />} title="No agents yet">Start one with N or from a workspace.</Empty>}>
        <For each={GROUPS}>
          {(g) => {
            const list = () => filtered().filter(g.test);
            return (
              <Show when={list().length}>
                <section class="mb-6">
                  <h2 class="mb-2 px-1 text-[12px] font-semibold uppercase tracking-[0.06em] text-fg-3">
                    {g.title} <span class="ml-1 tabular-nums">{list().length}</span>
                  </h2>
                  <div class="overflow-hidden rounded-2xl border border-line bg-panel">
                    <For each={list()}>{(a) => <AgentLine agent={a} />}</For>
                  </div>
                </section>
              </Show>
            );
          }}
        </For>
      </Show>
    </div>
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
            <Badge title="Runs outside Claude Hub">outside</Badge>
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
                <Show when={ag().adoptable}>
                  <Button
                    size="sm"
                    variant="primary"
                    title="Stop it where it runs and continue it in a hub workspace"
                    onClick={() =>
                      ask({
                        title: `Adopt “${ag().title}”?`,
                        body: `It stops where it runs now and continues in a Claude Hub workspace for ${ag().short_path}.${ag().status === "working" || ag().status === "waiting" ? " Its current turn gets cut off." : ""}`,
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
