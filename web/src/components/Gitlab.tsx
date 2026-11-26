import { For, Match, Show, Switch, createEffect, createMemo, createSignal, on, onCleanup, onMount, untrack } from "solid-js";
import type { JSX } from "solid-js";
import type { Issue, Mr, Thread, Todo } from "../api";
import { allMrs, entries, goWorkspace, isMention, isReview, markDone, openMr, pipelineTone, startIssue, todos, type Entry, type Kind, type Tab } from "../gitlab";
import { act, go, isBusy, now, route, state } from "../store";
import { age, cls, fuzzyWords } from "../util";
import { Bell, Branch, Chat, Check, Chevron, ChevronDown, Code, Done, External, Git, Layers, PanelLeft, Pipeline, Refresh, Search, Sparkles, Terminal } from "./icons";
import { Markdown } from "./Markdown";
import { JobMark, Pipeline as PipelineTab } from "./MrPipeline";
import { Changes, Commits, ReviewChat, chatMr, createReview, detailTab, setChatMr, setDetailTab, type DetailTab } from "./MrReview";
import { Badge, Button, Edge, Empty, IconButton } from "./ui";
import { focus, showList, toggleList } from "../layout";

function Lead(p: { mr: Mr }) {
  const m = () => p.mr;
  return (
    <span class="mt-[3px] grid size-4 shrink-0 place-items-center" title={m().approved ? "You approved it" : m().pipeline ? `Pipeline ${m().pipeline}` : "No pipeline"}>
      <Switch fallback={<JobMark status={m().pipeline ?? ""} />}>
        <Match when={m().approved}>
          <Done size={14} class="text-idle" />
        </Match>
      </Switch>
    </span>
  );
}

function MrRow(p: { mr: Mr; tab: Tab; group: string }) {
  const m = () => p.mr;
  const assigned = () => p.tab === "mine" && !state.gitlab.authored.some((a) => a.key === m().key);
  return (
    <div class="flex gap-2.5">
      <Lead mr={m()} />
      <div class="min-w-0 flex-1">
        <div class="line-clamp-2 break-words text-fg">
          <span class="text-fg-3">!{m().iid}</span> {m().title.replace(/^Draft:\s*/i, "")}
        </div>
        <div class="mt-0.5 flex items-center gap-1.5 text-[12px] text-fg-3">
          <span class="truncate">
            {m().project.split("/").pop()}
            {p.tab === "review" || assigned() ? ` · ${m().author}` : ""}
          </span>
          <Show when={assigned()}>
            <Badge>assigned</Badge>
          </Show>
          <Show when={m().draft && p.group !== "Drafts"}>
            <Badge>draft</Badge>
          </Show>
          <Show when={m().threads.length}>
            <span class="inline-flex shrink-0 items-center gap-0.5 text-waiting" title="Open threads">
              <Chat size={11} /> {m().threads.length}
            </span>
          </Show>
          <Show when={m().workspace}>
            <span class="inline-flex shrink-0 items-center text-accent" title={`Open in ${m().workspace}`}>
              <Layers size={11} />
            </span>
          </Show>
          <span class="ml-auto shrink-0 tabular-nums">{age(m().updated, now())}</span>
        </div>
      </div>
    </div>
  );
}

function IssueRow(p: { issue: Issue }) {
  const i = () => p.issue;
  return (
    <div class="flex gap-2.5">
      <span class="mt-[3px] grid size-4 shrink-0 place-items-center">
        <span class="size-2 rounded-full border-[1.5px] border-idle" />
      </span>
      <div class="min-w-0 flex-1">
        <div class="line-clamp-2 break-words text-fg">
          <span class="text-fg-3">#{i().iid}</span> {i().title}
        </div>
        <div class="mt-0.5 flex items-center gap-1.5 text-[12px] text-fg-3">
          <span class="truncate">{i().labels.slice(0, 2).join(" · ") || i().project.split("/").pop()}</span>
          <span class="ml-auto shrink-0 tabular-nums">{age(i().updated, now())}</span>
        </div>
      </div>
    </div>
  );
}

const TODO_ACTIONS: Record<string, string> = {
  mentioned: "mentioned you",
  directly_addressed: "wrote to you",
  review_requested: "asked for your review",
  approval_required: "asked for your approval",
  assigned: "assigned you",
  marked: "added a to-do",
  build_failed: "· pipeline failed",
  unmergeable: "· can't be merged",
  merge_train_removed: "· removed from the merge train",
  member_access_requested: "asked for access",
};

