import type { ITheme } from "@xterm/xterm";

export type Mode = "dark" | "light";

interface Spec {
  label: string;
  mode: Mode;
  ui: {
    bg: string;
    panel: string;
    raised: string;
    hover: string;
    line: string;
    "line-strong": string;
    fg: string;
    "fg-2": string;
    "fg-3": string;
    accent: string;
    "accent-fg": string;
    waiting: string;
    working: string;
    idle: string;
    ended: string;
    danger: string;
    add: string;
    del: string;
    "term-bg": string;
  };
  /** Hand-tuned tints; the rest are mixed from the color and the background. */
  soft?: Partial<Record<(typeof SOFT)[number], string>>;
  /** keyword, string, comment, number, title, type, meta, variable */
  hl: [string, string, string, string, string, string, string, string];
  /** black, red, green, yellow, blue, magenta, cyan, white, then the bright ones. */
  ansi: string[];
}

export interface Theme {
  id: string;
  label: string;
  mode: Mode;
  vars: Record<string, string>;
  term: ITheme;
}

const SOFT = ["accent", "waiting", "working", "idle", "danger", "add", "del"] as const;
const HL = ["keyword", "string", "comment", "number", "title", "type", "meta", "var"];

const SHADOW: Record<Mode, [string, string]> = {
  light: ["0 1px 2px rgb(0 0 0 / 0.04), 0 4px 16px rgb(0 0 0 / 0.06)", "0 8px 24px rgb(0 0 0 / 0.08), 0 24px 64px rgb(0 0 0 / 0.12)"],
  dark: ["0 1px 2px rgb(0 0 0 / 0.4), 0 4px 16px rgb(0 0 0 / 0.3)", "0 12px 32px rgb(0 0 0 / 0.5), 0 32px 80px rgb(0 0 0 / 0.5)"],
};

/** a over b by t (0–1), as #rrggbb. */
function mix(a: string, b: string, t: number): string {
  const n = (h: string, i: number) => parseInt(h.slice(1 + i * 2, 3 + i * 2), 16);
  return "#" + [0, 1, 2].map((i) => Math.round(n(b, i) + (n(a, i) - n(b, i)) * t).toString(16).padStart(2, "0")).join("");
}

function build(id: string, s: Spec): Theme {
  const vars: Record<string, string> = { ...s.ui };
  for (const k of SOFT) vars[`${k}-soft`] = s.soft?.[k] ?? mix(s.ui[k], s.ui.bg, s.mode === "dark" ? 0.16 : 0.12);
  HL.forEach((k, i) => (vars[`hl-${k}`] = s.hl[i]));
  [vars.shadow, vars["shadow-lg"]] = SHADOW[s.mode];
  const a = s.ansi;
  const term: ITheme = {
    background: s.ui["term-bg"],
    foreground: s.ui.fg,
    cursor: s.ui.accent,
    cursorAccent: s.ui["term-bg"],
    selectionBackground: s.ui.accent + (s.mode === "dark" ? "40" : "30"),
    black: a[0], red: a[1], green: a[2], yellow: a[3], blue: a[4], magenta: a[5], cyan: a[6], white: a[7],
    brightBlack: a[8], brightRed: a[9], brightGreen: a[10], brightYellow: a[11],
    brightBlue: a[12], brightMagenta: a[13], brightCyan: a[14], brightWhite: a[15],
  };
  return { id, label: s.label, mode: s.mode, vars, term };
}

