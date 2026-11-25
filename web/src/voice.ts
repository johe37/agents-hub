import { createSignal } from "solid-js";
import type { Agent } from "./api";
import { act, store, toast } from "./store";

// Not in TypeScript's DOM types; Chrome only has the webkit name.
const Recognition = (window as any).SpeechRecognition ?? (window as any).webkitSpeechRecognition;
export const voiceSupported = !!Recognition && "speechSynthesis" in window;

/** The agent you talk to, or null when voice is off. */
export const [voiceAgent, setVoiceAgent] = createSignal<string | null>(null);
export const [listening, setListening] = createSignal(false);
export const [heard, setHeard] = createSignal("");

let rec: any = null;

const YES = /^(yes|yeah|yep|ok(ay)?|sure|approve|go ahead|do it|ja|kör)\b/i;
const NO = /^(no|nope|deny|don'?t|stop|nej)\b/i;

export function toggleVoice(key: string) {
  if (voiceAgent() === key) return stopVoice();
  stopVoice();
  setVoiceAgent(key);
  listen(key);
}

export function stopVoice() {
  setVoiceAgent(null);
  rec?.abort();
  speechSynthesis.cancel();
}

function listen(key: string) {
  rec?.abort();
  const r = (rec = new Recognition());
  r.lang = navigator.language;
  r.interimResults = true;
  let final = "";
  r.onresult = (e: any) => {
    let interim = "";
    for (let i = e.resultIndex; i < e.results.length; i++) {
      const res = e.results[i];
      if (res.isFinal) final += res[0].transcript;
      else interim += res[0].transcript;
    }
    setHeard(final + interim);
  };
  r.onerror = (e: any) => {
    if (e.error === "no-speech" || e.error === "aborted") return;
    toast(`Voice stopped: ${e.error === "not-allowed" ? "the microphone is blocked" : e.error}`, true);
    setVoiceAgent(null);
  };
  r.onend = () => {
    if (rec !== r) return;
    setListening(false);
    setHeard("");
    if (voiceAgent() !== key) return;
    const text = final.trim();
    if (text) void send(key, text);
    else listen(key);
  };
  r.start();
  setListening(true);
}

async function send(key: string, text: string) {
  const a = store.agentsByKey().get(key);
  if (!a) return stopVoice();
  const ask = a.needs_permission;
  const r =
    ask && YES.test(text)
      ? await act({ type: "approve", agent: key })
      : ask && NO.test(text)
        ? await act({ type: "deny", agent: key })
        : await act({ type: "reply", agent: key, text }, { key: `reply:${key}` });
  // Otherwise the next turn ends with voiceTurn, which speaks and listens again.
  if (!r.ok || (ask && NO.test(text))) listen(key);
}

/** Called when an agent's status changes: speak what it says to the voice agent's user. */
export function voiceTurn(a: Agent) {
  if (a.key !== voiceAgent()) return;
  if (a.status === "waiting") say(a.key, a.needs_permission ? `${a.message ?? "It asks for permission"}. Yes or no?` : (a.message ?? "It waits for you"));
  else if (a.status === "idle") say(a.key, speakable(a.reply ?? "Done."));
}

function say(key: string, text: string) {
  rec?.abort();
  speechSynthesis.cancel();
  const u = new SpeechSynthesisUtterance(text);
  u.onend = () => voiceAgent() === key && listen(key);
  speechSynthesis.speak(u);
}

/** Markdown read aloud: no code, no markup, and short (Chrome cuts long utterances). */
export function speakable(md: string, max = 600): string {
  let t = md
    .replace(/```[\s\S]*?```/g, " (code on screen) ")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[`*_#>|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  if (t.length > max) t = t.slice(0, t.lastIndexOf(".", max) + 1 || max) + " The rest is on screen.";
  return t;
}
