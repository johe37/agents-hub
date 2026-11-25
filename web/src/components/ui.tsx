import { Show, splitProps, type JSX } from "solid-js";
import type { Status } from "../api";
import { STATUS, cls } from "../util";

export function StatusDot(p: { status: Status | null; size?: number; class?: string }) {
  const meta = () => (p.status ? STATUS[p.status] : null);
  return (
    <span
      class={cls("inline-block shrink-0 rounded-full", p.status === "working" && "animate-pulse-dot", p.class)}
      style={{
        width: `${p.size ?? 8}px`,
        height: `${p.size ?? 8}px`,
        background: meta()?.color ?? "transparent",
        color: meta()?.color,
        border: p.status ? undefined : "1.5px solid var(--line-strong)",
      }}
      title={meta()?.label}
    />
  );
}

export function StatusPill(p: { status: Status; class?: string }) {
  return (
    <span
      class={cls("inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11.5px] font-medium", p.class)}
      style={{ background: STATUS[p.status].soft, color: STATUS[p.status].color }}
    >
      <StatusDot status={p.status} size={6} />
      {STATUS[p.status].label}
    </span>
  );
}

type BtnProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger" | "approve";
  size?: "sm" | "md";
  loading?: boolean;
  kbd?: string;
};

export function Button(props: BtnProps) {
  const [p, rest] = splitProps(props, ["variant", "size", "loading", "kbd", "class", "children", "disabled"]);
  const v = () => p.variant ?? "secondary";
  return (
    <button
      {...rest}
      disabled={p.disabled || p.loading}
      class={cls(
        "relative inline-flex select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-lg font-medium transition-[background,color,box-shadow,transform] duration-100 active:scale-[0.97] disabled:pointer-events-none disabled:opacity-50",
        (p.size ?? "md") === "md" ? "h-8 px-3 text-[13px]" : "h-7 px-2.5 text-[12.5px]",
        v() === "primary" && "bg-accent text-accent-fg shadow-soft hover:brightness-110",
        v() === "approve" && "bg-idle text-white shadow-soft hover:brightness-110 dark:text-[#06140c]",
        v() === "secondary" && "border border-line-strong bg-raised text-fg hover:bg-hover",
        v() === "ghost" && "text-fg-2 hover:bg-hover hover:text-fg",
        v() === "danger" && "border border-line-strong bg-raised text-danger hover:bg-danger-soft",
        p.class,
      )}
    >
      <Show when={p.loading}>
        <span class="absolute inset-0 grid place-items-center">
          <Spinner />
        </span>
      </Show>
      <span class={cls("inline-flex items-center gap-1.5", p.loading && "invisible")}>
        {p.children}
        <Show when={p.kbd}>
          <span class="kbd ml-1 opacity-80">{p.kbd}</span>
        </Show>
      </span>
    </button>
  );
}

export function IconButton(props: JSX.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean }) {
  const [p, rest] = splitProps(props, ["label", "class", "children", "active"]);
  return (
    <button
      {...rest}
      aria-label={p.label}
      title={p.label}
      class={cls(
        "inline-grid size-8 shrink-0 place-items-center rounded-lg text-fg-2 transition-colors hover:bg-hover hover:text-fg",
        p.active && "bg-hover text-fg",
        p.class,
      )}
    >
      {p.children}
    </button>
  );
}

export function Spinner(p: { size?: number }) {
  return (
    <svg width={p.size ?? 14} height={p.size ?? 14} viewBox="0 0 24 24" class="animate-spin" aria-hidden="true">
      <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-opacity="0.2" stroke-width="3" />
      <path d="M21 12a9 9 0 0 0-9-9" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
    </svg>
  );
}

export function Kbd(p: { children: JSX.Element }) {
  return <span class="kbd">{p.children}</span>;
}

export function Badge(p: { children: JSX.Element; tone?: "neutral" | "accent" | "waiting" | "danger" | "add"; class?: string; title?: string }) {
  const tone = () => p.tone ?? "neutral";
  return (
    <span
      title={p.title}
      class={cls(
        "inline-flex h-5 items-center gap-1 rounded-md px-1.5 text-[11px] font-medium tabular-nums",
        tone() === "neutral" && "bg-hover text-fg-2",
        tone() === "accent" && "bg-accent-soft text-accent",
        tone() === "waiting" && "bg-waiting-soft text-waiting",
        tone() === "danger" && "bg-danger-soft text-danger",
        tone() === "add" && "bg-add-soft text-add",
        p.class,
      )}
    >
      {p.children}
    </span>
  );
}

export function Empty(p: { icon: JSX.Element; title: string; children?: JSX.Element; class?: string }) {
  return (
    <div class={cls("flex flex-col items-center justify-center px-6 py-16 text-center animate-fade-in", p.class)}>
      <div class="mb-4 grid size-12 place-items-center rounded-2xl border border-line bg-panel text-fg-3 shadow-soft">{p.icon}</div>
      <div class="text-[15px] font-semibold text-fg">{p.title}</div>
      <div class="mt-1 max-w-sm text-fg-2">{p.children}</div>
    </div>
  );
}

/** A tooltip-free hint: the label and its key, for menus and buttons. */
export function Hint(p: { label: string; keys?: string[] }) {
  return (
    <span class="inline-flex items-center gap-1.5">
      {p.label}
      <Show when={p.keys}>
        {p.keys!.map((k) => (
          <Kbd>{k}</Kbd>
        ))}
      </Show>
    </span>
  );
}
