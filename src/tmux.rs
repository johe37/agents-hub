//! The hidden tmux engine: a private tmux server where every session is a workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{shorten, state_dir};
use crate::theme::*;

pub const SOCKET_NAME: &str = "claude-hub";
pub const HUB_SESSION: &str = "_hub";
pub const POOL_SESSION: &str = "_pool";

pub fn summary_file() -> PathBuf {
    state_dir().join("summary")
}

pub fn request_file() -> PathBuf {
    state_dir().join("request")
}

const FIELDS: [&str; 14] = [
    "session_name",
    "@workspace_path",
    "session_path",
    "window_index",
    "window_name",
    "window_id",
    "pane_id",
    "pane_active",
    "pane_current_path",
    "pane_current_command",
    "pane_pipe",
    "pane_width",
    "pane_height",
    "@spare",
];

#[derive(Clone, Debug)]
pub struct Pane {
    pub session: String,
    pub workspace_path: String,
    pub session_path: String,
    pub window_index: u32,
    pub window_name: String,
    pub window_id: String,
    pub pane_id: String,
    pub active: bool,
    pub command: String,
    pub piped: bool,
    pub width: u16,
    pub height: u16,
    /// Pool windows of a pre-started Claude: "<unix start time> <branch> <commit>".
    pub spare: String,
}

/// Why a tmux action did not complete in place.
pub enum Fail {
    Error(String),
    /// Outside the hub server: the caller should exit and attach to this session.
    Attach(String),
}

pub type Res = Result<(), Fail>;

pub fn socket_path() -> String {
    let base = std::env::var("TMUX_TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let uid = uid();
    let p = format!("{base}/tmux-{uid}/{SOCKET_NAME}");
    std::fs::canonicalize(&p).map(|p| p.display().to_string()).unwrap_or(p)
}

fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    // The owner of /proc/self is the real uid; avoids a libc dependency.
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0)
}

pub fn inside_hub_server() -> bool {
    let current = std::env::var("TMUX").unwrap_or_default();
    let current = current.split(',').next().unwrap_or("");
    !current.is_empty() && std::fs::canonicalize(current).map(|p| p.display().to_string()).ok() == Some(socket_path())
}

fn run(args: &[&str]) -> Option<Output> {
    Command::new("tmux").arg("-L").arg(SOCKET_NAME).args(args).output().ok()
}

fn check(out: Option<Output>) -> Result<(), String> {
    match out {
        None => Err("tmux not found".into()),
        Some(o) if o.status.success() => Ok(()),
        Some(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            Err(if err.is_empty() { "tmux failed".into() } else { err })
        }
    }
}

fn simple(args: &[&str]) -> Res {
    check(run(args)).map_err(Fail::Error)
}

