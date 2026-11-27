//! The recent conversation of a session, read from the end of its transcript.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::sync::{LazyLock, Mutex};

use serde_json::{Value, json};

use crate::transcript::tool_summary;

/// Enough for the last few turns of a busy session without parsing a huge file.
const TAIL: u64 = 2 * 1024 * 1024;
const MAX_ITEMS: usize = 250;
const MAX_TEXT: usize = 20_000;
const MAX_RESULT: usize = 4_000;
const MAX_PATCH: usize = 60_000;

fn clip(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut cut = n;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

/// Long strings in a tool input would bloat the page; the full edit is in the diff view.
fn clip_values(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(clip(s, MAX_TEXT)),
        Value::Array(a) => Value::Array(a.iter().take(50).map(clip_values).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), clip_values(v))).collect()),
        v => v.clone(),
    }
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The edit as Claude Code applied it, as unified diff hunks, so the page can show real line numbers.
fn patch(r: &Value) -> Option<String> {
    let hunks = r["toolUseResult"]["structuredPatch"].as_array().filter(|h| !h.is_empty())?;
    let mut out = String::new();
    for h in hunks {
        let n = |k: &str| h[k].as_u64().unwrap_or(0);
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", n("oldStart"), n("oldLines"), n("newStart"), n("newLines")));
        for l in h["lines"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.push_str(l);
            out.push('\n');
        }
    }
    Some(clip(&out, MAX_PATCH))
}

/// A typed prompt keeps its line breaks here; injected context and command output are skipped.
fn prompt(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) if blocks.iter().all(|b| b["type"] == "text") => {
            blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Slash commands arrive wrapped in tags; show them as typed.
    if let Some(rest) = text.strip_prefix("<command-name>") {
        let name = rest.split("</command-name>").next().unwrap_or("").trim();
        let args = text.split("<command-args>").nth(1).and_then(|a| a.split("</command-args>").next()).unwrap_or("").trim();
        return (!name.is_empty()).then(|| if args.is_empty() { name.to_string() } else { format!("{name} {args}") });
    }
    (!text.starts_with('<')).then(|| clip(text, MAX_TEXT))
}

/// What a transcript held when it was last read, so the next request parses only what was written since.
#[derive(Default)]
struct Tail {
    ino: u64,
    /// The end of the last whole line read.
    offset: u64,
    /// The first line from offset is cut in half.
    skip: bool,
    truncated: bool,
    items: Vec<Value>,
    tools: HashMap<String, usize>,
    /// Prompts typed while Claude works wait here until Claude reads them.
    queue: Vec<String>,
}

const CACHED: usize = 16;
static CACHE: LazyLock<Mutex<HashMap<String, Tail>>> = LazyLock::new(Mutex::default);

pub fn read(path: &str) -> Value {
    let cached = CACHE.lock().unwrap().remove(path);
    let Some(tail) = Tail::update(path, cached.unwrap_or_default()) else { return json!({"items": [], "missing": true}) };
    let out = tail.json();
    let mut cache = CACHE.lock().unwrap();
    if cache.len() >= CACHED {
        if let Some(k) = cache.keys().next().cloned() {
            cache.remove(&k);
        }
    }
    cache.insert(path.to_string(), tail);
    out
}

impl Tail {
    fn update(path: &str, mut t: Tail) -> Option<Tail> {
        let mut f = std::fs::File::open(path).ok()?;
        let meta = f.metadata().ok()?;
        let size = meta.len();
        if meta.ino() != t.ino || size < t.offset {
            let offset = size.saturating_sub(TAIL);
            t = Tail { ino: meta.ino(), offset, skip: offset > 0, truncated: offset > 0, ..Tail::default() };
        }
        if size == t.offset {
            return Some(t);
        }
        let mut data = vec![];
        if f.seek(SeekFrom::Start(t.offset)).and_then(|_| f.read_to_end(&mut data)).is_err() {
            return Some(t);
        }
        let mut end = 0;
        if t.skip {
            let Some(nl) = data.iter().position(|b| *b == b'\n') else { return Some(t) };
            end = nl + 1;
            t.skip = false;
        }
        for line in data[end..].split_inclusive(|b| *b == b'\n') {
            match serde_json::from_str::<Value>(&String::from_utf8_lossy(line)) {
                Ok(r) => t.record(&r),
                // Claude has not written all of the last line yet.
                Err(_) if !line.ends_with(b"\n") => break,
                Err(_) => {}
            }
            end += line.len();
        }
        t.offset += end as u64;
        if t.items.len() > 2 * MAX_ITEMS {
            let cut = t.items.len() - MAX_ITEMS;
            t.items.drain(..cut);
            t.tools.retain(|_, i| {
                let keep = *i >= cut;
                *i = i.saturating_sub(cut);
                keep
            });
            t.truncated = true;
        }
        Some(t)
    }

    fn json(&self) -> Value {
        let cut = self.items.len().saturating_sub(MAX_ITEMS);
        let queued: Vec<String> = self.queue.iter().filter_map(|q| prompt(&json!(q))).collect();
        json!({"items": &self.items[cut..], "queued": queued, "truncated": self.truncated || cut > 0})
    }

