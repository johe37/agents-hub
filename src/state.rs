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
    /// The Claude process: recorded by the hook, or found in Claude Code's registry.
    pub pid: Option<u32>,
    path: PathBuf,
}

impl Session {
    /// Store the session as done with its turn, so the hub stops showing it as waiting for you.
    pub fn mark_done(&self) -> std::io::Result<()> {
        let mut record: Value = serde_json::from_slice(&std::fs::read(&self.path)?)?;
        record["event"]["hook_event_name"] = "Stop".into();
        let tmp = self.path.with_extension("done");
        std::fs::write(&tmp, record.to_string())?;
        // shortcut: a hook event landing between the read and the rename is lost; the agent's next event fixes the status.
        std::fs::rename(&tmp, &self.path)
    }
}

impl Session {
    /// A made-up session, for the demo town.
    pub fn demo(session_id: &str, cwd: &str, status: Status, timestamp: u64, tool: Option<&str>, notification_type: Option<&str>, message: Option<&str>) -> Session {
        Session {
            session_id: session_id.into(),
            cwd: cwd.into(),
            status,
            timestamp,
            tmux_socket: None,
            tmux_pane: None,
            tmux_session: None,
            transcript_path: None,
            message: message.map(String::from),
            notification_type: notification_type.map(String::from),
            tool: tool.map(String::from),
            permission_mode: Some("default".into()),
            pid: None,
            path: PathBuf::new(),
        }
    }
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
        pid: record["pid"].as_u64().and_then(|p| u32::try_from(p).ok()),
        path,
    })
}

/// Read the latest event of every session and mark dead ones stale.
/// `live_panes(socket)` returns (pane_id, tmux_session) for the panes alive on that tmux server.
/// `registry` maps session ids to their Claude process, from Claude Code's registry.
pub fn load_sessions(live_panes: impl Fn(&str) -> Vec<(String, String)>, stale_minutes: f64, registry: &HashMap<String, u32>) -> Vec<Session> {
    let now = now();
    let Ok(rd) = std::fs::read_dir(events_dir()) else { return vec![] };
    let mut sessions: Vec<Session> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(read_event)
        .collect();
    for s in &mut sessions {
        if let Some(&pid) = registry.get(&s.session_id) {
            s.pid = Some(pid);
        }
    }
    // A process runs one session at a time (/clear starts a new one), so older sessions of a pid are over.
    let mut newest_of_pid: HashMap<u32, (u64, String)> = HashMap::new();
    for s in sessions.iter().filter(|s| s.status.alive()) {
        if let Some(pid) = s.pid {
            let e = newest_of_pid.entry(pid).or_insert((0, String::new()));
            if s.timestamp >= e.0 {
                *e = (s.timestamp, s.session_id.clone());
            }
        }
    }

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
        // With a known process, it alone says whether the session lives: a pane can hold several
        // Claudes, for example in nvim terminals, which all report the pane nvim runs in.
        if let Some(pid) = s.pid {
            if !is_claude(pid) || newest_of_pid[&pid].1 != s.session_id {
                s.status = Status::Stale;
            } else if let (Some(sock), Some(pane)) = (&s.tmux_socket, &s.tmux_pane) {
                let live = live_cache.entry(sock.clone()).or_insert_with(|| live_panes(sock).into_iter().collect());
                s.tmux_session = live.get(pane).cloned();
            }
            continue;
        }
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

/// Whether pid is a running Claude process. The registry and hook records outlive crashed processes, and pids get reused.
pub fn is_claude(pid: u32) -> bool {
    comm(pid) == "claude"
}

pub fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|c| c.trim().to_string()).unwrap_or_default()
}

pub fn parent(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is in parentheses and may hold spaces; the parent pid is the second field after it.
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok().filter(|&p| p > 1)
}

/// The terminal a process reads from, e.g. /dev/pts/3.
pub fn tty_of(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/fd/0")).ok().map(|p| p.display().to_string()).filter(|p| p.starts_with("/dev/"))
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
            if !is_claude(pid) {
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
mod live_tests {
    #[test]
    fn project_dir_mangles_like_claude_code() {
        assert_eq!(super::project_dir("/var/home/me/repos/agents-hub"), "-var-home-me-repos-agents-hub");
        assert_eq!(super::project_dir("/a/b.c_d"), "-a-b-c-d");
    }
}
