import { For, Show, createEffect, on } from "solid-js";
import { confirmBox, dismiss, helpOpen, prefix, setConfirmBox, setHelpOpen, setPrefix, toasts } from "../store";
import { MOD, cls } from "../util";
import { Check, X } from "./icons";
import { Button, IconButton, Kbd, SectionTitle } from "./ui";

export function Toasts() {
  return (
    <div class="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-[min(24rem,calc(100vw-2rem))] flex-col gap-2" aria-live="polite">
      <For each={toasts()}>
        {(t) => (
          <div
            class={cls(
              "pointer-events-auto flex items-start gap-2.5 rounded-xl border bg-raised px-3.5 py-2.5 text-base shadow-float animate-slide-up",
              t.error ? "border-danger/40" : "border-line-strong",
            )}
          >
            <span class={cls("mt-0.5 grid size-4 shrink-0 place-items-center rounded-full", t.error ? "bg-danger text-white" : "bg-idle text-white dark:text-[#06140c]")}>
              {t.error ? <X size={10} stroke-width={3} /> : <Check size={10} stroke-width={3} />}
            </span>
            <span class="min-w-0 flex-1 break-words text-fg">{t.text}</span>
            <button class="-mr-1 shrink-0 text-fg-3 hover:text-fg" onClick={() => dismiss(t.id)} aria-label="Dismiss">
              <X size={14} />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}

export function Confirm() {
  let ok!: HTMLButtonElement;
  createEffect(on(confirmBox, (c) => c && queueMicrotask(() => ok?.focus())));
  const close = () => setConfirmBox(null);
  return (
    <Show when={confirmBox()}>
      {(c) => (
        <div class="fixed inset-0 z-[55] grid place-items-center bg-black/35 px-4 backdrop-blur-[2px] animate-fade-in" onMouseDown={(e) => e.target === e.currentTarget && close()}>
          <div
            class="w-full max-w-md rounded-2xl border border-line-strong bg-raised p-5 shadow-float animate-pop-in"
            role="alertdialog"
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Escape") close();
            }}
          >
            <h2 class="text-lg font-semibold text-fg">{c().title}</h2>
            <Show when={c().body}>
              <p class="mt-1.5 text-fg-2">{c().body}</p>
            </Show>
            <div class="mt-5 flex justify-end gap-2">
              <Button variant="ghost" onClick={close}>
                Cancel
              </Button>
              <Button
                ref={ok}
                variant={c().danger ? "danger" : "primary"}
                onClick={() => {
                  // c() throws once the box is closed, so take the action first.
                  const run = c().run;
                  close();
                  run();
                }}
              >
                {c().confirm}
              </Button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}

const KEYS: [string, [string, string][]][] = [
  [
    "Everywhere",
    [
      [`${MOD} k`, "Search and run anything"],
      ["Ctrl ⇧ k", "Palette, from inside a terminal"],
      ["Ctrl ⇧ Space", "Prefix: the next key in a terminal is a shortcut"],
      ["n", "New agent with a task"],
      ["⇧ c", "New chat: Claude outside any repo"],
      ["o", "Open a workspace"],
      ["g a / g l", "Go to agents / GitLab"],
      ["w", "Next agent that needs you"],
      ["|", "Hide or show the sidebar"],
      ["z", "Focus mode: only the content"],
      [",", "Settings"],
      ["?", "This help"],
    ],
  ],
  [
    "Agents",
    [
      ["j / k", "Next / previous waiting agent"],
      ["a / d", "Approve / deny"],
      ["r", "Reply"],
      ["m", "Mark done"],
      ["↵", "Open the agent"],
    ],
  ],
  [
    "Workspace",
    [
      ["[ / ]", "Previous / next window"],
      ["Alt 1–9", "Window by number"],
      ["Alt Shift ← / →", "Move the window left / right"],
      ["c / t", "New Claude / terminal"],
      ["v", "Chat or terminal for an agent"],
      ["s", "Show a window beside, or close the split"],
      ["m", "Mark the agent done"],
      ["\\", "Hide or show the side panel"],
    ],
  ],
  [
    "GitLab",
    [
      ["1 / 2 / 3 / 4", "To-Do / to review / my MRs / issues"],
      ["x", "Mark the to-do done"],
      ["j / k", "Next / previous"],
      ["/", "Filter"],
      ["c", "Claude: review, branch or issue"],
      ["t", "Terminal on the MR's code"],
      ["d / l / p", "Changes / commits / pipeline"],
      ["[ / ]", "Previous / next file"],
      ["Click, ⇧ click", "Select lines in a diff"],
      ["a / c", "Ask Claude / comment on them"],
      ["s", "Unified or side by side"],
      ["Esc", "Clear the selection, then back"],
      ["o", "Open in GitLab"],
      ["⇧ r", "Fetch again"],
      ["b", "Hide or show the list"],
      ["\\", "Claude chat on the MR"],
    ],
  ],
];

/** Replay a key outside the terminal, where the app's own shortcuts listen. */
export function forward(e: { key: string; code?: string; shiftKey?: boolean; altKey?: boolean }) {
  const { key, code, shiftKey, altKey } = e;
  document.body.dispatchEvent(new KeyboardEvent("keydown", { key, code, shiftKey, altKey, bubbles: true, cancelable: true }));
}

type PrefixItem = { key: string; shift?: boolean; label: string };

const PREFIX_ROOT: [string, PrefixItem[]][] = [
  [
    "Window",
    [
      { key: "c", label: "New Claude" },
      { key: "t", label: "New terminal" },
      { key: "[", label: "Previous window" },
      { key: "]", label: "Next window" },
      { key: "s", label: "Split or close the split" },
      { key: "v", label: "Chat or terminal" },
      { key: "m", label: "Mark the agent done" },
      { key: "\\", label: "Side panel" },
    ],
  ],
  [
    "App",
    [
      { key: "w", label: "Next agent that needs you" },
      { key: "n", label: "New agent with a task" },
      { key: "C", shift: true, label: "New chat" },
      { key: "o", label: "Open a workspace" },
      { key: "g", label: "Go to…" },
      { key: "z", label: "Focus mode" },
      { key: "|", label: "Sidebar" },
      { key: "?", label: "All shortcuts" },
    ],
  ],
];

const PREFIX_G: [string, PrefixItem[]][] = [
  [
    "Go to",
    [
      { key: "a", label: "Agents" },
      { key: "l", label: "GitLab" },
    ],
  ],
];

/** Run a key typed after the prefix. "g" starts a chord, so the prefix stays armed for its second key. */
export function pressPrefixed(key: string, shiftKey = false) {
  const chord = prefix();
  setPrefix(key === "g" && chord === "" ? "g" : null);
  if (key === "Escape") return;
  if (chord === "g") forward({ key: "g" });
  forward({ key, shiftKey });
}

export function PrefixMenu() {
  const label = (it: PrefixItem) => (it.shift ? `⇧ ${it.key.toLowerCase()}` : it.key);
  return (
    <Show when={prefix() !== null}>
      <div class="fixed inset-0 z-50 grid place-items-center bg-black/20 px-4 animate-fade-in" onMouseDown={(e) => e.target === e.currentTarget && setPrefix(null)} data-prefix-menu>
        <div class="w-full max-w-lg rounded-2xl border border-accent bg-raised p-5 shadow-float animate-pop-in">
          <div class="mb-3 flex items-center gap-2 text-sm text-fg-2">
            <span class="font-semibold text-fg">{prefix() === "g" ? "Go to" : "Prefix"}</span>
            <span>press a key</span>
            <span class="ml-auto flex items-center gap-1"><Kbd>esc</Kbd> cancels</span>
          </div>
          <div class={cls("grid gap-5", prefix() === "" && "sm:grid-cols-2")}>
            <For each={prefix() === "g" ? PREFIX_G : PREFIX_ROOT}>
              {([group, items]) => (
                <section>
                  <SectionTitle class="mb-1.5">{group}</SectionTitle>
                  <div class="flex flex-col">
                    <For each={items}>
                      {(it) => (
                        <button class="flex items-center gap-3 rounded-lg px-2 py-1.5 text-left text-sm text-fg-2 hover:bg-hover hover:text-fg" onMouseDown={(e) => e.preventDefault()} onClick={() => pressPrefixed(it.key, it.shift)}>
                          <span class="w-7 shrink-0"><Kbd>{label(it)}</Kbd></span>
                          {it.label}
                        </button>
                      )}
                    </For>
                  </div>
                </section>
              )}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}

export function Help() {
  return (
    <Show when={helpOpen()}>
      <div class="fixed inset-0 z-50 grid place-items-center bg-black/30 px-4 backdrop-blur-[2px] animate-fade-in" onMouseDown={(e) => e.target === e.currentTarget && setHelpOpen(false)}>
        <div class="w-full max-w-2xl rounded-2xl border border-line-strong bg-raised p-6 shadow-float animate-pop-in" onKeyDown={(e) => e.key === "Escape" && setHelpOpen(false)}>
          <div class="mb-4 flex items-center">
            <h2 class="text-lg font-semibold text-fg">Keyboard shortcuts</h2>
            <IconButton label="Close" class="ml-auto" onClick={() => setHelpOpen(false)} ref={(el) => queueMicrotask(() => el.focus())}>
              <X size={16} />
            </IconButton>
          </div>
          <div class="grid gap-6 sm:grid-cols-2">
            <For each={KEYS}>
              {([group, keys]) => (
                <section>
                  <SectionTitle class="mb-2">{group}</SectionTitle>
                  <div class="flex flex-col gap-2">
                    <For each={keys}>
                      {([k, label]) => (
                        <div class="flex items-center justify-between gap-3 text-sm">
                          <span class="text-fg-2">{label}</span>
                          <span class="flex shrink-0 gap-0.5">
                            <For each={k.split(" ")}>{(part) => (part === "/" ? <span class="px-0.5 text-fg-3">/</span> : <Kbd>{part}</Kbd>)}</For>
                          </span>
                        </div>
                      )}
                    </For>
                  </div>
                </section>
              )}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}
