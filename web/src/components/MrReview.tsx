import { For, Show, createEffect, createMemo, createResource, createSignal, on, onCleanup, onMount, type JSX } from "solid-js";
import { getJSON, type Agent, type Commit, type DraftNote, type FileChange, type Mr, type MrChanges, type Thread } from "../api";
import { highlightLines, language } from "../highlight";
import { act, isBusy, now, setDraft, state } from "../store";
import { age, alive, cls } from "../util";
import { Conversation } from "./Conversation";
import { parseHunks, type DiffLine } from "./Diff";
import { Chat, Check, ChevronDown, Rename, Sparkles, Trash, X } from "./icons";
import { Markdown } from "./Markdown";
import { ReviewDiff } from "./ReviewDiff";
import { Badge, Button, IconButton, Spinner } from "./ui";

export type DetailTab = "overview" | "commits" | "changes" | "pipeline";

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function save(key: string, v: string) {
  try {
    localStorage.setItem(key, v);
  } catch {}
}

const savedTab = load("mr-tab");
const [tab, setTabRaw] = createSignal<DetailTab>(savedTab === "commits" || savedTab === "changes" || savedTab === "pipeline" ? savedTab : "overview");
export const detailTab = tab;
export function setDetailTab(t: DetailTab) {
  setTabRaw(t);
  save("mr-tab", t);
}

const [split, setSplitRaw] = createSignal(load("diff-split") === "1");
const toggleSplit = () => {
  setSplitRaw(!split());
  save("diff-split", split() ? "1" : "0");
};

/** Which diff is shown: "" all changes, "c:<sha>" one commit, "s:<sha>" since that commit. */
type Mode = string;

interface Place {
  mr: string;
  mode: Mode;
  file: string;
  a: number;
  b: number;
}

/** The MR whose Claude shows in the side panel. */
export const [chatMr, setChatMr] = createSignal<string | null>(null);
/** A prompt for a Claude that is still starting. */
const [pendingAsk, setPendingAsk] = createSignal<{ mr: string; text: string } | null>(null);
/** The lines you last asked Claude about: "Comment with this" puts the answer there. */
const [lastAsk, setLastAsk] = createSignal<Place | null>(null);
const [composeReq, setComposeReq] = createSignal<(Place & { text: string }) | null>(null);
const [finishReq, setFinishReq] = createSignal<{ mr: string; text: string } | null>(null);

/** The MR's commits, diff and your draft comments, fetched once a tab needs them. */
export function createReview(mr: () => Mr) {
  const [mode, setMode] = createSignal<Mode>("");
  const [draftTick, setDraftTick] = createSignal(0);
  createEffect(on(() => mr().key, () => setMode(""), { defer: true }));
  const url = (key: string, extra = "") => `/api/mr/changes?mr=${encodeURIComponent(key)}${extra}`;
  const [all] = createResource(
    () => (tab() === "changes" || tab() === "commits") && ([mr().key, mr().updated] as const),
    async ([key]) => ({ key, ...(await getJSON<MrChanges>(url(key))) }),
  );
  const [one] = createResource(
    () => mode() && ([mr().key, mode()] as const),
    async ([key, m]) => ({ key, mode: m, ...(await getJSON<MrChanges>(url(key, m.startsWith("c:") ? `&commit=${m.slice(2)}` : `&since=${m.slice(2)}`))) }),
  );
  const [drafts] = createResource(
    () => (tab() === "changes" || tab() === "commits") && ([mr().key, draftTick()] as const),
    async ([key]) => ({ key, ...(await getJSON<{ drafts: DraftNote[] }>(`/api/mr/drafts?mr=${encodeURIComponent(key)}`)) }),
  );
  const data = () => {
    const d = all.latest;
    return d && d.key === mr().key ? d : undefined;
  };
  const files = () => {
    if (!mode()) return data()?.files;
    const d = one.latest;
    return d && d.key === mr().key && d.mode === mode() ? d.files : undefined;
  };
  const error = () => (mode() ? one.error : all.error) as Error | undefined;
  const draftList = () => {
    const d = drafts.latest;
    return d && d.key === mr().key ? d.drafts : [];
  };
  return { data, files, error, mode, setMode, drafts: draftList, reloadDrafts: () => setDraftTick((t) => t + 1) };
}

export type Review = ReturnType<typeof createReview>;

/** A Claude in the MR's checkout that can take a prompt, idle ones first. */
export function reviewAgent(m: Mr): Agent | undefined {
  if (!m.workspace) return undefined;
  const list = state.agents.filter((a) => a.workspace === m.workspace && a.window && a.reachable && alive(a.status));
  return list.find((a) => a.status === "idle") ?? list.find((a) => a.status === "waiting") ?? list[0];
}

/** Put text in the reply box of the MR's Claude, in the side panel; start Claude in the checkout first if none runs. */
export async function askClaude(m: Mr, text: string) {
  setChatMr(m.key);
  const a = reviewAgent(m);
  if (a) return setDraft({ agent: a.key, text });
  const starting = pendingAsk()?.mr === m.key;
  setPendingAsk({ mr: m.key, text });
  if (starting) return;
  const r = await act({ type: "checkout", mr: m.key, kind: "claude" }, { key: `checkout:${m.key}` });
  if (!r.ok) setPendingAsk(null);
}

