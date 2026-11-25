import { For, Show } from "solid-js";
import type { Issue, Mr } from "../api";
import { now, state } from "../store";
import { age } from "../util";
import { External, Git, Pipeline } from "./icons";
import { Badge, Empty } from "./ui";

function MrRow(p: { mr: Mr }) {
  const m = () => p.mr;
  return (
    <a href={m().url} target="_blank" rel="noopener" class="group flex items-center gap-3 border-b border-line px-4 py-3 last:border-b-0 hover:bg-hover/60">
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline gap-2">
          <span class="shrink-0 text-fg-3">!{m().iid}</span>
          <span class="truncate font-medium text-fg">{m().title}</span>
        </div>
        <div class="mt-0.5 flex flex-wrap items-center gap-1.5 text-[12.5px] text-fg-3">
          <span>{m().project}</span>
          <span>· {m().author}</span>
          <Show when={m().draft}>
            <Badge>Draft</Badge>
          </Show>
          <Show when={m().pipeline}>
            <Badge tone={m().pipeline === "success" ? "add" : m().pipeline === "failed" ? "danger" : "waiting"}>
              <Pipeline size={11} /> {m().pipeline}
            </Badge>
          </Show>
          <Show when={m().approved}>
            <Badge tone="add">You approved</Badge>
          </Show>
        </div>
      </div>
      <span class="shrink-0 text-[12px] tabular-nums text-fg-3">{age(m().updated, now())}</span>
      <External size={13} class="shrink-0 text-fg-3 opacity-0 group-hover:opacity-100" />
    </a>
  );
}

function IssueRow(p: { issue: Issue }) {
  const i = () => p.issue;
  return (
    <a href={i().url} target="_blank" rel="noopener" class="group flex items-center gap-3 border-b border-line px-4 py-3 last:border-b-0 hover:bg-hover/60">
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline gap-2">
          <span class="shrink-0 text-fg-3">#{i().iid}</span>
          <span class="truncate font-medium text-fg">{i().title}</span>
        </div>
        <div class="mt-0.5 flex flex-wrap items-center gap-1.5 text-[12.5px] text-fg-3">
          <span>{i().project}</span>
          <For each={i().labels.slice(0, 4)}>{(l) => <Badge>{l}</Badge>}</For>
        </div>
      </div>
      <span class="shrink-0 text-[12px] tabular-nums text-fg-3">{age(i().updated, now())}</span>
    </a>
  );
}

function Group(p: { title: string; children: any; count: number }) {
  return (
    <Show when={p.count}>
      <section class="mb-6">
        <h2 class="mb-2 px-1 text-[12px] font-semibold uppercase tracking-[0.06em] text-fg-3">
          {p.title} <span class="ml-1 tabular-nums">{p.count}</span>
        </h2>
        <div class="overflow-hidden rounded-2xl border border-line bg-panel">{p.children}</div>
      </section>
    </Show>
  );
}

export function Gitlab() {
  const g = () => state.gitlab;
  const total = () => g().reviews.length + g().authored.length + g().assigned.length + g().issues.length;
  return (
    <div class="mx-auto w-full max-w-5xl px-4 pb-24 pt-6 sm:px-8">
      <header class="mb-5">
        <h1 class="text-[22px] font-semibold tracking-[-0.02em] text-fg">GitLab</h1>
        <p class="mt-0.5 text-fg-2">Merge requests and issues that involve you.</p>
      </header>
      <For each={g().errors}>{([section, err]) => <div class="mb-3 rounded-xl border border-danger/30 bg-danger-soft px-4 py-2.5 text-danger">{section}: {err}</div>}</For>
      <Show when={g().loading && !total()}>
        <div class="flex flex-col gap-3">{[0, 1, 2].map(() => <div class="skeleton h-14" />)}</div>
      </Show>
      <Show when={!g().loading && !total() && !g().errors.length}>
        <Empty icon={<Git size={22} />} title="Nothing on GitLab">No reviews, merge requests or issues wait for you.</Empty>
      </Show>
      <Group title="Reviews" count={g().reviews.length}>
        <For each={g().reviews}>{(m) => <MrRow mr={m} />}</For>
      </Group>
      <Group title="Your merge requests" count={g().authored.length}>
        <For each={g().authored}>{(m) => <MrRow mr={m} />}</For>
      </Group>
      <Group title="Assigned merge requests" count={g().assigned.length}>
        <For each={g().assigned}>{(m) => <MrRow mr={m} />}</For>
      </Group>
      <Group title="Issues" count={g().issues.length}>
        <For each={g().issues}>{(i) => <IssueRow issue={i} />}</For>
      </Group>
    </div>
  );
}
