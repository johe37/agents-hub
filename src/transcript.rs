//! What a Claude Code session is about, read from its transcript (JSONL).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

use serde_json::Value;

const TEXT_LIMIT: usize = 3000;
const EARLIER_TURNS: usize = 4;
const INPUT_KEYS: [&str; 8] = ["command", "file_path", "pattern", "query", "url", "skill", "description", "prompt"];

#[derive(Clone, Debug)]
pub struct Turn {
    pub prompt: String,
    pub reply: Option<String>,
}

#[derive(Clone, Default, Debug)]
pub struct Transcript {
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub last_prompt: Option<String>,
    pub last_text: Option<String>,
    /// The turns before the last prompt, oldest first.
    pub earlier: Vec<Turn>,
    pub last_tool: Option<String>,
    pub model: Option<String>,
    pub turns: u32,
    pub cost_usd: Option<f64>,
    /// Set when the conversation moved to a new session (resume, continue); its transcript has the rest.
    continued_in: Option<String>,
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
    let text = flatten(&text);
    (!text.is_empty() && !text.starts_with('<')).then_some(text)
}

pub fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
                if let Some(prompt) = t.last_prompt.replace(text) {
                    t.earlier.push(Turn { prompt, reply: t.last_text.clone() });
                    if t.earlier.len() > EARLIER_TURNS {
                        t.earlier.remove(0);
                    }
                }
                t.turns += 1;
                // A new turn: the previous reply no longer answers the prompt on show.
                t.last_text = None;
                t.last_tool = None;
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
        Some("continued-in") => t.continued_in = r["continuedInSessionId"].as_str().filter(|id| crate::hooks::valid_id(id)).map(String::from),
        _ => {}
    }
}

/// Parses each transcript once, then only the bytes appended since the last look.
#[derive(Default)]
pub struct TranscriptCache {
    items: Mutex<HashMap<String, Transcript>>,
}

/// The transcript of session id: next to path, or in another project dir when the cwd changed.
fn sibling(path: &str, id: &str) -> Option<String> {
    let name = format!("{id}.jsonl");
    let p = std::path::Path::new(path);
    let next = p.with_file_name(&name);
    if next.exists() {
        return Some(next.display().to_string());
    }
    let projects = p.parent()?.parent()?;
    std::fs::read_dir(projects).ok()?.flatten().map(|d| d.path().join(&name)).find(|p| p.exists()).map(|p| p.display().to_string())
}

impl TranscriptCache {
    /// The latest state of the conversation that started in path, following it into newer sessions.
    pub fn get(&self, path: &str) -> Option<Transcript> {
        let mut t = self.read(path)?;
        let mut path = path.to_string();
        // Bounded, in case transcripts ever point at each other.
        for _ in 0..8 {
            let Some(next) = t.continued_in.as_deref().and_then(|id| sibling(&path, id)) else { break };
            let Some(n) = self.read(&next) else { break };
            let title = t.title.take();
            (t, path) = (n, next);
            t.title = t.title.take().or(title);
        }
        Some(t)
    }

    fn read(&self, path: &str) -> Option<Transcript> {
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
        apply(&mut t, &json!({"type": "user", "message": {"content": "next"}}));
        assert_eq!(t.last_text, None);
        assert_eq!(t.earlier.len(), 1);
        assert_eq!(t.earlier[0].prompt, "fix the bug");
        assert_eq!(t.earlier[0].reply.as_deref(), Some("done"));
    }

    #[test]
    fn follows_continued_sessions() {
        let dir = std::env::temp_dir().join(format!("claude-hub-test-{}", std::process::id()));
        let project = dir.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let line = |v: Value| format!("{v}\n");
        let old = project.join("aaa.jsonl");
        std::fs::write(&old, line(json!({"type": "ai-title", "aiTitle": "Old title"})) + &line(json!({"type": "user", "message": {"content": "old"}}))).unwrap();
        std::fs::write(project.join("bbb.jsonl"), line(json!({"type": "user", "message": {"content": "new"}}))).unwrap();
        let cache = TranscriptCache::default();
        assert_eq!(cache.get(old.to_str().unwrap()).unwrap().last_prompt.as_deref(), Some("old"));
        let mut f = std::fs::OpenOptions::new().append(true).open(&old).unwrap();
        std::io::Write::write_all(&mut f, line(json!({"type": "continued-in", "continuedInSessionId": "bbb"})).as_bytes()).unwrap();
        let t = cache.get(old.to_str().unwrap()).unwrap();
        assert_eq!(t.last_prompt.as_deref(), Some("new"));
        assert_eq!(t.title.as_deref(), Some("Old title"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
