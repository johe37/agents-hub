import { Index, Match, Show, Switch, createEffect, createMemo, createResource, createSignal, on, onCleanup, type JSX } from "solid-js";
import { getJSON, type Agent, type ConversationItem } from "../api";
import { act, isBusy, tilde } from "../store";
import { basename, cls, toolLine } from "../util";
import { EditView, TurnChanges, change, isEdit, keep, type Change, type Tool } from "./ChatEdits";
import { Check, ChevronDown, Stop, Wrench, X } from "./icons";
import { Markdown } from "./Markdown";
import { Reply } from "./Reply";
import { ToolDetail } from "./ToolCall";
import { Button, Spinner, StatusDot } from "./ui";

type Conv = { items: ConversationItem[]; queued?: string[]; truncated?: boolean; missing?: boolean };

const GREETINGS = ["Let's get ready to juggle!!!", "Toss me the first ball.", "Hands warm. Balls ready. What's the trick?", "Juggle time. What are we building?"];

/** Stable per agent, so the greeting does not change while you look at it. */
function greeting(key: string) {
  let h = 0;
  for (const c of key) h = (h * 31 + c.charCodeAt(0)) | 0;
  return GREETINGS[Math.abs(h) % GREETINGS.length];
}

const typing = (el: Element | null) => !!el && (el.matches("input, textarea, select") || (el as HTMLElement).isContentEditable);

type Entry = { kind: "msg"; item: ConversationItem } | { kind: "tools"; items: Tool[] } | { kind: "changes"; changes: Change[] };

function todoLine(input: Record<string, unknown>) {
  const todos = Array.isArray(input.todos) ? (input.todos as { status?: string }[]) : [];
  return todos.length ? `${todos.filter((t) => t.status === "completed").length}/${todos.length} done` : "";
}

/** Edits, failed calls and Claude's latest plan show open: they are what you want to see. */
function ToolRow(p: { item: Tool; agent: Agent; plan: boolean }) {
  const toggled = createMemo(() => keep<boolean | null>(`open:${p.item.id}`, null));
  const open = () => toggled()[0]() ?? (isEdit(p.item) || p.plan || !!p.item.result?.error);
  const line = () => (p.item.name === "TodoWrite" ? todoLine(p.item.input) : tilde(toolLine(p.item.input) ?? ""));
  const state = () => (p.item.result ? (p.item.result.error ? "error" : "ok") : "running");
  const edit = createMemo(() => change(p.item));
  return (
    <div class="group">
      <button
        class="flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1 text-left text-sm transition-colors hover:bg-hover"
        onClick={() => toggled()[1](!open())}
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
        <span class="min-w-0 truncate font-mono text-xs text-fg-3">{line().split("\n")[0]}</span>
        <Show when={edit()}>
          {(c) => (
            <span class="shrink-0 font-mono text-xs">
              <span class="text-add">+{c().added}</span> <span class="text-del">−{c().removed}</span>
            </span>
          )}
        </Show>
        <ChevronDown size={13} class={cls("ml-auto shrink-0 text-fg-3 opacity-0 transition-[opacity,transform] group-hover:opacity-100", open() && "rotate-180 opacity-100")} />
      </button>
      <Show when={open()}>
        <div class="mb-2 ml-9 mt-1 flex flex-col gap-2 animate-fade-in">
          <Show
            when={edit()}
            fallback={
              <>
                <ToolDetail name={p.item.name} input={p.item.input} compact />
                <Show when={p.item.result?.text}>
                  <pre class={cls("max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-lg border px-3 py-2 font-mono text-xs leading-relaxed", p.item.result!.error ? "border-danger/30 bg-danger-soft text-danger" : "border-line bg-bg text-fg-2")}>
                    {p.item.result!.text}
                  </pre>
                </Show>
              </>
            }
          >
            {(c) => <EditView id={p.item.id} agent={p.agent} change={c()} />}
          </Show>
        </div>
      </Show>
    </div>
  );
}

