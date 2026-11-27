//! Claude sessions, from the latest hook event each one reported.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::config::home;
use crate::hooks::{events_dir, valid_id};
use crate::transcript::tool_summary;

const PRUNE_AFTER: u64 = 24 * 3600;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Status {
    Waiting,
    Working,
    /// Done with its turn, but a wake-up or background task will resume it.
    Polling,
    Idle,
    Unknown,
    Ended,
    Stale,
}

impl Status {
    pub fn alive(self) -> bool {
        !matches!(self, Status::Ended | Status::Stale)
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub session_id: String,
    pub cwd: String,
    pub status: Status,
    pub timestamp: u64,
    pub tmux_socket: Option<String>,
    pub tmux_pane: Option<String>,
    pub tmux_session: Option<String>,
    pub transcript_path: Option<String>,
    pub message: Option<String>,
    pub notification_type: Option<String>,
    pub tool: Option<String>,
    pub permission_mode: Option<String>,
    /// The tool call a permission prompt asks about: {name, input}.
    pub pending_tool: Option<Value>,
    path: PathBuf,
}

impl Session {
    /// Store the session as done with its turn, so the hub stops showing it as waiting for you.
    pub fn mark_done(&self) -> std::io::Result<()> {
        let mut record: Value = serde_json::from_slice(&std::fs::read(&self.path)?)?;
        record["event"]["hook_event_name"] = "Stop".into();
        let tmp = self.path.with_extension("done");
        std::fs::write(&tmp, record.to_string())?;
        crate::history::append(&self.session_id, now(), Status::Idle);
        // shortcut: a hook event landing between the read and the rename is lost; the agent's next event fixes the status.
        std::fs::rename(&tmp, &self.path)
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn opt(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(String::from)
}

/// The status a stored hook record stands for.
pub fn status_of(record: &Value) -> Option<Status> {
    let event = &record["event"];
    let name = event["hook_event_name"].as_str()?;
    let mut status = match name {
        "SessionStart" | "Stop" => Status::Idle,
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => Status::Working,
        "Notification" => Status::Waiting,
        "SessionEnd" => Status::Ended,
        _ => return None,
    };
    if event["notification_type"] == "agent_completed" {
        // Sent to every open session when some other session finishes; it says nothing about this one.
        status = Status::Idle;
    }
    // The hook flags a turn that ended with a wake-up or background task pending: the agent resumes by itself.
    // idle_prompt comes a minute after Stop, so it counts as the same state.
    let turn_ended = name == "Stop" || event["notification_type"] == "idle_prompt";
    if turn_ended && record["polling"] == true {
        status = Status::Polling;
    }
    Some(status)
}

fn read_event(path: PathBuf) -> Option<Session> {
    let record: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let event = &record["event"];
    let name = event["hook_event_name"].as_str()?;
    let status = status_of(&record)?;
    let waiting = status == Status::Waiting;
    Some(Session {
        session_id: opt(&event["session_id"])?,
        cwd: opt(&event["cwd"]).unwrap_or_default(),
        status,
        timestamp: record["time"].as_u64().unwrap_or(0),
        tmux_socket: opt(&record["tmux"]),
        tmux_pane: opt(&record["tmux_pane"]),
        tmux_session: None,
        transcript_path: opt(&event["transcript_path"]),
        message: opt(&event["message"]).filter(|_| waiting),
        notification_type: opt(&event["notification_type"]).filter(|_| waiting),
        tool: (name == "PreToolUse").then(|| tool_summary(event["tool_name"].as_str(), &event["tool_input"])).flatten(),
        permission_mode: opt(&event["permission_mode"]),
        pending_tool: Some(record["pending_tool"].clone()).filter(|t| waiting && t.is_object()),
        path,
    })
}

/// Read the latest event of every session and mark dead ones stale.
/// `live_panes(socket)` returns (pane_id, tmux_session) for the panes alive on that tmux server.
pub fn load_sessions(live_panes: impl Fn(&str) -> Vec<(String, String)>, stale_minutes: f64) -> Vec<Session> {
    let now = now();
    let Ok(rd) = std::fs::read_dir(events_dir()) else { return vec![] };
    let mut sessions: Vec<Session> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(read_event)
        .collect();

    let mut newest_in_pane: HashMap<(String, String), (u64, String)> = HashMap::new();
    for s in sessions.iter().filter(|s| s.status.alive()) {
        if let (Some(sock), Some(pane)) = (&s.tmux_socket, &s.tmux_pane) {
            let e = newest_in_pane.entry((sock.clone(), pane.clone())).or_insert((0, String::new()));
            if s.timestamp >= e.0 {
                *e = (s.timestamp, s.session_id.clone());
            }
        }
    }

    let mut live_cache: HashMap<String, HashMap<String, String>> = HashMap::new();
    for s in sessions.iter_mut().filter(|s| s.status.alive()) {
        match (&s.tmux_socket, &s.tmux_pane) {
            (Some(sock), Some(pane)) => {
                let live = live_cache.entry(sock.clone()).or_insert_with(|| live_panes(sock).into_iter().collect());
                let newest = &newest_in_pane[&(sock.clone(), pane.clone())].1;
                // A pane runs one Claude at a time, so older sessions in it are dead.
                match live.get(pane) {
                    Some(tmux_session) if *newest == s.session_id => s.tmux_session = Some(tmux_session.clone()),
                    _ => s.status = Status::Stale,
                }
            }
            _ if now.saturating_sub(s.timestamp) as f64 > stale_minutes * 60.0 => s.status = Status::Stale,
            _ => {}
        }
    }

    // Without a transcript a session never got a prompt, so there is nothing to resume, e.g. a pre-started Claude.
    let useless = |s: &Session| !s.transcript_path.as_deref().is_some_and(|p| std::path::Path::new(p).exists());
    for s in sessions.iter().filter(|s| !s.status.alive() && (now.saturating_sub(s.timestamp) > PRUNE_AFTER || useless(s))) {
        let _ = std::fs::remove_file(&s.path);
    }
    sessions.sort_by_key(|s| (s.status, std::cmp::Reverse(s.timestamp)));
    sessions
}

/// A running interactive Claude, from Claude Code's own registry.
pub struct Live {
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub status: Status,
    /// (tmux socket, pane id), from the process environment.
    pub tmux: Option<(String, String)>,
    /// Runs in Claude Code's background daemon (`claude --bg`, a backgrounded or forked session), not in a terminal.
    pub background: bool,
}

// shortcut: ~/.claude/sessions is Claude Code's internal registry, not a public API; if its format changes, outside agents without hooks vanish and adopting needs a fallback (e.g. scanning /proc for claude).
pub fn live_claudes() -> Vec<Live> {
    let Ok(rd) = std::fs::read_dir(home().join(".claude/sessions")) else { return vec![] };
    rd.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let v: Value = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            let background = v["kind"] == "bg";
            if v["kind"] != "interactive" && !background {
                return None;
            }
            let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
            // The registry outlives crashed processes, and pids get reused.
            if std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?.trim() != "claude" {
                return None;
            }
            let session_id = opt(&v["sessionId"]).filter(|id| valid_id(id))?;
            let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
            let var = |name: &str| {
                let prefix = format!("{name}=");
                environ.split(|&b| b == 0).find_map(|kv| kv.strip_prefix(prefix.as_bytes())).map(|v| String::from_utf8_lossy(v).into_owned())
            };
            let tmux = var("TMUX").and_then(|t| t.split(',').next().map(String::from)).zip(var("TMUX_PANE"));
            let status = match v["status"].as_str() {
                Some("busy") => Status::Working,
                Some("idle") => Status::Idle,
                _ => Status::Unknown,
            };
            Some(Live { pid, session_id, cwd: opt(&v["cwd"]).unwrap_or_default(), status, tmux, background })
        })
        .collect()
}

/// Where Claude Code keeps the transcripts of sessions started in cwd.
fn project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

pub fn transcript_for(cwd: &str, session_id: &str) -> Option<String> {
    let p = home().join(".claude/projects").join(project_dir(cwd)).join(format!("{session_id}.jsonl"));
    p.exists().then(|| p.display().to_string())
}

pub fn age(ts: u64) -> String {
    let secs = now().saturating_sub(ts);
    for (unit, size) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if secs >= size {
            return format!("{}{unit}", secs / size);
        }
    }
    format!("{secs}s")
}