const todoWhat = (t: Todo) => `${t.author} ${TODO_ACTIONS[t.action] ?? t.action.replace(/_/g, " ")}`;
const todoRef = (t: Todo) => (t.iid == null ? "" : t.target_type === "MergeRequest" ? `!${t.iid}` : `#${t.iid}`);

function TodoMark(p: { todo: Todo }) {
  const tone = () => (isMention(p.todo) ? "text-waiting" : p.todo.action === "build_failed" || p.todo.action === "unmergeable" ? "text-danger" : "text-fg-3");
  return (
    <span class={cls("mt-[3px] grid size-4 shrink-0 place-items-center", tone())}>
      <Switch fallback={<Bell size={13} />}>
        <Match when={isMention(p.todo)}>
          <Chat size={13} />
        </Match>
        <Match when={p.todo.target_type === "MergeRequest" && p.todo.action !== "build_failed"}>
          <Git size={13} />
        </Match>
        <Match when={p.todo.action === "build_failed"}>
          <Pipeline size={13} />
        </Match>
      </Switch>
    </span>
  );
}

function TodoRow(p: { todo: Todo }) {
  const t = () => p.todo;
  return (
    <div class="flex gap-2.5">
      <TodoMark todo={t()} />
      <div class="min-w-0 flex-1">
        <div class="line-clamp-2 break-words text-fg">
          <span class="text-fg-3">{todoRef(t())}</span> {t().title}
        </div>
        <div class="mt-0.5 flex items-center gap-1.5 text-[12px] text-fg-3">
          <span class="truncate">
            {todoWhat(t())} · {t().project.split("/").pop()}
          </span>
          <span class="ml-auto shrink-0 tabular-nums">{age(t().created, now())}</span>
        </div>
      </div>
    </div>
  );
}

function TodoBanner(p: { todo: Todo; onDone: () => void }) {
  const t = () => p.todo;
  // GitLab repeats the title as the body for actions without a comment.
  const comment = () => (t().body.trim() && t().body.trim() !== t().title.trim() ? t().body : "");
  return (
    <div class={cls("mb-5 rounded-xl border px-4 py-3", isMention(t()) ? "border-waiting/40 bg-waiting-soft" : "border-line bg-panel")}>
      <div class="flex flex-wrap items-center gap-2 text-[13px]">
        <TodoMark todo={t()} />
        <span class="font-medium text-fg">{todoWhat(t())}</span>
        <span class="tabular-nums text-fg-3">{age(t().created, now())}</span>
        <div class="ml-auto flex items-center gap-2">
          <a href={t().url} target="_blank" rel="noopener" class="inline-flex h-7 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium text-fg-2 hover:bg-hover hover:text-fg">
            Open <External size={12} />
          </a>
          <Button size="sm" variant="primary" kbd="X" loading={isBusy(`todo:${t().id}`)} onClick={p.onDone} title="Mark the to-do done in GitLab">
            <Check size={13} /> Done
          </Button>
        </div>
      </div>
      <Show when={comment()}>
        <Markdown text={comment()} class="mt-2 text-[13.5px]" />
      </Show>
    </div>
  );
}

function TodoDetail(p: { todo: Todo; onDone: () => void }) {
  const t = () => p.todo;
  return (
    <div class="mx-auto w-full max-w-3xl px-4 pb-24 pt-5 sm:px-8">
      <div class="text-[12.5px] text-fg-3">{t().project}</div>
      <h1 class="mb-4 mt-1 text-[20px] font-semibold leading-snug tracking-[-0.02em] text-fg">
        <span class="text-fg-3">{todoRef(t())}</span> {t().title}
      </h1>
      <TodoBanner todo={t()} onDone={p.onDone} />
    </div>
  );
}

function Section(p: { title: string; children: any; actions?: any }) {
  return (
    <section class="mt-6">
      <div class="mb-2 flex items-center gap-2">
        <h2 class="text-[12px] font-semibold uppercase tracking-[0.06em] text-fg-3">{p.title}</h2>
        <div class="ml-auto flex items-center gap-1">{p.actions}</div>
      </div>
      {p.children}
    </section>
  );
}

