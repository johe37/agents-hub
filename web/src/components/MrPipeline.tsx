import { For, Match, Show, Switch, createEffect, createMemo, createResource, createSignal, on, onCleanup } from "solid-js";
import { getJSON, type Job, type Mr, type PipelineRun } from "../api";
import { ansiToHtml } from "../ansi";
import { act, isBusy, now } from "../store";
import { age, cls } from "../util";
import { Check, External, Play, Refresh, Sparkles, Stop, X } from "./icons";
import { askClaude } from "./MrReview";
import { Badge, Button, IconButton, Spinner, SectionTitle } from "./ui";

/** The job whose log the Pipelines tab shows. */
export const [pipeJob, setPipeJob] = createSignal<number | null>(null);
/** The older pipeline the tab shows; null for the head pipeline. */
const [pipeId, setPipeId] = createSignal<number | null>(null);

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
        <span class="text-sm font-bold leading-none text-waiting">!</span>
      </Match>
      <Match when={p.status === "running" || p.status === "pending"}>
        <Spinner size={11} />
      </Match>
    </Switch>
  );
}

const live = (s: string) => s === "running" || s === "pending" || s === "created" || s === "preparing" || s === "waiting_for_resource";

const done = (s: string) => !live(s) && s !== "manual" && s !== "skipped";

/** A stage's status from its jobs: the worst one wins. */
function stageStatus(jobs: Job[]): string {
  const has = (s: string) => jobs.some((j) => j.status === s);
  if (has("failed")) return "failed";
  if (jobs.some((j) => live(j.status))) return "running";
  if (has("warning")) return "warning";
  if (has("canceled")) return "canceled";
  if (jobs.every((j) => j.status === "skipped" || j.status === "manual")) return jobs[0]?.status ?? "";
  return "success";
}

function byStage(jobs: Job[]): [string, Job[]][] {
  const out: [string, Job[]][] = [];
  for (const j of jobs) {
    const s = out.find(([name]) => name === j.stage);
    if (s) s[1].push(j);
    else out.push([j.stage, [j]]);
  }
  return out;
}

const failurePrompt = (mr: Mr, j: Job) =>
  `Job ${j.name} (stage ${j.stage}) failed in the pipeline of merge request !${mr.iid} in ${mr.project}. Read its log with \`glab ci trace ${j.id}\`, find the cause and tell me how to fix it. `;
const retryKey = (mr: Mr, j: Job) => `retry:${mr.key}:${j.id}`;
const retryJob = (mr: Mr, j: Job) => act({ type: "retry_job", mr: mr.key, job: j.id }, { key: retryKey(mr, j) });

function JobPill(p: { mr: Mr; job: Job; selected: boolean; onPick: () => void }) {
  const j = () => p.job;
  const broke = () => j().status === "failed" || j().status === "warning";
  const small = "inline-flex items-center gap-1 rounded px-1 py-0.5 text-xs font-medium hover:bg-hover disabled:opacity-50";
  return (
    <div
      class={cls(
        "group rounded-lg border text-sm transition-colors",
        j().status === "failed" ? "border-danger/50 bg-danger-soft" : j().status === "warning" ? "border-waiting/40 bg-waiting-soft" : live(j().status) ? "border-working/40 bg-working-soft" : done(j().status) && j().status !== "canceled" ? "border-line bg-panel" : "border-dashed border-line",
        p.selected && "ring-2 ring-accent",
      )}
    >
      <button onClick={p.onPick} class="flex w-full items-center gap-2 px-2 py-1.5 text-left" title={p.selected ? "Hide the log" : "Show the log"}>
        <span class="grid w-3.5 shrink-0 place-items-center">
          <JobMark status={j().status} />
        </span>
        <span class={cls("truncate", j().status === "failed" ? "font-medium text-danger" : done(j().status) && j().status !== "canceled" ? "text-fg" : "text-fg-3")}>{j().name}</span>
        <span class="ml-auto shrink-0 tabular-nums text-xs text-fg-3">{duration(j().duration)}</span>
      </button>
      <Show when={broke()}>
        <div class="flex flex-wrap items-center gap-0.5 border-t border-current/10 px-1.5 py-1 text-fg-2">
          <button class={small} onClick={p.onPick}>
            {p.selected ? "Hide log" : "Log"}
          </button>
          <button class={small} disabled={isBusy(retryKey(p.mr, j()))} onClick={() => retryJob(p.mr, j())}>
            <Refresh size={11} /> Retry
          </button>
          <button class={small} onClick={() => askClaude(p.mr, failurePrompt(p.mr, j()))} title="Ask Claude in the MR's checkout about the failure">
            <Sparkles size={11} /> Ask Claude
          </button>
        </div>
      </Show>
    </div>
  );
}

