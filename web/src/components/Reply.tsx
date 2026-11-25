import { createEffect, createSignal } from "solid-js";
import type { Agent } from "../api";
import { act, draft, isBusy, setDraft } from "../store";
import { cls } from "../util";
import { heard, listening, toggleVoice, voiceAgent, voiceSupported } from "../voice";
import { Mic, Send } from "./icons";
import { Spinner } from "./ui";

/** One line to the agent: Enter sends it, as if typed in its terminal. */
export function Reply(p: { agent: Agent; placeholder?: string; ref?: (el: HTMLTextAreaElement) => void; autofocus?: boolean; class?: string }) {
  const [text, setText] = createSignal("");
  let area!: HTMLTextAreaElement;
  const key = () => `reply:${p.agent.key}`;
  const fit = () => {
    area.style.height = "auto";
    area.style.height = `${Math.min(area.scrollHeight, 160)}px`;
  };
  createEffect(() => {
    const d = draft();
    if (!d || d.agent !== p.agent.key || !area) return;
    setDraft(null);
    setText(d.text);
    area.value = d.text;
    fit();
    area.focus();
  });
  const send = async () => {
    const t = text().trim();
    if (!t) return;
    const r = await act({ type: "reply", agent: p.agent.key, text: t }, { key: key() });
    if (r.ok) {
      setText("");
      fit();
    }
  };
  const voice = () => voiceAgent() === p.agent.key;
  return (
    <div
      class={cls(
        "group flex items-end gap-2 rounded-xl border border-line-strong bg-raised px-3 py-1.5 transition-[border-color,box-shadow] focus-within:border-accent/60 focus-within:shadow-[0_0_0_3px_color-mix(in_oklab,var(--accent)_15%,transparent)]",
        p.class,
      )}
    >
      <textarea
        ref={(el) => {
          area = el;
          p.ref?.(el);
          if (p.autofocus) queueMicrotask(() => el.focus());
        }}
        rows={1}
        value={text()}
        disabled={!p.agent.reachable}
        placeholder={!p.agent.reachable ? "This agent cannot get input from here" : voice() ? (listening() ? heard() || "Listening…" : "Voice on: waiting for Claude…") : (p.placeholder ?? `Reply to ${p.agent.name}…`)}
        class="max-h-40 min-h-[26px] flex-1 resize-none bg-transparent py-[3px] text-[13.5px] text-fg outline-none placeholder:text-fg-3"
        onInput={(e) => {
          setText(e.currentTarget.value);
          fit();
        }}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            void send();
          } else if (e.key === "Escape") {
            e.currentTarget.blur();
          }
        }}
      />
      {voiceSupported && (
        <button
          class={cls(
            "mb-px grid size-7 shrink-0 place-items-center rounded-lg transition-all",
            voice() ? "bg-danger text-accent-fg" : "text-fg-3 hover:text-fg",
            voice() && listening() && "animate-pulse-dot",
          )}
          disabled={!p.agent.reachable}
          onClick={() => toggleVoice(p.agent.key)}
          aria-label={voice() ? "Stop voice chat" : "Voice chat"}
          aria-pressed={voice()}
          title={voice() ? "Stop voice chat" : "Voice chat: talk to Claude, hear the answer"}
        >
          <Mic size={14} />
        </button>
      )}
      <button
        class={cls(
          "mb-px grid size-7 shrink-0 place-items-center rounded-lg transition-all",
          text().trim() ? "bg-accent text-accent-fg hover:brightness-110" : "text-fg-3",
        )}
        disabled={!text().trim() || isBusy(key())}
        onClick={send}
        aria-label="Send"
        title="Send (Enter)"
      >
        {isBusy(key()) ? <Spinner size={13} /> : <Send size={14} />}
      </button>
    </div>
  );
}
