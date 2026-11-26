import { For, Show, createEffect, on } from "solid-js";
import { confirmBox, dismiss, helpOpen, setConfirmBox, setHelpOpen, toasts } from "../store";
import { MOD, cls } from "../util";
import { Check, X } from "./icons";
import { Button, IconButton, Kbd } from "./ui";

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
      [`${MOD} K`, "Search and run anything"],
      ["Ctrl ⇧ K", "Palette, from inside a terminal"],
      ["N", "New agent with a task"],
      ["⇧ C", "New chat: Claude outside any repo"],
      ["O", "Open a workspace"],
      ["G A / G L", "Go to agents / GitLab"],
      ["W", "Next agent that needs you"],
      ["|", "Hide or show the sidebar"],
      ["Z", "Focus mode: only the content"],
      [",", "Settings"],
      ["?", "This help"],
    ],
  ],
  [
    "Agents",
    [
      ["J / K", "Next / previous waiting agent"],
      ["A / D", "Approve / deny"],
      ["R", "Reply"],
      ["M", "Mark done"],
      ["↵", "Open the agent"],
    ],
  ],
  [
    "Workspace",
    [
      ["[ / ]", "Previous / next window"],
      ["Alt 1–9", "Window by number"],
      ["Alt Shift ← / →", "Move the window left / right"],
      ["C / T", "New Claude / terminal"],
      ["V", "Chat or terminal for an agent"],
      ["M", "Mark the agent done"],
      ["\\", "Hide or show the side panel"],
    ],
  ],
  [
    "GitLab",
    [
      ["1 / 2 / 3 / 4", "To-Do / to review / my MRs / issues"],
      ["x", "Mark the to-do done"],
      ["J / K", "Next / previous"],
      ["/", "Filter"],
      ["C", "Claude: review, branch or issue"],
      ["T", "Terminal on the MR's code"],
      ["D / L / P", "Changes / commits / pipeline"],
      ["[ / ]", "Previous / next file"],
      ["Click, ⇧ click", "Select lines in a diff"],
      ["A / C", "Ask Claude / comment on them"],
      ["S", "Unified or side by side"],
      ["Esc", "Clear the selection, then back"],
      ["O", "Open in GitLab"],
      ["⇧ R", "Fetch again"],
      ["B", "Hide or show the list"],
      ["\\", "Claude chat on the MR"],
    ],
  ],
];

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
                  <h3 class="mb-2 text-xs font-semibold uppercase tracking-[0.06em] text-fg-3">{group}</h3>
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