function ThreadBox(p: { mr: Mr; thread: Thread }) {
  const [text, setText] = createSignal("");
  const key = (what: string) => `${what}:${p.mr.key}:${p.thread.id}`;
  const send = async (resolve: boolean) => {
    const r = await act({ type: "thread_reply", mr: p.mr.key, thread: p.thread.id, text: text(), resolve }, { key: key(resolve ? "resolve" : "reply") });
    if (r.ok) setText("");
  };
  return (
    <article class="overflow-hidden rounded-xl border border-line bg-panel">
      <header class="flex items-center gap-2 border-b border-line bg-raised px-3 py-2 text-[12.5px]">
        <Code size={13} class="text-fg-3" />
        <span class="truncate font-mono text-fg-2">{p.thread.place}</span>
      </header>
      <div class="divide-y divide-line">
        <For each={p.thread.notes}>
          {(n) => (
            <div class="px-3.5 py-2.5">
              <div class="mb-1 text-[12px] font-medium text-fg-2">{n.author}</div>
              <Markdown text={n.body} class="text-[13px]" />
            </div>
          )}
        </For>
      </div>
      <div class="border-t border-line p-2.5">
        <textarea
          rows={1}
          value={text()}
          onInput={(e) => {
            setText(e.currentTarget.value);
            e.currentTarget.style.height = "auto";
            e.currentTarget.style.height = `${Math.min(e.currentTarget.scrollHeight, 200)}px`;
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              if (text().trim()) void send(false);
            } else if (e.key === "Escape") e.currentTarget.blur();
          }}
          placeholder="Reply on GitLab… (Ctrl+Enter sends)"
          class="w-full resize-none rounded-lg border border-line-strong bg-raised px-2.5 py-1.5 text-[13px] text-fg outline-none placeholder:text-fg-3 focus:border-accent/60"
        />
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <Button size="sm" variant="primary" disabled={!text().trim()} loading={isBusy(key("reply"))} onClick={() => send(false)}>
            Reply
          </Button>
          <Button size="sm" loading={isBusy(key("resolve"))} onClick={() => send(true)} title="Post the reply, if any, and resolve the thread">
            <Check size={13} /> {text().trim() ? "Reply and resolve" : "Resolve"}
          </Button>
          <Button size="sm" variant="ghost" class="ml-auto" loading={isBusy(`checkout:${p.mr.key}`)} onClick={() => openMr(p.mr, "claude", p.thread.prompt)} title="Put the thread in a Claude prompt in the MR's workspace, to edit before sending">
            <Sparkles size={13} /> Ask Claude to fix
          </Button>
        </div>
      </div>
    </article>
  );
}

function OpenButtons(p: { mr: Mr }) {
  const busy = () => isBusy(`checkout:${p.mr.key}`);
  const review = () => isReview(p.mr);
  const open = (k: Kind) => openMr(p.mr, k);
  return (
    <>
      <Show when={p.mr.workspace}>
        <Button size="sm" variant="primary" onClick={() => goWorkspace(p.mr)}>
          <Layers size={13} /> Go to {p.mr.workspace}
        </Button>
      </Show>
      <Button size="sm" variant={p.mr.workspace ? "secondary" : "primary"} loading={busy()} onClick={() => open("claude")} kbd="C" title={review() ? "Check out the code and start Claude with a review prompt to edit first" : "Open Claude on the MR's branch"}>
        <Sparkles size={13} /> {review() ? "Review with Claude" : "Claude"}
      </Button>
      <Button size="sm" disabled={busy()} onClick={() => open("shell")} kbd="T">
        <Terminal size={13} /> Shell
      </Button>
    </>
  );
}

const DETAIL_TABS: { id: DetailTab; label: string; key: string }[] = [
  { id: "overview", label: "Overview", key: "Esc" },
  { id: "commits", label: "Commits", key: "L" },
  { id: "changes", label: "Changes", key: "D" },
  { id: "pipeline", label: "Pipelines", key: "P" },
];

