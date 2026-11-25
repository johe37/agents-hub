import { For, Match, Show, Switch, createEffect, createMemo, createResource, createSignal, on, onCleanup } from "solid-js";
import { getJSON, type Job, type Mr } from "../api";
import { ansiToHtml } from "../ansi";
import { act, isBusy } from "../store";
import { cls } from "../util";
import { Check, External, Refresh, Sparkles, X } from "./icons";
import { askClaude } from "./MrReview";
import { Badge, Button, Spinner } from "./ui";

/** The job whose log the Pipeline tab shows. */
export const [pipeJob, setPipeJob] = createSignal<number | null>(null);

export function duration(s: number | null): string {
  if (s == null) return "";
  const m = Math.floor(s / 60);
  return m ? `${m}m ${Math.round(s % 60)}s` : `${Math.round(s)}s`;
}

export function JobMark(p: { status: string }) {
  return (
    <Switch fallback={<span class="size-2 rounded-full bg-fg-3/50" />}>
      <Match when={p.status === "success"}>
        <Check size={13} stroke-width={2.5} class="text-idle" />
      </Match>
      <Match when={p.status === "failed"}>
        <X size={13} stroke-width={2.5} class="text-danger" />
      </Match>
      <Match when={p.status === "warning"}>
        <span class="text-[12px] font-bold leading-none text-waiting">!</span>
      </Match>
      <Match when={p.status === "running" || p.status === "pending"}>
        <Spinner size={11} />
      </Match>
    </Switch>
  );
}

const live = (s: string) => s === "running" || s === "pending" || s === "created" || s === "preparing" || s === "waiting_for_resource";

/** The jobs in stage columns, in stage order. */
export function Stages(p: { mr: Mr; selected?: number | null; onPick: (j: Job) => void }) {
  const stages = createMemo(() => {
    const out: [string, Job[]][] = [];
    for (const j of p.mr.jobs) {
      const s = out.find(([name]) => name === j.stage);
      if (s) s[1].push(j);
      else out.push([j.stage, [j]]);
    }
    return out;
  });
  return (
    <div class="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-2">
      <For each={stages()}>
        {([stage, jobs]) => (
          <div class="rounded-xl border border-line bg-raised p-2">
            <div class="mb-1 px-1 text-[11.5px] font-medium uppercase tracking-wide text-fg-3">{stage}</div>
            <For each={jobs}>
              {(j) => (
                <button
                  onClick={() => p.onPick(j)}
                  class={cls("flex w-full items-center gap-2 rounded-md px-1.5 py-1 text-left text-[12.5px] hover:bg-hover", p.selected === j.id && "bg-hover ring-1 ring-inset ring-line-strong")}
                  title="Show the log"
                >
                  <span class="grid w-3.5 shrink-0 place-items-center">
                    <JobMark status={j.status} />
                  </span>
                  <span class={cls("truncate", j.status === "failed" ? "text-danger" : "text-fg")}>{j.name}</span>
                  <span class="ml-auto shrink-0 tabular-nums text-fg-3">{duration(j.duration)}</span>
                </button>
              )}
            </For>
          </div>
        )}
      </For>
    </div>
  );
}