/// Human name for a tmux key: M-h -> Alt+h, C-space -> Ctrl+Space.
pub fn key_label(key: &str) -> String {
    key.replace("M-", "Alt+").replace("C-", "Ctrl+").replace("S-", "Shift+").replace("space", "Space")
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn write_config(exe: &str, hub_key: &str) -> String {
    let hub_cmd = quote(&format!("{exe} ui"));
    let summary = quote(&summary_file().display().to_string());
    let label = key_label(hub_key);
    let key = quote(hub_key);
    let c = colors();
    let bar = css(c.bar);
    let fg = css(c.fg);
    let accent = css(c.accent);
    let on_accent = css(c.on_accent);
    let bright = css(c.bright);
    let dim = css(c.dim);
    let info = css(c.info);
    let muted = css(c.muted);
    let bg = css(c.bg);
    let border = css(c.border);
    let selection = css(c.selection);
    let text = format!(
        r##"# Your own tmux config first (prefix, keys, theme); the hub settings below win where they overlap.
source-file -q ~/.tmux.conf
source-file -q ~/.config/tmux/tmux.conf
set -g default-terminal "tmux-256color"
set -as terminal-features ",*:RGB"
# Pass clickable links (OSC 8) on to your terminal.
set -as terminal-features ",*:hyperlinks"
set -g escape-time 10
set -g mouse on
set -g history-limit 50000
set -g base-index 1
set -g renumber-windows on
set -g focus-events on
set -g status-interval 2
set -g status-position top
set -g status-justify left
set -g status-style "bg={bar},fg={fg}"
set -g status-left-length 120
set -g status-left "#[range=user|hub,bg={accent},fg={on_accent},bold]  ◀  Claude Hub  #[nobold]{label} #[norange,bg={bar}]  #[fg={accent}]◆ #[fg={bright},bold]#S#[nobold,fg={dim}]#{{?@workspace_label,  #{{@workspace_label}},}}   "
set -g status-right-length 80
set -g status-right "#{{?client_prefix,#[bg={info} fg={on_accent} bold] PREFIX #[bg={bar} nobold] ,}}#(cat {summary} 2>/dev/null)#[fg={dim}] %H:%M "
set -g window-status-separator ""
set -g window-status-format "#[fg={muted}]  #I #{{?#{{==:#W,claude}},◆ ,}}#W  "
set -g window-status-current-format "#[bg={bg},fg={accent}]▎#[fg={bright},bold]#I #{{?#{{==:#W,claude}},◆ ,}}#W  "
set -g message-style "bg={accent},fg={on_accent},bold"
set -g pane-border-status off
set -g pane-border-style "fg={border}"
set -g pane-active-border-style "fg={accent}"
set -g mode-style "bg={selection},fg={bright}"
bind -n MouseDown1Status if -F "#{{==:#{{mouse_status_range}},hub}}" "new-session -A -s {HUB_SESSION} {hub_cmd}" "select-window -t ="
bind -n {key} new-session -A -s {HUB_SESSION} {hub_cmd}
set -g @hub_key {key}
# The session and window lists hide the pool of spare shells.
bind s choose-tree -Zs -f "#{{!=:#{{session_name}},{POOL_SESSION}}}"
bind w choose-tree -Zw -f "#{{!=:#{{session_name}},{POOL_SESSION}}}"
"##
    );
    let path = state_dir().join("tmux.conf");
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(&path, text);
    path.display().to_string()
}

/// Changes whenever the claude-hub binary is rebuilt.
fn code_version(exe: &str) -> String {
    std::fs::metadata(exe)
        .and_then(|m| m.modified())
        .map(|t| format!("{:?}", t))
        .unwrap_or_default()
}

/// Start the hub session, or restart its UI when the binary changed since it started.
pub fn ensure_hub(exe: &str, config: &str) {
    let version = code_version(exe);
    let hub_cmd = format!("{exe} ui");
    if !list_sessions().iter().any(|s| s == HUB_SESSION) {
        let _ = Command::new("tmux")
            .args(["-L", SOCKET_NAME, "-f", config, "new-session", "-d", "-s", HUB_SESSION, &hub_cmd])
            .output();
    } else {
        let current = run(&["show-options", "-gqv", "@hub_version"]).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        if current.as_deref() != Some(version.as_str()) {
            let _ = restart_hub(exe);
        }
    }
    run(&["set-option", "-g", "@hub_version", &version]);
}

/// Kill the hub UI and start a new one. Workspaces and agents keep running.
pub fn restart_hub(exe: &str) -> Result<(), String> {
    check(run(&["respawn-pane", "-k", "-t", &format!("={HUB_SESSION}:"), &format!("{exe} ui")]))
}

pub fn kill_server() -> Result<(), String> {
    check(run(&["kill-server"]))
}

pub fn server_running() -> bool {
    run(&["list-sessions"]).is_some_and(|o| o.status.success())
}

pub fn source_config(config: &str, hub_key: &str) {
    // source-file adds bindings but never removes them, so drop the previous hub key first.
    // Servers started before @hub_key existed used the old default, C-q.
    let old = run(&["show-options", "-gqv", "@hub_key"]).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let old = old.filter(|k| !k.is_empty()).unwrap_or_else(|| "C-q".into());
    if old != hub_key {
        run(&["unbind-key", "-n", &old]);
    }
    run(&["source-file", config]);
}

/// Every pane on the hub server, including the hidden _hub and _pool sessions.
pub fn list_all_panes() -> Vec<Pane> {
    let fmt = FIELDS.iter().map(|f| format!("#{{{f}}}")).collect::<Vec<_>>().join("\t");
    let Some(out) = run(&["list-panes", "-a", "-F", &fmt]).filter(|o| o.status.success()) else { return vec![] };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let p: Vec<&str> = line.split('\t').collect();
            (p.len() == FIELDS.len()).then(|| Pane {
                session: p[0].into(),
                workspace_path: p[1].into(),
                session_path: p[2].into(),
                window_index: p[3].parse().unwrap_or(0),
                window_name: p[4].into(),
                window_id: p[5].into(),
                pane_id: p[6].into(),
                active: p[7] == "1",
                command: p[9].into(),
                piped: p[10] == "1",
                width: p[11].parse().unwrap_or(80),
                height: p[12].parse().unwrap_or(24),
                spare: p[13].into(),
            })
        })
        .collect()
}

