import { For, Match, Show, Switch, createEffect, createResource, createSignal, on, onCleanup, type JSX } from "solid-js";
import { getJSON, type Agent, type ConversationItem } from "../api";
import { act, isBusy, tilde } from "../store";
import { cls, toolLine } from "../util";
import { Check, ChevronDown, Stop, Wrench, X } from "./icons";
import { Markdown } from "./Markdown";
import { Reply } from "./Reply";
import { ToolDetail } from "./ToolCall";
import { Button, Spinner, StatusDot } from "./ui";

type Conv = { items: ConversationItem[]; truncated?: boolean; missing?: boolean };

function ToolRow(p: { item: Extract<ConversationItem, { kind: "tool" }> }) {
  const [open, setOpen] = createSignal(false);
  const line = () => tilde(toolLine(p.item.input) ?? "");
  const state = () => (p.item.result ? (p.item.result.error ? "error" : "ok") : "running");
  return (
    <div class="group">
      <button
        class="flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1 text-left text-[12.5px] transition-colors hover:bg-hover"
        onClick={() => setOpen((o) => !o)}
      >
        <span class={cls("grid size-5 shrink-0 place-items-center rounded-md", state() === "error" ? "bg-danger-soft text-danger" : "bg-hover text-fg-3")}>
          <Switch fallback={<Wrench size={11} />}>
            <Match when={state() === "running"}>
              <Spinner size={10} />
            </Match>
            <Match when={state() === "error"}>
              <X size={11} />
            </Match>
          </Switch>
        </span>
        <span class="shrink-0 font-medium text-fg-2">{p.item.name}</span>
        <span class="min-w-0 truncate font-mono text-[11.5px] text-fg-3">{line().split("\n")[0]}</span>
        <ChevronDown size={13} class={cls("ml-auto shrink-0 text-fg-3 opacity-0 transition-[opacity,transform] group-hover:opacity-100", open() && "rotate-180 opacity-100")} />
      </button>
      <Show when={open()}>
        <div class="mb-2 ml-9 mt-1 flex flex-col gap-2 animate-fade-in">
          <ToolDetail name={p.item.name} input={p.item.input} compact />
          <Show when={p.item.result?.text}>
            <pre class={cls("max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-lg border px-3 py-2 font-mono text-[11.5px] leading-relaxed", p.item.result!.error ? "border-danger/30 bg-danger-soft text-danger" : "border-line bg-bg text-fg-2")}>
              {p.item.result!.text}
            </pre>
          </Show>
        </div>
      </Show>
    </div>
  );
}

const FOLD = 3;

/** A long run of tool calls folds to its last few, so the messages around it stay readable. */
function ToolGroup(p: { items: Extract<ConversationItem, { kind: "tool" }>[] }) {
  const [open, setOpen] = createSignal(false);
  const hidden = () => (open() ? 0 : Math.max(0, p.items.length - FOLD));
  const counts = () => {
    const m = new Map<string, number>();
    for (const it of p.items.slice(0, hidden())) m.set(it.name, (m.get(it.name) ?? 0) + 1);
    return [...m].map(([n, c]) => (c > 1 ? `${n} ×${c}` : n)).join(", ");
  };
  return (
    <div class="-my-1 flex flex-col">
      <Show when={hidden() > 0}>
        <button class="flex items-center gap-2 rounded-lg px-2 py-1 text-left text-[12.5px] text-fg-3 hover:bg-hover hover:text-fg-2" onClick={() => setOpen(true)}>
          <span class="grid size-5 place-items-center rounded-md bg-hover">
            <Wrench size={11} />
          </span>
          {hidden()} more tool calls
          <span class="truncate text-fg-3/80">· {counts()}</span>
        </button>
      </Show>
      <For each={p.items.slice(hidden())}>{(it) => <ToolRow item={it} />}</For>
    </div>
  );
}

/** Tool calls between two messages fold into one group, so the text stays readable. */
function group(items: ConversationItem[]) {
  const out: ({ kind: "msg"; item: ConversationItem } | { kind: "tools"; items: Extract<ConversationItem, { kind: "tool" }>[] })[] = [];
  for (const it of items) {
    if (it.kind === "tool") {
      const last = out[out.length - 1];
      if (last?.kind === "tools") last.items.push(it);
      else out.push({ kind: "tools", items: [it] });
    } else out.push({ kind: "msg", item: it });
  }
  return out;
}

