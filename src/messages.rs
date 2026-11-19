//! Messages between agents: `claude-hub send`, and `tmux send-keys` calls spotted by the hook.
//! Each one is a small JSON file in the state directory, which the town reads to play the walk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, state_dir};
use crate::snapshot::{Agent, Snapshot, basename};
use crate::state::{Status, is_claude, now, parent};

const KEEP: u64 = 24 * 3600;
const PREVIEW: usize = 140;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub id: String,
    pub time: u64,
    /// Agent keys ("agent:<session id>"). No sender means you sent it.
    pub from: Option<String>,
    pub to: Option<String>,
    pub preview: String,
    /// "send" for `claude-hub send`, "tmux" for a send-keys call spotted by the hook.
    pub via: String,
    /// For "tmux": the -t target and -S socket of the send-keys call, resolved to an agent by the reader.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub socket: Option<String>,
}

pub fn dir() -> PathBuf {
    state_dir().join("messages")
}

fn clip(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= PREVIEW { s } else { s.chars().take(PREVIEW - 1).collect::<String>() + "…" }
}

/// Store a message event. Never fails loudly: it runs inside the hook too.
pub fn write(mut m: Message) {
    m.preview = clip(&m.preview);
    let dir = dir();
    let _ = std::fs::create_dir_all(&dir);
    let name = format!("{}-{}", m.time, m.id);
    let tmp = dir.join(format!(".{name}"));
    if std::fs::write(&tmp, serde_json::to_vec(&m).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join(format!("{name}.json")));
    }
}

fn new_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("{:x}{:x}", std::process::id(), nanos)
}

/// Messages from the last max_age seconds, oldest first. Removes ones older than a day.
pub fn recent(max_age: u64) -> Vec<Message> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return vec![] };
    let now = now();
    let mut out = vec![];
    for e in rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")) {
        let Some(m) = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<Message>(&b).ok()) else { continue };
        let age = now.saturating_sub(m.time);
        if age > KEEP {
            let _ = std::fs::remove_file(e.path());
        } else if age <= max_age {
            out.push(m);
        }
    }
    out.sort_by(|a, b| (a.time, &a.id).cmp(&(b.time, &b.id)));
    out
}

/// Called by the hook for a Bash tool call: a `tmux send-keys` to another pane is one agent talking to
/// another. Only a string check, so the hook stays fast; the town works out who the target is.
pub fn spot_send_keys(session_id: &str, event: &Value) {
    if event["hook_event_name"] != "PreToolUse" || event["tool_name"] != "Bash" {
        return;
    }
    let Some(cmd) = event["tool_input"]["command"].as_str() else { return };
    // claude-hub send records its own message.
    if !cmd.contains("send-keys") || cmd.contains("claude-hub send") {
        return;
    }
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let after = |flag: &str| words.iter().position(|w| *w == flag).and_then(|i| words.get(i + 1)).map(|w| w.trim_matches(['\'', '"']).to_string());
    let Some(target) = after("-t") else { return };
    // The text is whatever follows the target, minus key names.
    let start = words.iter().position(|w| w.trim_matches(['\'', '"']) == target).map_or(words.len(), |i| i + 1);
    let text: Vec<&str> = words[start..].iter().copied().filter(|w| !matches!(*w, "Enter" | "C-m" | "-l" | "--" | "&&" | ";")).collect();
    write(Message {
        id: new_id(),
        time: now(),
        from: Some(format!("agent:{session_id}")),
        to: None,
        preview: text.join(" ").trim_matches(['\'', '"']).to_string(),
        via: "tmux".into(),
        target: Some(target),
        socket: after("-S"),
    });
}

/// The agent a tmux send-keys target points at: a pane id, or a session (and window) name.
pub fn resolve_target<'a>(snap: &'a Snapshot, from: Option<&Agent>, target: &str, socket: Option<&str>) -> Option<&'a Agent> {
    let socket = socket.map(crate::tmux::canonical).or_else(|| from.and_then(|a| a.tmux.as_ref()).map(|t| crate::tmux::canonical(&t.0)));
    let same_server = |a: &Agent| match (&socket, &a.tmux) {
        (Some(s), Some(t)) => crate::tmux::canonical(&t.0) == *s,
        _ => true,
    };
    let alive = || snap.agents.iter().filter(|a| a.status.alive() && Some(a.key.as_str()) != from.map(|f| f.key.as_str()));
    if target.starts_with('%') {
        return alive().find(|a| same_server(a) && a.tmux.as_ref().is_some_and(|t| t.1 == target));
    }
    let session = target.split([':', '.']).next().unwrap_or(target);
    alive().find(|a| same_server(a) && (a.workspace.as_deref() == Some(session) || a.session.as_ref().and_then(|s| s.tmux_session.as_deref()) == Some(session)))
}

