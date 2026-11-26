import { createSignal } from "solid-js";
import { readStored, writeStored } from "./store";

export function stored(key: string, fallback: boolean) {
  const v = readStored(key);
  const [get, set] = createSignal(v === null ? fallback : v === "1");
  const put = (value: boolean) => {
    set(value);
    writeStored(key, value ? "1" : "0");
  };
  return [get, put] as const;
}

const [focus, setFocus] = stored("focus", false);
const [navHidden, setNavHidden] = stored("nav-hidden", false);
const [panelHidden, setPanelHidden] = stored("panel-hidden", readStored("panel") === "closed");
const [listHidden, setListHidden] = stored("gitlab-list-hidden", false);

export { focus };
export const toggleFocus = () => setFocus(!focus());

// In focus mode a toggle leaves focus and shows the part, as that is what you see change.
function toggler(hidden: () => boolean, setHidden: (v: boolean) => void) {
  return () => {
    if (focus()) {
      setFocus(false);
      setHidden(false);
    } else setHidden(!hidden());
  };
}

export const showNav = () => !navHidden() && !focus();
export const showPanel = () => !panelHidden() && !focus();
export const showList = () => !listHidden() && !focus();
export const toggleNav = toggler(navHidden, setNavHidden);
export const togglePanel = toggler(panelHidden, setPanelHidden);
export const toggleList = toggler(listHidden, setListHidden);

export interface Resizable {
  width: () => number;
  resizing: () => boolean;
  set: (w: number) => void;
  reset: () => void;
  /** dir is 1 when the column grows as the pointer moves right, -1 when it grows to the left. */
  start: (e: PointerEvent, dir: 1 | -1) => void;
}

/** A column width that the user drags, kept in this browser. */
export function resizable(key: string, initial: number, min: number, max: number): Resizable {
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(min, w)));
  const saved = Number(readStored(key));
  const [width, setWidth] = createSignal(saved ? clamp(saved) : initial);
  const [resizing, setResizing] = createSignal(false);
  const set = (w: number) => {
    setWidth(clamp(w));
    writeStored(key, String(width()));
  };
  const start = (e: PointerEvent, dir: 1 | -1) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = width();
    const move = (ev: PointerEvent) => setWidth(clamp(w0 + dir * (ev.clientX - x0)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      setResizing(false);
      writeStored(key, String(width()));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    setResizing(true);
  };
  return { width, resizing, set, reset: () => set(initial), start };
}

export const navWidth = resizable("nav-width", 248, 200, 480);
export const panelWidth = resizable("panel-width", 300, 240, 640);