function MrDetail(p: { mr: Mr; top?: JSX.Element }) {
  const m = () => p.mr;
  const review = createReview(m);
  const count = (t: DetailTab) => (t === "commits" ? review.data()?.commits?.length : t === "changes" ? review.data()?.files.length : undefined);
  return (
    <div class={cls("mx-auto w-full px-4 pb-24 pt-5 sm:px-8", focus() ? "max-w-none" : "max-w-[110rem]")}>
      {p.top}
      <div class="flex flex-wrap items-center gap-1.5 text-[12.5px] text-fg-3">
        <span>{m().project}</span>
        <span>·</span>
        <span class="inline-flex items-center gap-1">
          <Branch size={12} /> {m().branch}
        </span>
      </div>
      <h1 class="mt-1 text-[20px] font-semibold leading-snug tracking-[-0.02em] text-fg">
        <span class="text-fg-3">!{m().iid}</span> {m().title}
      </h1>
      <div class="mt-2 flex flex-wrap items-center gap-1.5 text-[12.5px]">
        <span class="text-fg-2">by {m().author}</span>
        <Show when={m().draft}>
          <Badge>Draft</Badge>
        </Show>
        <Show when={m().pipeline}>
          <button onClick={() => setDetailTab("pipeline")} title="Show the pipelines">
            <Badge tone={pipelineTone(m().pipeline)} class="hover:brightness-110">
              <Pipeline size={11} /> {m().pipeline}
            </Badge>
          </button>
        </Show>
        <Show when={m().merge_status}>
          <Badge>{m().merge_status!.replace(/_/g, " ")}</Badge>
        </Show>
        <Show when={m().approved_by.length}>
          <Badge tone="add">
            <Check size={11} /> {m().approved_by.join(", ")}
          </Badge>
        </Show>
      </div>
      <div class="mt-4 flex flex-wrap items-center gap-2">
        <OpenButtons mr={m()} />
        <Button size="sm" variant={chatMr() === m().key ? "secondary" : "ghost"} onClick={() => setChatMr(chatMr() === m().key ? null : m().key)} title="Claude in the MR's checkout, next to the diff">
          <Chat size={13} /> Chat
        </Button>
        <Show when={isReview(m())}>
          <Button
            size="sm"
            variant={m().approved ? "ghost" : "approve"}
            loading={isBusy(`approve-mr:${m().key}`)}
            onClick={() => act({ type: "mr_approve", mr: m().key, approve: !m().approved, head: review.data()?.version?.head }, { key: `approve-mr:${m().key}` })}
          >
            <Check size={13} /> {m().approved ? "Revoke approval" : "Approve"}
          </Button>
        </Show>
        <a href={m().url} target="_blank" rel="noopener" class="ml-auto inline-flex h-7 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium text-fg-2 hover:bg-hover hover:text-fg">
          Open in GitLab <External size={12} />
        </a>
      </div>

      <div class="mt-5 flex gap-1 border-b border-line text-[13px]" role="tablist">
        <For each={DETAIL_TABS}>
          {(t) => (
            <button
              role="tab"
              aria-selected={detailTab() === t.id}
              title={`${t.label} (${t.key})`}
              onClick={() => setDetailTab(t.id)}
              class={cls("-mb-px flex items-center gap-1.5 border-b-2 px-3 py-2 font-medium transition-colors", detailTab() === t.id ? "border-accent text-fg" : "border-transparent text-fg-2 hover:text-fg")}
            >
              <Show when={t.id === "pipeline" && m().pipeline}>
                <span class="grid w-3.5 place-items-center">
                  <JobMark status={m().pipeline!} />
                </span>
              </Show>
              {t.label}
              <Show when={count(t.id) != null}>
                <span class="rounded-full bg-hover px-1.5 text-[11px] tabular-nums text-fg-3">{count(t.id)}</span>
              </Show>
            </button>
          )}
        </For>
      </div>

      <Switch>
        <Match when={detailTab() === "commits"}>
          <div class="mt-4">
            <Commits review={review} />
          </div>
        </Match>
        <Match when={detailTab() === "changes"}>
          <div class="mt-4">
            <Changes mr={m()} review={review} thread={(t) => <ThreadBox mr={m()} thread={t} />} />
          </div>
        </Match>
        <Match when={detailTab() === "pipeline"}>
          <div class="mt-4">
            <PipelineTab mr={m()} />
          </div>
        </Match>
        <Match when={detailTab() === "overview"}>
          <Overview mr={m()} />
        </Match>
      </Switch>
    </div>
  );
}

