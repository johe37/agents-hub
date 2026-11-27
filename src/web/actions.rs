//! What the page can ask the hub to do. Each action runs on a blocking thread.

use serde::Deserialize;

use super::Hub;
use super::model::{issue_key, mr_key, mr_workspace};
use crate::config::{Section, shorten};
use crate::gitlab::Mr;
use crate::snapshot::Cmd;
use crate::snapshot::Agent;
use crate::state::Status;
use crate::tmux::{self, Fail};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Approve { agent: String },
    Deny { agent: String },
    Reply { agent: String, text: String },
    /// Type text into the agent's input without sending it.
    Type { agent: String, text: String },
    Interrupt { agent: String },
    MarkDone { agent: String },
    Resume { agent: String },
    CloseAgent { agent: String },
    Adopt { agent: String },
    NewWindow { workspace: String, kind: String },
    /// Open file, relative to the workspace's repo root, in $EDITOR at line.
    OpenFile { workspace: String, file: String, line: Option<u32> },
    SetAgent { command: String },
    NewAgent { path: String, prompt: Option<String> },
    /// Without kind, only finds the open workspace for path; with it, opens one with a window of that kind.
    OpenWorkspace { path: String, kind: Option<String> },
    CloseWorkspace { workspace: String },
    CloseWindow { window: String },
    /// Put window right before another window of its workspace, or last without one.
    MoveWindow { window: String, before: Option<String> },
    RenameWorkspace { workspace: String, name: String },
    Rescan,
    /// Check out a merge request and open a workspace for it, or find the one that has it.
    /// With kind, the workspace opens (if it is not open) with a window of that kind; without, only the code is checked out.
    Checkout { mr: String, kind: Option<String> },
    /// The local clone of a GitLab project.
    /// Check out a worktree for the issue with this key, cloning the project into the cache when missing.
    IssueCheckout { issue: String },
    ThreadReply { mr: String, thread: String, text: String, resolve: bool },
    /// Push the workspace's branch, opening a draft merge request when it has none.
    Publish { workspace: String },
    /// Commit the staged changes, or all of them when nothing is staged.
    Commit { workspace: String, message: String },
    GitlabRefresh,
    /// Mark one To-Do done, or all of them without an id.
    TodoDone { id: Option<u64> },
    /// A comment on an MR, on a diff line with a GitLab position. A draft waits for FinishReview.
    MrNote { mr: String, body: String, position: Option<serde_json::Value>, draft: bool },
    DraftEdit { mr: String, id: u64, body: String },
    DraftDelete { mr: String, id: u64 },
    /// Publish your draft comments, with an optional summary, and approve when asked.
    /// head is the commit you reviewed: the next review can show what changed since.
    FinishReview { mr: String, summary: String, approve: bool, head: Option<String> },
    MrApprove { mr: String, approve: bool, head: Option<String> },
    RetryJob { mr: String, job: u64 },
    RetryPipeline { mr: String, pipeline: u64 },
    CancelPipeline { mr: String, pipeline: u64 },
    RunPipeline { mr: String },
    ReopenWorkspace { id: String },
    ForgetClosed { id: String },
}

