import { For, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { getJSON, type Status } from "../api";
import { stored } from "../layout";
import { agentRoute, go, readStored, state, writeStored, now } from "../store";
import { STATUS, alive, cls } from "../util";
import { SectionTitle } from "./ui";

type Span = [number, number, Status];
interface Lane {
  id: string;
  agent: string | null;
  title: string;
  name: string | null;
  status: Status | null;
  first: number;
  spans: Span[];
}
interface Timeline {
  from: number;
  until: number;
  lanes: Lane[];
}

const RANGES = [1, 4, 8, 24];
const TICK_MINUTES: Record<number, number> = { 1: 10, 4: 30, 8: 60, 24: 180 };
const SHOWN = 8;
const BUCKETS = 240;

function duration(secs: number): string {
  const s = Math.round(secs);
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h${m % 60 ? ` ${m % 60}m` : ""}`;
}

const clock = (ts: number) => new Date(ts * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });

function fill(s: Status): string {
  switch (s) {
    case "working":
      return "var(--working)";
    case "polling":
      return "repeating-linear-gradient(135deg, var(--working) 0 3px, color-mix(in oklab, var(--working) 40%, transparent) 3px 6px)";
    case "waiting":
      return "var(--waiting)";
    case "idle":
      return "color-mix(in oklab, var(--idle) 28%, transparent)";
    default:
      return "color-mix(in oklab, var(--fg-3) 25%, transparent)";
  }
}

/** Swimlanes of every agent's status over the last hours, with how many worked and waited at once above them. */
export function Activity() {
  const [hours, setHoursSignal] = createSignal(Number(readStored("activity-hours")) || 8);
  const setHours = (h: number) => {
    setHoursSignal(h);
    writeStored("activity-hours", String(h));
  };
  const [hidden, setHidden] = stored("activity-hidden", false);
  const [data, setData] = createSignal<Timeline | null>(null);
  const [all, setAll] = createSignal(false);
  const [tip, setTip] = createSignal<{ x: number; y: number; lane: Lane; span: Span } | null>(null);

  const load = () => getJSON<Timeline>(`/api/timeline?hours=${hours()}`).then(setData, () => {});
  // A status change in any agent moves a lane, so the page loads again; between loads the live lanes grow with the clock.
  const statuses = () => state.agents.map((a) => a.key + a.status).join();
  createEffect(on([hours, statuses], () => !hidden() && load()));
  createEffect(on(hidden, (h) => !h && load(), { defer: true }));
  const timer = setInterval(() => !hidden() && !document.hidden && load(), 60_000);
  onCleanup(() => clearInterval(timer));

  const view = createMemo(() => {
    const d = data();
    if (!d) return null;
    const until = Math.max(d.until, Math.floor(now() / 1000));
    const from = until - hours() * 3600;
    const lanes = d.lanes
      .map((l) => {
        const live = l.status !== null && alive(l.status);
        const spans = l.spans
          .map(([s, e, st], i): Span => [Math.max(s, from), live && i === l.spans.length - 1 && e >= d.until - 1 ? until : e, st])
          .filter(([s, e]) => e > s);
        return { ...l, spans, live };
      })
      .filter((l) => l.spans.length);
    return { from, until, lanes };
  });

  const stats = createMemo(() => {
    const v = view();
    let working = 0;
    let waiting = 0;
    let longest = 0;
    for (const l of v?.lanes ?? [])
      for (const [s, e, st] of l.spans) {
        if (st === "working" || st === "polling") working += e - s;
        if (st === "waiting") {
          waiting += e - s;
          longest = Math.max(longest, e - s);
        }
      }
    return { working, waiting, longest };
  });

  // How many agents worked and waited at once, per slice of the window.
  const crowd = createMemo(() => {
    const v = view();
    if (!v) return null;
    const size = (v.until - v.from) / BUCKETS;
    const work = new Float32Array(BUCKETS);
    const wait = new Float32Array(BUCKETS);
    for (const l of v.lanes)
      for (const [s, e, st] of l.spans) {
        const into = st === "waiting" ? wait : st === "working" || st === "polling" ? work : null;
        if (!into) continue;
        for (let b = Math.max(0, Math.floor((s - v.from) / size)); b < BUCKETS; b++) {
          const bs = v.from + b * size;
          if (bs >= e) break;
          into[b] += (Math.min(e, bs + size) - Math.max(s, bs)) / size;
        }
      }
    const peak = Math.max(1, ...work.map((w, i) => w + wait[i]));
    const area = (top: (i: number) => number, base: (i: number) => number) => {
      const pts: string[] = [];
      for (let i = 0; i < BUCKETS; i++) pts.push(`${i},${(1 - top(i) / peak) * 100}`);
      for (let i = BUCKETS - 1; i >= 0; i--) pts.push(`${i},${(1 - base(i) / peak) * 100}`);
      return pts.join(" ");
    };
    return {
      peak: Math.round(peak),
      work: area((i) => work[i], () => 0),
      wait: area((i) => work[i] + wait[i], (i) => work[i]),
    };
  });

  const ticks = createMemo(() => {
    const v = view();
    if (!v) return [];
    const step = TICK_MINUTES[hours()] * 60;
    const out: number[] = [];
    // Local clock times: whole hours land on the hour in every time zone with whole-hour offsets.
    const offset = new Date().getTimezoneOffset() * 60;
    for (let t = Math.ceil((v.from - offset) / step) * step + offset; t < v.until; t += step) out.push(t);
    return out;
  });

  const pct = (t: number) => {
    const v = view()!;
    return ((t - v.from) / (v.until - v.from)) * 100;
  };
  const shown = () => (all() ? view()?.lanes : view()?.lanes.slice(-SHOWN)) ?? [];
  const open = (l: Lane) => {
    const a = l.agent && state.agents.find((x) => x.key === l.agent);
    if (a) go(agentRoute(a));
  };

  return (
    <section class="mb-8">
      <div class="mb-2 flex flex-wrap items-center gap-x-4 gap-y-1 px-1">
        <SectionTitle as="h2" open={!hidden()} onToggle={() => setHidden(!hidden())} class="mr-auto">
          Activity
        </SectionTitle>
        <Show when={!hidden() && view()?.lanes.length}>
          <div class="flex items-center gap-3 text-sm tabular-nums text-fg-3">
            <span title="Agent time spent working">
              <span class="mr-1 inline-block size-2 rounded-full bg-working" />
              {duration(stats().working)}
            </span>
            <span title="Agent time spent waiting for you">
              <span class="mr-1 inline-block size-2 rounded-full bg-waiting" />
              {duration(stats().waiting)}
            </span>
            <Show when={stats().longest >= 60}>
              <span title="Longest wait for you">longest wait {duration(stats().longest)}</span>
            </Show>
          </div>
        </Show>
        <Show when={!hidden()}>
          <div class="flex rounded-lg border border-line bg-raised p-0.5 text-xs">
            <For each={RANGES}>
              {(h) => (
                <button
                  class={cls("rounded-md px-2 py-0.5 tabular-nums", hours() === h ? "bg-hover font-semibold text-fg" : "text-fg-3 hover:text-fg-2")}
                  onClick={() => setHours(h)}
                  aria-pressed={hours() === h}
                >
                  {h}h
                </button>
              )}
            </For>
          </div>
        </Show>
      </div>

      <Show when={!hidden()}>
        <Show
          when={view()?.lanes.length}
          fallback={
            <div class="rounded-2xl border border-dashed border-line px-4 py-6 text-center text-sm text-fg-3">
              {data() ? `No agent activity in the last ${hours()}h. Lanes show up here as agents work.` : "Loading activity…"}
            </div>
          }
        >
          <div class="overflow-hidden rounded-2xl border border-line bg-panel px-4 pb-3 pt-3" onMouseLeave={() => setTip(null)}>
            <div class="flex">
              <div class="w-32 shrink-0 pr-3 sm:w-44">
                <div class="flex h-12 flex-col justify-end text-xs text-fg-3">
                  <span>At once</span>
                  <span class="tabular-nums">peak {crowd()?.peak}</span>
                </div>
                <div class="h-5" />
                <For each={shown()}>
                  {(l) => (
                    <button class="flex h-7 w-full min-w-0 items-center gap-1.5 text-left" onClick={() => open(l)} title={l.title}>
                      <span class={cls("size-1.5 shrink-0 rounded-full", l.live && l.status === "working" && "animate-pulse-dot")} style={{ background: l.status ? STATUS[l.status].color : "var(--ended)", color: l.status ? STATUS[l.status].color : undefined }} />
                      <span class={cls("truncate text-sm", l.live ? "text-fg" : "text-fg-3")}>{l.title}</span>
                    </button>
                  )}
                </For>
              </div>

              <div class="relative min-w-0 flex-1">
                <For each={ticks()}>
                  {(t) => <div class="pointer-events-none absolute bottom-0 top-12 border-l border-dashed border-line" style={{ left: `${pct(t)}%` }} />}
                </For>
                <div class="pointer-events-none absolute bottom-0 top-0 right-0 border-r-2 border-accent/50" />

                <svg class="block h-12 w-full" viewBox={`0 0 ${BUCKETS - 1} 100`} preserveAspectRatio="none" aria-label="Agents working and waiting at once">
                  <polygon points={crowd()?.work} fill="var(--working)" fill-opacity="0.35" />
                  <polygon points={crowd()?.wait} fill="var(--waiting)" fill-opacity="0.55" />
                </svg>
                <div class="relative h-5 text-[11px] tabular-nums text-fg-3">
                  <For each={ticks()}>
                    {(t) => (
                      <span class="absolute top-0.5 -translate-x-1/2 whitespace-nowrap" style={{ left: `${pct(t)}%` }}>
                        {clock(t)}
                      </span>
                    )}
                  </For>
                </div>

                <For each={shown()}>
                  {(l) => (
                    <div class="relative flex h-7 items-center">
                      <div class="relative h-4 w-full cursor-pointer rounded bg-hover/40" onClick={() => open(l)}>
                        <For each={l.spans}>
                          {(sp, i) => (
                            <div
                              class={cls(
                                "absolute inset-y-0 transition-[filter] hover:brightness-110",
                                sp[2] === "waiting" && "shadow-[0_0_8px_color-mix(in_oklab,var(--waiting)_45%,transparent)]",
                                l.live && i() === l.spans.length - 1 && sp[2] !== "idle" && "lane-live",
                              )}
                              style={{ left: `${pct(sp[0])}%`, width: `max(2px, ${pct(sp[1]) - pct(sp[0])}%)`, background: fill(sp[2]), "border-radius": "3px" }}
                              onMouseMove={(e) => setTip({ x: e.clientX, y: e.clientY, lane: l, span: sp })}
                              onMouseLeave={() => setTip(null)}
                            />
                          )}
                        </For>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </div>

            <div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-fg-3">
              <For each={(["working", "polling", "waiting", "idle"] as Status[])}>
                {(s) => (
                  <span class="inline-flex items-center gap-1.5">
                    <span class="inline-block h-2 w-3.5 rounded-sm" style={{ background: fill(s) }} />
                    {STATUS[s].label}
                  </span>
                )}
              </For>
              <Show when={(view()?.lanes.length ?? 0) > SHOWN}>
                <button class="ml-auto hover:text-fg-2" onClick={() => setAll(!all())}>
                  {all() ? "Show fewer" : `Show all ${view()!.lanes.length}`}
                </button>
              </Show>
            </div>
          </div>
        </Show>
      </Show>

      <Show when={tip()}>
        {(t) => (
          <div
            class="pointer-events-none fixed z-50 max-w-72 rounded-lg border border-line-strong bg-raised px-3 py-2 text-sm shadow-float"
            style={{ left: `${t().x}px`, top: `${t().y - 12}px`, transform: "translate(-50%, -100%)" }}
          >
            <div class="truncate font-medium text-fg">{t().lane.title}</div>
            <div class="mt-0.5 flex items-center gap-2 text-fg-2">
              <span class="inline-block size-2 rounded-full" style={{ background: STATUS[t().span[2]].color }} />
              {STATUS[t().span[2]].label}
              <span class="tabular-nums text-fg-3">
                {clock(t().span[0])}–{clock(t().span[1])} · {duration(t().span[1] - t().span[0])}
              </span>
            </div>
            <Show when={t().lane.name}>
              <div class="mt-0.5 text-xs text-fg-3">{t().lane.name}</div>
            </Show>
          </div>
        )}
      </Show>
    </section>
  );
}