function Overview(p: { mr: Mr }) {
  const m = () => p.mr;
  return (
    <>
      <Show when={m().description.trim()}>
        <Section title="Description">
          <div class="rounded-xl border border-line bg-panel px-4 py-3">
            <Markdown text={m().description} class="text-[13.5px]" />
          </div>
        </Section>
      </Show>

      <Section title={`Open threads ${m().threads.length ? m().threads.length : ""}`}>
        <Show when={m().threads.length} fallback={<div class="rounded-xl border border-dashed border-line px-4 py-5 text-center text-fg-3">No open threads.</div>}>
          <div class="flex flex-col gap-3">
            <For each={m().threads}>{(t) => <ThreadBox mr={m()} thread={t} />}</For>
          </div>
        </Show>
      </Section>
    </>
  );
}

function IssueDetail(p: { issue: Issue; top?: JSX.Element }) {
  const i = () => p.issue;
  return (
    <div class="mx-auto w-full max-w-3xl px-4 pb-24 pt-5 sm:px-8">
      {p.top}
      <div class="text-[12.5px] text-fg-3">{i().project}</div>
      <h1 class="mt-1 text-[20px] font-semibold leading-snug tracking-[-0.02em] text-fg">
        <span class="text-fg-3">#{i().iid}</span> {i().title}
      </h1>
      <div class="mt-2 flex flex-wrap items-center gap-1.5 text-[12.5px]">
        <span class="text-fg-2">by {i().author}</span>
        <Show when={i().milestone}>
          <Badge tone="accent">{i().milestone}</Badge>
        </Show>
        <For each={i().labels}>{(l) => <Badge>{l}</Badge>}</For>
      </div>
      <div class="mt-4 flex flex-wrap items-center gap-2">
        <Button size="sm" variant="primary" loading={isBusy(`issue:${i().key}`)} onClick={() => startIssue(i())} kbd="C" title="Start Claude in your clone with a prompt about the issue, to edit first">
          <Sparkles size={13} /> Work on it with Claude
        </Button>
        <a href={i().url} target="_blank" rel="noopener" class="ml-auto inline-flex h-7 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium text-fg-2 hover:bg-hover hover:text-fg">
          Open in GitLab <External size={12} />
        </a>
      </div>
      <Section title="Description">
        <div class="rounded-xl border border-line bg-panel px-4 py-3">
          <Show when={i().description.trim()} fallback={<span class="text-fg-3">No description.</span>}>
            <Markdown text={i().description} class="text-[13.5px]" />
          </Show>
        </div>
      </Section>
    </div>
  );
}

const TABS: { id: Tab; label: string; key: string }[] = [
  { id: "todo", label: "To-Do", key: "1" },
  { id: "review", label: "To review", key: "2" },
  { id: "mine", label: "My MRs", key: "3" },
  { id: "issues", label: "Issues", key: "4" },
];

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function save(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* ignore */
  }
}

const [tab, setTabRaw] = createSignal<Tab>(load("gitlab-tab", "todo"));
const LIST_WIDTH = 384;
const clampWidth = (w: number) => Math.round(Math.min(640, Math.max(288, w)));
const [listWidth, setListWidth] = createSignal(clampWidth(load("gitlab-list-width", LIST_WIDTH)));
const [resizing, setResizing] = createSignal(false);

function startResize(e: PointerEvent) {
  if (e.button !== 0) return;
  e.preventDefault();
  const x0 = e.clientX;
  const w0 = listWidth();
  const move = (ev: PointerEvent) => setListWidth(clampWidth(w0 + ev.clientX - x0));
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    setResizing(false);
    save("gitlab-list-width", listWidth());
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  setResizing(true);
}

function resetWidth() {
  setListWidth(LIST_WIDTH);
  save("gitlab-list-width", LIST_WIDTH);
}
const setTab = (t: Tab) => (setTabRaw(t), save("gitlab-tab", t));
// Approved reviews are done: folded until you open them.
const [folded, setFoldedRaw] = createSignal<string[]>(load("gitlab-folded", ["review:Approved"]));
const toggleFold = (id: string) => {
  const next = folded().includes(id) ? folded().filter((f) => f !== id) : [...folded(), id];
  setFoldedRaw(next);
  save("gitlab-folded", next);
};

