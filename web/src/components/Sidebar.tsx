import { For, Show, createMemo, createSignal } from "solid-js";
import type { UsageWindow } from "../api";
import { act, closeWorkspace, connection, cycleTheme, enableNotifications, go, href, notifyPermission, now, isBusy, openPalette, reopen, route, setHelpOpen, state, store, themePref } from "../store";
import { MOD, age, cls, until, worst } from "../util";
import { Bell, BellOff, Bot, ChevronDown, Git, Inbox, Keyboard, Monitor, Moon, Plus, Search, Sun, Undo, X } from "./icons";
import { attention } from "../gitlab";
import { IconButton, Spinner, StatusDot } from "./ui";

function stored(key: string, fallback: boolean): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}

const [showClosed, setShowClosedRaw] = createSignal(stored("show-closed", false));
const setShowClosed = (v: boolean) => {
  setShowClosedRaw(v);
  try {
    localStorage.setItem("show-closed", v ? "1" : "0");
  } catch {
    /* ignore */
  }
};

function NavItem(p: { to: Parameters<typeof href>[0]; icon: any; label: string; count?: number; tone?: "waiting"; active: boolean; onNavigate?: () => void }) {
  return (
    <a
      href={href(p.to)}
      onClick={() => p.onNavigate?.()}
      class={cls(
        "flex h-8 items-center gap-2.5 rounded-lg px-2.5 text-[13px] font-medium transition-colors",
        p.active ? "bg-hover text-fg" : "text-fg-2 hover:bg-hover/70 hover:text-fg",
      )}
    >
      <span class={cls(p.active ? "text-fg" : "text-fg-3")}>{p.icon}</span>
      {p.label}
      <Show when={p.count}>
        <span
          class={cls(
            "ml-auto min-w-5 rounded-full px-1.5 text-center text-[11px] font-semibold tabular-nums leading-5",
            p.tone === "waiting" ? "bg-waiting text-[#1d1405]" : "bg-hover text-fg-2",
          )}
        >
          {p.count}
        </span>
      </Show>
    </a>
  );
}

function Meter(p: { label: string; w: UsageWindow }) {
  const tone = () => (p.w.used >= 90 ? "var(--danger)" : p.w.used >= 70 ? "var(--waiting)" : "var(--fg-3)");
  return (
    <div class="flex flex-col gap-1" title={`${p.label}: ${p.w.used.toFixed(0)}% used, resets in ${until(p.w.resets_at, now())}`}>
      <div class="flex items-baseline justify-between text-[11px] text-fg-3">
        <span>{p.label}</span>
        <span class="tabular-nums">
          {p.w.used.toFixed(0)}% · {until(p.w.resets_at, now())}
        </span>
      </div>
      <div class="h-1 overflow-hidden rounded-full bg-hover">
        <div class="h-full rounded-full transition-[width] duration-500" style={{ width: `${Math.min(100, p.w.used)}%`, background: tone() }} />
      </div>
    </div>
  );
}