#[cfg(test)]
mod polling_tests {
    use serde_json::json;

    use super::*;

    fn status_of(name: &str, polling: bool) -> Status {
        status_with_type(name, "", polling)
    }

    fn status_with_type(name: &str, notification_type: &str, polling: bool) -> Status {
        let dir = std::env::temp_dir().join(format!("claude-hub-state-{}-{name}-{notification_type}-{polling}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.json");
        let event = json!({"hook_event_name": name, "session_id": "abc", "notification_type": notification_type});
        let record = json!({"time": 1, "polling": polling, "event": event});
        std::fs::write(&path, record.to_string()).unwrap();
        let status = read_event(path).unwrap().status;
        std::fs::remove_dir_all(dir).unwrap();
        status
    }

    #[test]
    fn stop_while_polling_is_polling() {
        assert_eq!(status_of("Stop", true), Status::Polling);
        assert_eq!(status_of("Stop", false), Status::Idle);
    }

    #[test]
    fn the_flag_only_matters_after_stop() {
        assert_eq!(status_of("UserPromptSubmit", true), Status::Working);
        assert_eq!(status_of("PreToolUse", true), Status::Working);
        assert_eq!(status_of("Notification", true), Status::Waiting);
    }

    #[test]
    fn idle_prompt_while_polling_is_still_polling() {
        // Claude Code sends idle_prompt a minute after a turn ends, also when a wake-up or background task is pending.
        assert_eq!(status_with_type("Notification", "idle_prompt", true), Status::Polling);
        assert_eq!(status_with_type("Notification", "idle_prompt", false), Status::Waiting);
    }

    #[test]
    fn a_permission_prompt_while_polling_still_needs_you() {
        assert_eq!(status_with_type("Notification", "permission_prompt", true), Status::Waiting);
    }

    #[test]
    fn polling_sorts_between_working_and_idle() {
        assert!(Status::Working < Status::Polling && Status::Polling < Status::Idle);
        assert!(Status::Polling.alive());
    }
}

#[cfg(test)]
mod live_tests {
    #[test]
    fn project_dir_mangles_like_claude_code() {
        assert_eq!(super::project_dir("/var/home/me/repos/agents-hub"), "-var-home-me-repos-agents-hub");
        assert_eq!(super::project_dir("/a/b.c_d"), "-a-b-c-d");
    }
}