function Log(p: { mr: Mr; job: Job }) {
  // A running job's log grows: fetch it again every few seconds.
  const [tick, setTick] = createSignal(0);
  createEffect(() => {
    if (!live(p.job.status)) return;
    const t = setInterval(() => setTick((n) => n + 1), 3000);
    onCleanup(() => clearInterval(t));
  });
  const [log] = createResource(
    () => [p.mr.key, p.job.id, p.job.status, tick()] as const,
    async ([key, id]) => ({ id, ...(await getJSON<{ log: string; truncated: boolean }>(`/api/mr/job?mr=${encodeURIComponent(key)}&job=${id}`)) }),
  );
  const current = () => (log.latest?.id === p.job.id ? log.latest : undefined);
  const html = createMemo(() => (current() ? ansiToHtml(current()!.log) : ""));

  let pre: HTMLPreElement | undefined;
  let stuck = true;
  createEffect(on(() => p.job.id, () => (stuck = true)));
  createEffect(
    on(html, () => {
      if (stuck && pre) requestAnimationFrame(() => pre && (pre.scrollTop = pre.scrollHeight));
    }),
  );

  const retryKey = () => `retry:${p.mr.key}:${p.job.id}`;
  const prompt = () =>
    `Job ${p.job.name} (stage ${p.job.stage}) failed in the pipeline of merge request !${p.mr.iid} in ${p.mr.project}. Read its log with \`glab ci trace ${p.job.id}\`, find the cause and tell me how to fix it. `;
  return (
    <section class="mt-4 overflow-hidden rounded-xl border border-line bg-panel">
      <header class="flex flex-wrap items-center gap-2 border-b border-line bg-raised px-3 py-2 text-[12.5px]">
        <span class="grid w-3.5 place-items-center">
          <JobMark status={p.job.status} />
        </span>
        <span class="font-medium text-fg">{p.job.name}</span>
        <span class="text-fg-3">
          {p.job.stage}
          {p.job.duration != null ? ` · ${duration(p.job.duration)}` : ""}
        </span>
        <Show when={log.loading}>
          <Spinner size={11} />
        </Show>
        <span class="ml-auto flex flex-wrap items-center gap-2">
          <Show when={p.job.status === "failed" || p.job.status === "warning"}>
            <Button size="sm" variant="ghost" onClick={() => askClaude(p.mr, prompt())} title="Ask Claude in the MR's checkout about the failure">
              <Sparkles size={13} /> Ask Claude
            </Button>
          </Show>
          <Show when={!live(p.job.status) && p.job.status !== "manual" && p.job.status !== "skipped"}>
            <Button size="sm" loading={isBusy(retryKey())} onClick={() => act({ type: "retry_job", mr: p.mr.key, job: p.job.id }, { key: retryKey() })}>
              <Refresh size={13} /> Retry
            </Button>
          </Show>
          <a href={p.job.url} target="_blank" rel="noopener" class="inline-flex items-center gap-1 text-fg-2 hover:text-fg">
            Open <External size={11} />
          </a>
        </span>
      </header>
      <Switch>
        <Match when={log.error && !current()}>
          <div class="px-4 py-3 text-[13px] text-danger">Could not fetch the log: {(log.error as Error).message}</div>
        </Match>
        <Match when={!current()}>
          <div class="flex items-center gap-2 px-4 py-6 text-fg-3">
            <Spinner size={14} /> Fetching the log…
          </div>
        </Match>
        <Match when={current()}>
          <Show when={current()!.truncated}>
            <div class="border-b border-line px-4 py-1.5 text-[12px] text-fg-3">Only the end of the log shows here. Open the job for all of it.</div>
          </Show>
          <pre
            ref={pre}
            onScroll={(e) => (stuck = e.currentTarget.scrollHeight - e.currentTarget.scrollTop - e.currentTarget.clientHeight < 40)}
            class="max-h-[70vh] overflow-auto whitespace-pre-wrap break-words bg-bg px-4 py-3 font-mono text-[12px] leading-[1.55] text-fg-2"
            innerHTML={html() || "The log is empty."}
          />
        </Match>
      </Switch>
    </section>
  );
}

export function Pipeline(p: { mr: Mr }) {
  // Without a pick, show the first failed job: that is what you came for.
  createEffect(
    on(
      () => p.mr.key,
      () => {
        if (!p.mr.jobs.some((j) => j.id === pipeJob())) setPipeJob(p.mr.jobs.find((j) => j.status === "failed")?.id ?? null);
      },
    ),
  );
  const job = () => p.mr.jobs.find((j) => j.id === pipeJob());
  const failed = () => p.mr.jobs.filter((j) => j.status === "failed").length;
  return (
    <Show when={p.mr.jobs.length} fallback={<div class="rounded-xl border border-dashed border-line px-4 py-5 text-center text-fg-3">This merge request has no pipeline.</div>}>
      <div class="mb-3 flex flex-wrap items-center gap-2 text-[12.5px]">
        <Show when={p.mr.pipeline}>
          <Badge tone={p.mr.pipeline === "success" ? "add" : p.mr.pipeline === "failed" ? "danger" : "waiting"}>{p.mr.pipeline}</Badge>
        </Show>
        <span class="text-fg-3">
          {p.mr.jobs.length} jobs{failed() ? ` · ${failed()} failed` : ""}
        </span>
        <Show when={p.mr.pipeline_url}>
          <a href={p.mr.pipeline_url!} target="_blank" rel="noopener" class="ml-auto inline-flex items-center gap-1 text-fg-2 hover:text-fg">
            Open in GitLab <External size={11} />
          </a>
        </Show>
      </div>
      <Stages mr={p.mr} selected={pipeJob()} onPick={(j) => setPipeJob(j.id)} />
      <Show when={job()} fallback={<div class="mt-4 text-center text-[12.5px] text-fg-3">Click a job to see its log.</div>}>
        {(j) => <Log mr={p.mr} job={j()} />}
      </Show>
    </Show>
  );
}