/// The agent running this process, if a Claude is among its parents.
fn calling_agent(snap: &Snapshot) -> Option<&Agent> {
    let mut pid = std::process::id();
    for _ in 0..8 {
        pid = parent(pid)?;
        if is_claude(pid) {
            return snap.agents.iter().find(|a| a.pid == Some(pid) && a.status.alive());
        }
    }
    None
}

/// Agents that `name` may mean: a session id (or its start), a workspace, a repo or directory name, or a title.
fn matches(a: &Agent, name: &str, git: &crate::git::GitCache) -> bool {
    let lower = name.to_lowercase();
    let id = a.session_id().unwrap_or("");
    (name.len() >= 4 && id.starts_with(name))
        || a.key == name
        || a.workspace.as_deref() == Some(name)
        || basename(&a.path) == name
        || git.repo_root(&a.path).is_some_and(|r| basename(&r) == name)
        || a.title().to_lowercase() == lower
}

pub fn send(cfg: Config, args: &[String]) -> std::process::ExitCode {
    use std::process::ExitCode;
    let (Some(name), true) = (args.first(), args.len() >= 2) else {
        eprintln!("usage: claude-hub send <agent-or-workspace> <message>\n\nThe target is a session id (or its first characters), a workspace, repo or directory name, or an agent's title.");
        return ExitCode::FAILURE;
    };
    let text = args[1..].join(" ");
    let snap = crate::snapshot::collect_once(cfg);
    let git = crate::git::GitCache::default();
    let me = calling_agent(&snap);
    let mut found: Vec<&Agent> =
        snap.agents.iter().filter(|a| a.status.alive() && Some(a.key.as_str()) != me.map(|m| m.key.as_str()) && matches(a, name, &git)).collect();
    if found.is_empty() {
        eprintln!("claude-hub: no running agent matches '{name}'. `claude-hub debug-sessions` lists them.");
        return ExitCode::FAILURE;
    }
    // Several agents in one workspace: prefer one that can take input, then one that is free.
    found.sort_by_key(|a| (a.target.is_none(), a.needs_permission(), a.status != Status::Idle, a.status != Status::Waiting));
    let shops: std::collections::HashSet<String> = found.iter().map(|a| git.repo_root(&a.path).unwrap_or_else(|| a.path.clone())).collect();
    if shops.len() > 1 {
        eprintln!("claude-hub: '{name}' matches agents in several places; use a session id:");
        for a in &found {
            eprintln!("  {}  {}  {}", a.session_id().map(|s| &s[..8.min(s.len())]).unwrap_or("-"), a.title(), crate::config::shorten(&a.path));
        }
        return ExitCode::FAILURE;
    }
    let to = found[0];
    let body = match me {
        Some(m) => format!(
            "[Message from the agent '{}' in {} via claude-hub. To answer, run: claude-hub send {} \"<answer>\"] {text}",
            m.title(),
            basename(&m.path),
            m.session_id().map(|s| &s[..8.min(s.len())]).unwrap_or("")
        ),
        None => text.clone(),
    };
    if let Err(e) = to.send_message(&body) {
        eprintln!("claude-hub: cannot send to {}: {e}", to.title());
        return ExitCode::FAILURE;
    }
    write(Message { id: new_id(), time: now(), from: me.map(|m| m.key.clone()), to: Some(to.key.clone()), preview: text, via: "send".into(), target: None, socket: None });
    println!("Sent to {} ({}).", to.title(), crate::config::shorten(&to.path));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_previews() {
        assert_eq!(clip("  hello\n  world "), "hello world");
        assert_eq!(clip(&"x".repeat(500)).chars().count(), PREVIEW);
    }
}