/// Window name prefix of the pre-started Claude in the pool; the rest of the name is its path.
const CLAUDE_SPARE: &str = "claude:";
/// Window name prefix of a Claude that starts to replace an outdated spare. Nobody takes it before it replaces the spare.
const CLAUDE_NEXT: &str = "claude-next:";

/// Pre-started shells waiting in the pool, as (path, window_id). A spare's window name is its path.
pub fn spares(panes: &[Pane]) -> Vec<(String, String)> {
    panes
        .iter()
        .filter(|p| p.session == POOL_SESSION && !p.window_name.starts_with(CLAUDE_SPARE) && !p.window_name.starts_with(CLAUDE_NEXT))
        .map(|p| (p.window_name.clone(), p.window_id.clone()))
        .collect()
}

/// Pre-started Claudes ready to take, as (path, window_id).
pub fn claude_spares(panes: &[Pane]) -> Vec<(String, String)> {
    panes
        .iter()
        .filter(|p| p.session == POOL_SESSION)
        .filter_map(|p| Some((p.window_name.strip_prefix(CLAUDE_SPARE)?.to_string(), p.window_id.clone())))
        .collect()
}

pub struct ClaudeWindow {
    pub path: String,
    pub window_id: String,
    pub pane_id: String,
    /// Still starting to replace the spare for path.
    pub next: bool,
    /// Unix time it started, and (branch, commit) then, kept in the window so a restarted hub can take it over.
    pub started: Option<u64>,
    pub checkout: (Option<String>, Option<String>),
}

/// All Claudes in the pool, ready or starting.
pub fn claude_pool(panes: &[Pane]) -> Vec<ClaudeWindow> {
    panes
        .iter()
        .filter(|p| p.session == POOL_SESSION)
        .filter_map(|p| {
            let (path, next) = match p.window_name.strip_prefix(CLAUDE_SPARE) {
                Some(path) => (path, false),
                None => (p.window_name.strip_prefix(CLAUDE_NEXT)?, true),
            };
            let mut meta = p.spare.split(' ');
            let started = meta.next().and_then(|t| t.parse().ok());
            let mut field = || meta.next().filter(|v| !v.is_empty()).map(String::from);
            let checkout = (field(), field());
            Some(ClaudeWindow { path: path.to_string(), window_id: p.window_id.clone(), pane_id: p.pane_id.clone(), next, started, checkout })
        })
        .collect()
}

/// The pre-started window for path that runs command, looked up fresh: the snapshot may miss one started a moment ago.
pub fn spare_for(path: &str, command: Option<&str>) -> Option<String> {
    let pool = list_all_panes();
    let spares = match command {
        None => spares(&pool),
        Some("claude") => claude_spares(&pool),
        Some(_) => return None,
    };
    spares.into_iter().find(|(p, _)| p == path).map(|(_, w)| w)
}

pub fn spawn_spare(path: &str) {
    spawn_in_pool(path, path, None);
}

/// Start Claude in the pool, and return its window id. A next one waits for promote_claude before anyone takes it.
pub fn spawn_claude_spare(path: &str, next: bool, checkout: &(Option<String>, Option<String>)) -> Option<String> {
    let prefix = if next { CLAUDE_NEXT } else { CLAUDE_SPARE };
    let id = spawn_in_pool(&format!("{prefix}{path}"), path, Some("claude"))?;
    let (branch, commit) = (checkout.0.as_deref().unwrap_or(""), checkout.1.as_deref().unwrap_or(""));
    let meta = format!("{} {branch} {commit}", crate::state::now());
    run(&["set-option", "-w", "-t", &id, "@spare", &meta]);
    Some(id)
}

/// Make a starting Claude the spare to take for path.
pub fn promote_claude(window_id: &str, path: &str) {
    run(&["rename-window", "-t", window_id, &format!("{CLAUDE_SPARE}{path}")]);
}