export function Sidebar(p: { onNavigate?: () => void }) {
  const r = route;
  const workspaces = createMemo(() => [...state.workspaces].sort((a, b) => a.name.localeCompare(b.name)));
  const active = (name: string) => {
    const x = r();
    return x.view === "workspace" && x.name === name;
  };
  return (
    <div class="flex h-full w-full flex-col">
      <div class="flex items-center gap-2 px-4 pb-2 pt-4">
        <div class="grid size-7 place-items-center rounded-lg bg-accent text-accent-fg shadow-soft">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M12 2.5 14.2 9.8 21.5 12l-7.3 2.2L12 21.5l-2.2-7.3L2.5 12l7.3-2.2z" />
          </svg>
        </div>
        <span class="text-[14px] font-semibold tracking-[-0.01em] text-fg">Claude Hub</span>
        <span class={cls("ml-auto size-2 rounded-full", connection() === "live" ? "bg-idle" : connection() === "connecting" ? "bg-waiting" : "bg-danger")} title={connection() === "live" ? "Live" : connection() === "connecting" ? "Connecting" : "Offline: reconnecting"} />
      </div>

      <div class="px-3 pb-2">
        <button
          onClick={() => openPalette()}
          class="flex h-8 w-full items-center gap-2 rounded-lg border border-line bg-raised px-2.5 text-[13px] text-fg-3 shadow-soft transition-colors hover:border-line-strong hover:text-fg-2"
        >
          <Search size={14} />
          Search or run…
          <span class="ml-auto flex gap-0.5">
            <span class="kbd">{MOD}</span>
            <span class="kbd">K</span>
          </span>
        </button>
      </div>

      <nav class="flex flex-col gap-0.5 px-3 py-1">
        <NavItem to={{ view: "inbox" }} icon={<Inbox size={15} />} label="Inbox" count={store.waiting().length} tone="waiting" active={r().view === "inbox"} onNavigate={p.onNavigate} />
        <NavItem to={{ view: "agents" }} icon={<Bot size={15} />} label="Agents" count={store.live().length} active={r().view === "agents" || r().view === "agent"} onNavigate={p.onNavigate} />
        <Show when={state.gitlab.enabled}>
          <NavItem
            to={{ view: "gitlab", key: null }}
            icon={<Git size={15} />}
            label="GitLab"
            count={attention()}
            tone="waiting"
            active={r().view === "gitlab"}
            onNavigate={p.onNavigate}
          />
        </Show>
      </nav>

      <div class="mt-4 flex items-center px-5 pb-1">
        <span class="text-[11px] font-semibold uppercase tracking-[0.07em] text-fg-3">Workspaces</span>
        <button class="ml-auto grid size-5 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg" onClick={() => openPalette("open")} title="Open a workspace (O)">
          <Plus size={14} />
        </button>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        <For each={workspaces()} fallback={<div class="px-2.5 py-2 text-[12.5px] text-fg-3">No workspaces open. Press O to open one.</div>}>
          {(w) => {
            const agents = () => store.byWorkspace().get(w.name) ?? [];
            const waiting = () => agents().filter((a) => a.status === "waiting").length;
            return (
              <a
                href={href({ view: "workspace", name: w.name, window: null })}
                onClick={(e) => {
                  e.preventDefault();
                  p.onNavigate?.();
                  go({ view: "workspace", name: w.name, window: null });
                }}
                class={cls(
                  "group flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 transition-colors",
                  active(w.name) ? "bg-hover" : "hover:bg-hover/70",
                )}
              >
                <StatusDot status={worst(agents())} size={7} />
                <div class="min-w-0 flex-1">
                  <div class={cls("truncate text-[13px] font-medium", active(w.name) ? "text-fg" : "text-fg-2 group-hover:text-fg")}>{w.name}</div>
                  <Show when={w.git?.branch}>
                    <div class="truncate text-[11.5px] text-fg-3">
                      {w.git!.branch}
                      <Show when={w.git!.dirty}>
                        <span class="text-waiting"> · {w.git!.dirty}</span>
                      </Show>
                    </div>
                  </Show>
                </div>
                <Show when={waiting()}>
                  <span class="rounded-full bg-waiting px-1.5 text-[10.5px] font-semibold leading-[18px] text-[#1d1405] group-hover:hidden">{waiting()}</span>
                </Show>
                <button
                  class="-mr-1 hidden size-5 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-danger group-hover:grid"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    closeWorkspace(w);
                  }}
                  aria-label={`Close workspace ${w.name}`}
                  title="Close workspace"
                >
                  <X size={12} />
                </button>
              </a>
            );
          }}
        </For>
        <Show when={state.closed.length}>
          <button
            class="mt-3 flex w-full items-center gap-1 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.07em] text-fg-3 hover:text-fg-2"
            onClick={() => setShowClosed(!showClosed())}
            aria-expanded={showClosed()}
          >
            <ChevronDown size={12} class={cls("transition-transform", !showClosed() && "-rotate-90")} />
            Recently closed <span class="ml-1 tabular-nums">{state.closed.length}</span>
          </button>
          <Show when={showClosed()}>
            <For each={state.closed}>
              {(c) => (
                <div class="group flex items-center gap-1 rounded-lg hover:bg-hover/70">
                  <button
                    class="flex min-w-0 flex-1 items-center gap-2.5 px-2.5 py-1.5 text-left"
                    title={`Open ${c.short_path} again with ${c.windows.length} window${c.windows.length === 1 ? "" : "s"}${c.windows.some((w) => w.resume) ? "; its agents resume" : ""}`}
                    disabled={isBusy(`reopen:${c.id}`)}
                    onClick={() => {
                      p.onNavigate?.();
                      void reopen(c.id);
                    }}
                  >
                    <Show when={isBusy(`reopen:${c.id}`)} fallback={<Undo size={13} class="shrink-0 text-fg-3" />}>
                      <Spinner size={13} />
                    </Show>
                    <div class="min-w-0 flex-1">
                      <div class="truncate text-[13px] text-fg-2 group-hover:text-fg">{c.name}</div>
                      <div class="truncate text-[11.5px] text-fg-3">
                        {c.windows.length} window{c.windows.length === 1 ? "" : "s"} · {age(c.closed_at, now())} ago
                      </div>
                    </div>
                  </button>
                  <button
                    class="mr-1 grid size-5 shrink-0 place-items-center rounded text-fg-3 opacity-0 hover:bg-hover hover:text-fg group-hover:opacity-100"
                    onClick={() => act({ type: "forget_closed", id: c.id }, { quiet: true })}
                    aria-label={`Remove ${c.name} from the list`}
                    title="Remove from the list"
                  >
                    <X size={12} />
                  </button>
                </div>
              )}
            </For>
          </Show>
        </Show>
      </div>

      <div class="border-t border-line px-4 py-3">
        <Show when={state.usage}>
          <div class="mb-3 flex flex-col gap-2">
            <Show when={state.usage!.five_hour}>
              <Meter label="5-hour limit" w={state.usage!.five_hour!} />
            </Show>
            <Show when={state.usage!.seven_day}>
              <Meter label="Weekly limit" w={state.usage!.seven_day!} />
            </Show>
          </div>
        </Show>
        <div class="-mx-1.5 flex items-center gap-0.5">
          <IconButton label={notifyPermission() === "granted" ? "Notifications are on" : "Turn on notifications"} onClick={() => notifyPermission() !== "granted" && enableNotifications()} active={notifyPermission() === "granted"}>
            <Show when={notifyPermission() === "granted"} fallback={<BellOff size={15} />}>
              <Bell size={15} />
            </Show>
          </IconButton>
          <IconButton label={`Theme: ${themePref()}`} onClick={cycleTheme}>
            {themePref() === "dark" ? <Moon size={15} /> : themePref() === "light" ? <Sun size={15} /> : <Monitor size={15} />}
          </IconButton>
          <IconButton label="Keyboard shortcuts (?)" onClick={() => setHelpOpen(true)}>
            <Keyboard size={15} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}