/** The stages as columns from left to right, as GitLab draws them. */
function Stages(p: { mr: Mr; jobs: Job[]; selected: number | null; onPick: (j: Job) => void }) {
  const stages = createMemo(() => byStage(p.jobs));
  return (
    <div class="overflow-x-auto pb-2">
      <div class="flex w-max items-start">
        <For each={stages()}>
          {([stage, jobs], i) => {
            const failed = () => jobs.filter((j) => j.status === "failed").length;
            return (
              <>
                <Show when={i() > 0}>
                  <div class="mt-[15px] h-px w-5 shrink-0 bg-line-strong" />
                </Show>
                <div class="w-52 shrink-0">
                  <div class="mb-1.5 flex items-center gap-1.5 px-1 text-xs">
                    <span class="grid w-3.5 place-items-center">
                      <JobMark status={stageStatus(jobs)} />
                    </span>
                    <span class="truncate font-medium uppercase tracking-wide text-fg-2">{stage}</span>
                    <span class={cls("ml-auto shrink-0 tabular-nums", failed() ? "text-danger" : "text-fg-3")}>{failed() ? `${failed()}/${jobs.length} failed` : jobs.length}</span>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <For each={jobs}>{(j) => <JobPill mr={p.mr} job={j} selected={p.selected === j.id} onPick={() => p.onPick(j)} />}</For>
                  </div>
                </div>
              </>
            );
          }}
        </For>
      </div>
    </div>
  );
}

/** One line on how the pipeline went, with the failed jobs to open. */
function Summary(p: { status: string | null; jobs: Job[]; duration: number | null; onPick: (j: Job) => void }) {
  const failed = () => p.jobs.filter((j) => j.status === "failed");
  const failedStages = () => [...new Set(failed().map((j) => j.stage))];
  const running = () => byStage(p.jobs).find(([, js]) => js.some((j) => live(j.status)))?.[0];
  const finished = () => p.jobs.filter((j) => done(j.status)).length;
  return (
    <div class="mb-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-base">
      <Switch fallback={<span class="text-fg-2">{p.status ?? "No status"}</span>}>
        <Match when={failed().length}>
          <span class="font-medium text-danger">
            Failed in {failedStages().join(", ")} · {failed().length} {failed().length === 1 ? "job" : "jobs"}:
          </span>
          <For each={failed()}>
            {(j) => (
              <button class="rounded-md bg-danger-soft px-1.5 py-0.5 text-sm font-medium text-danger hover:underline" onClick={() => p.onPick(j)} title="Show the log">
                {j.name}
              </button>
            )}
          </For>
        </Match>
        <Match when={running()}>
          <span class="text-fg-2">
            Running in <span class="font-medium text-fg">{running()}</span> · {finished()}/{p.jobs.length} jobs done
          </span>
        </Match>
        <Match when={p.status === "success"}>
          <span class="font-medium text-idle">Passed</span>
          <span class="text-fg-3">· {p.jobs.length} jobs</span>
        </Match>
      </Switch>
      <Show when={p.duration != null && !running()}>
        <span class="text-fg-3">· {duration(p.duration)}</span>
      </Show>
    </div>
  );
}

function Log(p: { mr: Mr; job: Job; onClose: () => void }) {
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

  let box: HTMLElement | undefined;
  createEffect(on(() => p.job.id, () => requestAnimationFrame(() => box?.scrollIntoView({ behavior: "smooth", block: "nearest" }))));
  return (
    <section ref={box} class="mt-3 scroll-mb-4 overflow-hidden rounded-xl border border-line bg-panel">
      <header class="flex flex-wrap items-center gap-2 border-b border-line bg-raised px-3 py-2 text-sm">
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
            <Button size="sm" variant="ghost" onClick={() => askClaude(p.mr, failurePrompt(p.mr, p.job))} title="Ask Claude in the MR's checkout about the failure">
              <Sparkles size={13} /> Ask Claude
            </Button>
          </Show>
          <Show when={!live(p.job.status) && p.job.status !== "manual" && p.job.status !== "skipped"}>
            <Button size="sm" loading={isBusy(retryKey(p.mr, p.job))} onClick={() => retryJob(p.mr, p.job)}>
              <Refresh size={13} /> Retry
            </Button>
          </Show>
          <a href={p.job.url} target="_blank" rel="noopener" class="inline-flex items-center gap-1 text-fg-2 hover:text-fg">
            Open <External size={11} />
          </a>
          <IconButton label="Close the log" onClick={p.onClose}>
            <X size={14} />
          </IconButton>
        </span>
      </header>
      <Switch>
        <Match when={log.error && !current()}>
          <div class="px-4 py-3 text-base text-danger">Could not fetch the log: {(log.error as Error).message}</div>
        </Match>
        <Match when={!current()}>
          <div class="flex items-center gap-2 px-4 py-6 text-fg-3">
            <Spinner size={14} /> Fetching the log…
          </div>
        </Match>
        <Match when={current()}>
          <Show when={current()!.truncated}>
            <div class="border-b border-line px-4 py-1.5 text-sm text-fg-3">Only the end of the log shows here. Open the job for all of it.</div>
          </Show>
          <pre
            ref={pre}
            onScroll={(e) => (stuck = e.currentTarget.scrollHeight - e.currentTarget.scrollTop - e.currentTarget.clientHeight < 40)}
            class="max-h-[70vh] overflow-auto whitespace-pre-wrap break-words bg-bg px-4 py-3 font-mono text-sm leading-[1.55] text-fg-2"
            innerHTML={html() || "The log is empty."}
          />
        </Match>
      </Switch>
    </section>
  );
}

