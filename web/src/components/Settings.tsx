import { For, Show, type JSX } from "solid-js";
import {
  ask,
  configTheme,
  connection,
  enableNotifications,
  notifyPermission,
  openPalette,
  pickTheme,
  setHelpOpen,
  setSettingsOpen,
  settingsOpen,
  state,
  theme,
  themeChoice,
} from "../store";
import {
  TERM_SIZES,
  agentView,
  notifyFinished,
  notifyInFocus,
  notifySound,
  notifyWaiting,
  resetBrowserSettings,
  setAgentView,
  setNotifyFinished,
  setNotifyInFocus,
  setNotifySound,
  setNotifyWaiting,
  setShowFiveHour,
  setShowWeekly,
  setTabsMode,
  setTermFontSize,
  showFiveHour,
  showWeekly,
  tabsMode,
  termFontSize,
} from "../settings";
import { choiceLabel } from "../themes";
import { cls } from "../util";
import { X } from "./icons";
import { Button, IconButton } from "./ui";

function Section(p: { title: string; children: JSX.Element }) {
  return (
    <section class="border-t border-line py-4 first:border-t-0 first:pt-0">
      <h3 class="mb-3 text-[11.5px] font-semibold uppercase tracking-[0.06em] text-fg-3">{p.title}</h3>
      <div class="flex flex-col gap-3">{p.children}</div>
    </section>
  );
}

function Row(p: { label: string; hint?: JSX.Element; children: JSX.Element }) {
  return (
    <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-1.5">
      <div class="min-w-0">
        <div class="text-[13px] text-fg">{p.label}</div>
        <Show when={p.hint}>
          <div class="text-[12px] text-fg-3">{p.hint}</div>
        </Show>
      </div>
      <div class="flex shrink-0 items-center gap-2">{p.children}</div>
    </div>
  );
}

function Segmented<T extends string | number>(p: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div class="flex rounded-lg bg-hover p-0.5 text-[12px]" role="radiogroup" aria-label={p.label}>
      <For each={p.options}>
        {([v, label]) => (
          <button
            role="radio"
            aria-checked={p.value === v}
            class={cls("rounded-md px-2.5 py-1", p.value === v ? "bg-raised text-fg shadow-soft" : "text-fg-2 hover:text-fg")}
            onClick={() => p.onChange(v)}
          >
            {label}
          </button>
        )}
      </For>
    </div>
  );
}

function Switch(p: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={p.on}
      aria-label={p.label}
      disabled={p.disabled}
      class={cls("relative h-5 w-9 rounded-full transition-colors disabled:opacity-40", p.on ? "bg-accent" : "bg-line-strong")}
      onClick={() => p.onChange(!p.on)}
    >
      <span class={cls("absolute top-0.5 size-4 rounded-full bg-white shadow-soft transition-[left]", p.on ? "left-[18px]" : "left-0.5")} />
    </button>
  );
}

const PERMISSION: Record<NotificationPermission, string> = {
  granted: "On in this browser",
  denied: "Blocked: allow them in the browser's site settings",
  default: "Not turned on yet",
};

