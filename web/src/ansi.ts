const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** A CI log as HTML: SGR colors become spans, GitLab's section markers and other escapes go away. */
export function ansiToHtml(log: string): string {
  const text = log.replace(/section_(?:start|end):\d+:[^\r\n]*\r?/g, "").replace(/\x1b\[0?K/g, "");
  let fg: string | null = null;
  let bold = false;
  const out: string[] = [];
  for (const raw of text.split("\n")) {
    // A progress bar rewrites its line with \r: keep what shows last.
    const parts = raw.split("\r").filter((x) => x !== "");
    const line = parts.length ? parts[parts.length - 1] : "";
    let html = "";
    let open = false;
    const start = () => {
      if (fg || bold) {
        html += `<span class="${[fg && `ansi-${fg}`, bold && "font-bold"].filter(Boolean).join(" ")}">`;
        open = true;
      }
    };
    start();
    for (const tok of line.split(/(\x1b\[[\d;?]*[A-Za-z])/)) {
      const m = /^\x1b\[([\d;]*)m$/.exec(tok);
      if (m) {
        if (open) html += "</span>";
        open = false;
        for (const c of (m[1] || "0").split(";").map(Number)) {
          if (c === 0) {
            fg = null;
            bold = false;
          } else if (c === 1) bold = true;
          else if (c === 22) bold = false;
          else if (c === 39) fg = null;
          else if ((c >= 30 && c <= 37) || (c >= 90 && c <= 97)) fg = String(c >= 90 ? c - 60 : c);
        }
        start();
      } else if (!tok.startsWith("\x1b")) {
        html += esc(tok);
      }
    }
    if (open) html += "</span>";
    out.push(html);
  }
  return out.join("\n");
}
