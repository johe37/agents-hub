import { createMemo } from "solid-js";
import { Marked } from "marked";
import { FINDINGS_LANG, parseFindings } from "../findings";
import { cls } from "../util";

const escape = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// Transcripts are local, but a reply can quote any HTML; show it as text, never run it.
const md = new Marked({
  gfm: true,
  breaks: false,
  renderer: {
    html({ text }) {
      return escape(text);
    },
    link({ href, text }) {
      const safe = /^(https?:|mailto:|#)/i.test(href) ? href : "#";
      return `<a href="${escape(safe)}" target="_blank" rel="noopener noreferrer">${text}</a>`;
    },
    image({ text }) {
      return escape(text);
    },
    // The findings of a review show in the diff; here they would be a wall of JSON.
    code({ text, lang }) {
      if (lang !== FINDINGS_LANG) return false;
      const n = parseFindings("```" + FINDINGS_LANG + "\n" + text + "\n```")?.length;
      const what = n == null ? "Findings Jugglr could not read" : `${n} ${n === 1 ? "finding" : "findings"}: they show in the diff, on the Changes tab`;
      return `<div class="my-2 rounded-lg border border-accent/30 bg-accent/10 px-3 py-2 text-sm text-fg-2">✦ ${what}</div>`;
    },
  },
});

const cache = new Map<string, string>();

export function render(text: string): string {
  let html = cache.get(text);
  if (html === undefined) {
    html = md.parse(text, { async: false }) as string;
    if (cache.size > 400) cache.clear();
    cache.set(text, html);
  }
  return html;
}

export function Markdown(p: { text: string; class?: string }) {
  const html = createMemo(() => render(p.text));
  return <div class={cls("prose-chat", p.class)} innerHTML={html()} />;
}
