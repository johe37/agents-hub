import { Match, Show, Switch, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { AgentPage, Agents } from "./components/Agents";
import { Gitlab } from "./components/Gitlab";
import { Menu } from "./components/icons";
import { Inbox } from "./components/Inbox";
import { NewAgent } from "./components/NewAgent";
import { Confirm, DiffModal, Help, Toasts } from "./components/Overlays";
import { Palette } from "./components/Palette";
import { Sidebar } from "./components/Sidebar";
import { IconButton } from "./components/ui";
import { WorkspaceView } from "./components/WorkspaceView";
import {
  agentRoute,
  composer,
  confirmBox,
  connection,
  diffView,
  go,
  helpOpen,
  openPalette,
  paletteOpen,
  route,
  setComposer,
  setHelpOpen,
  state,
  store,
  theme,
} from "./store";
import { cls } from "./util";

function drawFavicon(waiting: number, dark: boolean) {
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d")!;
  g.fillStyle = dark ? "#e07b53" : "#c15f3c";
  g.beginPath();
  g.roundRect(4, 4, 56, 56, 14);
  g.fill();
  g.fillStyle = dark ? "#160d08" : "#fff";
  g.beginPath();
  const star = [32, 13, 37, 27, 51, 32, 37, 37, 32, 51, 27, 37, 13, 32, 27, 27];
  g.moveTo(star[0], star[1]);
  for (let i = 2; i < star.length; i += 2) g.lineTo(star[i], star[i + 1]);
  g.closePath();
  g.fill();
  if (waiting) {
    g.fillStyle = "#f0b44c";
    g.strokeStyle = dark ? "#0c0c0e" : "#fff";
    g.lineWidth = 5;
    g.beginPath();
    g.arc(50, 14, 12, 0, Math.PI * 2);
    g.fill();
    g.stroke();
  }
  (document.getElementById("favicon") as HTMLLinkElement).href = c.toDataURL();
}

export function App() {
  const [drawer, setDrawer] = createSignal(false);

  createEffect(() => {
    document.documentElement.dataset.theme = theme();
  });
  createEffect(() => {
    const n = store.waiting().length;
    document.title = n ? `(${n}) Claude Hub` : "Claude Hub";
    drawFavicon(n, theme() === "dark");
  });

  // Single keys only act when nothing else wants the keyboard: inputs and terminals keep theirs.
  // Capture phase: this runs before the views' own keys, so "g a" never reaches the inbox as "a".
  let pendingG = 0;
  const onKey = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k" && !e.shiftKey) {
      if ((e.target as HTMLElement).closest(".xterm")) return;
      e.preventDefault();
      openPalette();
      return;
    }
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    if (paletteOpen() || composer() || confirmBox() || diffView() || helpOpen()) return;
    const t = e.target as HTMLElement;
    if (t.closest("input,textarea,select,[contenteditable],.xterm")) return;
    const k = e.key;
    if (pendingG && Date.now() - pendingG < 800) {
      pendingG = 0;
      if (k === "i") go({ view: "inbox" });
      else if (k === "a") go({ view: "agents" });
      else if (k === "l") go({ view: "gitlab" });
      else return;
      e.preventDefault();
      return;
    }
    switch (k) {
      case "g":
        pendingG = Date.now();
        break;
      case "n":
      case "N": {
        const r = route();
        setComposer({ path: r.view === "workspace" ? (state.workspaces.find((w) => w.name === r.name)?.path ?? null) : null });
        break;
      }
      case "o":
        if (route().view === "inbox") return;
        openPalette("open");
        break;
      case "O":
        openPalette("open");
        break;
      case "w": {
        const list = store.waiting();
        if (!list.length) return;
        const r = route();
        const cur = r.view === "workspace" ? list.findIndex((a) => a.window === r.window) : -1;
        go(agentRoute(list[(cur + 1) % list.length]));
        break;
      }
      case "?":
        setHelpOpen(true);
        break;
      case "/": {
        const f = document.querySelector<HTMLInputElement>("[data-filter]");
        if (!f) return openPalette();
        f.focus();
        break;
      }
      default:
        return;
    }
    e.preventDefault();
  };
  onMount(() => window.addEventListener("keydown", onKey, true));
  onCleanup(() => window.removeEventListener("keydown", onKey, true));

  createEffect(() => {
    route();
    setDrawer(false);
  });

  return (
    <div class="flex h-full min-h-0">
      <aside class="hidden w-[248px] shrink-0 border-r border-line bg-panel md:block">
        <Sidebar />
      </aside>
      <Show when={drawer()}>
        <div class="fixed inset-0 z-40 bg-black/30 md:hidden animate-fade-in" onClick={() => setDrawer(false)}>
          <aside class="h-full w-[272px] border-r border-line bg-panel shadow-float animate-slide-up" onClick={(e) => e.stopPropagation()}>
            <Sidebar onNavigate={() => setDrawer(false)} />
          </aside>
        </div>
      </Show>
      <div class="flex min-w-0 flex-1 flex-col">
        <div class="flex items-center gap-2 border-b border-line px-2 py-1.5 md:hidden">
          <IconButton label="Menu" onClick={() => setDrawer(true)}>
            <Menu size={17} />
          </IconButton>
          <span class="text-[13px] font-semibold">Claude Hub</span>
          <Show when={store.waiting().length}>
            <span class="ml-auto mr-2 rounded-full bg-waiting px-2 text-[11px] font-semibold leading-5 text-[#1d1405]">{store.waiting().length} waiting</span>
          </Show>
        </div>
        <Show when={connection() === "offline"}>
          <div class="border-b border-danger/30 bg-danger-soft px-4 py-1.5 text-center text-[12.5px] text-danger animate-fade-in">
            Lost the connection to claude-hub. Reconnecting… Is <code class="font-mono">claude-hub web</code> still running?
          </div>
        </Show>
        <main class={cls("min-h-0 flex-1", route().view === "workspace" || route().view === "agent" ? "overflow-hidden" : "overflow-y-auto")}>
          <Switch>
            <Match when={route().view === "inbox"}>
              <Inbox />
            </Match>
            <Match when={route().view === "agents"}>
              <Agents />
            </Match>
            <Match when={route().view === "gitlab"}>
              <Gitlab />
            </Match>
            <Match when={(() => { const r = route(); return r.view === "workspace" ? r : null; })()}>
              {(r) => <WorkspaceView name={r().name} window={r().window} />}
            </Match>
            <Match when={(() => { const r = route(); return r.view === "agent" ? r : null; })()}>
              {(r) => <AgentPage agentKey={r().key} />}
            </Match>
          </Switch>
        </main>
      </div>
      <Palette />
      <NewAgent />
      <DiffModal />
      <Help />
      <Confirm />
      <Toasts />
    </div>
  );
}