export function Gitlab() {
  const g = () => state.gitlab;
  const [q, setQ] = createSignal("");
  const all = createMemo(entries);
  const selected = () => {
    const r = route();
    return r.view === "gitlab" ? r.key : null;
  };
  const current = createMemo(() => {
    const k = selected();
    if (!k) return null;
    const e = all().find((x) => x.key === k);
    if (e) return e;
    const mr = allMrs().find((m) => m.key === k);
    return mr ? ({ key: k, tab: "mine", group: "", mr } as Entry) : null;
  });
  // Opening an item from elsewhere shows its tab.
  createEffect(() => {
    const t = current()?.tab;
    if (t && t !== untrack(tab) && all().some((e) => e.key === current()!.key)) setTab(t);
  });

  const inTab = createMemo(() => {
    const query = q().trim();
    return all().filter((e) => {
      if (e.tab !== tab()) return false;
      if (!query) return true;
      const text = e.todo
        ? `${e.todo.title} ${e.todo.project} ${e.todo.author} ${e.todo.action} ${todoRef(e.todo)}`
        : e.mr ? `${e.mr.title} ${e.mr.project} ${e.mr.branch} ${e.mr.author} !${e.mr.iid}` : `${e.issue!.title} ${e.issue!.project} ${e.issue!.labels.join(" ")} #${e.issue!.iid}`;
      return fuzzyWords(query, text) !== null;
    });
  });
  const groups = createMemo(() => {
    const out: [string, Entry[]][] = [];
    for (const e of inTab()) {
      const gr = out.find(([t]) => t === e.group);
      if (gr) gr[1].push(e);
      else out.push([e.group, [e]]);
    }
    return out;
  });
  const isFolded = (group: string) => !q().trim() && folded().includes(`${tab()}:${group}`);
  const visible = createMemo(() => groups().flatMap(([group, items]) => (isFolded(group) ? [] : items)));
  const count = (t: Tab) => all().filter((e) => e.tab === t).length;
  // Two clicks, so one slip does not empty the list.
  const [armed, setArmed] = createSignal(false);
  let disarm: number | undefined;
  const markAll = () => {
    if (!armed()) {
      setArmed(true);
      clearTimeout(disarm);
      disarm = window.setTimeout(() => setArmed(false), 3000);
      return;
    }
    setArmed(false);
    void markDone();
  };
  onCleanup(() => clearTimeout(disarm));

  const select = (key: string, replace = false) => go({ view: "gitlab", key }, replace);

  // A switch of tab shows its first item, not one from the tab you left.
  createEffect(
    on(
      tab,
      () => {
        if (current()?.tab === tab()) return;
        const first = untrack(visible)[0];
        if (first && matchMedia("(min-width: 1024px)").matches) select(first.key, true);
        else go({ view: "gitlab", key: null }, true);
      },
      { defer: true },
    ),
  );

  // A wide screen shows the first item instead of an empty pane.
  createEffect(() => {
    if (!selected() && visible().length && matchMedia("(min-width: 1024px)").matches) select(visible()[0].key, true);
  });

  let listEl!: HTMLDivElement;
  createEffect(() => {
    const k = selected();
    listEl?.querySelector<HTMLElement>(`[data-key="${CSS.escape(k ?? "")}"]`)?.scrollIntoView({ block: "nearest" });
  });

  // The next item takes the place of a to-do you mark done.
  const done = (t: Todo) => {
    const l = visible();
    const i = l.findIndex((x) => x.key === t.key);
    const next = l[i + 1] ?? l[i - 1];
    void markDone(t);
    if (selected() === t.key) {
      if (next) select(next.key, true);
      else go({ view: "gitlab", key: null }, true);
    }
  };

  const chat = createMemo(() => {
    const m = current()?.mr;
    return m && chatMr() === m.key ? m : null;
  });

  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    if ((e.target as HTMLElement).closest("input,textarea,[contenteditable],.xterm")) return;
    const l = visible();
    const i = l.findIndex((x) => x.key === selected());
    const cur = current();
    const t = TABS.find((x) => x.key === e.key);
    if (t) {
      setTab(t.id);
      e.preventDefault();
      return;
    }
    switch (e.key) {
      case "j":
      case "ArrowDown":
        if (l.length) select(l[Math.min(i + 1, l.length - 1)].key, true);
        break;
      case "k":
      case "ArrowUp":
        if (l.length) select(l[Math.max(i - 1, 0)].key, true);
        break;
      case "o":
      case "Enter":
        if (!cur) return;
        window.open((cur.todo ?? cur.mr ?? cur.issue)!.url, "_blank", "noopener");
        break;
      case "x":
        if (!cur?.todo) return;
        done(cur.todo);
        break;
      case "c":
        if (cur?.mr) void openMr(cur.mr, "claude");
        else if (cur?.issue) void startIssue(cur.issue);
        else return;
        break;
      case "t":
        if (!cur?.mr) return;
        void openMr(cur.mr, "shell");
        break;
      case "R":
        void act({ type: "gitlab_refresh" });
        break;
      case "b":
        toggleList();
        break;
      case "\\":
        if (!cur?.mr) return;
        setChatMr(chatMr() === cur.mr.key ? null : cur.mr.key);
        break;
      case "d":
      case "l":
      case "p":
        if (!cur?.mr) return;
        setDetailTab(e.key === "d" ? "changes" : e.key === "l" ? "commits" : "pipeline");
        break;
      case "Escape":
        if (!selected()) return;
        if (cur?.mr && detailTab() !== "overview") setDetailTab("overview");
        else go({ view: "gitlab", key: null });
        break;
      default:
        return;
    }
    e.preventDefault();
  };
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));

  return (
    <div class="flex h-full min-h-0">
      <div
        style={{ "--list-w": `${listWidth()}px` }}
        class={cls(
          "relative min-h-0 w-full flex-col border-line lg:w-[var(--list-w)] lg:shrink-0 lg:border-r",
          selected() ? "hidden" : "flex",
          !showList() ? "lg:hidden" : chat() ? "lg:hidden 2xl:flex" : "lg:flex",
        )}
      >
        <div
          role="separator"
          aria-orientation="vertical"
          title="Drag to resize. Double-click to reset."
          onPointerDown={startResize}
          onDblClick={resetWidth}
          class={cls("absolute inset-y-0 -right-[3px] z-10 hidden w-[5px] cursor-col-resize transition-colors hover:bg-accent/40 lg:block", resizing() && "bg-accent/40")}
        />
        <header class="px-4 pb-2 pt-4 sm:px-5">
          <div class="flex items-center gap-2">
            <h1 class="text-[20px] font-semibold tracking-[-0.02em] text-fg">GitLab</h1>
            <Button size="sm" variant="ghost" class="ml-auto" title="Fetch everything again (Shift+R)" loading={isBusy(JSON.stringify({ type: "gitlab_refresh" }))} onClick={() => act({ type: "gitlab_refresh" })}>
              <Refresh size={13} />
            </Button>
            <IconButton label="Hide the list (B)" onClick={toggleList} class="-mr-1 hidden size-7 lg:inline-grid">
              <PanelLeft size={15} />
            </IconButton>
          </div>
          <div class="mt-3 flex rounded-lg bg-hover p-0.5 text-[12.5px]" role="tablist">
            <For each={TABS}>
              {(t) => (
                <button
                  role="tab"
                  aria-selected={tab() === t.id}
                  title={`${t.label} (${t.key})`}
                  onClick={() => setTab(t.id)}
                  class={cls("flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1.5 font-medium transition-colors", tab() === t.id ? "bg-raised text-fg shadow-soft" : "text-fg-2 hover:text-fg")}
                >
                  {t.label}
                  <span class="tabular-nums text-fg-3">{count(t.id)}</span>
                </button>
              )}
            </For>
          </div>
          <label class="mt-2 flex h-8 items-center gap-2 rounded-lg border border-line-strong bg-raised px-2.5 focus-within:border-accent/60">
            <Search size={13} class="text-fg-3" />
            <input
              value={q()}
              onInput={(e) => setQ(e.currentTarget.value)}
              onKeyDown={(e) => e.key === "Escape" && (setQ(""), e.currentTarget.blur())}
              placeholder="Filter by title, repo, branch or author"
              class="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-fg-3"
              data-filter
            />
          </label>
        </header>
        <div ref={listEl} class="min-h-0 flex-1 overflow-y-auto px-2 pb-8">
          <For each={g().errors}>{([section, err]) => <div class="mx-2 mb-2 rounded-lg border border-danger/30 bg-danger-soft px-3 py-2 text-[12.5px] text-danger">{section}: {err}</div>}</For>
          <Show when={g().loading && !all().length}>
            <div class="flex flex-col gap-2 px-2">{[0, 1, 2, 3].map(() => <div class="skeleton h-14" />)}</div>
          </Show>
          <Show when={!g().loading && !inTab().length}>
            <div class="px-3 py-8 text-center text-[12.5px] text-fg-3">
              {q().trim() ? "Nothing matches." : tab() === "todo" ? "Your To-Do list is empty." : tab() === "review" ? "No reviews wait for you." : tab() === "mine" ? "You have no open merge requests." : "No issues are assigned to you."}
            </div>
          </Show>
          <Show when={tab() === "todo" && todos().length > 1 && !q().trim()}>
            <div class="flex justify-end px-1 pt-1">
              <Button size="sm" variant={armed() ? "danger" : "ghost"} loading={isBusy("todo:all")} onClick={markAll} title="Mark every to-do done in GitLab">
                <Check size={13} /> {armed() ? `Mark all ${todos().length} done?` : "Mark all done"}
              </Button>
            </div>
          </Show>
          <For each={groups()}>
            {([title, items]) => (
              <section class="mt-2">
                <button
                  class="flex w-full items-center gap-1 rounded-md px-2 py-1.5 text-[11.5px] font-semibold uppercase tracking-[0.06em] text-fg-3 hover:text-fg-2"
                  onClick={() => toggleFold(`${tab()}:${title}`)}
                  aria-expanded={!isFolded(title)}
                >
                  <ChevronDown size={12} class={cls("transition-transform", isFolded(title) && "-rotate-90")} />
                  {title} <span class="ml-1 tabular-nums">{items.length}</span>
                </button>
                <Show when={!isFolded(title)}>
                  <For each={items}>
                    {(e) => (
                      <div class="group relative">
                        <button
                          data-key={e.key}
                          title={(e.todo ?? e.mr ?? e.issue)!.title}
                          onClick={() => select(e.key)}
                          class={cls("block w-full rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors", selected() === e.key ? "bg-hover ring-1 ring-inset ring-line-strong" : "hover:bg-hover/60")}
                        >
                          {e.todo ? <TodoRow todo={e.todo} /> : e.mr ? <MrRow mr={e.mr} tab={e.tab} group={e.group} /> : <IssueRow issue={e.issue!} />}
                        </button>
                        <Show when={e.todo}>
                          {(t) => (
                            <IconButton label="Done (X)" onClick={() => done(t())} class="absolute right-1.5 top-1.5 size-6 bg-raised opacity-0 shadow-soft group-hover:opacity-100 focus-visible:opacity-100">
                              <Check size={13} />
                            </IconButton>
                          )}
                        </Show>
                      </div>
                    )}
                  </For>
                </Show>
              </section>
            )}
          </For>
        </div>
      </div>
      <Show when={!showList()}>
        <div class="hidden lg:flex">
          <Edge side="left" label={focus() ? "Leave focus mode (Z)" : "Show the list (B)"} onClick={toggleList} />
        </div>
      </Show>
      <div class={cls("min-h-0 min-w-0 flex-1 overflow-y-auto", !selected() && "hidden lg:block")}>
        <Show when={selected()}>
          <button class="mx-4 mt-3 inline-flex items-center gap-1 text-[12.5px] text-fg-2 hover:text-fg lg:hidden" onClick={() => go({ view: "gitlab", key: null })}>
            <Chevron size={13} class="rotate-180" /> All
          </button>
        </Show>
        <Show when={current()} fallback={<Empty icon={<Git size={22} />} title={selected() ? "It is gone" : "Pick one"} class="h-full">{selected() ? "It was merged, closed or left your lists." : "Select a merge request or issue."}</Empty>}>
          {(c) => (
            <Switch>
              <Match when={c().todo && c().mr}>{(m) => <MrDetail mr={m()} top={<TodoBanner todo={c().todo!} onDone={() => done(c().todo!)} />} />}</Match>
              <Match when={c().todo && c().issue}>{(i) => <IssueDetail issue={i()} top={<TodoBanner todo={c().todo!} onDone={() => done(c().todo!)} />} />}</Match>
              <Match when={c().todo}>{(t) => <TodoDetail todo={t()} onDone={() => done(t())} />}</Match>
              <Match when={c().mr}>{(m) => <MrDetail mr={m()} />}</Match>
              <Match when={c().issue}>{(i) => <IssueDetail issue={i()} />}</Match>
            </Switch>
          )}
        </Show>
      </div>
      <Show when={chat()}>{(m) => <ReviewChat mr={m()} />}</Show>
    </div>
  );
}
