import { createSignal } from "solid-js";

/** Settings live in this browser only. Where config.toml has the same setting, the browser wins. */
function setting<T extends string | number | boolean>(key: string, fallback: T, valid: (v: unknown) => boolean = () => true) {
  let initial = fallback;
  try {
    const raw = localStorage.getItem(`setting:${key}`);
    if (raw !== null) {
      const v = JSON.parse(raw);
      if (typeof v === typeof fallback && valid(v)) initial = v;
    }
  } catch {
    /* storage can be blocked */
  }
  const [get, set] = createSignal<T>(initial);
  const put = (v: T) => {
    set(() => v);
    try {
      localStorage.setItem(`setting:${key}`, JSON.stringify(v));
    } catch {
      /* ignore */
    }
  };
  return [get, put] as const;
}

const oneOf = (...xs: string[]) => (v: unknown) => xs.includes(v as string);

export type TabsMode = "both" | "tabs" | "tree";
export const [tabsMode, setTabsMode] = setting<TabsMode>("tabs-mode", "both", oneOf("both", "tabs", "tree"));

export type AgentView = "terminal" | "chat" | "last";
export const [agentView, setAgentView] = setting<AgentView>("agent-view", "terminal", oneOf("terminal", "chat", "last"));
/** The view picked last, for agentView "last". */
export const [lastAgentView, setLastAgentView] = setting<"terminal" | "chat">("last-agent-view", "terminal", oneOf("terminal", "chat"));

export const [notifyWaiting, setNotifyWaiting] = setting<boolean>("notify-waiting", true);
export const [notifyFinished, setNotifyFinished] = setting<boolean>("notify-finished", true);
export const [notifySound, setNotifySound] = setting<boolean>("notify-sound", true);
export const [notifyInFocus, setNotifyInFocus] = setting<boolean>("notify-in-focus", false);

export const TERM_SIZES = [11, 12, 13, 14, 15, 16, 18];
export const [termFontSize, setTermFontSize] = setting<number>("term-font-size", 13, (v) => TERM_SIZES.includes(v as number));

export const [showFiveHour, setShowFiveHour] = setting<boolean>("show-five-hour", true);
export const [showWeekly, setShowWeekly] = setting<boolean>("show-weekly", true);

export function resetBrowserSettings() {
  try {
    localStorage.clear();
  } catch {
    /* ignore */
  }
  location.reload();
}
