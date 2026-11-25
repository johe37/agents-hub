//! What the page can ask the hub to do. Each action runs on a blocking thread.

use serde::Deserialize;

use super::Hub;
use crate::config::shorten;
use crate::snapshot::Agent;
use crate::state::Status;
use crate::tmux::{self, Fail};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Approve { agent: String },
    Deny { agent: String },
    Reply { agent: String, text: String },
    Interrupt { agent: String },
    MarkDone { agent: String },
    Resume { agent: String },
    CloseAgent { agent: String },
    Adopt { agent: String },
    NewWindow { workspace: String, kind: String },
    NewAgent { path: String, prompt: Option<String> },
    OpenWorkspace { path: String },
    CloseWorkspace { workspace: String },
    CloseWindow { window: String },
    RenameWorkspace { workspace: String, name: String },
    Rescan,
}

#[derive(Default)]
pub struct Done {
    pub message: Option<String>,
    /// Where the result lives, so the page can go there.
    pub workspace: Option<String>,
    pub window: Option<String>,
}

fn said(message: impl Into<String>) -> Done {
    Done { message: Some(message.into()), ..Done::default() }
}

/// Outside the hub's tmux server, a switch is never done; the page shows the window instead.
fn res(r: tmux::Res) -> Result<(), String> {
    match r {
        Ok(()) | Err(Fail::Attach(_)) => Ok(()),
        Err(Fail::Error(e)) => Err(e),
    }
}

fn agent(hub: &Hub, key: &str) -> Result<Agent, String> {
    hub.snapshot().agents.iter().find(|a| a.key == key).cloned().ok_or_else(|| "This agent is gone".to_string())
}

fn target(a: &Agent) -> Result<(String, String), String> {
    a.target.clone().ok_or_else(|| "The hub does not know where this agent reads its input".to_string())
}

fn short_id(a: &Agent) -> Result<String, String> {
    let id = a.session_id().filter(|id| crate::hooks::valid_id(id)).ok_or("This agent has no session id")?;
    Ok(id[..8.min(id.len())].to_string())
}

/// The workspace for path: one that is open, or a new one.
fn workspace_for(hub: &Hub, path: &str) -> Result<String, String> {
    let real = tmux::canonical(path);
    let open = hub.snapshot().workspaces.iter().find(|w| tmux::canonical(&w.path) == real).map(|w| w.name.clone());
    match open {
        Some(name) => Ok(name),
        None => tmux::ensure_workspace(&real),
    }
}

/// A window in session: a pre-started one from the pool when there is one, which shows its prompt at once.
fn window(session: &str, path: &str, name: &str, command: Option<&str>) -> Result<String, String> {
    if let Some(spare) = tmux::spare_for(path, command) {
        let moved = tmux::call(&["move-window", "-d", "-s", &spare, "-t", &format!("={session}:")])
            .and_then(|_| tmux::call(&["rename-window", "-t", &spare, name]));
        if moved.is_ok() {
            return Ok(spare);
        }
    }
    tmux::open_window(session, path, name, command)
}

