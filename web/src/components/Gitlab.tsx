import { For, Match, Show, Switch, createEffect, createMemo, createSignal, on, onCleanup, onMount, untrack } from "solid-js";
import type { Issue, Job, Mr, Thread } from "../api";
import { allMrs, entries, goWorkspace, isReview, openMr, pipelineTone, startIssue, type Entry, type Kind, type Tab } from "../gitlab";
import { act, go, isBusy, now, route, state } from "../store";
import { age, cls, fuzzyWords } from "../util";
import { Branch, Chat, Check, Chevron, ChevronDown, Code, Done, External, Git, Layers, Pipeline, Refresh, Search, Sparkles, Terminal, X } from "./icons";
import { Markdown } from "./Markdown";
import { Badge, Button, Empty, Spinner } from "./ui";

function duration(s: number | null): string {
  if (s == null) return "";
  const m = Math.floor(s / 60);
  return m ? `${m}m ${Math.round(s % 60)}s` : `${Math.round(s)}s`;
}

function JobMark(p: { status: string }) {
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
        <div class="flex items-baseline gap-1.5">
          <span class="truncate text-fg">
            <span class="text-fg-3">!{m().iid}</span> {m().title.replace(/^Draft:\s*/i, "")}
          </span>
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
        <div class="truncate text-fg">
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

function Pipe(p: { mr: Mr }) {
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
    <Section
      title="Pipeline"
      actions={
        <Show when={p.mr.pipeline_url}>
          <a href={p.mr.pipeline_url!} target="_blank" rel="noopener" class="inline-flex items-center gap-1 text-[12px] text-fg-2 hover:text-fg">
            Open <External size={11} />
          </a>
        </Show>
      }
    >
      <div class="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-2">
        <For each={stages()}>
          {([stage, jobs]) => (
            <div class="rounded-xl border border-line bg-raised p-2">
              <div class="mb-1 px-1 text-[11.5px] font-medium uppercase tracking-wide text-fg-3">{stage}</div>
              <For each={jobs}>
                {(j) => (
                  <a href={j.url} target="_blank" rel="noopener" class="flex items-center gap-2 rounded-md px-1.5 py-1 text-[12.5px] hover:bg-hover">
                    <span class="grid w-3.5 shrink-0 place-items-center">
                      <JobMark status={j.status} />
                    </span>
                    <span class={cls("truncate", j.status === "failed" ? "text-danger" : "text-fg")}>{j.name}</span>
                    <span class="ml-auto shrink-0 tabular-nums text-fg-3">{duration(j.duration)}</span>
                  </a>
                )}
              </For>
            </div>
          )}
        </For>
      </div>
    </Section>
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
      <Button size="sm" disabled={busy()} onClick={() => open("nvim")} kbd="E">
        <Code size={13} /> Editor
      </Button>
    </>
  );
}

function MrDetail(p: { mr: Mr }) {
  const m = () => p.mr;
  return (
    <div class="mx-auto w-full max-w-3xl px-4 pb-24 pt-5 sm:px-8">
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
          <Badge tone={pipelineTone(m().pipeline)}>
            <Pipeline size={11} /> {m().pipeline}
          </Badge>
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
        <a href={m().url} target="_blank" rel="noopener" class="ml-auto inline-flex h-7 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium text-fg-2 hover:bg-hover hover:text-fg">
          Open in GitLab <External size={12} />
        </a>
      </div>

      <Show when={m().jobs.length}>
        <Pipe mr={m()} />
      </Show>

      <Section title={`Open threads ${m().threads.length ? m().threads.length : ""}`}>
        <Show when={m().threads.length} fallback={<div class="rounded-xl border border-dashed border-line px-4 py-5 text-center text-fg-3">No open threads.</div>}>
          <div class="flex flex-col gap-3">
            <For each={m().threads}>{(t) => <ThreadBox mr={m()} thread={t} />}</For>
          </div>
        </Show>
      </Section>

      <Show when={m().description.trim()}>
        <Section title="Description">
          <div class="rounded-xl border border-line bg-panel px-4 py-3">
            <Markdown text={m().description} class="text-[13.5px]" />
          </div>
        </Section>
      </Show>
    </div>
  );
}