#[derive(Default)]
pub struct Done {
    pub message: Option<String>,
    /// Where the result lives, so the page can go there.
    pub workspace: Option<String>,
    pub window: Option<String>,
    pub path: Option<String>,
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

/// The open workspace for path, if any.
fn open_for(hub: &Hub, path: &str) -> Option<String> {
    let real = tmux::canonical(path);
    hub.snapshot().workspaces.iter().find(|w| tmux::canonical(&w.path) == real).map(|w| w.name.clone())
}

/// The name a new workspace for path gets: a review checkout says so.
fn workspace_name(path: &str) -> String {
    let p = std::path::Path::new(path);
    let base = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if p.starts_with(crate::gitlab::reviews_dir()) { format!("review-{base}") } else { base }
}

/// A new workspace for path whose first window is kind, and that window's id.
fn new_workspace(path: &str, kind: &str, command: Option<&str>) -> Result<(String, Option<String>), String> {
    let session = tmux::ensure_workspace_with(path, &workspace_name(path), kind, command)?;
    let w = tmux::call(&["list-windows", "-t", &format!("={session}:"), "-F", "#{window_id}"])?.lines().next().map(String::from);
    Ok((session, w))
}

/// The window kind; `claude` is its old name, which a page loaded before an upgrade still sends.
fn check_kind(kind: &str) -> Result<String, String> {
    match kind {
        "agent" | "claude" => Ok("agent".into()),
        "shell" | "nvim" => Ok(kind.into()),
        _ => Err(format!("Unknown window kind {kind}")),
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

/// The command that opens path at line in $EDITOR, nvim without one, and the editor's name.
fn editor_at(path: &str, line: Option<u32>) -> (String, String) {
    let editor = std::env::var("EDITOR").ok().filter(|e| !e.trim().is_empty()).unwrap_or_else(|| "nvim".into());
    let name = editor.split_whitespace().next().and_then(|p| p.rsplit('/').next()).unwrap_or("editor").to_string();
    let command = match (line, name.as_str()) {
        (None, _) => format!("{editor} {}", tmux::shell_quote(path)),
        (Some(n), "hx" | "helix") => format!("{editor} {}", tmux::shell_quote(&format!("{path}:{n}"))),
        (Some(n), "code" | "codium" | "cursor") => format!("{editor} -g {}", tmux::shell_quote(&format!("{path}:{n}"))),
        (Some(n), _) => format!("{editor} +{n} {}", tmux::shell_quote(path)),
    };
    (command, name)
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Type prompt into the agent in window and send it, once the agent is ready to read it.
fn send_when_ready(window: &str, prompt: &str) {
    let Ok(pane) = tmux::call(&["display-message", "-p", "-t", window, "#{pane_id}"]) else { return };
    let text = one_line(prompt);
    std::thread::spawn(move || {
        if tmux::wait_for_agent(&pane) {
            let _ = tmux::send_text(&tmux::socket_path(), &pane, &text);
        }
    });
}

/// The MR with key, preferring a workspace's copy: it refreshes more often.
pub(super) fn find_mr(hub: &Hub, key: &str) -> Result<Mr, String> {
    let snap = hub.snapshot();
    let is = |m: &&Mr| mr_key(m) == key;
    snap.workspaces
        .iter()
        .filter_map(|w| w.mr.as_ref())
        .find(is)
        .or_else(|| snap.reviews.iter().chain(&snap.authored).chain(&snap.assigned).find(is))
        .cloned()
        .ok_or_else(|| "This merge request is gone".to_string())
}

/// key is what the page waits on: it hears the clone's progress.
fn clone_of(hub: &Hub, project: &str, url: &str, key: &str) -> Result<String, String> {
    let dirs = hub.snapshot().dirs.clone();
    crate::gitlab::ensure_clone_with(&dirs, project, url, &|pct| hub.progress(key, "clone", Some(pct)))
}

/// Fetch the MR again soon, also in the workspace that has it.
fn forget_mr(hub: &Hub, key: &str) {
    let path = hub.snapshot().workspaces.iter().find(|w| w.mr.as_ref().is_some_and(|wm| mr_key(wm) == key)).map(|w| w.path.clone());
    forget_gitlab(hub, path.as_deref());
}

fn forget_gitlab(hub: &Hub, path: Option<&str>) {
    let cmd = hub.cmd.lock().unwrap();
    for s in [Section::GitlabReviews, Section::GitlabMergeRequests, Section::GitlabAssignedMergeRequests, Section::GitlabIssues, Section::GitlabTodos] {
        let _ = cmd.send(Cmd::ForgetInbox(s));
    }
    if let Some(p) = path {
        let _ = cmd.send(Cmd::ForgetMr(p.to_string()));
    }
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
        Action::Type { agent: key, text } => {
            let a = agent(hub, &key)?;
            let (sock, pane) = target(&a)?;
            // A newline would send the prompt in Claude Code.
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                return Err("Nothing to type".into());
            }
            tmux::type_text(&sock, &pane, &format!("{text} "))?;
            Ok(Done::default())
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
            let command = format!("claude --resume {id}");
            let (session, w) = match open_for(hub, &a.path) {
                Some(session) => {
                    let w = tmux::open_window(&session, &a.path, "claude", Some(&command))?;
                    (session, Some(w))
                }
                None => new_workspace(&a.path, "claude", Some(&command))?,
            };
            Ok(Done { message: Some(format!("Resumed in {session}")), workspace: Some(session), window: w, path: None })
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
            let w = a.window_id.as_ref().ok_or("This agent runs outside jugglr. Close it in its own terminal, or adopt it.")?;
            res(tmux::kill_window(w))?;
            Ok(said(format!("Closed {}", a.title())))
        }
        Action::Adopt { agent: key } => {
            let a = agent(hub, &key)?;
            if a.workspace.is_some() || !a.status.alive() {
                return Err("Only running agents outside jugglr can be adopted".into());
            }
            let (Some(pid), Some(id)) = (a.pid, a.session_id()) else {
                return Err("Can't find this agent's process. Adopting needs a recent Claude Code.".into());
            };
            res(tmux::adopt(pid, id, &a.path))?;
            Ok(said(format!("Adopted {} into {}", a.title(), shorten(&a.path))))
        }
        Action::NewWindow { workspace, kind } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            let kind = check_kind(&kind)?;
            let w = window(&ws.name, &ws.path, &crate::app::window_name(&kind), crate::app::window_command(&kind).as_deref())?;
            Ok(Done { message: None, workspace: Some(ws.name), window: Some(w), path: None })
        }
        Action::OpenFile { workspace, file, line } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            let root = super::model::toplevel(&ws.path).unwrap_or_else(|| ws.path.clone());
            let full = std::path::Path::new(&root).join(&file);
            if file.split('/').any(|p| p == "..") || !full.is_file() {
                return Err(format!("Can't open {file}: it is not in the checkout. A later commit removed or moved it."));
            }
            let (command, name) = editor_at(&full.to_string_lossy(), line);
            let w = tmux::open_window(&ws.name, &ws.path, &name, Some(&command))?;
            Ok(Done { message: None, workspace: Some(ws.name), window: Some(w), path: None })
        }
        Action::NewAgent { path, prompt } => {
            let (command, name, label) = (crate::agent::command(), crate::agent::name(), crate::agent::label());
            crate::agent::require(Some(&command))?;
            let path = crate::config::expand(path.trim());
            std::fs::create_dir_all(&path).map_err(|e| format!("Could not create {}: {e}", shorten(&path)))?;
            let path = tmux::canonical(&path);
            let prompt = prompt.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
            // Claude takes the prompt as an argument; other agents get it typed in once they are ready.
            let claude = crate::agent::is_claude();
            let start = match &prompt {
                Some(p) if claude => format!("{command} {}", tmux::shell_quote(p)),
                _ => command.clone(),
            };
            let message = |session: &str| if prompt.is_some() { format!("Started an agent in {session}") } else { format!("Opened {label} in {session}") };
            let Some(session) = open_for(hub, &path) else {
                let (session, w) = new_workspace(&path, &name, Some(&start))?;
                if let (Some(p), Some(w), false) = (&prompt, &w, claude) {
                    send_when_ready(w, p);
                }
                return Ok(Done { message: Some(message(&session)), workspace: Some(session), window: w, path: Some(path) });
            };
            let w = match &prompt {
                // A pre-started Claude is ready to read input, so the prompt can be typed in at once.
                Some(p) => match tmux::spare_for(&path, Some(&command)) {
                    Some(spare) => {
                        tmux::call(&["move-window", "-d", "-s", &spare, "-t", &format!("={session}:")])?;
                        tmux::call(&["rename-window", "-t", &spare, &name])?;
                        let pane = tmux::call(&["display-message", "-p", "-t", &spare, "#{pane_id}"])?;
                        tmux::send_text(&tmux::socket_path(), &pane, &one_line(p))?;
                        spare
                    }
                    None => {
                        let w = tmux::open_window(&session, &path, &name, Some(&start))?;
                        if !claude {
                            send_when_ready(&w, p);
                        }
                        w
                    }
                },
                None => window(&session, &path, &name, Some(&command))?,
            };
            Ok(Done { message: Some(message(&session)), workspace: Some(session), window: Some(w), path: None })
        }
        Action::SetAgent { command } => {
            crate::agent::set(&command)?;
            Ok(said(format!("New agents start with {}", command.trim())))
        }
        Action::OpenWorkspace { path, kind } => {
            let path = crate::config::expand(path.trim());
            if !std::path::Path::new(&path).is_dir() {
                return Err(format!("{} is not a directory", shorten(&path)));
            }
            let path = tmux::canonical(&path);
            let open = open_for(hub, &path);
            let Some(kind) = kind else {
                return Ok(Done { workspace: open, path: Some(path), ..Done::default() });
            };
            let kind = check_kind(&kind)?;
            let (name, command) = (crate::app::window_name(&kind), crate::app::window_command(&kind));
            if let Some(session) = open {
                let w = window(&session, &path, &name, command.as_deref())?;
                return Ok(Done { workspace: Some(session), window: Some(w), path: Some(path), ..Done::default() });
            }
            let (session, w) = new_workspace(&path, &name, command.as_deref())?;
            Ok(Done { message: Some(format!("Opened {session}")), workspace: Some(session), window: w, path: Some(path) })
        }
        Action::CloseWorkspace { workspace } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            let p = std::path::Path::new(&ws.path);
            let closed = crate::closed::snapshot(&ws);
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
            // Only a workspace that did close is worth reopening.
            if let Err(e) = crate::closed::record(closed) {
                return Ok(said(format!("Closed {}, but could not remember it: {e}", ws.name)));
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
        Action::MoveWindow { window, before } => {
            let session = hub
                .snapshot()
                .workspaces
                .iter()
                .find(|w| w.windows.iter().any(|win| win.window_id == window))
                .map(|w| w.name.clone())
                .ok_or("This window is gone")?;
            tmux::move_window(&session, &window, before.as_deref())?;
            Ok(Done::default())
        }
        Action::RenameWorkspace { workspace, name } => {
            let taken: Vec<String> = tmux::list_sessions().into_iter().filter(|s| *s != workspace).collect();
            let name = tmux::unique_session_name(name.trim(), &taken);
            res(tmux::rename_session(&workspace, &name))?;
            Ok(Done { message: Some(format!("Renamed to {name}")), workspace: Some(name), window: None, path: None })
        }
        Action::Rescan => {
            let _ = hub.cmd.lock().unwrap().send(Cmd::Rescan);
            Ok(said("Scanning for repos"))
        }
        Action::Checkout { mr: key, kind } => {
            let kind = kind.as_deref().map(check_kind).transpose()?;
            let m = find_mr(hub, &key)?;
            let (open, review) = {
                let snap = hub.snapshot();
                (mr_workspace(&m, &snap).map(|w| (w.name.clone(), w.path.clone())), snap.reviews.iter().any(|r| mr_key(r) == key))
            };
            if let Some((name, path)) = open {
                let w = match &kind {
                    Some(k) => Some(window(&name, &path, &crate::app::window_name(k), crate::app::window_command(k).as_deref())?),
                    None => None,
                };
                return Ok(Done { workspace: Some(name), window: w, path: Some(path), ..Done::default() });
            }
            let busy = format!("checkout:{key}");
            let repo = clone_of(hub, &m.project, &m.url, &busy)?;
            hub.progress(&busy, "checkout", None);
            let dir = if review { crate::gitlab::checkout(&repo, &m)? } else { crate::gitlab::checkout_branch(&repo, &m, &m.branch_dir())? };
            let Some(k) = kind else {
                return Ok(Done { message: Some(format!("Checked out !{}", m.iid)), path: Some(dir), ..Done::default() });
            };
            hub.progress(&busy, "start", None);
            let (session, w) = new_workspace(&dir, &crate::app::window_name(&k), crate::app::window_command(&k).as_deref())?;
            Ok(Done { message: Some(format!("Checked out !{} in {session}", m.iid)), workspace: Some(session), window: w, path: Some(dir) })
        }
        Action::IssueCheckout { issue: key } => {
            let issue = hub.snapshot().issues.iter().find(|i| issue_key(i) == key).cloned().ok_or("This issue is gone")?;
            let busy = format!("issue:{key}");
            let repo = clone_of(hub, &issue.project, &issue.url, &busy)?;
            hub.progress(&busy, "checkout", None);
            let dir = crate::gitlab::issue_worktree(&repo, &issue, &issue.dir())?;
            Ok(Done { path: Some(dir), ..Done::default() })
        }
        Action::ThreadReply { mr: key, thread, text, resolve } => {
            let m = find_mr(hub, &key)?;
            let th = m.threads.iter().find(|t| t.id == thread).ok_or("This thread is resolved or gone")?;
            let text = text.trim();
            if text.is_empty() && !resolve {
                return Err("Nothing to send".into());
            }
            let path = hub.snapshot().workspaces.iter().find(|w| w.mr.as_ref().is_some_and(|wm| mr_key(wm) == key)).map(|w| w.path.clone());
            let cwd = path.clone().unwrap_or_else(|| crate::config::home().display().to_string());
            if !text.is_empty() {
                crate::gitlab::reply(&cwd, &m, &thread, text)?;
            }
            if resolve {
                crate::gitlab::resolve(&cwd, &m, &thread)?;
            }
            forget_gitlab(hub, path.as_deref());
            let done = if resolve { format!("Resolved the thread at {} on !{}", th.place(), m.iid) } else { format!("Replied on !{} at {}", m.iid, th.place()) };
            Ok(said(done))
        }
        Action::Publish { workspace } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            let branch = ws.git.as_ref().and_then(|g| g.branch.clone()).ok_or("This workspace is not on a git branch")?;
            let open_mr = ws.mr.is_none() && !matches!(branch.as_str(), "main" | "master");
            let r = crate::gitlab::publish(&ws.path, open_mr);
            forget_gitlab(hub, Some(&ws.path));
            Ok(said(r?))
        }
        Action::Commit { workspace, message } => {
            let ws = hub.snapshot().workspaces.iter().find(|w| w.name == workspace).cloned().ok_or("This workspace is gone")?;
            if message.trim().is_empty() {
                return Err("Write a commit message".into());
            }
            Ok(said(crate::gitlab::commit(&ws.path, message.trim())?))
        }
        Action::ReopenWorkspace { id } => {
            let c = crate::closed::get(&id).ok_or("It is no longer in the list")?;
            let dir = if std::path::Path::new(&c.path).is_dir() {
                c.path.clone()
            } else {
                // A review or branch checkout is deleted on close: check the merge request out again.
                let m = c.mr.as_ref().ok_or_else(|| format!("{} is gone", shorten(&c.path)))?;
                match run(hub, Action::Checkout { mr: format!("{}!{}", m.project_id, m.iid), kind: None }) {
                    Ok(Done { path: Some(p), .. }) => p,
                    Ok(_) => return Err("The checkout gave no directory".into()),
                    Err(e) => return Err(format!("{} is gone, and checking out !{} again failed: {e}", shorten(&c.path), m.iid)),
                }
            };
            let session = crate::closed::reopen(&c, &dir)?;
            Ok(Done { message: Some(format!("Reopened {session}")), workspace: Some(session), ..Done::default() })
        }
        Action::ForgetClosed { id } => {
            crate::closed::forget(&id)?;
            Ok(Done::default())
        }
        Action::MrNote { mr: key, body, position, draft } => {
            let m = find_mr(hub, &key)?;
            let body = body.trim();
            if body.is_empty() {
                return Err("The comment is empty".into());
            }
            if position.as_ref().is_some_and(|p| !p.is_object()) {
                return Err("Bad position".into());
            }
            crate::gitlab::add_note(&m, body, position.as_ref(), draft)?;
            if draft {
                return Ok(said("Added to your review"));
            }
            forget_gitlab(hub, None);
            Ok(said(format!("Commented on !{}", m.iid)))
        }
        Action::DraftEdit { mr: key, id, body } => {
            let m = find_mr(hub, &key)?;
            if body.trim().is_empty() {
                return Err("The comment is empty".into());
            }
            crate::gitlab::edit_draft(&m, id, body.trim())?;
            Ok(Done::default())
        }
        Action::DraftDelete { mr: key, id } => {
            crate::gitlab::delete_draft(&find_mr(hub, &key)?, id)?;
            Ok(said("Deleted the draft comment"))
        }
        Action::FinishReview { mr: key, summary, approve, head } => {
            let m = find_mr(hub, &key)?;
            if !summary.trim().is_empty() {
                crate::gitlab::add_note(&m, summary.trim(), None, true)?;
            }
            if !crate::gitlab::drafts(&m)?.is_empty() {
                crate::gitlab::publish_drafts(&m)?;
            }
            if approve && !m.approved {
                crate::gitlab::approve(&m, true)?;
            }
            if let Some(h) = head.as_deref().filter(|h| super::review::is_sha(h)) {
                super::review::mark_reviewed(&key, h);
            }
            forget_gitlab(hub, None);
            Ok(said(if approve { format!("Published your review and approved !{}", m.iid) } else { format!("Published your review on !{}", m.iid) }))
        }
        Action::MrApprove { mr: key, approve, head } => {
            let m = find_mr(hub, &key)?;
            crate::gitlab::approve(&m, approve)?;
            if let Some(h) = head.as_deref().filter(|h| approve && super::review::is_sha(h)) {
                super::review::mark_reviewed(&key, h);
            }
            forget_gitlab(hub, None);
            Ok(said(if approve { format!("Approved !{}", m.iid) } else { format!("Revoked your approval of !{}", m.iid) }))
        }
        Action::RetryJob { mr: key, job } => {
            let m = find_mr(hub, &key)?;
            let j = super::review::job_of(&m, &key, job).ok_or("The job is not in this merge request's pipelines")?;
            crate::gitlab::retry_job(&j.url, j.id)?;
            forget_mr(hub, &key);
            Ok(said(format!("Retrying {}", j.name)))
        }
        Action::RetryPipeline { mr: key, pipeline } => {
            let p = super::review::pipeline_of(&find_mr(hub, &key)?, &key, pipeline)?;
            crate::gitlab::retry_pipeline(&p)?;
            forget_mr(hub, &key);
            Ok(said(format!("Retrying the failed jobs of pipeline #{}", p.id)))
        }
        Action::CancelPipeline { mr: key, pipeline } => {
            let p = super::review::pipeline_of(&find_mr(hub, &key)?, &key, pipeline)?;
            crate::gitlab::cancel_pipeline(&p)?;
            forget_mr(hub, &key);
            Ok(said(format!("Canceled pipeline #{}", p.id)))
        }
        Action::RunPipeline { mr: key } => {
            let m = find_mr(hub, &key)?;
            crate::gitlab::run_pipeline(&m)?;
            forget_mr(hub, &key);
            Ok(said(format!("Started a pipeline for !{}", m.iid)))
        }
        Action::GitlabRefresh => {
            let paths: Vec<String> = hub.snapshot().workspaces.iter().filter(|w| w.mr.is_some()).map(|w| w.path.clone()).collect();
            forget_gitlab(hub, None);
            for p in paths {
                let _ = hub.cmd.lock().unwrap().send(Cmd::ForgetMr(p));
            }
            Ok(said("Refreshing GitLab"))
        }
        Action::TodoDone { id } => {
            crate::gitlab::todo_done(id)?;
            let _ = hub.cmd.lock().unwrap().send(Cmd::ForgetInbox(Section::GitlabTodos));
            Ok(said(if id.is_some() { "Marked done" } else { "Marked all done" }))
        }
    }
}