export function Settings() {
  const close = () => setSettingsOpen(false);
  const granted = () => notifyPermission() === "granted";
  return (
    <Show when={settingsOpen()}>
      <div class="fixed inset-0 z-50 grid place-items-center bg-black/30 px-4 backdrop-blur-[2px] animate-fade-in" onMouseDown={(e) => e.target === e.currentTarget && close()}>
        <div
          class="flex max-h-[min(44rem,calc(100vh-2rem))] w-full max-w-xl flex-col rounded-2xl border border-line-strong bg-raised shadow-float animate-pop-in"
          role="dialog"
          aria-label="Settings"
          onKeyDown={(e) => e.key === "Escape" && close()}
        >
          <div class="flex items-center px-6 pb-3 pt-5">
            <h2 class="text-[16px] font-semibold text-fg">Settings</h2>
            <IconButton label="Close (Esc)" class="ml-auto" onClick={close} ref={(el) => queueMicrotask(() => el.focus())}>
              <X size={16} />
            </IconButton>
          </div>
          <div class="min-h-0 flex-1 overflow-y-auto px-6 pb-6">
            <p class="mb-4 text-[12px] text-fg-3">These settings apply to this browser. Where config.toml has the same setting, this browser's choice wins.</p>

            <Section title="Appearance">
              <Row
                label="Theme"
                hint={
                  <Show when={themeChoice()} fallback={<>From config.toml</>}>
                    Overrides config.toml ({choiceLabel(configTheme())})
                  </Show>
                }
              >
                <span class="text-[12.5px] text-fg-2">{theme().label}</span>
                <Show when={themeChoice()}>
                  <Button size="sm" variant="ghost" onClick={() => pickTheme(null)}>
                    Use config
                  </Button>
                </Show>
                <Button size="sm" onClick={() => (close(), openPalette("theme"))}>
                  Change…
                </Button>
              </Row>
            </Section>

            <Section title="Layout">
              <Row label="Windows in a workspace" hint="Tabs above the content, a tree in the sidebar, or both">
                <Segmented
                  label="Windows in a workspace"
                  value={tabsMode()}
                  onChange={setTabsMode}
                  options={[
                    ["both", "Both"],
                    ["tabs", "Tabs"],
                    ["tree", "Tree"],
                  ]}
                />
              </Row>
              <Row label="5-hour limit" hint="Meter in the sidebar">
                <Switch label="5-hour limit" on={showFiveHour()} onChange={setShowFiveHour} />
              </Row>
              <Row label="Weekly limit" hint="Meter in the sidebar">
                <Switch label="Weekly limit" on={showWeekly()} onChange={setShowWeekly} />
              </Row>
            </Section>

            <Section title="Agents">
              <Row label="Open agents in" hint="Press V in a workspace to switch">
                <Segmented
                  label="Open agents in"
                  value={agentView()}
                  onChange={setAgentView}
                  options={[
                    ["terminal", "Terminal"],
                    ["chat", "Chat"],
                    ["last", "Last used"],
                  ]}
                />
              </Row>
            </Section>

            <Section title="Terminal">
              <Row label="Font size">
                <select
                  class="rounded-md border border-line bg-bg px-2 py-1 text-[12.5px] text-fg"
                  value={termFontSize()}
                  onChange={(e) => setTermFontSize(Number(e.currentTarget.value))}
                >
                  <For each={TERM_SIZES}>{(n) => <option value={n}>{n} px</option>}</For>
                </select>
              </Row>
            </Section>

            <Section title="Notifications">
              <Row label="Browser notifications" hint={PERMISSION[notifyPermission()]}>
                <Show when={!granted()}>
                  <Button size="sm" onClick={enableNotifications}>
                    Turn on
                  </Button>
                </Show>
              </Row>
              <Row label="An agent waits for you">
                <Switch label="An agent waits for you" on={notifyWaiting()} onChange={setNotifyWaiting} disabled={!granted()} />
              </Row>
              <Row label="An agent finishes its turn">
                <Switch label="An agent finishes its turn" on={notifyFinished()} onChange={setNotifyFinished} disabled={!granted()} />
              </Row>
              <Row label="Sound" hint="Only for agents that wait for you">
                <Switch label="Sound" on={notifySound()} onChange={setNotifySound} disabled={!granted()} />
              </Row>
              <Row label="Notify while jugglr is in view" hint="Also when the tab is visible and focused">
                <Switch label="Notify while jugglr is in view" on={notifyInFocus()} onChange={setNotifyInFocus} disabled={!granted()} />
              </Row>
            </Section>

            <Section title="About">
              <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[12.5px]">
                <dt class="text-fg-3">Version</dt>
                <dd class="font-mono text-fg-2">{state.version || "…"}</dd>
                <dt class="text-fg-3">Server</dt>
                <dd class="truncate font-mono text-fg-2">{location.host}</dd>
                <dt class="text-fg-3">Connection</dt>
                <dd class="text-fg-2">{connection() === "live" ? "Live" : connection() === "connecting" ? "Connecting" : "Offline"}</dd>
                <dt class="text-fg-3">Config</dt>
                <dd class="truncate font-mono text-fg-2">~/.config/jugglr/config.toml</dd>
                <dt class="text-fg-3">Chats</dt>
                <dd class="truncate font-mono text-fg-2">{state.chat_dir}</dd>
              </dl>
              <div class="flex flex-wrap gap-2">
                <Button size="sm" onClick={() => (close(), setHelpOpen(true))}>
                  Keyboard shortcuts
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  class="text-danger"
                  onClick={() =>
                    ask({
                      title: "Reset settings?",
                      body: "It forgets all settings, the theme, the layout and the last shown windows. The page then loads again.",
                      confirm: "Reset",
                      danger: true,
                      run: resetBrowserSettings,
                    })
                  }
                >
                  Reset settings
                </Button>
              </div>
            </Section>
          </div>
        </div>
      </div>
    </Show>
  );
}