const SPECS: Record<string, Spec> = {
  "jugglr-light": {
    label: "Jugglr light",
    mode: "light",
    ui: {
      bg: "#fbfbfa", panel: "#ffffff", raised: "#ffffff", hover: "#f1f1ef", line: "#e9e8e4", "line-strong": "#d9d7d1",
      fg: "#1b1a18", "fg-2": "#5b5853", "fg-3": "#8e8a83", accent: "#c15f3c", "accent-fg": "#ffffff",
      waiting: "#b7791f", working: "#4f6bed", idle: "#2f855a", ended: "#9b978f", danger: "#d64545", add: "#1f7a46", del: "#b4372f",
      "term-bg": "#fdfdfc",
    },
    soft: { accent: "#f7e6de", waiting: "#fdf3dc", working: "#e8ecfd", idle: "#e3f4ea", danger: "#fbe5e5", add: "#e4f5ea", del: "#fbe7e5" },
    hl: ["#a626a4", "#3f8f3e", "#9a9ba1", "#986801", "#3f6fd8", "#b07a00", "#0f8a9a", "#c9473d"],
    ansi: [
      "#1b1a18", "#c4362c", "#1f7a46", "#9a6700", "#3451d1", "#8250df", "#1b7c83", "#6e6a63",
      "#8e8a83", "#d64545", "#2f855a", "#b7791f", "#4f6bed", "#a475f9", "#3192aa", "#3a3833",
    ],
  },
  "jugglr-dark": {
    label: "Jugglr dark",
    mode: "dark",
    ui: {
      bg: "#0c0c0e", panel: "#121215", raised: "#18181c", hover: "#1e1e23", line: "#24242a", "line-strong": "#33333b",
      fg: "#ececf1", "fg-2": "#a3a3ad", "fg-3": "#6c6c77", accent: "#e07b53", "accent-fg": "#160d08",
      waiting: "#f0b44c", working: "#7c93ff", idle: "#5cc98a", ended: "#5f5f69", danger: "#f2706a", add: "#6fd394", del: "#ff8a80",
      "term-bg": "#0e0e11",
    },
    soft: { accent: "#2e1d16", waiting: "#2d2413", working: "#1b2040", idle: "#14281d", danger: "#33171a", add: "#10281a", del: "#33161a" },
    hl: ["#c678dd", "#98c379", "#7f848e", "#d19a66", "#61afef", "#e5c07b", "#56b6c2", "#e06c75"],
    ansi: [
      "#1c1c21", "#ff7b72", "#6fd394", "#f0b44c", "#7c93ff", "#d2a8ff", "#6ad7e5", "#d0d0d8",
      "#5b5b66", "#ffa198", "#8ee6ad", "#f7cc7a", "#a5b4ff", "#e2c5ff", "#9becf5", "#ffffff",
    ],
  },
  "dark-modern": {
    label: "Dark Modern",
    mode: "dark",
    ui: {
      bg: "#181818", panel: "#1f1f1f", raised: "#252526", hover: "#2a2d2e", line: "#2b2b2b", "line-strong": "#3c3c3c",
      fg: "#cccccc", "fg-2": "#9d9d9d", "fg-3": "#6e7681", accent: "#0078d4", "accent-fg": "#ffffff",
      waiting: "#cca700", working: "#3794ff", idle: "#2ea043", ended: "#6e7681", danger: "#f85149", add: "#3fb950", del: "#f85149",
      "term-bg": "#181818",
    },
    hl: ["#c586c0", "#ce9178", "#6a9955", "#b5cea8", "#dcdcaa", "#4ec9b0", "#569cd6", "#9cdcfe"],
    ansi: [
      "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
      "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#e5e5e5",
    ],
  },
  "light-modern": {
    label: "Light Modern",
    mode: "light",
    ui: {
      bg: "#f8f8f8", panel: "#ffffff", raised: "#ffffff", hover: "#f2f2f2", line: "#e5e5e5", "line-strong": "#cecece",
      fg: "#3b3b3b", "fg-2": "#616161", "fg-3": "#8b949e", accent: "#005fb8", "accent-fg": "#ffffff",
      waiting: "#bf8803", working: "#1a85ff", idle: "#388a34", ended: "#8b949e", danger: "#e51400", add: "#388a34", del: "#c4314b",
      "term-bg": "#ffffff",
    },
    hl: ["#af00db", "#a31515", "#008000", "#098658", "#795e26", "#267f99", "#0000ff", "#001080"],
    ansi: [
      "#000000", "#cd3131", "#107c10", "#949800", "#0451a5", "#bc05bc", "#0598bc", "#555555",
      "#666666", "#cd3131", "#14ce14", "#b5ba00", "#0451a5", "#bc05bc", "#0598bc", "#a5a5a5",
    ],
  },
  catppuccin: {
    label: "Catppuccin Mocha",
    mode: "dark",
    ui: {
      bg: "#181825", panel: "#1e1e2e", raised: "#24243a", hover: "#313244", line: "#313244", "line-strong": "#45475a",
      fg: "#cdd6f4", "fg-2": "#a6adc8", "fg-3": "#7f849c", accent: "#cba6f7", "accent-fg": "#1e1e2e",
      waiting: "#f9e2af", working: "#89b4fa", idle: "#a6e3a1", ended: "#6c7086", danger: "#f38ba8", add: "#a6e3a1", del: "#f38ba8",
      "term-bg": "#1e1e2e",
    },
    hl: ["#cba6f7", "#a6e3a1", "#9399b2", "#fab387", "#89b4fa", "#f9e2af", "#94e2d5", "#eba0ac"],
    ansi: [
      "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
      "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
    ],
  },
  gruvbox: {
    label: "Gruvbox",
    mode: "dark",
    ui: {
      bg: "#1d2021", panel: "#282828", raised: "#32302f", hover: "#3c3836", line: "#3c3836", "line-strong": "#504945",
      fg: "#ebdbb2", "fg-2": "#a89984", "fg-3": "#7c6f64", accent: "#fe8019", "accent-fg": "#282828",
      waiting: "#fabd2f", working: "#83a598", idle: "#b8bb26", ended: "#7c6f64", danger: "#fb4934", add: "#b8bb26", del: "#fb4934",
      "term-bg": "#282828",
    },
    hl: ["#fb4934", "#b8bb26", "#928374", "#d3869b", "#8ec07c", "#fabd2f", "#83a598", "#fe8019"],
    ansi: [
      "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
      "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
    ],
  },
  "tokyo-night": {
    label: "Tokyo Night",
    mode: "dark",
    ui: {
      bg: "#16161e", panel: "#1a1b26", raised: "#1f2335", hover: "#292e42", line: "#292e42", "line-strong": "#3b4261",
      fg: "#c0caf5", "fg-2": "#a9b1d6", "fg-3": "#737aa2", accent: "#7aa2f7", "accent-fg": "#1a1b26",
      waiting: "#e0af68", working: "#7dcfff", idle: "#9ece6a", ended: "#565f89", danger: "#f7768e", add: "#9ece6a", del: "#f7768e",
      "term-bg": "#1a1b26",
    },
    hl: ["#bb9af7", "#9ece6a", "#565f89", "#ff9e64", "#7aa2f7", "#2ac3de", "#7dcfff", "#f7768e"],
    ansi: [
      "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
      "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
    ],
  },
  // Oxocarbon has no yellow or orange: waiting and yellow are pink, as in the TUI theme.
  oxocarbon: {
    label: "Oxocarbon",
    mode: "dark",
    ui: {
      bg: "#131313", panel: "#161616", raised: "#1e1e1e", hover: "#262626", line: "#262626", "line-strong": "#393939",
      fg: "#dde1e6", "fg-2": "#a8a8a8", "fg-3": "#6f6f6f", accent: "#78a9ff", "accent-fg": "#161616",
      waiting: "#ff7eb6", working: "#33b1ff", idle: "#42be65", ended: "#525252", danger: "#ee5396", add: "#42be65", del: "#ee5396",
      "term-bg": "#161616",
    },
    hl: ["#78a9ff", "#42be65", "#6f6f6f", "#82cfff", "#ff7eb6", "#08bdba", "#3ddbd9", "#be95ff"],
    ansi: [
      "#262626", "#ee5396", "#42be65", "#ff7eb6", "#78a9ff", "#be95ff", "#3ddbd9", "#dde1e6",
      "#525252", "#ee5396", "#42be65", "#ff7eb6", "#33b1ff", "#be95ff", "#08bdba", "#ffffff",
    ],
  },
};

export const THEMES: Theme[] = Object.entries(SPECS).map(([id, s]) => build(id, s));

/** What you can pick: a theme, or "jugglr" that follows the system's light or dark mode. */
export const CHOICES = ["jugglr", ...THEMES.map((t) => t.id)];

export function isChoice(v: unknown): v is string {
  return typeof v === "string" && CHOICES.includes(v);
}

export function choiceLabel(c: string): string {
  return c === "jugglr" ? "Jugglr (follow system)" : (THEMES.find((t) => t.id === c)?.label ?? c);
}

export function resolve(choice: string, systemDark: boolean): Theme {
  const id = choice === "jugglr" ? (systemDark ? "jugglr-dark" : "jugglr-light") : choice;
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

export function apply(t: Theme) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(t.vars)) root.style.setProperty(`--${k}`, v);
  root.style.colorScheme = t.mode;
  root.dataset.mode = t.mode;
}
