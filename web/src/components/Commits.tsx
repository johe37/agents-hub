import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { getJSON, type Commit, type Workspace } from "../api";
import { setDiffView, toast } from "../store";
import { cls } from "../util";
import { ArrowUp, Branch, Check, Chevron, ChevronDown, Copy, FileDiff, Search, X } from "./icons";
import { Ago, SectionTitle, Spinner } from "./ui";

const PAGE = 50;

type Mode = "own" | "all";
type Ref = [kind: string, name: string];

interface Page {
  commits: Commit[];
  more: boolean;
  mode?: Mode;
  base?: string | null;
  error?: string;
}

interface Branches {
  current: string | null;
  branches: [kind: "local" | "remote", name: string, ref: string][];
}

const FILE_TONE: Record<string, string> = { M: "text-waiting", A: "text-add", D: "text-del", R: "text-working" };

const REF_TONE: Record<string, string> = {
  head: "bg-accent-soft text-accent",
  local: "bg-hover text-fg-2",
  remote: "bg-working-soft text-working",
  tag: "bg-waiting-soft text-waiting",
};

/** The clipboard API needs a secure page; the page is often plain http on the LAN. */
async function copy(text: string, what: string) {
  try {
    if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(text);
    else {
      const t = document.createElement("textarea");
      t.value = text;
      t.style.position = "fixed";
      t.style.opacity = "0";
      document.body.append(t);
      t.select();
      const ok = document.execCommand("copy");
      t.remove();
      if (!ok) throw new Error();
    }
    toast(`Copied ${what}`);
  } catch {
    toast(`Could not copy ${what}`, true);
  }
}

/** A list fixed to the screen at a point, so the scrolling side panel never cuts it off. */
/** anchor is the button that opens it: a press there must not close it just before the click opens it again. */
function Popup(p: { at: { x: number; y: number }; anchor?: HTMLElement; onClose: () => void; class?: string; children: JSX.Element }) {
  let el: HTMLDivElement | undefined;
  const outside = (e: MouseEvent) => {
    const t = e.target as Node;
    if (!el?.contains(t) && !p.anchor?.contains(t)) p.onClose();
  };
  const key = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    p.onClose();
  };
  document.addEventListener("mousedown", outside);
  window.addEventListener("keydown", key, true);
  window.addEventListener("resize", p.onClose);
  onCleanup(() => {
    document.removeEventListener("mousedown", outside);
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("resize", p.onClose);
  });
  const [pos, setPos] = createSignal(p.at);
  // Keep it on the screen.
  queueMicrotask(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ x: Math.max(8, Math.min(p.at.x, window.innerWidth - r.width - 8)), y: Math.max(8, Math.min(p.at.y, window.innerHeight - r.height - 8)) });
  });
  return (
    <Portal>
      <div
        ref={el}
        class={cls("fixed z-[60] rounded-xl border border-line-strong bg-raised p-1 shadow-float animate-pop-in", p.class)}
        style={{ left: `${pos().x}px`, top: `${pos().y}px` }}
      >
        {p.children}
      </div>
    </Portal>
  );
}

function MenuItem(p: { icon?: JSX.Element; label: string; run: () => void; close: () => void }) {
  return (
    <button
      role="menuitem"
      class="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-base text-fg outline-none hover:bg-hover focus:bg-hover"
      onClick={() => (p.close(), p.run())}
    >
      <span class="grid w-4 place-items-center text-fg-3">{p.icon}</span>
      <span class="flex-1">{p.label}</span>
    </button>
  );
}