fn spawn_in_pool(name: &str, path: &str, command: Option<&str>) -> Option<String> {
    let target = format!("={POOL_SESSION}:");
    let tail: Vec<&str> = ["-P", "-F", "#{window_id}", "-c", path, "-n", name].into_iter().chain(command).collect();
    // The pool session goes away with its last window, so create it when it is missing.
    let out = [vec!["new-window", "-d", "-t", &target], vec!["new-session", "-d", "-s", POOL_SESSION]]
        .into_iter()
        .find_map(|head| run(&[head, tail.clone()].concat()).filter(|o| o.status.success()))?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether a client is attached to any hub session, the hub view or a workspace.
/// Without a hub session, the hub runs in some terminal and counts as in use.
pub fn hub_in_use() -> bool {
    let Some(out) = run(&["list-sessions", "-F", "#{session_name} #{session_attached}"]).filter(|o| o.status.success()) else { return true };
    let sessions: Vec<(String, String)> =
        String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.rsplit_once(' ')).map(|(a, b)| (a.to_string(), b.to_string())).collect();
    !sessions.iter().any(|(s, _)| s == HUB_SESSION) || sessions.iter().any(|(_, n)| n != "0")
}

/// Pane ids on any tmux server, given its socket path, as (pane_id, session_name).
pub fn live_panes(socket: &str) -> Vec<(String, String)> {
    let Ok(out) = Command::new("tmux").args(["-S", socket, "list-panes", "-a", "-F", "#{pane_id}\t#{session_name}"]).output()
    else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(a, b)| (a.to_string(), b.to_string())))
        .collect()
}

pub fn list_sessions() -> Vec<String> {
    match run(&["list-sessions", "-F", "#{session_name}"]) {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect(),
        _ => vec![],
    }
}