function commentFrom(m: Mr, text: string) {
  const la = lastAsk();
  setDetailTab("changes");
  // A place in one commit's diff has no GitLab position: the answer goes in the review's summary.
  if (la && la.mr === m.key && !la.mode) setComposeReq({ ...la, text });
  else setFinishReq({ mr: m.key, text });
}

export function ReviewChat(p: { mr: Mr }) {
  const agent = () => reviewAgent(p.mr);
  createEffect(() => {
    const a = agent();
    const pa = pendingAsk();
    if (a && pa && pa.mr === p.mr.key) {
      setDraft({ agent: a.key, text: pa.text });
      setPendingAsk(null);
    }
  });
  const starting = () => pendingAsk()?.mr === p.mr.key || isBusy(`checkout:${p.mr.key}`);
  return (
    <aside class="fixed inset-0 z-40 flex min-h-0 flex-col bg-panel lg:static lg:z-auto lg:w-[30rem] lg:shrink-0 lg:border-l lg:border-line xl:w-[34rem]">
      <header class="flex items-center gap-2 border-b border-line px-4 py-2.5">
        <Sparkles size={14} class="text-accent" />
        <span class="truncate text-[13px] font-medium text-fg">Claude on !{p.mr.iid}</span>
        <Show when={agent()}>{(a) => <span class="truncate text-[12px] text-fg-3">{a().workspace}</span>}</Show>
        <IconButton label="Close" class="ml-auto" onClick={() => setChatMr(null)}>
          <X size={15} />
        </IconButton>
      </header>
      <Show
        when={agent()}
        fallback={
          <div class="grid flex-1 place-items-center px-6 text-center text-[13px] text-fg-3">
            <Show
              when={starting()}
              fallback={
                <div class="flex flex-col items-center gap-3">
                  No Claude runs in the MR's checkout.
                  <Button size="sm" variant="primary" onClick={() => askClaude(p.mr, "")}>
                    <Sparkles size={13} /> Start Claude
                  </Button>
                </div>
              }
            >
              <span class="flex items-center gap-2">
                <Spinner size={14} /> Starting Claude in the MR's checkout…
              </span>
            </Show>
          </div>
        }
      >
        {(a) => (
          <div class="min-h-0 flex-1">
            <Conversation
              agent={a()}
              action={(text) => (
                <button class="mt-1.5 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[12px] text-fg-3 hover:bg-hover hover:text-fg" onClick={() => commentFrom(p.mr, text)} title="Open a comment with this text on the lines you asked about">
                  <Chat size={12} /> Comment with this
                </button>
              )}
            />
          </div>
        )}
      </Show>
    </aside>
  );
}

interface ParsedFile extends FileChange {
  name: string;
  lines: DiffLine[];
  added: number;
  removed: number;
  hash: string;
}

/** FNV-1a: a file stays marked as viewed until its diff changes. */
function hash(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(16);
}

function viewedStore(mrKey: () => string) {
  const key = () => `mr-viewed:${mrKey()}`;
  const read = (): Record<string, string> => {
    try {
      return JSON.parse(load(key()) ?? "{}");
    } catch {
      return {};
    }
  };
  const [viewed, setViewed] = createSignal(read());
  createEffect(on(mrKey, () => setViewed(read()), { defer: true }));
  const toggle = (f: ParsedFile) => {
    const v = { ...viewed() };
    if (v[f.name] === f.hash) delete v[f.name];
    else v[f.name] = f.hash;
    setViewed(v);
    save(key(), JSON.stringify(v));
  };
  return { is: (f: ParsedFile) => viewed()[f.name] === f.hash, toggle };
}

const BIG = 1000;
const span = (xs: number[]) => (xs[0] === xs[xs.length - 1] ? `${xs[0]}` : `${xs[0]}–${xs[xs.length - 1]}`);

/** "lines 12–18", in the new file where it can. */
function linesLabel(sel: DiffLine[]): string {
  const news = sel.filter((l) => l.type !== "del").map((l) => l.new!);
  const olds = sel.filter((l) => l.type === "del").map((l) => l.old!);
  if (!news.length) return `removed lines ${span(olds)}`;
  return olds.length ? `lines ${span(news)} and removed lines ${span(olds)}` : `lines ${span(news)}`;
}

/** A selection from FILE to FILE is the whole file. */
const FILE = -1;

function askPrompt(mr: Mr, f: ParsedFile, sel: DiffLine[], mode: Mode, v: MrChanges["version"]): string {
  const s = (x: string) => x.slice(0, 12);
  const path = f.new_path || f.old_path;
  const cmd = mode.startsWith("c:")
    ? `git show ${s(mode.slice(2))} -- ${path}`
    : mode.startsWith("s:")
      ? `git diff ${s(mode.slice(2))} ${v ? s(v.head) : "HEAD"} -- ${path}`
      : v
        ? `git diff ${s(v.base)} ${s(v.head)} -- ${path}`
        : `glab mr diff ${mr.iid}`;
  return `About merge request !${mr.iid} in ${mr.project}: ${path}${sel.length ? `, ${linesLabel(sel)}` : ""} (see \`${cmd}\`). `;
}

