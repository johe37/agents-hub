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