pub fn run(hub: &Hub, action: Action) -> Result<Done, String> {
    match action {
        Action::Approve { agent: key } | Action::Deny { agent: key } if !agent(hub, &key)?.needs_permission() => {
            Err("This agent no longer asks for permission".into())
        }
        Action::Approve { agent: key } => {
            let a = agent(hub, &key)?;
            let (sock, pane) = target(&a)?;
            // Enter takes the highlighted "Yes" in Claude Code's permission dialog.
            tmux::send_key(&sock, &pane, "Enter")?;
            Ok(said(format!("Approved → {}", a.title())))
        }
        Action::Deny { agent: key } => {
            let a = agent(hub, &key)?;
            let (sock, pane) = target(&a)?;
            tmux::send_key(&sock, &pane, "Escape")?;
            Ok(said(format!("Denied → {}", a.title())))
        }
        Action::Reply { agent: key, text } => {
            let a = agent(hub, &key)?;
            let (sock, pane) = target(&a)?;
            let text = text.trim();
            if text.is_empty() {
                return Err("Nothing to send".into());
            }
            // A newline would send the prompt early in Claude Code; the page sends one paragraph.
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            // Text typed into the permission dialog would be lost: decline the tool first, then Claude reads the prompt.
            if a.needs_permission() {
                tmux::send_key(&sock, &pane, "Escape")?;
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            tmux::send_text(&sock, &pane, &text)?;
            Ok(said(format!("Sent → {}", a.title())))
        }
        Action::Interrupt { agent: key } => {
            let a = agent(hub, &key)?;
            let (sock, pane) = target(&a)?;
            tmux::send_key(&sock, &pane, "Escape")?;
            Ok(said(format!("Interrupted {}", a.title())))
        }
        Action::MarkDone { agent: key } => {
            let a = agent(hub, &key)?;
            match a.session.as_ref().filter(|s| s.status == Status::Waiting) {
                Some(s) => s.mark_done().map_err(|e| e.to_string())?,
                None => return Err("This agent is not waiting for you".into()),
            }
            Ok(said(format!("Marked done: {}", a.title())))
        }
        Action::Resume { agent: key } => {
            let a = agent(hub, &key)?;
            if a.status.alive() {
                return Err("This agent still runs".into());
            }
            let id = a.session_id().filter(|id| crate::hooks::valid_id(id)).ok_or("This agent has no session id")?;
            let session = workspace_for(hub, &a.path)?;
            let w = tmux::open_window(&session, &a.path, "claude", Some(&format!("claude --resume {id}")))?;
            Ok(Done { message: Some(format!("Resumed in {session}")), workspace: Some(session), window: Some(w) })
        }
        Action::CloseAgent { agent: key } => {
            let a = agent(hub, &key)?;
            if a.background && a.status.alive() {
                let id = short_id(&a)?;
                let out = std::process::Command::new("claude").args(["stop", &id]).output().map_err(|e| format!("claude stop: {e}"))?;
                if !out.status.success() {
                    return Err(format!("claude stop: {}", String::from_utf8_lossy(&out.stderr).trim()));
                }
                return Ok(said(format!("Stopped {}", a.title())));
            }
            let w = a.window_id.as_ref().ok_or("This agent runs outside Claude Hub. Close it in its own terminal, or adopt it.")?;
            res(tmux::kill_window(w))?;
            Ok(said(format!("Closed {}", a.title())))
        }
        Action::Adopt { agent: key } => {
            let a = agent(hub, &key)?;
            if a.workspace.is_some() || !a.status.alive() {
                return Err("Only running agents outside Claude Hub can be adopted".into());
            }
            let (Some(pid), Some(id)) = (a.pid, a.session_id()) else {
                return Err("Can't find this agent's process. Adopting needs a recent Claude Code.".into());
            };
            res(tmux::adopt(pid, id, &a.path))?;
            Ok(said(format!("Adopted {} into {}", a.title(), shorten(&a.path))))
        }
        Action::NewWindow { workspace, kind } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            if !matches!(kind.as_str(), "claude" | "shell" | "nvim") {
                return Err(format!("Unknown window kind {kind}"));
            }
            let w = window(&ws.name, &ws.path, &kind, crate::app::window_command(&kind))?;
            Ok(Done { message: None, workspace: Some(ws.name), window: Some(w) })
        }
        Action::NewAgent { path, prompt } => {
            let path = crate::config::expand(path.trim());
            std::fs::create_dir_all(&path).map_err(|e| format!("Could not create {}: {e}", shorten(&path)))?;
            let path = tmux::canonical(&path);
            let session = workspace_for(hub, &path)?;
            let prompt = prompt.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
            let w = match &prompt {
                // A pre-started Claude is ready to read input, so the prompt can be typed in at once.
                Some(p) => match tmux::spare_for(&path, Some("claude")) {
                    Some(spare) => {
                        tmux::call(&["move-window", "-d", "-s", &spare, "-t", &format!("={session}:")])?;
                        tmux::call(&["rename-window", "-t", &spare, "claude"])?;
                        let pane = tmux::call(&["display-message", "-p", "-t", &spare, "#{pane_id}"])?;
                        tmux::send_text(&tmux::socket_path(), &pane, &p.split_whitespace().collect::<Vec<_>>().join(" "))?;
                        spare
                    }
                    None => tmux::open_window(&session, &path, "claude", Some(&format!("claude {}", tmux::shell_quote(p))))?,
                },
                None => window(&session, &path, "claude", Some("claude"))?,
            };
            let message = if prompt.is_some() { format!("Started an agent in {session}") } else { format!("Opened Claude in {session}") };
            Ok(Done { message: Some(message), workspace: Some(session), window: Some(w) })
        }
        Action::OpenWorkspace { path } => {
            let path = crate::config::expand(path.trim());
            if !std::path::Path::new(&path).is_dir() {
                return Err(format!("{} is not a directory", shorten(&path)));
            }
            let session = workspace_for(hub, &path)?;
            Ok(Done { message: None, workspace: Some(session), window: None })
        }
        Action::CloseWorkspace { workspace } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            let p = std::path::Path::new(&ws.path);
            if p.starts_with(crate::gitlab::reviews_dir()) {
                res(tmux::kill_session(&ws.name))?;
                crate::gitlab::remove_checkout(&ws.path)?;
            } else if p.starts_with(crate::gitlab::branches_dir()) {
                // Removing first keeps the workspace open when git refuses because of uncommitted changes.
                crate::gitlab::remove_branch_checkout(&ws.path)?;
                res(tmux::kill_session(&ws.name))?;
            } else {
                res(tmux::kill_session(&ws.name))?;
            }
            Ok(said(format!("Closed {}", ws.name)))
        }
        Action::CloseWindow { window } => {
            let known = hub.snapshot().workspaces.iter().any(|w| w.windows.iter().any(|win| win.window_id == window));
            if !known {
                return Err("This window is gone".into());
            }
            res(tmux::kill_window(&window))?;
            Ok(Done::default())
        }
        Action::RenameWorkspace { workspace, name } => {
            let taken: Vec<String> = tmux::list_sessions().into_iter().filter(|s| *s != workspace).collect();
            let name = tmux::unique_session_name(name.trim(), &taken);
            res(tmux::rename_session(&workspace, &name))?;
            Ok(Done { message: Some(format!("Renamed to {name}")), workspace: Some(name), window: None })
        }
        Action::Rescan => {
            let _ = hub.cmd.lock().unwrap().send(crate::snapshot::Cmd::Rescan);
            Ok(said("Scanning for repos"))
        }
    }
}