function IssueDetail(p: { issue: Issue }) {
  const i = () => p.issue;
  return (
    <div class="mx-auto w-full max-w-3xl px-4 pb-24 pt-5 sm:px-8">
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
  { id: "review", label: "To review", key: "1" },
  { id: "mine", label: "My MRs", key: "2" },
  { id: "issues", label: "Issues", key: "3" },
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

const [tab, setTabRaw] = createSignal<Tab>(load("gitlab-tab", "review"));
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
      const text = e.mr ? `${e.mr.title} ${e.mr.project} ${e.mr.branch} ${e.mr.author} !${e.mr.iid}` : `${e.issue!.title} ${e.issue!.project} ${e.issue!.labels.join(" ")} #${e.issue!.iid}`;
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
        window.open((cur.mr ?? cur.issue)!.url, "_blank", "noopener");
        break;
      case "c":
        if (cur?.mr) void openMr(cur.mr, "claude");
        else if (cur?.issue) void startIssue(cur.issue);
        else return;
        break;
      case "t":
      case "e":
        if (!cur?.mr) return;
        void openMr(cur.mr, e.key === "t" ? "shell" : "nvim");
        break;
      case "R":
        void act({ type: "gitlab_refresh" });
        break;
      case "Escape":
        if (!selected()) return;
        go({ view: "gitlab", key: null });
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
      <div class={cls("flex min-h-0 w-full flex-col border-line lg:w-[24rem] lg:shrink-0 lg:border-r", selected() && "hidden lg:flex")}>
        <header class="px-4 pb-2 pt-4 sm:px-5">
          <div class="flex items-center gap-2">
            <h1 class="text-[20px] font-semibold tracking-[-0.02em] text-fg">GitLab</h1>
            <Button size="sm" variant="ghost" class="ml-auto" title="Fetch everything again (Shift+R)" loading={isBusy(JSON.stringify({ type: "gitlab_refresh" }))} onClick={() => act({ type: "gitlab_refresh" })}>
              <Refresh size={13} />
            </Button>
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
              {q().trim() ? "Nothing matches." : tab() === "review" ? "No reviews wait for you." : tab() === "mine" ? "You have no open merge requests." : "No issues are assigned to you."}
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
                      <button
                        data-key={e.key}
                        onClick={() => select(e.key)}
                        class={cls("block w-full rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors", selected() === e.key ? "bg-hover ring-1 ring-inset ring-line-strong" : "hover:bg-hover/60")}
                      >
                        {e.mr ? <MrRow mr={e.mr} tab={e.tab} group={e.group} /> : <IssueRow issue={e.issue!} />}
                      </button>
                    )}
                  </For>
                </Show>
              </section>
            )}
          </For>
        </div>
      </div>
      <div class={cls("min-h-0 min-w-0 flex-1 overflow-y-auto", !selected() && "hidden lg:block")}>
        <Show when={selected()}>
          <button class="mx-4 mt-3 inline-flex items-center gap-1 text-[12.5px] text-fg-2 hover:text-fg lg:hidden" onClick={() => go({ view: "gitlab", key: null })}>
            <Chevron size={13} class="rotate-180" /> All
          </button>
        </Show>
        <Show when={current()} fallback={<Empty icon={<Git size={22} />} title={selected() ? "It is gone" : "Pick one"} class="h-full">{selected() ? "It was merged, closed or left your lists." : "Select a merge request or issue."}</Empty>}>
          {(c) => (
            <Switch>
              <Match when={c().mr}>{(m) => <MrDetail mr={m()} />}</Match>
              <Match when={c().issue}>{(i) => <IssueDetail issue={i()} />}</Match>
            </Switch>
          )}
        </Show>
      </div>
    </div>
  );
}