function BranchPicker(p: { path: string; at: { x: number; y: number; width: number }; anchor: HTMLElement; current: string | null; onPick: (ref: string | null) => void; onClose: () => void }) {
  const [data, setData] = createSignal<Branches | null>(null);
  const [query, setQuery] = createSignal("");
  const [sel, setSel] = createSignal(0);
  getJSON<Branches>(`/api/branches?path=${encodeURIComponent(p.path)}`).then(setData, (e) => toast(`Could not list branches: ${(e as Error).message}`, true));
  const items = createMemo(() => {
    const q = query().trim().toLowerCase();
    const all: { ref: string | null; name: string; kind: string }[] = [{ ref: null, name: "Checked-out branch", kind: "head" }];
    for (const [kind, name, ref] of data()?.branches ?? []) all.push({ ref, name, kind });
    return q ? all.filter((b) => b.ref && b.name.toLowerCase().includes(q)) : all;
  });
  createEffect(on(query, () => setSel(0)));
  let list: HTMLDivElement | undefined;
  createEffect(() => list?.querySelector(`[data-i="${sel()}"]`)?.scrollIntoView({ block: "nearest" }));
  const pick = (ref: string | null) => (p.onPick(ref), p.onClose());
  const key = (e: KeyboardEvent) => {
    const n = items().length;
    if (e.key === "ArrowDown") setSel((i) => (i + 1) % n);
    else if (e.key === "ArrowUp") setSel((i) => (i - 1 + n) % n);
    else if (e.key === "Enter") items()[sel()] && pick(items()[sel()].ref);
    else return;
    e.preventDefault();
  };
  return (
    <Popup at={p.at} anchor={p.anchor} onClose={p.onClose} class="flex max-h-[60vh] flex-col">
      <div style={{ width: `${Math.max(p.at.width, 256)}px` }} class="flex min-h-0 flex-col">
        <input
          ref={(el) => queueMicrotask(() => el.focus())}
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={key}
          placeholder="Filter branches"
          aria-label="Filter branches"
          class="m-1 h-7 rounded-md border border-line-strong bg-bg px-2 text-sm outline-none placeholder:text-fg-3 focus:border-accent/60"
        />
        <div ref={list} role="listbox" class="min-h-0 flex-1 overflow-y-auto">
          <Show when={data()} fallback={<div class="grid place-items-center py-4 text-fg-3"><Spinner /></div>}>
            <For each={items()} fallback={<div class="px-2.5 py-2 text-sm text-fg-3">No branch matches</div>}>
              {(b, i) => (
                <button
                  role="option"
                  data-i={i()}
                  aria-selected={sel() === i()}
                  class={cls("flex w-full items-center gap-2 rounded-lg px-2.5 py-1 text-left text-sm", sel() === i() ? "bg-hover text-fg" : "text-fg-2")}
                  onMouseMove={() => setSel(i())}
                  onClick={() => pick(b.ref)}
                >
                  <span class={cls("w-12 shrink-0 text-xs", b.kind === "remote" ? "text-working" : "text-fg-3")}>{b.kind === "head" ? "" : b.kind}</span>
                  <span class={cls("min-w-0 flex-1 truncate", b.kind !== "head" && "font-mono")}>{b.name}</span>
                  <Show when={b.ref === p.current}>
                    <Check size={13} class="shrink-0 text-accent" />
                  </Show>
                </button>
              )}
            </For>
          </Show>
        </div>
      </div>
    </Popup>
  );
}