pub fn unique_session_name(name: &str, taken: &[String]) -> String {
    // tmux rejects '.' and ':' in session names.
    let base: String = name.chars().map(|c| if c == '.' || c == ':' || c.is_whitespace() { '_' } else { c }).collect();
    let base = base.trim_start_matches('_');
    let base = if base.is_empty() { "workspace" } else { base };
    let mut candidate = base.to_string();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

type Cmd = Vec<String>;

fn cmd(args: &[&str]) -> Cmd {
    args.iter().map(|s| s.to_string()).collect()
}

fn chain(commands: &[Cmd]) -> Vec<String> {
    let mut args = vec![];
    for c in commands {
        if !args.is_empty() {
            args.push(";".to_string());
        }
        args.extend(c.iter().cloned());
    }
    args
}

/// Run tmux commands and switch to session, all in one tmux call so it feels instant.
fn run_and_switch(session: &str, commands: Vec<Cmd>) -> Res {
    let inside = inside_hub_server();
    let mut commands = commands;
    if inside {
        commands.push(cmd(&["switch-client", "-t", &format!("={session}")]));
    }
    if !commands.is_empty() {
        run_chain(&commands).map_err(Fail::Error)?;
    }
    if inside { Ok(()) } else { Err(Fail::Attach(session.to_string())) }
}

fn run_chain(commands: &[Cmd]) -> Result<(), String> {
    let args = chain(commands);
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    check(run(&refs))
}

pub fn switch_client(session: &str) -> Res {
    run_and_switch(session, vec![])
}

pub fn jump_to_window(session: &str, window_id: &str) -> Res {
    // Selecting first makes the window current in its session, so an attach lands there too.
    run_and_switch(session, vec![cmd(&["select-window", "-t", window_id])])
}

fn create_commands(name: &str, path: &str, window: &str, command: Option<&str>, spare: Option<&str>) -> Vec<Cmd> {
    let target = format!("={name}:");
    let options = [
        cmd(&["set-option", "-t", &target, "@workspace_path", path]),
        cmd(&["set-option", "-t", &target, "@workspace_label", &shorten(path)]),
    ];
    let mut out = match spare {
        // A session always starts with a window, so start with a cheap placeholder and swap in the spare.
        Some(spare) => vec![
            cmd(&["new-session", "-d", "-s", name, "-c", path, "-n", "_placeholder", "cat"]),
            cmd(&["move-window", "-s", spare, "-t", &target]),
            cmd(&["kill-window", "-t", &format!("={name}:_placeholder")]),
            cmd(&["rename-window", "-t", spare, window]),
        ],
        None => {
            let mut c = cmd(&["new-session", "-d", "-s", name, "-c", path, "-n", window]);
            c.extend(command.map(String::from));
            vec![c]
        }
    };
    out.extend(options);
    out
}

pub fn create_workspace(name: &str, path: &str, window: &str, command: Option<&str>, spare: Option<&str>) -> Res {
    // Switch only after the spare is in, so a failing switch never makes a second session.
    if spare.is_some() && run_chain(&create_commands(name, path, window, command, spare)).is_ok() {
        return run_and_switch(name, vec![]);
    }
    run_and_switch(name, create_commands(name, path, window, command, None))
}

/// The workspace for path, created (without switching) if needed.
pub fn ensure_workspace(path: &str) -> Result<String, String> {
    let real = canonical(path);
    let panes: Vec<Pane> = list_all_panes().into_iter().filter(|p| !p.session.starts_with('_')).collect();
    if let Some(p) = panes.iter().find(|p| canonical(if p.workspace_path.is_empty() { &p.session_path } else { &p.workspace_path }) == real) {
        return Ok(p.session.clone());
    }
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let name = unique_session_name(&name, &list_sessions());
    let args = chain(&create_commands(&name, path, "shell", None, None));
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    check(run(&refs))?;
    Ok(name)
}

/// Open a review checkout as a workspace with window in it, and switch to it. For Claude, the review prompt is typed but not sent.
/// A checkout that already has a workspace is only switched to.
/// Go to the workspace for dir, or open one named name with a window of this kind; Claude gets prompt typed in, if any.
pub fn open_checkout(dir: &str, name: &str, window: &str, command: Option<&str>, prompt: Option<&str>) -> Result<String, String> {
    let real = canonical(dir);
    let open = list_all_panes()
        .into_iter()
        .filter(|p| !p.session.starts_with('_'))
        .find(|p| canonical(if p.workspace_path.is_empty() { &p.session_path } else { &p.workspace_path }) == real);
    if let Some(p) = open {
        let _ = switch_client(&p.session);
        return Ok(format!("Opened {}", p.session));
    }
    let name = unique_session_name(name, &list_sessions());
    let args = chain(&create_commands(&name, dir, window, command, None));
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    check(run(&refs))?;
    let _ = switch_client(&name);
    let Some(prompt) = prompt.filter(|_| command == Some("claude")) else {
        return Ok(format!("Opened {name}"));
    };
    let pane = list_all_panes().into_iter().find(|p| p.session == name).map(|p| p.pane_id).ok_or("Claude exited at once in the review")?;
    if !wait_for_claude(&pane) {
        return Ok(format!("Opened {name}, but Claude took too long to start, so the review prompt is not typed in"));
    }
    type_text(&socket_path(), &pane, prompt)?;
    Ok(format!("Review ready in {name}: check the prompt, then Enter starts it"))
}

/// Open Claude in a new window of the workspace for path, with prompt typed but not sent, and switch to it.
pub fn open_issue(path: &str, prompt: &str) -> Result<String, String> {
    let session = ensure_workspace(path)?;
    let out = run(&["new-window", "-P", "-F", "#{pane_id}", "-t", &format!("={session}:"), "-c", path, "-n", "claude", "claude"]).ok_or("tmux not found")?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let pane = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let _ = switch_client(&session);
    if !wait_for_claude(&pane) {
        return Ok(format!("Opened Claude in {session}, but it took too long to start, so the issue is not typed in"));
    }
    type_text(&socket_path(), &pane, prompt)?;
    Ok(format!("Issue ready in {session}: check the prompt, then Enter starts it"))
}

/// Wait until Claude in pane reports its first hook event: before that it drops typed input.
fn wait_for_claude(pane: &str) -> bool {
    let dir = crate::hooks::events_dir();
    for _ in 0..100 {
        let started = std::fs::read_dir(&dir).into_iter().flatten().flatten().any(|e| {
            std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).is_some_and(|v| v["tmux_pane"] == pane)
        });
        if started {
            // shortcut: the input box appears a moment after SessionStart; wait for the prompt on screen if typing still gets lost.
            std::thread::sleep(std::time::Duration::from_millis(800));
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    false
}

pub fn canonical(path: &str) -> String {
    std::fs::canonicalize(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string())
}

/// Open a window, taken from the pool when spare is a pre-started window that runs command.
pub fn new_window(session: &str, cwd: &str, name: &str, command: Option<&str>, spare: Option<&str>) -> Res {
    if let Some(spare) = spare {
        // Switch only after the spare is in, so a failing switch never starts a second window.
        let moved = run_chain(&[
            cmd(&["move-window", "-s", spare, "-t", &format!("={session}:")]),
            cmd(&["rename-window", "-t", spare, name]),
            cmd(&["select-window", "-t", spare]),
        ]);
        if moved.is_ok() {
            return run_and_switch(session, vec![]);
        }
    }
    let mut c = cmd(&["new-window", "-t", &format!("={session}:"), "-c", cwd, "-n", name]);
    c.extend(command.map(String::from));
    run_and_switch(session, vec![c])
}

/// Stop the Claude process and continue its session in a hub workspace for path.
pub fn adopt(pid: u32, session_id: &str, path: &str) -> Res {
    if !crate::hooks::valid_id(session_id) {
        return Err(Fail::Error(format!("Odd session id {session_id:?}, not adopting it")));
    }
    let proc_dir = PathBuf::from(format!("/proc/{pid}"));
    let killed = Command::new("kill").args(["-TERM", &pid.to_string()]).status().is_ok_and(|s| s.success());
    if !killed && proc_dir.exists() {
        return Err(Fail::Error("Could not stop the agent, so it was not adopted".into()));
    }
    let start = std::time::Instant::now();
    // Two processes writing one transcript would garble it, so resume only once the old one is gone.
    while proc_dir.exists() {
        if start.elapsed() > std::time::Duration::from_secs(5) {
            return Err(Fail::Error("The agent did not stop within 5 s, so it was not adopted".into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    resume(session_id, path)
}

/// Continue a Claude session in a new window of the hub workspace for path.
pub fn resume(session_id: &str, path: &str) -> Res {
    if !crate::hooks::valid_id(session_id) {
        return Err(Fail::Error(format!("Odd session id {session_id:?}, not resuming it")));
    }
    let session = ensure_workspace(path).map_err(Fail::Error)?;
    new_window(&session, path, "claude", Some(&format!("claude --resume {session_id}")), None)
}

/// Run a tmux command on the hub server and return its output.
pub fn call(args: &[&str]) -> Result<String, String> {
    let out = run(args).ok_or("tmux not found")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "tmux failed".into() } else { err });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Open a window in session without switching to it, and return its window id.
pub fn open_window(session: &str, cwd: &str, name: &str, command: Option<&str>) -> Result<String, String> {
    let target = format!("={session}:");
    let mut args = vec!["new-window", "-d", "-P", "-F", "#{window_id}", "-t", &target, "-c", cwd, "-n", name];
    args.extend(command);
    call(&args)
}

pub fn shell_quote(s: &str) -> String {
    quote(s)
}

/// Stream the pane's output to a file, for the preview in the hub.
pub fn pipe_pane(pane_id: &str, path: &str) {
    run(&["pipe-pane", "-O", "-t", pane_id, &format!("cat >> {}", quote(path))]);
}

pub fn kill_session(session: &str) -> Res {
    simple(&["kill-session", "-t", &format!("={session}")])
}

pub fn kill_window(window_id: &str) -> Res {
    simple(&["kill-window", "-t", window_id])
}

pub fn rename_session(session: &str, new: &str) -> Res {
    simple(&["rename-session", "-t", &format!("={session}"), new])
}

pub fn detach() {
    run(&["detach-client"]);
}

pub fn hide_hub_status() {
    run(&["set-option", "-t", &format!("={HUB_SESSION}:"), "status", "off"]);
}

fn on_socket(socket: &str, args: &[&str]) -> Result<(), String> {
    check(Command::new("tmux").arg("-S").arg(socket).args(args).output().ok())
}

/// Type text into a pane and press Enter, as if the user wrote it.
pub fn send_text(socket: &str, pane: &str, text: &str) -> Result<(), String> {
    type_text(socket, pane, text)?;
    // Claude Code reads a burst of text as a paste and turns an Enter inside it into a newline; let the paste end first.
    std::thread::sleep(std::time::Duration::from_millis(150));
    on_socket(socket, &["send-keys", "-t", pane, "Enter"])
}

/// Type text into a pane without pressing Enter.
pub fn type_text(socket: &str, pane: &str, text: &str) -> Result<(), String> {
    // Its own call: tmux reads a trailing ';' in any argument as a command separator and drops it.
    on_socket(socket, &["send-keys", "-t", pane, "-l", "--", text])
}

/// Put text in a tmux buffer and, through the terminal (OSC 52), in the system clipboard.
pub fn copy(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut child = Command::new("tmux")
        .args(["-L", SOCKET_NAME, "load-buffer", "-w", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("tmux: {e}"))?;
    child.stdin.take().unwrap().write_all(text.as_bytes()).map_err(|e| format!("tmux: {e}"))?;
    check(child.wait_with_output().ok())
}

/// Press a named key (Enter, Escape, ...) in a pane.
pub fn send_key(socket: &str, pane: &str, key: &str) -> Result<(), String> {
    on_socket(socket, &["send-keys", "-t", pane, key])
}

/// What send-keys needs to press key in a pane: Ok for text typed as is, Err for a tmux key name.
pub fn key_for(k: KeyEvent) -> Option<Result<String, String>> {
    let m = k.modifiers;
    let mut prefix = String::new();
    if m.contains(KeyModifiers::CONTROL) {
        prefix.push_str("C-");
    }
    if m.contains(KeyModifiers::ALT) {
        prefix.push_str("M-");
    }
    let name = match k.code {
        KeyCode::Char(' ') if !prefix.is_empty() => "Space".into(),
        KeyCode::Char(c) if prefix.is_empty() => return Some(Ok(c.to_string())),
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "BTab".into(),
        KeyCode::Backspace => "BSpace".into(),
        KeyCode::Esc => "Escape".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PPage".into(),
        KeyCode::PageDown => "NPage".into(),
        KeyCode::Delete => "DC".into(),
        KeyCode::Insert => "IC".into(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return None,
    };
    if m.contains(KeyModifiers::SHIFT) && !matches!(k.code, KeyCode::Char(_) | KeyCode::BackTab) {
        prefix.push_str("S-");
    }
    Some(Err(format!("{prefix}{name}")))
}

/// Replace this process with a tmux client attached to session.
pub fn exec_attach(config: &str, session: &str) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    // Inside another tmux this nests a client; tmux refuses that unless $TMUX is unset.
    Command::new("tmux")
        .args(["-L", SOCKET_NAME, "-f", config, "attach-session", "-t", &format!("={session}")])
        .env_remove("TMUX")
        .exec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_names_are_valid_and_unique() {
        assert_eq!(unique_session_name("my.repo", &[]), "my_repo");
        assert_eq!(unique_session_name(".dot", &[]), "dot");
        assert_eq!(unique_session_name("a", &["a".into(), "a-2".into()]), "a-3");
        assert_eq!(unique_session_name("", &[]), "workspace");
    }

    #[test]
    fn keys_map_to_send_keys() {
        let k = |c, m| key_for(KeyEvent::new(c, m));
        assert_eq!(k(KeyCode::Char('A'), KeyModifiers::SHIFT), Some(Ok("A".into())));
        assert_eq!(k(KeyCode::Char('c'), KeyModifiers::CONTROL), Some(Err("C-c".into())));
        assert_eq!(k(KeyCode::Char('x'), KeyModifiers::ALT), Some(Err("M-x".into())));
        assert_eq!(k(KeyCode::Left, KeyModifiers::CONTROL), Some(Err("C-Left".into())));
        assert_eq!(k(KeyCode::Esc, KeyModifiers::NONE), Some(Err("Escape".into())));
    }

    #[test]
    fn claude_pool_reads_spare_metadata() {
        let pane = |name: &str, spare: &str| Pane {
            session: POOL_SESSION.into(),
            workspace_path: String::new(),
            session_path: String::new(),
            window_index: 1,
            window_name: name.into(),
            window_id: "@1".into(),
            pane_id: "%1".into(),
            active: true,
            command: "claude".into(),
            piped: false,
            width: 80,
            height: 24,
            spare: spare.into(),
        };
        let pool = claude_pool(&[pane("claude:/r", "100 main abc"), pane("claude-next:/r", "200  abc"), pane("claude:/old", "")]);
        assert_eq!((pool[0].started, pool[0].checkout.clone(), pool[0].next), (Some(100), (Some("main".into()), Some("abc".into())), false));
        assert_eq!((pool[1].started, pool[1].checkout.clone(), pool[1].next), (Some(200), (None, Some("abc".into())), true));
        assert_eq!((pool[2].started, pool[2].checkout.clone()), (None, (None, None)));
    }

    #[test]
    fn chain_separates_commands() {
        assert_eq!(chain(&[cmd(&["a", "b"]), cmd(&["c"])]), ["a", "b", ";", "c"]);
    }
}