async function sha1(text: string): Promise<string | null> {
  if (!crypto.subtle) return null;
  const buf = await crypto.subtle.digest("SHA-1", new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** GitLab's position for a comment on the whole file, like its "Comment on this file". */
function filePosition(v: NonNullable<MrChanges["version"]>, f: ParsedFile): Record<string, unknown> {
  return { position_type: "file", base_sha: v.base, start_sha: v.start, head_sha: v.head, old_path: f.old_path, new_path: f.new_path };
}

/** GitLab's position for a comment on lines a..b; it sits on the last one. */
async function position(v: NonNullable<MrChanges["version"]>, f: ParsedFile, a: DiffLine, b: DiffLine): Promise<Record<string, unknown>> {
  const pos: Record<string, unknown> = {
    position_type: "text",
    base_sha: v.base,
    start_sha: v.start,
    head_sha: v.head,
    old_path: f.old_path,
    new_path: f.new_path,
  };
  if (b.type !== "add") pos.old_line = b.old;
  if (b.type !== "del") pos.new_line = b.new;
  const h = a !== b ? await sha1(f.new_path) : null;
  if (h) {
    const point = (l: DiffLine) => ({ line_code: `${h}_${l.pos![0]}_${l.pos![1]}`, type: l.type === "add" ? "new" : l.type === "del" ? "old" : null, old_line: l.old ?? null, new_line: l.new ?? null });
    pos.line_range = { start: point(a), end: point(b) };
  }
  return pos;
}

function Mark(p: { f: FileChange }) {
  const [letter, color, title] = p.f.new_file ? ["A", "text-add", "Added"] : p.f.deleted ? ["D", "text-del", "Deleted"] : p.f.renamed ? ["R", "text-accent", "Renamed"] : ["M", "text-waiting", "Changed"];
  return (
    <span class={cls("w-3 shrink-0 text-center font-mono text-[11px] font-semibold", color)} title={title}>
      {letter}
    </span>
  );
}

function Counts(p: { added: number; removed: number }) {
  return (
    <span class="shrink-0 font-mono text-[11.5px] tabular-nums">
      <span class="text-add">+{p.added}</span> <span class="text-del">−{p.removed}</span>
    </span>
  );
}

function Loading(p: { review: Review; children: JSX.Element }) {
  return (
    <Show
      when={!p.review.error()}
      fallback={<div class="rounded-xl border border-danger/30 bg-danger-soft px-4 py-3 text-[13px] text-danger">Could not fetch the changes: {p.review.error()!.message}</div>}
    >
      {p.children}
    </Show>
  );
}

function TextBox(p: { value: string; onInput: (v: string) => void; placeholder: string; onSubmit: () => void; onCancel?: () => void; autofocus?: boolean; rows?: number }) {
  return (
    <textarea
      rows={p.rows ?? 3}
      value={p.value}
      ref={(el) => p.autofocus && queueMicrotask(() => (el.focus(), el.setSelectionRange(el.value.length, el.value.length)))}
      onInput={(e) => p.onInput(e.currentTarget.value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          p.onSubmit();
        } else if (e.key === "Escape") {
          e.preventDefault();
          p.onCancel ? p.onCancel() : e.currentTarget.blur();
        }
      }}
      placeholder={p.placeholder}
      class="w-full resize-y rounded-lg border border-line-strong bg-raised px-2.5 py-1.5 font-sans text-[13px] text-fg outline-none placeholder:text-fg-3 focus:border-accent/60"
    />
  );
}

export function Commits(p: { review: Review }) {
  const pick = (c: Commit) => {
    p.review.setMode(`c:${c.sha}`);
    setDetailTab("changes");
  };
  return (
    <Loading review={p.review}>
      <Show when={p.review.data()} fallback={<div class="flex flex-col gap-2">{[0, 1, 2].map(() => <div class="skeleton h-12" />)}</div>}>
        {(d) => (
          <div class="divide-y divide-line overflow-hidden rounded-xl border border-line bg-panel">
            <For each={d().commits} fallback={<div class="px-4 py-5 text-center text-fg-3">No commits.</div>}>
              {(c) => (
                <button class="flex w-full items-baseline gap-3 px-4 py-2.5 text-left hover:bg-hover/60" onClick={() => pick(c)} title={c.message.trim()}>
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-[13.5px] text-fg">{c.title}</span>
                    <span class="text-[12px] text-fg-3">
                      {c.author} · {age(c.created, now())} ago
                    </span>
                  </span>
                  <span class="shrink-0 font-mono text-[12px] text-fg-2">{c.short}</span>
                </button>
              )}
            </For>
          </div>
        )}
      </Show>
    </Loading>
  );
}

function DraftBox(p: { mr: Mr; draft: DraftNote; onChange: () => void }) {
  const [editing, setEditing] = createSignal(false);
  const [text, setText] = createSignal("");
  const key = (what: string) => `${what}:${p.mr.key}:${p.draft.id}`;
  const saveEdit = async () => {
    const r = await act({ type: "draft_edit", mr: p.mr.key, id: p.draft.id, body: text() }, { key: key("edit"), quiet: true });
    if (r.ok) {
      setEditing(false);
      p.onChange();
    }
  };
  const remove = async () => {
    const r = await act({ type: "draft_delete", mr: p.mr.key, id: p.draft.id }, { key: key("delete") });
    if (r.ok) p.onChange();
  };
  return (
    <article class="max-w-3xl overflow-hidden rounded-xl border border-waiting/40 bg-panel">
      <header class="flex items-center gap-2 border-b border-line bg-waiting-soft/40 px-3 py-1.5 text-[12px]">
        <Badge tone="waiting">Draft</Badge>
        <span class="text-fg-3">Only you see it until you finish the review</span>
        <span class="ml-auto flex gap-1">
          <IconButton label="Edit" onClick={() => (setText(p.draft.body), setEditing(true))}>
            <Rename size={13} />
          </IconButton>
          <IconButton label="Delete" disabled={isBusy(key("delete"))} onClick={remove}>
            <Trash size={13} />
          </IconButton>
        </span>
      </header>
      <Show when={editing()} fallback={<Markdown text={p.draft.body} class="px-3.5 py-2.5 text-[13px]" />}>
        <div class="p-2.5">
          <TextBox value={text()} onInput={setText} placeholder="Comment" onSubmit={saveEdit} onCancel={() => setEditing(false)} autofocus />
          <div class="mt-2 flex gap-2">
            <Button size="sm" variant="primary" disabled={!text().trim()} loading={isBusy(key("edit"))} onClick={saveEdit}>
              Save
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              Cancel
            </Button>
          </div>
        </div>
      </Show>
    </article>
  );
}

function CommentForm(p: { label: string; text: string; onText: (t: string) => void; busy: boolean; onSubmit: (draft: boolean) => void; onCancel: () => void }) {
  return (
    <div class="max-w-3xl rounded-xl border border-line-strong bg-panel p-2.5">
      <div class="mb-1.5 text-[12px] text-fg-3">Comment on {p.label}</div>
      <TextBox value={p.text} onInput={p.onText} placeholder="Write a comment… (Ctrl+Enter adds it to your review)" onSubmit={() => p.text.trim() && p.onSubmit(true)} onCancel={p.onCancel} autofocus />
      <div class="mt-2 flex flex-wrap items-center gap-2">
        <Button size="sm" variant="primary" disabled={!p.text.trim()} loading={p.busy} onClick={() => p.onSubmit(true)} title="Keep it as a draft until you finish the review">
          Add to review
        </Button>
        <Button size="sm" disabled={!p.text.trim() || p.busy} onClick={() => p.onSubmit(false)} title="Post it on GitLab now">
          Comment now
        </Button>
        <Button size="sm" variant="ghost" onClick={p.onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

/** Highlighted HTML per line: old and new text each in one piece, so a comment over several lines stays a comment. */
function highlightFile(lines: DiffLine[], lang: string): (string | undefined)[] {
  const oldIdx: number[] = [];
  const newIdx: number[] = [];
  lines.forEach((l, i) => {
    if (l.type === "del" || l.type === "ctx") oldIdx.push(i);
    if (l.type === "add" || l.type === "ctx") newIdx.push(i);
  });
  const out: (string | undefined)[] = new Array(lines.length);
  try {
    const oldHtml = highlightLines(oldIdx.map((i) => lines[i].text).join("\n"), lang);
    const newHtml = highlightLines(newIdx.map((i) => lines[i].text).join("\n"), lang);
    oldIdx.forEach((i, k) => (out[i] = oldHtml[k]));
    newIdx.forEach((i, k) => (out[i] = newHtml[k]));
  } catch {
    return [];
  }
  return out;
}

interface FileCtx {
  mr: Mr;
  review: Review;
  thread: (t: Thread) => JSX.Element;
  sel: () => { file: string; a: number; b: number } | null;
  pick: (f: ParsedFile, i: number, extend: boolean) => void;
  select: (f: ParsedFile, a: number, b: number) => void;
  clearSel: () => void;
  form: () => { file: string; a: number; b: number; text: string } | null;
  setFormText: (t: string) => void;
  openForm: (f: ParsedFile) => void;
  openFileForm: (f: ParsedFile) => void;
  closeForm: () => void;
  submit: (f: ParsedFile, draft: boolean) => void;
  ask: (f: ParsedFile) => void;
  askFile: (f: ParsedFile) => void;
}

const lineKeys = (l: DiffLine) => (l.type === "del" ? [`o${l.old}`] : l.type === "add" ? [`n${l.new}`] : l.type === "ctx" ? [`n${l.new}`, `o${l.old}`] : []);

function FileBox(p: { f: ParsedFile; index: number; viewed: boolean; onViewed: () => void; threads: Thread[]; drafts: DraftNote[]; open: boolean; onOpen: (o: boolean) => void; ctx: FileCtx }) {
  const [full, setFull] = createSignal(false);
  // An MR can change hundreds of files: draw a diff only when it comes near the screen.
  const [near, setNear] = createSignal(false);
  let el!: HTMLElement;
  onMount(() => {
    const io = new IntersectionObserver(([e]) => e.isIntersecting && (setNear(true), io.disconnect()), { rootMargin: "1500px 0px" });
    io.observe(el);
    onCleanup(() => io.disconnect());
  });
  const lang = language(p.f.name);
  const html = createMemo(() => (near() && lang && p.f.lines.length <= 4000 ? highlightFile(p.f.lines, lang) : undefined));

  // Threads and drafts sit under their line; one whose line is not in the diff goes on top.
  const placed = createMemo(() => {
    const shown = new Set(p.f.lines.flatMap(lineKeys));
    const map = new Map<string, { threads: Thread[]; drafts: DraftNote[] }>();
    const loose = { threads: [] as Thread[], drafts: [] as DraftNote[] };
    const put = (line: number | null, old: boolean, add: (g: typeof loose) => void) => {
      const k = line == null ? null : `${old ? "o" : "n"}${line}`;
      if (k && shown.has(k)) {
        if (!map.has(k)) map.set(k, { threads: [], drafts: [] });
        add(map.get(k)!);
      } else add(loose);
    };
    for (const t of p.threads) put(t.line, t.old_line, (g) => g.threads.push(t));
    for (const d of p.drafts) put(d.line, d.old_line, (g) => g.drafts.push(d));
    return { map, loose };
  });

  const mine = () => {
    const s = p.ctx.sel();
    return s && s.file === p.f.name ? ([Math.min(s.a, s.b), Math.max(s.a, s.b)] as [number, number]) : null;
  };
  const form = () => {
    const f = p.ctx.form();
    return f && f.file === p.f.name ? f : null;
  };
  const label = (a: number, b: number) => linesLabel(p.f.lines.slice(a, b + 1));
  const Notes = (q: { g: { threads: Thread[]; drafts: DraftNote[] } }) => (
    <div class="flex flex-col gap-2">
      {q.g.threads.map((t) => p.ctx.thread(t))}
      {q.g.drafts.map((d) => (
        <DraftBox mr={p.ctx.mr} draft={d} onChange={p.ctx.review.reloadDrafts} />
      ))}
    </div>
  );

  const groupsAt = (i: number) => lineKeys(p.f.lines[i]).flatMap((k) => (placed().map.has(k) ? [placed().map.get(k)!] : []));
  const formAt = (i: number) => {
    const fm = form();
    return !!fm && Math.max(fm.a, fm.b) === i;
  };
  const barAt = (i: number) => mine()?.[1] === i && !formAt(i);
  const has = (i: number) => groupsAt(i).length > 0 || barAt(i) || formAt(i);
  const after = (i: number) => (
    <div class="flex flex-col gap-2">
      <For each={groupsAt(i)}>{(g) => <Notes g={g} />}</For>
      <Show when={barAt(i)}>
        <div class="flex flex-wrap items-center gap-2 animate-pop-in">
          <span class="text-[12px] text-fg-3">{mine() && label(mine()![0], mine()![1])}</span>
          <Button size="sm" variant="primary" onClick={() => p.ctx.ask(p.f)} kbd="A" title="Put these lines in a prompt for Claude in the MR's checkout">
            <Sparkles size={13} /> Ask Claude
          </Button>
          <Button size="sm" disabled={!!p.ctx.review.mode()} onClick={() => p.ctx.openForm(p.f)} kbd="C" title={p.ctx.review.mode() ? "Comments go on all changes: switch to them first" : "Comment on these lines"}>
            <Chat size={13} /> Comment
          </Button>
          <Button size="sm" variant="ghost" onClick={p.ctx.clearSel} title="Clear the selection (Esc)">
            <X size={13} />
          </Button>
        </div>
      </Show>
      <Show when={formAt(i)}>
        <CommentForm
          label={form() ? label(Math.min(form()!.a, form()!.b), Math.max(form()!.a, form()!.b)) : ""}
          text={form()?.text ?? ""}
          onText={p.ctx.setFormText}
          busy={isBusy(`note:${p.ctx.mr.key}`)}
          onSubmit={(draft) => p.ctx.submit(p.f, draft)}
          onCancel={p.ctx.closeForm}
        />
      </Show>
    </div>
  );

  const looseCount = () => placed().loose.threads.length + placed().loose.drafts.length;
  const fileForm = () => form()?.a === FILE;
  return (
    <section ref={el} id={`mr-file-${p.index}`} class="scroll-mt-2 rounded-xl border border-line bg-panel">
      <header class="sticky top-0 z-[1] flex items-center gap-2 rounded-t-xl border-b border-line bg-raised/95 px-3 py-2 backdrop-blur">
        <button class="grid size-5 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg" onClick={() => p.onOpen(!p.open)} aria-expanded={p.open} title={p.open ? "Fold" : "Unfold"}>
          <ChevronDown size={13} class={cls("transition-transform", !p.open && "-rotate-90")} />
        </button>
        <Mark f={p.f} />
        <span class="min-w-0 truncate font-mono text-[12px] text-fg" title={p.f.name}>
          <Show when={p.f.renamed && p.f.old_path !== p.f.new_path}>
            <span class="text-fg-3">{p.f.old_path} → </span>
          </Show>
          {p.f.name}
        </span>
        <Show when={p.threads.length}>
          <span class="shrink-0 text-[11.5px] text-waiting">{p.threads.length} open</span>
        </Show>
        <Show when={p.drafts.length}>
          <span class="shrink-0 text-[11.5px] text-fg-3">{p.drafts.length} draft</span>
        </Show>
        <span class="ml-auto" />
        <button class="grid size-6 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-accent" onClick={() => p.ctx.askFile(p.f)} title="Ask Claude about this file">
          <Sparkles size={13} />
        </button>
        <button
          class="grid size-6 shrink-0 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-40"
          disabled={!!p.ctx.review.mode()}
          onClick={() => (p.onOpen(true), p.ctx.openFileForm(p.f))}
          title={p.ctx.review.mode() ? "Comments go on all changes: switch to them first" : "Comment on this file"}
        >
          <Chat size={13} />
        </button>
        <Counts added={p.f.added} removed={p.f.removed} />
        <label class="flex shrink-0 cursor-pointer select-none items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[12px] text-fg-2 hover:bg-hover">
          <input type="checkbox" checked={p.viewed} onChange={() => p.onViewed()} class="accent-[var(--accent)]" />
          Viewed
        </label>
      </header>
      <Show when={p.open}>
        <Show when={looseCount() || fileForm()}>
          <div class="flex max-w-3xl flex-col gap-2 border-b border-line p-3">
            <Notes g={placed().loose} />
            <Show when={fileForm()}>
              <CommentForm
                label="the whole file"
                text={form()?.text ?? ""}
                onText={p.ctx.setFormText}
                busy={isBusy(`note:${p.ctx.mr.key}`)}
                onSubmit={(draft) => p.ctx.submit(p.f, draft)}
                onCancel={p.ctx.closeForm}
              />
            </Show>
          </div>
        </Show>
        <Show
          when={!p.f.too_large && p.f.lines.length && (full() || p.f.lines.length <= BIG)}
          fallback={
            <div class="px-4 py-4 text-[13px] text-fg-3">
              {p.f.too_large ? (
                "GitLab does not show this diff: it is too large."
              ) : p.f.lines.length ? (
                <>
                  A large diff: {p.f.lines.length} lines.{" "}
                  <button class="text-accent hover:underline" onClick={() => setFull(true)}>
                    Show it
                  </button>
                </>
              ) : p.f.new_file || p.f.deleted || p.f.renamed ? (
                "No text changes. It is a binary file, or only its name changed."
              ) : (
                "No text changes. It is a binary file, or only its mode changed."
              )}
            </div>
          }
        >
          <Show when={near()} fallback={<div style={{ height: `${p.f.lines.length * 19.2}px` }} />}>
            <ReviewDiff lines={p.f.lines} html={html()} split={split()} sel={mine()} onPick={(i, extend) => p.ctx.pick(p.f, i, extend)} onSelect={(a, b) => p.ctx.select(p.f, a, b)} has={has} after={after} />
          </Show>
        </Show>
      </Show>
    </section>
  );
}

function FinishPanel(p: { mr: Mr; review: Review; summary: string; onSummary: (t: string) => void; onClose: () => void }) {
  const [approve, setApprove] = createSignal(false);
  const key = () => `finish:${p.mr.key}`;
  const submit = async () => {
    const r = await act({ type: "finish_review", mr: p.mr.key, summary: p.summary, approve: approve(), head: p.review.data()?.version?.head }, { key: key() });
    if (r.ok) {
      p.onSummary("");
      p.onClose();
      p.review.reloadDrafts();
    }
  };
  const n = () => p.review.drafts().length;
  return (
    <div class="mb-4 rounded-xl border border-line-strong bg-panel p-3 animate-pop-in">
      <div class="mb-2 text-[13px] font-medium text-fg">Finish your review</div>
      <Show when={p.review.drafts().some((d) => !d.path)}>
        <div class="mb-2 flex flex-col gap-2">
          <For each={p.review.drafts().filter((d) => !d.path)}>{(d) => <DraftBox mr={p.mr} draft={d} onChange={p.review.reloadDrafts} />}</For>
        </div>
      </Show>
      <TextBox value={p.summary} onInput={p.onSummary} placeholder="A summary for the author (optional)" onSubmit={submit} onCancel={p.onClose} rows={3} autofocus />
      <div class="mt-2 flex flex-wrap items-center gap-3">
        <label class="flex cursor-pointer select-none items-center gap-1.5 text-[13px] text-fg-2">
          <input type="checkbox" checked={approve()} disabled={p.mr.approved} onChange={(e) => setApprove(e.currentTarget.checked)} class="accent-[var(--accent)]" />
          {p.mr.approved ? "You approved it already" : "Approve the merge request"}
        </label>
        <span class="text-[12px] text-fg-3">
          {n()} draft {n() === 1 ? "comment" : "comments"} will be published
        </span>
        <span class="ml-auto flex gap-2">
          <Button size="sm" variant="ghost" onClick={p.onClose}>
            Cancel
          </Button>
          <Button size="sm" variant="primary" disabled={!n() && !p.summary.trim() && !approve()} loading={isBusy(key())} onClick={submit}>
            Submit review
          </Button>
        </span>
      </div>
    </div>
  );
}

export function Changes(p: { mr: Mr; review: Review; thread: (t: Thread) => JSX.Element }) {
  const files = createMemo<ParsedFile[] | undefined>(() =>
    p.review.files()?.map((f) => ({ ...f, name: f.new_path || f.old_path, ...parseHunks(f.diff), hash: hash(f.diff) })),
  );
  const viewed = viewedStore(() => p.mr.key);
  const [open, setOpen] = createSignal<Record<string, boolean>>({});
  const [sel, setSel] = createSignal<{ file: string; a: number; b: number } | null>(null);
  const [form, setForm] = createSignal<{ file: string; a: number; b: number; text: string } | null>(null);
  const [finishing, setFinishing] = createSignal(false);
  const [summary, setSummary] = createSignal("");
  createEffect(
    on(
      [() => p.mr.key, p.review.mode],
      () => {
        setOpen({});
        setSel(null);
        setForm(null);
      },
      { defer: true },
    ),
  );
  createEffect(on(() => p.mr.key, () => (setFinishing(false), setSummary("")), { defer: true }));

  const isOpen = (f: ParsedFile) => open()[f.name] ?? !viewed.is(f);
  const [current, setCurrent] = createSignal(0);
  const total = createMemo(() => (files() ?? []).reduce((a, f) => ({ added: a.added + f.added, removed: a.removed + f.removed }), { added: 0, removed: 0 }));
  // Positions of threads and drafts are in the MR's latest version, not in one commit's diff.
  const onFile = <T extends { path: string | null }>(list: T[], f: ParsedFile) => (p.review.mode() ? [] : list.filter((t) => t.path && (t.path === f.new_path || t.path === f.old_path)));

  const goTo = (i: number) => {
    const l = files();
    if (!l?.length) return;
    const n = Math.max(0, Math.min(i, l.length - 1));
    setCurrent(n);
    document.getElementById(`mr-file-${n}`)?.scrollIntoView({ block: "start" });
  };
  const markViewed = (f: ParsedFile, i: number) => {
    const was = viewed.is(f);
    viewed.toggle(f);
    setOpen((o) => ({ ...o, [f.name]: was }));
    if (!was) goTo(i);
  };

  const fileOf = (name: string) => files()?.find((f) => f.name === name);
  const sameHunk = (f: ParsedFile, x: number, y: number) => !f.lines.slice(Math.min(x, y), Math.max(x, y) + 1).some((l) => l.type === "hunk");

  const ctx: FileCtx = {
    get mr() {
      return p.mr;
    },
    get review() {
      return p.review;
    },
    get thread() {
      return p.thread;
    },
    sel,
    pick: (f, i, extend) => {
      const s = sel();
      if (extend && s && s.file === f.name && sameHunk(f, s.a, i)) setSel({ ...s, b: i });
      else if (s && s.file === f.name && s.a === i && s.b === i) setSel(null);
      else setSel({ file: f.name, a: i, b: i });
    },
    select: (f, a, b) => setSel({ file: f.name, a, b }),
    clearSel: () => setSel(null),
    form,
    setFormText: (t) => setForm((x) => x && { ...x, text: t }),
    openForm: (f) => {
      const s = sel();
      if (s && s.file === f.name && !p.review.mode()) setForm({ ...s, text: form()?.text ?? "" });
    },
    openFileForm: (f) => {
      if (p.review.mode()) return;
      setSel(null);
      setForm({ file: f.name, a: FILE, b: FILE, text: form()?.text ?? "" });
    },
    closeForm: () => (setForm(null), setSel(null)),
    submit: async (f, draft) => {
      const fm = form();
      const v = p.review.data()?.version;
      if (!fm || !v) return;
      const a = Math.min(fm.a, fm.b);
      const b = Math.max(fm.a, fm.b);
      const pos = a === FILE ? filePosition(v, f) : await position(v, f, f.lines[a], f.lines[b]);
      const r = await act({ type: "mr_note", mr: p.mr.key, body: fm.text, position: pos, draft }, { key: `note:${p.mr.key}` });
      if (!r.ok) return;
      setForm(null);
      setSel(null);
      if (draft) p.review.reloadDrafts();
    },
    ask: (f) => {
      const s = sel();
      if (!s || s.file !== f.name) return;
      const a = Math.min(s.a, s.b);
      const b = Math.max(s.a, s.b);
      setLastAsk({ mr: p.mr.key, mode: p.review.mode(), file: f.name, a, b });
      void askClaude(p.mr, askPrompt(p.mr, f, f.lines.slice(a, b + 1), p.review.mode(), p.review.data()?.version));
    },
    askFile: (f) => {
      setLastAsk({ mr: p.mr.key, mode: p.review.mode(), file: f.name, a: FILE, b: FILE });
      void askClaude(p.mr, askPrompt(p.mr, f, [], p.review.mode(), p.review.data()?.version));
    },
  };

  // "Comment with this" from the chat: open the form on the lines Claude was asked about.
  createEffect(() => {
    const req = composeReq();
    if (!req || req.mr !== p.mr.key) return;
    if (p.review.mode()) p.review.setMode("");
    const f = fileOf(req.file);
    if (!f) return;
    setComposeReq(null);
    setOpen((o) => ({ ...o, [f.name]: true }));
    setSel({ file: f.name, a: req.a, b: req.b });
    setForm({ file: f.name, a: req.a, b: req.b, text: req.text });
    const i = files()!.indexOf(f);
    requestAnimationFrame(() => document.getElementById(`mr-file-${i}`)?.scrollIntoView({ block: "start" }));
  });
  createEffect(() => {
    const req = finishReq();
    if (!req || req.mr !== p.mr.key) return;
    setFinishReq(null);
    setSummary((s) => (s.trim() ? `${s.trim()}\n\n${req.text}` : req.text));
    setFinishing(true);
  });

  // Capture phase: with lines selected, these keys act on them before the GitLab view's own keys.
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    if ((e.target as HTMLElement).closest("input,textarea,select,[contenteditable],.xterm")) return;
    const s = sel();
    const f = s && fileOf(s.file);
    if (e.key === "]") goTo(current() + 1);
    else if (e.key === "[") goTo(current() - 1);
    else if (e.key === "s") toggleSplit();
    else if (e.key === "Escape" && (s || form())) (setForm(null), setSel(null));
    else if (e.key === "a" && f) ctx.ask(f);
    else if (e.key === "c" && f && !p.review.mode()) ctx.openForm(f);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };
  onMount(() => window.addEventListener("keydown", onKey, true));
  onCleanup(() => window.removeEventListener("keydown", onKey, true));

  const reviewed = () => p.review.data()?.reviewed;
  const drafts = () => p.review.drafts();

  return (
    <Loading review={p.review}>
      <div class="mb-3 flex flex-wrap items-center gap-2">
        <label class="flex h-8 min-w-0 max-w-full items-center rounded-lg border border-line-strong bg-raised pl-2.5 text-[12.5px] focus-within:border-accent/60">
          <select class="min-w-0 max-w-[28rem] truncate bg-transparent py-1 pr-2 text-fg outline-none" value={p.review.mode()} onChange={(e) => p.review.setMode(e.currentTarget.value)}>
            <option value="">All changes</option>
            <Show when={reviewed()}>{(r) => <option value={`s:${r()}`}>Since your last review ({r().slice(0, 8)})</option>}</Show>
            <For each={p.review.data()?.commits}>
              {(c) => (
                <option value={`c:${c.sha}`}>
                  {c.short} {c.title}
                </option>
              )}
            </For>
          </select>
        </label>
        <Show when={files()}>
          {(l) => (
            <span class="text-[12.5px] text-fg-3">
              {l().length} {l().length === 1 ? "file" : "files"} · <Counts {...total()} />
            </span>
          )}
        </Show>
        <Show when={reviewed() && !p.review.mode()}>
          <Button size="sm" variant="ghost" onClick={() => p.review.setMode(`s:${reviewed()}`)} title="Show only what changed since you last finished a review or approved">
            Changes since your last review
          </Button>
        </Show>
        <Show when={p.review.mode()}>
          <Button size="sm" variant="ghost" onClick={() => p.review.setMode("")}>
            Show all changes
          </Button>
        </Show>
        <span class="ml-auto flex items-center gap-2">
          <div class="flex rounded-lg bg-hover p-0.5 text-[12px]" role="group" title="Layout (S)">
            <button class={cls("rounded-md px-2 py-1", !split() ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => split() && toggleSplit()}>
              Unified
            </button>
            <button class={cls("rounded-md px-2 py-1", split() ? "bg-raised text-fg shadow-soft" : "text-fg-2")} onClick={() => !split() && toggleSplit()}>
              Side by side
            </button>
          </div>
          <Button size="sm" variant={drafts().length ? "primary" : "secondary"} onClick={() => setFinishing(!finishing())} title="Publish your draft comments, with a summary, and approve">
            Finish review
            <Show when={drafts().length}>
              <span class="rounded-full bg-black/15 px-1.5 text-[11px] tabular-nums">{drafts().length}</span>
            </Show>
          </Button>
        </span>
      </div>
      <Show when={finishing()}>
        <FinishPanel mr={p.mr} review={p.review} summary={summary()} onSummary={setSummary} onClose={() => setFinishing(false)} />
      </Show>
      <Show when={files()} fallback={<div class="flex items-center gap-2 py-10 text-fg-3"><Spinner size={14} /> Fetching the changes…</div>}>
        {(l) => (
          <Show when={l().length} fallback={<div class="rounded-xl border border-dashed border-line px-4 py-5 text-center text-fg-3">No changes.</div>}>
            <div class="mb-2 text-[12px] text-fg-3">Press a line number and drag to select lines. Let go to ask Claude about them or comment.</div>
            <div class="grid items-start gap-4 xl:grid-cols-[16rem_minmax(0,1fr)]">
              <nav class="sticky top-2 hidden max-h-[calc(100vh-6rem)] overflow-y-auto rounded-xl border border-line bg-panel py-1 xl:block">
                <For each={l()}>
                  {(f, i) => (
                    <button
                      class={cls("flex w-full items-center gap-1.5 px-2.5 py-1 text-left text-[12px] hover:bg-hover/60", current() === i() && "bg-hover")}
                      onClick={() => goTo(i())}
                      title={f.name}
                    >
                      <Mark f={f} />
                      <span class={cls("min-w-0 flex-1 truncate", viewed.is(f) ? "text-fg-3" : "text-fg")}>
                        {f.name.split("/").pop()}
                        <span class="ml-1 text-fg-3">{f.name.includes("/") ? f.name.slice(0, f.name.lastIndexOf("/")) : ""}</span>
                      </span>
                      <Show when={viewed.is(f)} fallback={<Counts added={f.added} removed={f.removed} />}>
                        <Check size={12} class="shrink-0 text-idle" />
                      </Show>
                    </button>
                  )}
                </For>
              </nav>
              <div class="flex min-w-0 flex-col gap-4">
                <For each={l()}>
                  {(f, i) => (
                    <FileBox
                      f={f}
                      index={i()}
                      viewed={viewed.is(f)}
                      onViewed={() => markViewed(f, i() + 1)}
                      threads={onFile(p.mr.threads, f)}
                      drafts={onFile(drafts(), f)}
                      open={isOpen(f)}
                      onOpen={(o) => setOpen((x) => ({ ...x, [f.name]: o }))}
                      ctx={ctx}
                    />
                  )}
                </For>
              </div>
            </div>
          </Show>
        )}
      </Show>
    </Loading>
  );
}
