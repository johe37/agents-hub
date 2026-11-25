import { createMemo } from "solid-js";
import { Marked } from "marked";
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