const FOLD = 3;

/** A long run of tool calls folds to its last few, so the messages around it stay readable. Edits never fold. */
function ToolGroup(p: { items: Tool[]; agent: Agent; plan: string | null }) {
  const [open, setOpen] = createSignal(false);
  const keepShown = (it: Tool, i: number) => isEdit(it) || it.id === p.plan || !!it.result?.error || i >= p.items.length - FOLD;
  const shown = createMemo(() => (open() ? p.items : p.items.filter(keepShown)));
  const hidden = () => p.items.length - shown().length;
  const counts = () => {
    const m = new Map<string, number>();
    p.items.forEach((it, i) => keepShown(it, i) || m.set(it.name, (m.get(it.name) ?? 0) + 1));
    return [...m].map(([n, c]) => (c > 1 ? `${n} ×${c}` : n)).join(", ");
  };
  return (
    <div class="-my-1 flex flex-col">
      <Show when={hidden() > 0}>
        <button class="flex items-center gap-2 rounded-lg px-2 py-1 text-left text-sm text-fg-3 hover:bg-hover hover:text-fg-2" onClick={() => setOpen(true)}>
          <span class="grid size-5 place-items-center rounded-md bg-hover">
            <Wrench size={11} />
          </span>
          {hidden()} more tool calls
          <span class="truncate text-fg-3/80">· {counts()}</span>
        </button>
      </Show>
      <Index each={shown()}>{(it) => <ToolRow item={it()} agent={p.agent} plan={it().id === p.plan} />}</Index>
    </div>
  );
}

const message = (e: Entry, kind: "user" | "assistant") => (e.kind === "msg" && e.item.kind === kind ? e.item : null);