export function Commits(p: { ws: Workspace }) {
  const [rev, setRev] = createSignal<string | null>(null);
  const [mode, setMode] = createSignal<Mode | null>(null);
  const [search, setSearch] = createSignal("");
  const [query, setQuery] = createSignal("");
  const [searching, setSearching] = createSignal(false);
  const [items, setItems] = createSignal<Commit[]>([]);
  const [page, setPage] = createSignal<Omit<Page, "commits">>({ more: false });
  const [loading, setLoading] = createSignal(false);
  const [open, setOpen] = createSignal<Set<string>>(new Set());
  const [files, setFiles] = createSignal<Record<string, [string, string][] | undefined>>({});
  const [picker, setPicker] = createSignal<{ x: number; y: number; width: number } | null>(null);
  const [menu, setMenu] = createSignal<{ x: number; y: number; c: Commit } | null>(null);

  let timer: number | undefined;
  createEffect(on(search, (s) => (clearTimeout(timer), (timer = window.setTimeout(() => setQuery(s.trim()), 250)))));
  onCleanup(() => clearTimeout(timer));

  createEffect(
    on(
      () => p.ws.path,
      () => {
        setRev(null);
        setMode(null);
        setSearch("");
        setSearching(false);
        setOpen(new Set<string>());
        setFiles({});
      },
      { defer: true },
    ),
  );

  // Strings, so a state update that leaves them the same does not refetch.
  const view = createMemo(() => JSON.stringify([p.ws.path, rev(), mode(), query()]));
  const head = createMemo(() => p.ws.git?.commit ?? "");

  let seq = 0;
  async function load(skip: number, limit: number) {
    const id = ++seq;
    setLoading(true);
    const m = skip ? page().mode : mode();
    const q = new URLSearchParams({ path: p.ws.path, skip: String(skip), limit: String(limit) });
    if (rev()) q.set("rev", rev()!);
    if (m) q.set("mode", m);
    if (query()) q.set("q", query());
    try {
      const r = await getJSON<Page>(`/api/commits?${q}`);
      if (id !== seq) return;
      const { commits, ...rest } = r;
      setPage(rest);
      // Keep the objects of unchanged commits, so their rows are not built again.
      const old = new Map(items().map((c) => [c.sha, c]));
      const fresh = commits.map((c) => {
        const o = old.get(c.sha);
        return o && JSON.stringify(o) === JSON.stringify(c) ? o : c;
      });
      setItems(skip ? [...items().slice(0, skip), ...fresh] : fresh);
    } catch (e) {
      if (id === seq) setPage({ more: false, error: (e as Error).message });
    } finally {
      if (id === seq) setLoading(false);
    }
  }

  // A new view starts at the first page; a moved HEAD reloads as many as were shown.
  createEffect(
    on([view, head], ([v], prev) => {
      const same = prev?.[0] === v;
      if (!same) setItems([]);
      void load(0, same ? Math.min(500, Math.max(PAGE, items().length)) : PAGE);
    }),
  );
  const more = () => !loading() && page().more && void load(items().length, PAGE);

  const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && more(), { rootMargin: "200px" });
  onCleanup(() => io.disconnect());
  let pickerBtn!: HTMLButtonElement;

  const toggle = (sha: string) => {
    const s = new Set(open());
    if (s.has(sha)) s.delete(sha);
    else {
      s.add(sha);
      if (!files()[sha])
        getJSON<[string, string][]>(`/api/commits/files?path=${encodeURIComponent(p.ws.path)}&sha=${sha}`).then(
          (f) => setFiles((all) => ({ ...all, [sha]: f })),
          (e) => toast(`Could not list files: ${(e as Error).message}`, true),
        );
    }
    setOpen(s);
  };
  const show = (c: Commit, file: string | null) => setDiffView({ workspace: p.ws.name, path: p.ws.path, file, title: c.title, commit: c });
  const browse = ([kind, name]: Ref) => {
    setRev(kind === "tag" ? `refs/tags/${name}` : kind === "remote" ? `refs/remotes/${name}` : name === "HEAD" ? null : `refs/heads/${name}`);
    setMode("all");
  };

  const revLabel = () => {
    const r = rev();
    if (!r) return p.ws.git?.branch ?? "HEAD";
    return r.replace(/^refs\/(heads|remotes|tags)\//, "");
  };
  const empty = () => {
    if (page().error) return page().error;
    if (query()) return "No commit matches";
    if (page().mode === "own") return `No commits that ${page().base ?? "the default branch"} lacks`;
    return "No commits";
  };

  return (
    <section class="shrink-0 border-t border-line p-3">
      <div class="mb-2 flex items-center gap-1 px-1">
        <SectionTitle class="flex-1">Commits</SectionTitle>
        <Show when={page().base}>
          <div class="flex rounded-md bg-hover p-0.5 text-xs" role="group" aria-label="Which commits">
            <For each={["own", "all"] as Mode[]}>
              {(m) => (
                <button
                  class={cls("rounded px-1.5 py-0.5", page().mode === m ? "bg-raised text-fg shadow-sm" : "text-fg-3 hover:text-fg")}
                  aria-pressed={page().mode === m}
                  title={m === "own" ? `Commits that ${page().base} lacks` : "The whole history"}
                  onClick={() => setMode(m)}
                >
                  {m === "own" ? "Own" : "All"}
                </button>
              )}
            </For>
          </div>
        </Show>
        <button
          class={cls("grid size-6 place-items-center rounded-md hover:bg-hover hover:text-fg", searching() ? "text-fg" : "text-fg-3")}
          aria-label="Search commits"
          title="Search commits"
          aria-pressed={searching()}
          onClick={() => {
            if (searching()) setSearch("");
            setSearching(!searching());
          }}
        >
          <Search size={13} />
        </button>
      </div>

      <button
        ref={pickerBtn}
        class="mb-1.5 flex w-full min-w-0 items-center gap-1.5 rounded-md border border-line px-2 py-1 text-left text-sm text-fg-2 hover:border-line-strong hover:text-fg"
        title="Show another branch"
        aria-haspopup="listbox"
        aria-expanded={!!picker()}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setPicker(picker() ? null : { x: r.left, y: r.bottom + 4, width: r.width });
        }}
      >
        <Branch size={13} class="shrink-0 text-fg-3" />
        <span class="min-w-0 flex-1 truncate font-mono">{revLabel()}</span>
        <Show when={rev()}>
          <span
            role="button"
            class="grid size-4 shrink-0 place-items-center rounded text-fg-3 hover:text-fg"
            title="Back to the checked-out branch"
            onClick={(e) => (e.stopPropagation(), setRev(null), setMode(null))}
          >
            <X size={12} />
          </span>
        </Show>
        <ChevronDown size={12} class="shrink-0 text-fg-3" />
      </button>

      <Show when={searching()}>
        <input
          ref={(el) => queueMicrotask(() => el.focus())}
          value={search()}
          onInput={(e) => setSearch(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), setSearch(""), setSearching(false))}
          placeholder="Message, author or SHA"
          aria-label="Search commits"
          class="mb-1.5 h-7 w-full rounded-md border border-line-strong bg-raised px-2 text-sm outline-none placeholder:text-fg-3 focus:border-accent/60"
        />
      </Show>

      <div class="flex flex-col">
        <For each={items()} fallback={<div class="px-2 py-3 text-sm text-fg-3">{loading() ? <Spinner size={14} /> : empty()}</div>}>
          {(c) => (
            <div>
              <div
                class={cls("group flex min-w-0 items-start gap-1 rounded-md px-1 py-1 hover:bg-hover", menu()?.c.sha === c.sha && "bg-hover")}
                onContextMenu={(e) => (e.preventDefault(), setMenu({ x: e.clientX, y: e.clientY, c }))}
              >
                <button
                  class="mt-0.5 grid size-4 shrink-0 place-items-center text-fg-3 hover:text-fg"
                  aria-label={open().has(c.sha) ? "Hide files" : "Show files"}
                  aria-expanded={open().has(c.sha)}
                  onClick={() => toggle(c.sha)}
                >
                  <Chevron size={12} class={cls("transition-transform", open().has(c.sha) && "rotate-90")} />
                </button>
                <button class="min-w-0 flex-1 text-left" title={c.message ? `${c.title}\n\n${c.message}` : c.title} onClick={() => show(c, null)}>
                  <div class="truncate text-sm text-fg">{c.title}</div>
                  <div class="flex min-w-0 items-center gap-1 text-xs text-fg-3">
                    <span class="font-mono">{c.short}</span>
                    <Show when={c.unpushed}>
                      <span title="Not pushed" class="text-waiting">
                        <ArrowUp size={11} />
                      </span>
                    </Show>
                    <span class="min-w-0 truncate">
                      · {c.author} · <Ago ts={c.created} suffix />
                    </span>
                  </div>
                </button>
                <button
                  class="mt-0.5 grid size-5 shrink-0 place-items-center rounded text-fg-3 opacity-0 hover:bg-raised hover:text-fg focus:opacity-100 group-hover:opacity-100"
                  aria-label="Copy SHA"
                  title={`Copy ${c.sha}`}
                  onClick={() => copy(c.sha, c.short)}
                >
                  <Copy size={12} />
                </button>
              </div>
              <Show when={c.refs?.length}>
                <div class="mb-0.5 ml-6 flex flex-wrap gap-1">
                  <For each={c.refs}>
                    {(r) => (
                      <button
                        class={cls("max-w-full truncate rounded px-1 font-mono text-[11px] leading-4 hover:brightness-110", REF_TONE[r[0]] ?? REF_TONE.local)}
                        title={r[0] === "tag" ? `Tag ${r[1]}: show its history` : `Show ${r[1]}`}
                        onClick={() => browse(r as Ref)}
                      >
                        {r[0] === "tag" ? `# ${r[1]}` : r[1]}
                      </button>
                    )}
                  </For>
                </div>
              </Show>
              <Show when={open().has(c.sha)}>
                <div class="mb-1 ml-5 flex flex-col">
                  <Show when={files()[c.sha]} fallback={<div class="px-1.5 py-0.5"><Spinner size={12} /></div>}>
                    {(list) => (
                      <For each={list()} fallback={<div class="px-1.5 py-0.5 text-sm text-fg-3">No files</div>}>
                        {([code, file]) => {
                          const i = file.lastIndexOf("/");
                          return (
                            <button class="flex min-w-0 items-center gap-2 rounded-md px-1.5 py-0.5 text-left hover:bg-hover" title={file} onClick={() => show(c, file)}>
                              <span class={cls("w-3 shrink-0 text-center font-mono text-xs font-semibold", FILE_TONE[code] ?? "text-fg-2")}>{code}</span>
                              <span class="min-w-0 truncate font-mono text-sm">
                                <span class="text-fg">{file.slice(i + 1)}</span>
                                <Show when={i > 0}>
                                  <span class="ml-1.5 text-fg-3">{file.slice(0, i)}</span>
                                </Show>
                              </span>
                            </button>
                          );
                        }}
                      </For>
                    )}
                  </Show>
                </div>
              </Show>
            </div>
          )}
        </For>
        <Show when={items().length && page().more}>
          <div ref={(el) => (io.observe(el), onCleanup(() => io.unobserve(el)))}>
            <button class="flex w-full items-center justify-center gap-2 rounded-md py-1.5 text-sm text-fg-3 hover:bg-hover hover:text-fg" disabled={loading()} onClick={more}>
              {loading() ? <Spinner size={13} /> : "Load more"}
            </button>
          </div>
        </Show>
      </div>

      <Show when={picker()}>
        {(at) => <BranchPicker path={p.ws.path} at={at()} anchor={pickerBtn} current={rev()} onPick={(r) => (setRev(r), setMode(r ? "all" : null))} onClose={() => setPicker(null)} />}
      </Show>
      <Show when={menu()}>
        {(m) => {
          const close = () => setMenu(null);
          return (
            <Popup at={m()} onClose={close} class="min-w-[12rem]">
              <div role="menu">
                <MenuItem icon={<FileDiff size={14} />} label="Show changes" run={() => show(m().c, null)} close={close} />
                <MenuItem icon={<Copy size={14} />} label="Copy SHA" run={() => copy(m().c.sha, "SHA")} close={close} />
                <MenuItem label="Copy short SHA" run={() => copy(m().c.short, "short SHA")} close={close} />
                <MenuItem label="Copy message" run={() => copy(m().c.message ? `${m().c.title}\n\n${m().c.message}` : m().c.title, "message")} close={close} />
              </div>
            </Popup>
          );
        }}
      </Show>
    </section>
  );
}