/** action adds a control under each answer from Claude. */
export function Conversation(p: { agent: Agent; action?: (text: string) => JSX.Element }) {
  const a = () => p.agent;
  // Reload when the transcript changes; a working agent writes many times a second, so at most twice a second.
  const [tick, setTick] = createSignal(a().activity);
  let timer: number | undefined;
  createEffect(
    on(
      () => a().activity,
      (v) => {
        clearTimeout(timer);
        timer = window.setTimeout(() => setTick(v), 400);
      },
    ),
  );
  onCleanup(() => clearTimeout(timer));
  const [conv] = createResource(
    () => [a().key, tick()] as const,
    ([key]) => getJSON<Conv>(`/api/conversation?agent=${encodeURIComponent(key)}`),
  );

  let scroller!: HTMLDivElement;
  const [stuck, setStuck] = createSignal(true);
  const onScroll = () => setStuck(scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80);
  const toBottom = (smooth = false) => scroller?.scrollTo({ top: scroller.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  createEffect(
    on(
      () => conv.latest?.items.length,
      () => {
        if (stuck()) requestAnimationFrame(() => toBottom());
      },
    ),
  );
  createEffect(on(() => a().key, () => setStuck(true)));

  return (
    <div class="relative flex h-full min-h-0 flex-col">
      <div ref={scroller} onScroll={onScroll} class="relative min-h-0 flex-1 overflow-y-auto">
        <div class="mx-auto flex w-full max-w-3xl flex-col gap-4 px-4 py-6 sm:px-8">
          <Show when={conv.latest} fallback={<div class="flex flex-col gap-3">{[0, 1, 2].map(() => <div class="skeleton h-16" />)}</div>}>
            <Show when={conv.latest!.truncated}>
              <div class="text-center text-[12px] text-fg-3">Earlier messages are in the full transcript</div>
            </Show>
            <Show when={!conv.latest!.items.length}>
              <div class="py-16 text-center text-fg-3">No messages yet. Write the first prompt below.</div>
            </Show>
            <For each={group(conv.latest!.items)}>
              {(g) => (
                <Switch>
                  <Match when={g.kind === "tools" && g}>
                    {(t) => <ToolGroup items={t().items} />}
                  </Match>
                  <Match when={g.kind === "msg" && g.item.kind === "user" && g.item}>
                    {(m) => (
                      <div class="ml-auto max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-br-md bg-accent-soft px-4 py-2.5 text-fg animate-fade-in">
                        {m().text}
                      </div>
                    )}
                  </Match>
                  <Match when={g.kind === "msg" && g.item.kind === "assistant" && g.item}>
                    {(m) => (
                      <div>
                        <Markdown text={m().text} class="text-[14px] leading-relaxed text-fg animate-fade-in" />
                        {p.action?.(m().text)}
                      </div>
                    )}
                  </Match>
                </Switch>
              )}
            </For>
            <Show when={a().status === "working"}>
              <div class="flex items-center gap-2 text-[12.5px] text-working">
                <StatusDot status="working" size={7} />
                <span class="truncate">{a().tool ?? "Thinking…"}</span>
              </div>
            </Show>
          </Show>
        </div>
      </div>
      <Show when={!stuck()}>
        <button
          class="absolute bottom-28 left-1/2 z-10 -translate-x-1/2 rounded-full border border-line-strong bg-raised px-3 py-1 text-[12px] text-fg-2 shadow-float hover:text-fg animate-pop-in"
          onClick={() => toBottom(true)}
        >
          ↓ Latest
        </button>
      </Show>
      <Composer agent={a()} />
    </div>
  );
}

export function Composer(p: { agent: Agent }) {
  const a = () => p.agent;
  return (
    <div class="border-t border-line bg-panel/80 backdrop-blur">
      <div class="mx-auto w-full max-w-3xl px-4 pb-4 pt-3 sm:px-8">
        <Show when={a().needs_permission}>
          <div class="mb-3 rounded-xl border border-waiting/40 bg-waiting-soft/50 p-3 animate-pop-in">
            <div class="mb-2 text-[12.5px] font-medium text-waiting">{a().message ?? "Claude asks for permission"}</div>
            <Show when={a().pending_tool}>
              <div class="mb-3">
                <ToolDetail name={a().pending_tool!.name} input={a().pending_tool!.input} compact />
              </div>
            </Show>
            <div class="flex gap-2">
              <Button variant="approve" size="sm" disabled={!a().reachable} loading={isBusy(`approve:${a().key}`)} onClick={() => act({ type: "approve", agent: a().key }, { key: `approve:${a().key}` })}>
                <Check size={14} stroke-width={2.25} /> Approve
              </Button>
              <Button size="sm" disabled={!a().reachable} loading={isBusy(`deny:${a().key}`)} onClick={() => act({ type: "deny", agent: a().key }, { key: `deny:${a().key}` })}>
                <X size={14} /> Deny
              </Button>
            </div>
          </div>
        </Show>
        <div class="flex items-end gap-2">
          <Reply agent={a()} class="flex-1" placeholder={a().status === "working" ? "Queue a message for Claude…" : undefined} />
          <Show when={a().status === "working" && a().reachable}>
            <Button variant="secondary" class="h-[38px]" title="Interrupt (Escape in Claude)" onClick={() => act({ type: "interrupt", agent: a().key })}>
              <Stop size={13} /> Stop
            </Button>
          </Show>
        </div>
      </div>
    </div>
  );
}