/** The number at the end of a pipeline's URL, `…/-/pipelines/123`. */
const idOf = (url: string | null) => Number(url?.match(/\/pipelines\/(\d+)/)?.[1]) || null;

function StageDots(p: { stages: PipelineRun["stages"] }) {
  return (
    <span class="flex items-center gap-1">
      <For each={p.stages}>
        {(s) => (
          <span class="grid w-3.5 place-items-center" title={`${s.name}: ${s.status}`}>
            <JobMark status={s.status} />
          </span>
        )}
      </For>
    </span>
  );
}

export function Pipeline(p: { mr: Mr }) {
  const headId = () => idOf(p.mr.pipeline_url);
  const shownId = () => pipeId() ?? headId();
  const older = () => pipeId() != null && pipeId() !== headId();

  // Poll while something runs; otherwise now and then, for pipelines started elsewhere.
  const [tick, setTick] = createSignal(0);
  const [list, { refetch }] = createResource(
    () => [p.mr.key, p.mr.pipeline_url, p.mr.pipeline, tick()] as const,
    ([key]) => getJSON<{ pipelines: PipelineRun[] }>(`/api/mr/pipelines?mr=${encodeURIComponent(key)}`).then((r) => ({ key, pipelines: r.pipelines })),
  );
  const pipelines = () => (list.latest?.key === p.mr.key ? list.latest.pipelines : []);
  const running = () => live(p.mr.pipeline ?? "") || pipelines().some((x) => live(x.status));
  createEffect(() => {
    const t = setInterval(() => setTick((n) => n + 1), running() ? 10_000 : 60_000);
    onCleanup(() => clearInterval(t));
  });

  const shown = () => pipelines().find((x) => x.id === shownId());
  const earlier = () => pipelines().filter((x) => x.id !== headId());
  const [oldJobs] = createResource(
    () => (older() ? ([p.mr.key, pipeId()!, shown()?.status, live(shown()?.status ?? "") ? tick() : 0] as const) : null),
    async ([key, id]) => ({ id, ...(await getJSON<{ jobs: Job[] }>(`/api/mr/pipeline?mr=${encodeURIComponent(key)}&id=${id}`)) }),
  );
  const jobs = (): Job[] | undefined => (older() ? (oldJobs.latest?.id === pipeId() ? oldJobs.latest.jobs : undefined) : p.mr.jobs);
  const status = () => (older() ? shown()?.status : p.mr.pipeline) ?? null;
  const url = () => (older() ? shown()?.url : p.mr.pipeline_url) ?? null;

  createEffect(on(() => p.mr.key, () => setPipeId(null)));
  createEffect(
    on(jobs, (js) => {
      if (js && pipeJob() != null && !js.some((j) => j.id === pipeJob())) setPipeJob(null);
    }),
  );
  const job = () => jobs()?.find((j) => j.id === pipeJob());

  const busyKey = (what: string) => `${what}:${p.mr.key}:${shownId()}`;
  const run = (action: Parameters<typeof act>[0], key: string) => act(action, { key }).then((r) => r.ok && refetch());
  let top: HTMLDivElement | undefined;
  const pick = (id: number | null) => {
    setPipeId(id);
    top?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  };

  const runButton = () => (
    <Button size="sm" variant="ghost" loading={isBusy(`run:${p.mr.key}`)} onClick={() => run({ type: "run_pipeline", mr: p.mr.key }, `run:${p.mr.key}`)} title="Start a new pipeline for the merge request">
      <Play size={13} /> Run pipeline
    </Button>
  );

  return (
    <>
      <Show
        when={shownId()}
        fallback={
          <div class="flex flex-col items-center gap-3 rounded-xl border border-dashed border-line px-4 py-5 text-center text-fg-3">
            This merge request has no pipeline.
            {runButton()}
          </div>
        }
      >
        <div ref={top} class="mb-3 flex scroll-mt-4 flex-wrap items-center gap-2 text-sm">
          <Show when={status()}>
            <Badge tone={status() === "success" ? "add" : status() === "failed" ? "danger" : "waiting"}>{status()!}</Badge>
          </Show>
          <span class="font-medium text-fg">#{shownId()}</span>
          <Show when={older()}>
            <span class="text-fg-3">older pipeline ·</span>
            <button class="text-accent hover:underline" onClick={() => pick(null)}>
              Show the latest
            </button>
          </Show>
          <span class="ml-auto flex flex-wrap items-center gap-2">
            <Show when={status() && live(status()!)}>
              <Button size="sm" variant="ghost" loading={isBusy(busyKey("cancel"))} onClick={() => run({ type: "cancel_pipeline", mr: p.mr.key, pipeline: shownId()! }, busyKey("cancel"))}>
                <Stop size={13} /> Cancel
              </Button>
            </Show>
            <Show when={status() === "failed" || status() === "canceled"}>
              <Button size="sm" loading={isBusy(busyKey("retry"))} onClick={() => run({ type: "retry_pipeline", mr: p.mr.key, pipeline: shownId()! }, busyKey("retry"))} title="Retry the failed and canceled jobs">
                <Refresh size={13} /> Retry
              </Button>
            </Show>
            {runButton()}
            <Show when={url()}>
              <a href={url()!} target="_blank" rel="noopener" class="inline-flex items-center gap-1 text-fg-2 hover:text-fg">
                Open in GitLab <External size={11} />
              </a>
            </Show>
          </span>
        </div>
        <Switch>
          <Match when={older() && oldJobs.error && !jobs()}>
            <div class="px-1 py-3 text-base text-danger">Could not fetch the jobs: {(oldJobs.error as Error).message}</div>
          </Match>
          <Match when={!jobs()}>
            <div class="flex items-center gap-2 px-1 py-4 text-fg-3">
              <Spinner size={14} /> Fetching the jobs…
            </div>
          </Match>
          <Match when={jobs()}>
            {(js) => (
              <>
                <Summary status={status()} jobs={js()} duration={shown()?.duration ?? null} onPick={(j) => setPipeJob(j.id)} />
                <Stages mr={p.mr} jobs={js()} selected={pipeJob()} onPick={(j) => setPipeJob(pipeJob() === j.id ? null : j.id)} />
                <Show when={job()}>{(j) => <Log mr={p.mr} job={j()} onClose={() => setPipeJob(null)} />}</Show>
              </>
            )}
          </Match>
        </Switch>
      </Show>

      <section class="mt-8">
        <SectionTitle class="mb-2 flex items-center gap-2">
          Earlier pipelines
          <Show when={list.loading}>
            <Spinner size={11} />
          </Show>
        </SectionTitle>
        <Switch>
          <Match when={list.error && !list.latest}>
            <div class="text-base text-danger">Could not fetch the pipelines: {(list.error as Error).message}</div>
          </Match>
          <Match when={list.latest && !earlier().length}>
            <div class="rounded-xl border border-dashed border-line px-4 py-4 text-center text-fg-3">No earlier pipelines.</div>
          </Match>
          <Match when={earlier().length}>
            <div class="overflow-hidden rounded-xl border border-line bg-panel">
              <For each={earlier()}>
                {(x) => (
                  <button
                    onClick={() => pick(x.id)}
                    class={cls(
                      "flex w-full flex-wrap items-center gap-x-3 gap-y-1 border-b border-line px-3 py-2 text-left text-sm last:border-b-0 hover:bg-hover",
                      x.id === pipeId() && "bg-hover",
                    )}
                    title="Show its jobs"
                  >
                    <span class="grid w-3.5 shrink-0 place-items-center" title={x.status}>
                      <JobMark status={x.status} />
                    </span>
                    <span class="w-16 shrink-0 font-medium tabular-nums text-fg">#{x.id}</span>
                    <span class="shrink-0 font-mono text-xs text-fg-3">{x.sha.slice(0, 8)}</span>
                    <span class="min-w-0 flex-1 truncate text-fg-2">{x.title}</span>
                    <StageDots stages={x.stages} />
                    <span class="w-14 shrink-0 text-right tabular-nums text-fg-3">{duration(x.duration)}</span>
                    <span class="w-28 shrink-0 truncate text-right text-fg-3" title={new Date(x.created * 1000).toLocaleString()}>
                      {age(x.created, now())} ago{x.user ? ` · ${x.user}` : ""}
                    </span>
                  </button>
                )}
              </For>
            </div>
          </Match>
        </Switch>
      </section>
    </>
  );
}