    fn record(&mut self, r: &Value) {
        if r["isSidechain"] == true {
            return;
        }
        let (items, tools, queue) = (&mut self.items, &mut self.tools, &mut self.queue);
        let ts = r["timestamp"].clone();
        match r["type"].as_str() {
            Some("user") => {
                let content = &r["message"]["content"];
                if let Some(blocks) = content.as_array().filter(|b| b.iter().any(|b| b["type"] == "tool_result")) {
                    let results: Vec<_> = blocks.iter().filter(|b| b["type"] == "tool_result").collect();
                    // toolUseResult belongs to the record, so it is only safe to use with one result in it.
                    let patch = if results.len() == 1 { patch(&r) } else { None };
                    for b in results {
                        let Some(&i) = b["tool_use_id"].as_str().and_then(|id| tools.get(id)) else { continue };
                        items[i]["result"] = json!({
                            "text": clip(&result_text(&b["content"]), MAX_RESULT),
                            "error": b["is_error"] == true,
                        });
                        if let Some(p) = &patch {
                            items[i]["result"]["patch"] = json!(p);
                        }
                    }
                } else if r["isMeta"] != true {
                    if let Some(text) = prompt(content) {
                        items.push(json!({"kind": "user", "text": text, "ts": ts}));
                    }
                }
            }
            Some("assistant") => {
                for b in r["message"]["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => {
                            let text = b["text"].as_str().unwrap_or("").trim();
                            if !text.is_empty() {
                                items.push(json!({"kind": "assistant", "text": clip(text, MAX_TEXT), "ts": ts}));
                            }
                        }
                        Some("tool_use") => {
                            let id = b["id"].as_str().unwrap_or("").to_string();
                            tools.insert(id.clone(), items.len());
                            items.push(json!({
                                "kind": "tool",
                                "id": id,
                                "name": b["name"],
                                "summary": tool_summary(b["name"].as_str(), &b["input"]),
                                "input": clip_values(&b["input"]),
                                "ts": ts,
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Some("queue-operation") => {
                let content = r["content"].as_str().unwrap_or("");
                match r["operation"].as_str() {
                    Some("enqueue") => queue.push(content.to_string()),
                    // The next turn starts with it as a normal prompt.
                    Some("dequeue") if !queue.is_empty() => {
                        queue.remove(0);
                    }
                    // Claude read it during the turn (an attachment follows), or it was deleted.
                    Some("remove") => {
                        if let Some(i) = queue.iter().position(|q| q == content) {
                            queue.remove(i);
                        }
                    }
                    Some("popAll") => queue.clear(),
                    _ => {}
                }
            }
            Some("attachment") if r["attachment"]["type"] == "queued_command" && r["attachment"]["commandMode"] != "bash" => {
                if let Some(text) = prompt(&r["attachment"]["prompt"]) {
                    items.push(json!({"kind": "user", "text": text, "ts": ts}));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_prompts_replies_and_tool_results() {
        let dir = std::env::temp_dir().join(format!("jugglr-conv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let lines = [
            json!({"type": "user", "message": {"content": "Fix it\nplease"}, "timestamp": "t1"}),
            json!({"type": "user", "isMeta": true, "message": {"content": "injected"}}),
            json!({"type": "user", "message": {"content": "<command-name>/review</command-name><command-args>42</command-args>"}}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "On it."}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "a\nb", "is_error": false}]}}),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t2", "name": "Edit", "input": {"file_path": "/a", "old_string": "x", "new_string": "y"}}]}}),
            json!({
                "type": "user",
                "message": {"content": [{"type": "tool_result", "tool_use_id": "t2", "content": "ok"}]},
                "toolUseResult": {"structuredPatch": [{"oldStart": 3, "oldLines": 2, "newStart": 3, "newLines": 2, "lines": [" a", "-x", "+y"]}]},
            }),
        ];
        std::fs::write(&path, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let out = read(path.to_str().unwrap());
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 5);
        assert_eq!(items[0]["text"], "Fix it\nplease");
        assert_eq!(items[1]["text"], "/review 42");
        assert_eq!(items[2]["kind"], "assistant");
        assert_eq!(items[3]["summary"], "Bash: ls");
        assert_eq!(items[3]["result"]["text"], "a\nb");
        assert!(items[3]["result"]["patch"].is_null());
        assert_eq!(items[4]["result"]["patch"], "@@ -3,2 +3,2 @@\n a\n-x\n+y\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shows_queued_prompts_until_claude_reads_them() {
        let dir = std::env::temp_dir().join(format!("jugglr-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let op = |o: &str, c: Option<&str>| json!({"type": "queue-operation", "operation": o, "content": c});
        let lines = [
            json!({"type": "user", "message": {"content": "Start"}}),
            op("enqueue", Some("Read mid-turn")),
            op("enqueue", Some("Next turn")),
            op("enqueue", Some("Still waiting")),
            op("remove", Some("Read mid-turn")),
            json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": "Read mid-turn", "commandMode": "prompt"}}),
            op("dequeue", None),
            json!({"type": "user", "message": {"content": "Next turn"}}),
        ];
        std::fs::write(&path, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let out = read(path.to_str().unwrap());
        let texts: Vec<_> = out["items"].as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["Start", "Read mid-turn", "Next turn"]);
        assert_eq!(out["queued"], json!(["Still waiting"]));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_only_what_was_written_since() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("jugglr-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let p = path.to_str().unwrap();
        let line = |t: &str| json!({"type": "user", "message": {"content": t}}).to_string();
        std::fs::write(&path, format!("{}\n", line("one"))).unwrap();
        let texts = |v: Value| v["items"].as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(texts(read(p)), ["one"]);
        let two = line("two");
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(two[..10].as_bytes()).unwrap();
        assert_eq!(texts(read(p)), ["one"], "a half-written line waits");
        f.write_all(format!("{}\n", &two[10..]).as_bytes()).unwrap();
        assert_eq!(texts(read(p)), ["one", "two"]);
        std::fs::write(&path, format!("{}\n", line("new"))).unwrap();
        assert_eq!(texts(read(p)), ["new"], "a rewritten file is read again");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
