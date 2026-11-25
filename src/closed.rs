//! Recently closed workspaces, so one can be opened again with its windows and agents.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::snapshot::Workspace;
use crate::tmux;

const KEEP: usize = 10;
const MAX_AGE_SECS: u64 = 7 * 24 * 3600;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClosedWindow {
    /// agent, editor or shell.
    pub kind: String,
    pub name: String,
    /// The editor that ran, so it opens again.
    #[serde(default)]
    pub command: Option<String>,
    /// The Claude session to resume.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// The merge request a review or branch checkout was for: the checkout is deleted on close.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClosedMr {
    pub project_id: u64,
    pub iid: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClosedWorkspace {
    pub id: String,
    pub name: String,
    pub path: String,
    /// Seconds since the epoch.
    pub closed_at: u64,
    pub windows: Vec<ClosedWindow>,
    #[serde(default)]
    pub mr: Option<ClosedMr>,
}

fn file() -> PathBuf {
    crate::config::state_dir().join("closed.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn read(path: &Path) -> Vec<ClosedWorkspace> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write(path: &Path, list: &[ClosedWorkspace]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Newest first, without the ones too old to keep.
fn fresh(mut list: Vec<ClosedWorkspace>, now: u64) -> Vec<ClosedWorkspace> {
    list.retain(|c| now.saturating_sub(c.closed_at) < MAX_AGE_SECS);
    list.sort_by(|a, b| b.closed_at.cmp(&a.closed_at));
    list.truncate(KEEP);
    list
}

pub fn list() -> Vec<ClosedWorkspace> {
    fresh(read(&file()), now())
}

/// What to keep of ws: its windows in order, minus processes that would not start the same again.
pub fn snapshot(ws: &Workspace) -> ClosedWorkspace {
    let windows = ws
        .windows
        .iter()
        .filter_map(|w| {
            let kind = w.kind();
            (kind != "process").then(|| ClosedWindow {
                kind: kind.to_string(),
                name: w.name.clone(),
                command: (kind == "editor").then(|| w.command.clone()),
                session_id: w.claude.as_ref().map(|s| s.session_id.clone()).filter(|id| crate::hooks::valid_id(id)),
            })
        })
        .collect();
    let mr = ws.mr.as_ref().map(|m| ClosedMr { project_id: m.project_id, iid: m.iid });
    let closed_at = now();
    ClosedWorkspace { id: format!("{}-{closed_at}", ws.name), name: ws.name.clone(), path: ws.path.clone(), closed_at, windows, mr }
}

/// Remember ws before it closes. A newer close of the same directory replaces the older one.
pub fn record(c: ClosedWorkspace) -> Result<(), String> {
    let mut all = read(&file());
    all.retain(|x| x.path != c.path);
    all.push(c);
    write(&file(), &fresh(all, now()))
}

pub fn get(id: &str) -> Option<ClosedWorkspace> {
    list().into_iter().find(|c| c.id == id)
}

pub fn forget(id: &str) -> Result<(), String> {
    let mut all = read(&file());
    all.retain(|c| c.id != id);
    write(&file(), &all)
}

fn command(w: &ClosedWindow) -> Option<String> {
    match w.kind.as_str() {
        "agent" => Some(match &w.session_id {
            Some(id) => format!("claude --resume {id}"),
            None => "claude".into(),
        }),
        "editor" => w.command.clone().or_else(|| Some("nvim".into())),
        _ => None,
    }
}

/// Open c again in dir, its windows in order, Claude sessions resumed; nothing is switched to.
/// Returns the workspace's name. It is no longer in the list after.
pub fn reopen(c: &ClosedWorkspace, dir: &str) -> Result<String, String> {
    let before = tmux::list_sessions();
    let session = tmux::ensure_workspace_named(dir, &c.name)?;
    // A new workspace starts with one shell; it stands in for the first shell, if there is one.
    let mut spare_shell = if before.contains(&session) {
        None
    } else {
        tmux::call(&["list-windows", "-t", &format!("={session}:"), "-F", "#{window_id}"])?.lines().next().map(String::from)
    };
    for w in &c.windows {
        if w.kind == "shell" {
            if let Some(first) = spare_shell.take() {
                let _ = tmux::call(&["rename-window", "-t", &first, &w.name]);
                continue;
            }
        }
        tmux::open_window(&session, dir, &w.name, command(w).as_deref())?;
    }
    if let Some(first) = spare_shell.filter(|_| !c.windows.is_empty()) {
        let _ = tmux::call(&["kill-window", "-t", &first]);
    }
    forget(&c.id)?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(name: &str, path: &str, at: u64) -> ClosedWorkspace {
        ClosedWorkspace { id: format!("{name}-{at}"), name: name.into(), path: path.into(), closed_at: at, windows: vec![], mr: None }
    }

    #[test]
    fn keeps_the_newest_ten_of_the_last_week() {
        let now = 10 * MAX_AGE_SECS;
        let mut all: Vec<_> = (0..12).map(|i| closed(&format!("w{i}"), &format!("/p{i}"), now - i * 60)).collect();
        all.push(closed("old", "/old", now - MAX_AGE_SECS - 1));
        let kept = fresh(all, now);
        assert_eq!(kept.len(), KEEP);
        assert_eq!(kept[0].name, "w0");
        assert!(kept.iter().all(|c| c.name != "old" && c.name != "w11"));
    }

    #[test]
    fn reads_and_writes_the_list() {
        let dir = std::env::temp_dir().join(format!("jugglr-closed-{}", std::process::id()));
        let path = dir.join("closed.json");
        let mut c = closed("api", "/src/api", 5);
        c.windows.push(ClosedWindow { kind: "agent".into(), name: "claude".into(), command: None, session_id: Some("abc".into()) });
        write(&path, std::slice::from_ref(&c)).unwrap();
        assert_eq!(read(&path), vec![c.clone()]);
        assert_eq!(command(&c.windows[0]).as_deref(), Some("claude --resume abc"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
