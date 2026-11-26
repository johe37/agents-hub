import { Show } from "solid-js";
import type { Agent } from "../api";
import { act, agentRoute, go, isBusy, now } from "../store";
import { age, cls } from "../util";
import { Branch, Check, Chevron, Done, Hand, X } from "./icons";
import { Markdown } from "./Markdown";
import { Reply } from "./Reply";
import { ToolDetail } from "./ToolCall";
import { Button, StatusPill } from "./ui";

/** A waiting agent with everything needed to answer it without leaving the agents view. */
export function AgentCard(p: { agent: Agent; focused?: boolean; onFocus?: () => void; replyRef?: (el: HTMLTextAreaElement) => void }) {
  const a = () => p.agent;
  const approve = () => act({ type: "approve", agent: a().key }, { key: `approve:${a().key}` });
  const deny = () => act({ type: "deny", agent: a().key }, { key: `deny:${a().key}` });
  return (
    <article
      data-juggle
      tabIndex={0}
      aria-label={`${a().title}, waiting`}
      aria-current={p.focused ? "true" : undefined}
      onFocusIn={() => p.onFocus?.()}
      onClick={() => p.onFocus?.()}
      class={cls(
        "animate-slide-up overflow-hidden rounded-2xl border bg-panel transition-[border-color,box-shadow] duration-150",
        p.focused ? "border-waiting/50 shadow-[0_0_0_3px_color-mix(in_oklab,var(--waiting)_14%,transparent)]" : "border-line shadow-soft hover:border-line-strong",
      )}
    >
      <header class="flex items-center gap-2 px-4 pt-3.5">
        <StatusPill status={a().status} />
        <span class="truncate text-sm font-medium text-fg-2">{a().name}</span>
        <Show when={a().branch}>
          <span class="inline-flex min-w-0 items-center gap-1 text-sm text-fg-3">
            <Branch size={12} />
            <span class="truncate">{a().branch}</span>
          </span>
        </Show>
        <span class="ml-auto shrink-0 text-sm tabular-nums text-fg-3">{age(a().timestamp, now())} ago</span>
        <button
          class="-mr-1.5 inline-flex shrink-0 items-center gap-0.5 rounded-md px-1.5 py-0.5 text-sm text-fg-2 hover:bg-hover hover:text-fg"
          onClick={(e) => {
            e.stopPropagation();
            go(agentRoute(a()));
          }}
        >
          Open <Chevron size={13} />
        </button>
      </header>
      <div class="px-4 pb-1 pt-1.5">
        <h3 class="text-lg font-semibold leading-snug tracking-[-0.01em] text-fg">{a().title}</h3>
        <Show when={a().message}>
          <p class="mt-0.5 flex items-center gap-1.5 text-fg-2">
            <Hand size={14} class="shrink-0 text-waiting" />
            {a().message}
          </p>
        </Show>
      </div>

      <Show
        when={a().needs_permission}
        fallback={
          <Show when={a().reply}>
            <div class="mx-4 mt-2 max-h-56 overflow-y-auto rounded-xl border border-line bg-bg px-3.5 py-2.5 text-base text-fg-2">
              <Markdown text={a().reply!} />
            </div>
          </Show>
        }
      >
        <div class="mx-4 mt-2 rounded-xl border border-line bg-raised p-3">
          <Show when={a().pending_tool} fallback={<div class="text-fg-3">{a().tool ?? "Claude asks to use a tool. Open the agent to see the dialog."}</div>}>
            <div class="mb-2 flex items-center gap-2 text-sm font-medium uppercase tracking-wide text-fg-3">{a().pending_tool!.name}</div>
            <ToolDetail name={a().pending_tool!.name} input={a().pending_tool!.input} compact />
          </Show>
        </div>
      </Show>

      <footer class="flex flex-wrap items-center gap-2 px-4 pb-4 pt-3">
        <Show
          when={a().needs_permission}
          fallback={
            <>
              <Reply agent={a()} ref={p.replyRef} class="min-w-[16rem] flex-1" />
              <Button variant="ghost" size="sm" onClick={() => act({ type: "mark_done", agent: a().key })} title="Stop showing it as waiting (m)">
                <Done size={14} /> Done
              </Button>
            </>
          }
        >
          <Button variant="approve" onClick={approve} disabled={!a().reachable} title={a().reachable ? undefined : "Answer it in its own terminal"} loading={isBusy(`approve:${a().key}`)} kbd={p.focused ? "A" : undefined}>
            <Check size={15} stroke-width={2.25} /> Approve
          </Button>
          <Button variant="secondary" onClick={deny} disabled={!a().reachable} loading={isBusy(`deny:${a().key}`)} kbd={p.focused ? "D" : undefined}>
            <X size={15} /> Deny
          </Button>
          <Reply agent={a()} ref={p.replyRef} placeholder="Or tell Claude what to do instead…" class="min-w-[14rem] flex-1" />
        </Show>
      </footer>
    </article>
  );
}
