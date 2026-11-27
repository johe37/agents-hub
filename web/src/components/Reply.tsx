import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import { getJSON, type Agent } from "../api";
import { act, draft, isBusy, setDraft, toast } from "../store";
import { basename, cls, fuzzy } from "../util";
import { heard, listening, toggleVoice, voiceAgent, voiceSupported } from "../voice";
import { ImagePlus, Mic, Send } from "./icons";
import { Spinner } from "./ui";

type Completions = { commands: { name: string; description: string }[]; files: string[] };
type Token = { kind: "/" | "@"; start: number; query: string };
type Item = { value: string; label: string; detail: string };

const lists = new Map<string, { at: number; data: Promise<Completions> }>();
function completions(agent: string): Promise<Completions> {
  const c = lists.get(agent);
  if (c && Date.now() - c.at < 30_000) return c.data;
  const data = getJSON<Completions>(`/api/complete?agent=${encodeURIComponent(agent)}`).catch(() => ({ commands: [], files: [] }));
  lists.set(agent, { at: Date.now(), data });
  return data;
}

/** The word at the caret, when it asks for a completion. Claude Code only reads a slash command at the start of the prompt. */
function tokenAt(text: string, caret: number): Token | null {
  const m = /(^|\s)([/@])(\S*)$/.exec(text.slice(0, caret));
  if (!m) return null;
  const start = m.index + m[1].length;
  if (m[2] === "/" && text.slice(0, start).trim()) return null;
  return { kind: m[2] as "/" | "@", start, query: m[3] };
}

const MAX_ITEMS = 50;

function rank(tok: Token, data: Completions): Item[] {
  if (tok.kind === "/") {
    const scored = data.commands.flatMap((c) => {
      const m = fuzzy(tok.query, c.name);
      return m ? [[m.score, { value: `/${c.name}`, label: `/${c.name}`, detail: c.description }] as const] : [];
    });
    if (tok.query) scored.sort((a, b) => b[0] - a[0]);
    return scored.slice(0, MAX_ITEMS).map(([, it]) => it);
  }
  if (!tok.query) return data.files.slice(0, MAX_ITEMS).map((f) => ({ value: `@${f}`, label: basename(f), detail: f }));
  const scored: [number, string][] = [];
  for (const f of data.files) {
    const m = fuzzy(tok.query, f);
    if (m) scored.push([m.score, f]);
  }
  scored.sort((a, b) => b[0] - a[0]);
  return scored.slice(0, MAX_ITEMS).map(([, f]) => ({ value: `@${f}`, label: basename(f), detail: f }));
}

async function upload(file: File): Promise<string> {
  const r = await fetch("/api/upload", { method: "POST", headers: { "Content-Type": file.type, "x-jugglr": "1" }, body: file });
  if (!r.ok) throw new Error(await r.text());
  return ((await r.json()) as { path: string }).path;
}

