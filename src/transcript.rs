//! What a Claude Code session is about, read from its transcript (JSONL).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

use serde_json::Value;

const TEXT_LIMIT: usize = 3000;
const INPUT_KEYS: [&str; 8] = ["command", "file_path", "pattern", "query", "url", "skill", "description", "prompt"];

#[derive(Clone, Default, Debug)]
pub struct Transcript {
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub last_prompt: Option<String>,
    pub last_text: Option<String>,
    pub last_tool: Option<String>,
    pub model: Option<String>,
    pub turns: u32,
    pub cost_usd: Option<f64>,
    offset: u64,
}

pub fn tool_summary(name: Option<&str>, input: &Value) -> Option<String> {
    let name = name.filter(|n| !n.is_empty())?;
    for key in INPUT_KEYS {
        if let Some(v) = input[key].as_str().map(str::trim).filter(|v| !v.is_empty()) {
            let first: String = v.lines().next().unwrap_or("").chars().take(160).collect();
            return Some(format!("{name}: {first}"));
        }
    }
    Some(name.to_string())
}

/// The text of a prompt typed by the user, or None for tool results and injected context.
fn prompt_text(record: &Value) -> Option<String> {
    if record["isMeta"] == true || record["isSidechain"] == true {
        return None;
    }
    let content = &record["message"]["content"];
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            if blocks.iter().any(|b| b["type"] != "text") {
                return None;
            }
            blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => return None,
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && !text.starts_with('<')).then_some(text)
}

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

fn apply(t: &mut Transcript, r: &Value) {
    match r["type"].as_str() {
        Some("ai-title") => {
            if let Some(title) = r["aiTitle"].as_str().filter(|s| !s.is_empty()) {
                t.title = Some(title.into());
            }
        }
        Some("user") => {
            if let Some(text) = prompt_text(r) {
                t.first_prompt.get_or_insert_with(|| text.clone());
                t.last_prompt = Some(text);
                t.turns += 1;
            }
        }
        Some("assistant") => {
            let m = &r["message"];
            if let Some(model) = m["model"].as_str() {
                t.model = Some(model.into());
            }
            for block in m["content"].as_array().into_iter().flatten() {
                match block["type"].as_str() {
                    Some("text") => {
                        let text = block["text"].as_str().unwrap_or("").trim();
                        if !text.is_empty() {
                            t.last_text = Some(tail(text, TEXT_LIMIT));
                        }
                    }
                    Some("tool_use") => t.last_tool = tool_summary(block["name"].as_str(), &block["input"]),
                    _ => {}
                }
            }
        }
        Some("cost-state") => t.cost_usd = r["totalCostUSD"].as_f64(),
        _ => {}
    }
}

/// Parses each transcript once, then only the bytes appended since the last look.
#[derive(Default)]
pub struct TranscriptCache {
    items: Mutex<HashMap<String, Transcript>>,
}

impl TranscriptCache {
    pub fn get(&self, path: &str) -> Option<Transcript> {
        let size = std::fs::metadata(path).ok()?.len();
        let mut t = self.items.lock().unwrap().get(path).cloned().unwrap_or_default();
        if t.offset > size {
            t = Transcript::default();
        }
        if t.offset < size {
            let mut data = vec![];
            if let Ok(mut f) = std::fs::File::open(path) {
                let _ = f.seek(SeekFrom::Start(t.offset)).and_then(|_| f.take(size - t.offset).read_to_end(&mut data));
            }
            // A line still being written has no newline yet; it is parsed on the next refresh.
            let complete = data.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            for line in data[..complete].split(|&b| b == b'\n') {
                if let Ok(record) = serde_json::from_slice::<Value>(line) {
                    apply(&mut t, &record);
                }
            }
            t.offset += complete as u64;
            self.items.lock().unwrap().insert(path.to_string(), t.clone());
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_prompts_titles_and_tools() {
        let mut t = Transcript::default();
        apply(&mut t, &json!({"type": "user", "message": {"content": "fix   the\nbug"}}));
        apply(&mut t, &json!({"type": "user", "message": {"content": [{"type": "tool_result"}]}}));
        apply(&mut t, &json!({"type": "user", "message": {"content": "<command>x</command>"}}));
        apply(&mut t, &json!({"type": "ai-title", "aiTitle": "Bug fix"}));
        apply(&mut t, &json!({"type": "assistant", "message": {"model": "m", "content": [
            {"type": "text", "text": " done "},
            {"type": "tool_use", "name": "Bash", "input": {"command": "ls\nrm"}}]}}));
        assert_eq!(t.first_prompt.as_deref(), Some("fix the bug"));
        assert_eq!(t.turns, 1);
        assert_eq!(t.title.as_deref(), Some("Bug fix"));
        assert_eq!(t.last_text.as_deref(), Some("done"));
        assert_eq!(t.last_tool.as_deref(), Some("Bash: ls"));
    }
}
