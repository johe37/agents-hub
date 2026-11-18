//! Claude sessions, from the latest hook event each one reported.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::hooks::events_dir;
use crate::transcript::tool_summary;

const PRUNE_AFTER: u64 = 24 * 3600;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Status {
    Waiting,
    Working,
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
    path: PathBuf,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn opt(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(String::from)
}

fn read_event(path: PathBuf) -> Option<Session> {
    let record: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
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

    for s in sessions.iter().filter(|s| !s.status.alive() && now.saturating_sub(s.timestamp) > PRUNE_AFTER) {
        let _ = std::fs::remove_file(&s.path);
    }
    sessions.sort_by_key(|s| (s.status, std::cmp::Reverse(s.timestamp)));
    sessions
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