/** One line to the agent: Enter sends it, as if typed in its terminal. / completes commands, @ completes files. */
export function Reply(p: { agent: Agent; placeholder?: string; ref?: (el: HTMLTextAreaElement) => void; autofocus?: boolean; class?: string }) {
  const [text, setText] = createSignal("");
  let area!: HTMLTextAreaElement;
  let picker: HTMLDivElement | undefined;
  let fileInput!: HTMLInputElement;
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

  const [tok, setTok] = createSignal<Token | null>(null);
  const [data, setData] = createSignal<Completions | null>(null);
  const [sel, setSel] = createSignal(0);
  // Escape closes the list until the caret leaves the word.
  let dismissed = -1;
  const check = () => {
    const t = tokenAt(area.value, area.selectionStart);
    if (!t || t.start !== dismissed) dismissed = -1;
    const open = t && dismissed < 0 ? t : null;
    if (open?.query !== tok()?.query || open?.kind !== tok()?.kind) setSel(0);
    setTok(open);
    if (open) void completions(p.agent.key).then(setData);
  };
  const items = createMemo(() => {
    const t = tok();
    const d = data();
    return t && d ? rank(t, d) : [];
  });
  createEffect(() => picker?.querySelector(`[data-i="${sel()}"]`)?.scrollIntoView({ block: "nearest" }));

  const replace = (start: number, end: number, insert: string) => {
    const v = area.value;
    const next = v.slice(0, start) + insert + v.slice(end);
    area.value = next;
    setText(next);
    const caret = start + insert.length;
    area.setSelectionRange(caret, caret);
    fit();
  };
  const accept = (it: Item) => {
    const t = tok();
    if (!t) return;
    replace(t.start, area.selectionStart, `${it.value} `);
    setTok(null);
    area.focus();
  };

  const [uploading, setUploading] = createSignal(0);
  const attach = async (files: File[]) => {
    const images = files.filter((f) => f.type.startsWith("image/"));
    if (!images.length) return false;
    setUploading((n) => n + images.length);
    for (const f of images) {
      try {
        const path = await upload(f);
        const at = area.selectionStart;
        const before = area.value.slice(0, at);
        const pad = before && !/\s$/.test(before) ? " " : "";
        replace(at, area.selectionEnd, `${pad}${path} `);
      } catch (e) {
        toast(`Could not attach the image: ${(e as Error).message}`, true);
      } finally {
        setUploading((n) => n - 1);
      }
    }
    area.focus();
    return true;
  };

  const send = async () => {
    const t = text().trim();
    if (!t || uploading()) return;
    const r = await act({ type: "reply", agent: p.agent.key, text: t }, { key: key() });
    if (r.ok) {
      setText("");
      area.value = "";
      fit();
      setTok(null);
    }
  };
  const voice = () => voiceAgent() === p.agent.key;
  const onKeyDown = (e: KeyboardEvent & { currentTarget: HTMLTextAreaElement }) => {
    e.stopPropagation();
    const list = items();
    if (tok() && list.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setSel((i) => (i + (e.key === "ArrowDown" ? 1 : list.length - 1)) % list.length);
        return;
      }
      if ((e.key === "Enter" && !e.shiftKey && !e.isComposing) || e.key === "Tab") {
        e.preventDefault();
        accept(list[sel()]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        dismissed = tok()!.start;
        setTok(null);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    } else if (e.key === "Escape") {
      e.currentTarget.blur();
    }
  };
  return (
    <div
      class={cls(
        "group relative flex items-end gap-2 rounded-xl border border-line-strong bg-raised px-3 py-1.5 transition-[border-color,box-shadow] focus-within:border-accent/60 focus-within:shadow-[0_0_0_3px_color-mix(in_oklab,var(--accent)_15%,transparent)]",
        p.class,
      )}
      onDragOver={(e) => {
        if (p.agent.reachable && e.dataTransfer?.types.includes("Files")) e.preventDefault();
      }}
      onDrop={(e) => {
        const files = [...(e.dataTransfer?.files ?? [])];
        if (!p.agent.reachable || !files.length) return;
        e.preventDefault();
        void attach(files);
      }}
    >
      <Show when={tok() && items().length}>
        <div
          ref={picker}
          class="absolute inset-x-0 bottom-full z-20 mb-2 max-h-72 overflow-y-auto rounded-xl border border-line-strong bg-raised p-1 shadow-float animate-pop-in"
          role="listbox"
        >
          <For each={items()}>
            {(it, i) => (
              <button
                data-i={i()}
                role="option"
                aria-selected={i() === sel()}
                class={cls("flex w-full min-w-0 items-baseline gap-3 rounded-lg px-2.5 py-1.5 text-left", i() === sel() ? "bg-hover text-fg" : "text-fg-2")}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setSel(i())}
                onClick={() => accept(it)}
              >
                <span class="shrink-0 font-mono text-sm">{it.label}</span>
                <span class="min-w-0 truncate text-xs text-fg-3">{it.detail}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
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
        class="max-h-40 min-h-[26px] flex-1 resize-none bg-transparent py-[3px] text-base text-fg outline-none placeholder:text-fg-3"
        onInput={(e) => {
          setText(e.currentTarget.value);
          fit();
          check();
        }}
        onClick={check}
        onKeyUp={(e) => (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "Home" || e.key === "End") && check()}
        onBlur={() => setTok(null)}
        onPaste={(e) => {
          const files = [...(e.clipboardData?.files ?? [])];
          if (files.some((f) => f.type.startsWith("image/"))) {
            e.preventDefault();
            void attach(files);
          }
        }}
        onKeyDown={onKeyDown}
      />
      <input
        ref={fileInput}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        multiple
        class="hidden"
        onChange={(e) => {
          void attach([...(e.currentTarget.files ?? [])]);
          e.currentTarget.value = "";
        }}
      />
      <button
        class="mb-px grid size-7 shrink-0 place-items-center rounded-lg text-fg-3 transition-all hover:text-fg"
        disabled={!p.agent.reachable || uploading() > 0}
        onClick={() => fileInput.click()}
        aria-label="Attach an image"
        title="Attach an image (or paste or drop one)"
      >
        {uploading() ? <Spinner size={13} /> : <ImagePlus size={14} />}
      </button>
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
        disabled={!text().trim() || isBusy(key()) || uploading() > 0}
        onClick={send}
        aria-label="Send"
        title="Send (Enter)"
      >
        {isBusy(key()) ? <Spinner size={13} /> : <Send size={14} />}
      </button>
    </div>
  );
}
