//! The recent conversation of a session, read from the end of its transcript.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

use serde_json::{Value, json};

use crate::transcript::tool_summary;

/// Enough for the last few turns of a busy session without parsing a huge file.
const TAIL: u64 = 2 * 1024 * 1024;
const MAX_ITEMS: usize = 250;
const MAX_TEXT: usize = 20_000;
const MAX_RESULT: usize = 4_000;

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

pub fn read(path: &str) -> Value {
    let Ok(mut f) = std::fs::File::open(path) else { return json!({"items": [], "missing": true}) };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = size.saturating_sub(TAIL);
    let mut data = vec![];
    if f.seek(SeekFrom::Start(start)).and_then(|_| f.read_to_end(&mut data)).is_err() {
        return json!({"items": []});
    }
    let text = String::from_utf8_lossy(&data);
    let mut lines = text.lines();
    if start > 0 {
        // The first line is cut in half.
        lines.next();
    }

    let mut items: Vec<Value> = vec![];
    let mut tools: HashMap<String, usize> = HashMap::new();
    for line in lines {
        let Ok(r) = serde_json::from_str::<Value>(line) else { continue };
        if r["isSidechain"] == true {
            continue;
        }
        let ts = r["timestamp"].clone();
        match r["type"].as_str() {
            Some("user") => {
                let content = &r["message"]["content"];
                if let Some(blocks) = content.as_array().filter(|b| b.iter().any(|b| b["type"] == "tool_result")) {
                    for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                        let Some(&i) = b["tool_use_id"].as_str().and_then(|id| tools.get(id)) else { continue };
                        items[i]["result"] = json!({
                            "text": clip(&result_text(&b["content"]), MAX_RESULT),
                            "error": b["is_error"] == true,
                        });
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
            _ => {}
        }
    }
    let cut = items.len().saturating_sub(MAX_ITEMS);
    json!({"items": items.split_off(cut), "truncated": start > 0 || cut > 0})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_prompts_replies_and_tool_results() {
        let dir = std::env::temp_dir().join(format!("claude-hub-conv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let lines = [
            json!({"type": "user", "message": {"content": "Fix it\nplease"}, "timestamp": "t1"}),
            json!({"type": "user", "isMeta": true, "message": {"content": "injected"}}),
            json!({"type": "user", "message": {"content": "<command-name>/review</command-name><command-args>42</command-args>"}}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "On it."}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "a\nb", "is_error": false}]}}),
        ];
        std::fs::write(&path, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let out = read(path.to_str().unwrap());
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["text"], "Fix it\nplease");
        assert_eq!(items[1]["text"], "/review 42");
        assert_eq!(items[2]["kind"], "assistant");
        assert_eq!(items[3]["summary"], "Bash: ls");
        assert_eq!(items[3]["result"]["text"], "a\nb");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