/** Tool calls between two messages fold into one group, and a turn that edited files ends with a list of them. */
function group(items: ConversationItem[], done: boolean) {
  const out: Entry[] = [];
  let turn: Change[] = [];
  const flush = () => {
    if (turn.length) out.push({ kind: "changes", changes: turn });
    turn = [];
  };
  for (const it of items) {
    if (it.kind === "user") flush();
    if (it.kind === "tool") {
      const c = change(it);
      if (c) turn.push(c);
      const last = out[out.length - 1];
      if (last?.kind === "tools") last.items.push(it);
      else out.push({ kind: "tools", items: [it] });
    } else out.push({ kind: "msg", item: it });
  }
  if (done) flush();
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

  const plan = createMemo(() => {
    let id: string | null = null;
    for (const it of conv.latest?.items ?? []) if (it.kind === "tool" && it.name === "TodoWrite") id = it.id;
    return id;
  });

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

  let replyEl: HTMLTextAreaElement | undefined;
  let focusedFor: string | null = null;
  // Focus the reply box once per agent, when it shows. Not on phones: the keyboard would cover the page.
  createEffect(() => {
    if (conv.state !== "ready" || focusedFor === a().key) return;
    focusedFor = a().key;
    if (typing(document.activeElement) || !matchMedia("(min-width: 640px)").matches) return;
    requestAnimationFrame(() => replyEl?.focus());
  });

  return (
    <div class="relative flex h-full min-h-0 flex-col">
      <div ref={scroller} onScroll={onScroll} class="relative min-h-0 flex-1 overflow-y-auto">
        <div class="mx-auto flex w-full max-w-5xl flex-col gap-4 px-4 py-6 sm:px-8">
          <Show when={conv.latest} fallback={<div class="flex flex-col gap-3">{[0, 1, 2].map(() => <div class="skeleton h-16" />)}</div>}>
            <Show when={conv.latest!.truncated}>
              <div class="text-center text-sm text-fg-3">Earlier messages are in the full transcript</div>
            </Show>
            <Show when={!conv.latest!.items.length}>
              <div class="flex flex-col items-center gap-2 py-16 text-center animate-fade-in">
                <img src="/logo-juggling.webp" alt="" class="mb-2 size-24" />
                <div class="text-xl font-semibold text-fg">{greeting(a().key)}</div>
                <div class="font-mono text-sm text-fg-3">
                  {basename(a().path)}
                  <Show when={a().branch}> · {a().branch}</Show>
                </div>
                <div class="text-sm text-fg-3">Write the first prompt below.</div>
              </div>
            </Show>
            <Index each={group(conv.latest!.items, a().status !== "working")}>
              {(g) => (
                <Switch>
                  <Match when={g().kind === "tools" && (g() as Extract<Entry, { kind: "tools" }>)}>
                    {(t) => <ToolGroup items={t().items} agent={a()} plan={plan()} />}
                  </Match>
                  <Match when={g().kind === "changes" && (g() as Extract<Entry, { kind: "changes" }>)}>
                    {(c) => <TurnChanges agent={a()} changes={c().changes} />}
                  </Match>
                  <Match when={message(g(), "user")}>
                    {(m) => (
                      <div class="ml-auto max-w-[min(85%,65ch)] whitespace-pre-wrap break-words rounded-2xl rounded-br-md bg-accent-soft px-4 py-2.5 text-fg animate-fade-in">
                        {m().text}
                      </div>
                    )}
                  </Match>
                  <Match when={message(g(), "assistant")}>
                    {(m) => (
                      <div>
                        <Markdown text={m().text} class="chat-measure text-base leading-relaxed text-fg animate-fade-in" />
                        {p.action?.(m().text)}
                      </div>
                    )}
                  </Match>
                </Switch>
              )}
            </Index>
            <Index each={conv.latest!.queued ?? []}>
              {(t) => (
                <div class="ml-auto flex max-w-[min(85%,65ch)] flex-col items-end gap-1 animate-fade-in" title="Claude reads it when it gets to a pause">
                  <div class="whitespace-pre-wrap break-words rounded-2xl rounded-br-md border border-dashed border-accent/50 px-4 py-2.5 text-fg-2">{t()}</div>
                  <span class="text-xs text-fg-3">Queued</span>
                </div>
              )}
            </Index>
            <Show when={a().status === "working"}>
              <div class="flex items-center gap-2 text-sm text-working">
                <StatusDot status="working" size={7} />
                <span class="truncate">{a().tool ?? "Thinking…"}</span>
              </div>
            </Show>
          </Show>
        </div>
      </div>
      <Show when={!stuck()}>
        <button
          class="absolute bottom-28 left-1/2 z-10 -translate-x-1/2 rounded-full border border-line-strong bg-raised px-3 py-1 text-sm text-fg-2 shadow-float hover:text-fg animate-pop-in"
          onClick={() => toBottom(true)}
        >
          ↓ Latest
        </button>
      </Show>
      <Composer agent={a()} ref={(el) => (replyEl = el)} />
    </div>
  );
}

export function Composer(p: { agent: Agent; ref?: (el: HTMLTextAreaElement) => void }) {
  const a = () => p.agent;
  return (
    <div class="border-t border-line bg-panel/80 backdrop-blur">
      <div class="mx-auto w-full max-w-5xl px-4 pb-4 pt-3 sm:px-8">
        <Show when={a().needs_permission}>
          <div class="mb-3 rounded-xl border border-waiting/40 bg-waiting-soft/50 p-3 animate-pop-in">
            <div class="mb-2 text-sm font-medium text-waiting">{a().message ?? "Claude asks for permission"}</div>
            <Show when={a().pending_tool}>
              {(t) => (
                <div class="mb-3">
                  <Show when={change({ kind: "tool", id: "pending", ts: "", summary: null, ...t() })} fallback={<ToolDetail name={t().name} input={t().input} compact />}>
                    {(c) => <EditView id={`pending:${a().key}`} agent={a()} change={c()} class="max-h-[40vh]" />}
                  </Show>
                </div>
              )}
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
          <Reply agent={a()} ref={p.ref} class="flex-1" placeholder={a().status === "working" ? "Queue a message for Claude…" : undefined} />
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
