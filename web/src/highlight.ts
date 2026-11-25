import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const LANGS = { bash, css, dockerfile, go, ini, java, javascript, json, makefile, markdown, python, rust, sql, typescript, xml, yaml };
for (const [name, lang] of Object.entries(LANGS)) hljs.registerLanguage(name, lang);

const BY_EXT: Record<string, string> = {
  sh: "bash", bash: "bash", zsh: "bash", css: "css", go: "go", ini: "ini", toml: "ini", cfg: "ini", conf: "ini", java: "java",
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript", json: "json", md: "markdown", py: "python",
  rs: "rust", sql: "sql", ts: "typescript", tsx: "typescript", html: "xml", xml: "xml", svg: "xml", vue: "xml",
  yml: "yaml", yaml: "yaml", j2: "yaml",
};
const BY_NAME: Record<string, string> = { Dockerfile: "dockerfile", Containerfile: "dockerfile", Makefile: "makefile", Jenkinsfile: "java" };

export function language(path: string): string | null {
  const name = path.split("/").pop() ?? "";
  if (BY_NAME[name]) return BY_NAME[name];
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return BY_EXT[ext] ?? null;
}

/** Highlighted HTML for each line of text. Spans that cross a line end close there and open again. */
export function highlightLines(text: string, lang: string): string[] {
  const html = hljs.highlight(text, { language: lang, ignoreIllegals: true }).value;
  const lines: string[] = [];
  const open: string[] = [];
  let cur = "";
  for (const tok of html.match(/<span[^>]*>|<\/span>|\n|[^<\n]+/g) ?? []) {
    if (tok === "\n") {
      lines.push(cur + "</span>".repeat(open.length));
      cur = open.join("");
    } else {
      if (tok.startsWith("<span")) open.push(tok);
      else if (tok === "</span>") open.pop();
      cur += tok;
    }
  }
  lines.push(cur);
  return lines;
}
