//! The hidden tmux engine: a private tmux server where every session is a workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

const FIELDS: [&str; 13] = [
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
set -g window-status-format "#[fg={muted}]  #I #{{?#{{==:#W,claude}},✻ ,}}#W  "
set -g window-status-current-format "#[bg={bg},fg={accent}]▎#[fg={bright},bold]#I #{{?#{{==:#W,claude}},✻ ,}}#W  "
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
            run(&["respawn-pane", "-k", "-t", &format!("={HUB_SESSION}:"), &hub_cmd]);
        }
    }
    run(&["set-option", "-g", "@hub_version", &version]);
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
            })
        })
        .collect()
}

/// Pre-started shells waiting in the pool, as (path, window_id). A spare's window name is its path.
pub fn spares(panes: &[Pane]) -> Vec<(String, String)> {
    panes.iter().filter(|p| p.session == POOL_SESSION).map(|p| (p.window_name.clone(), p.window_id.clone())).collect()
}

pub fn spawn_spare(path: &str, pool_exists: bool) {
    if pool_exists {
        run(&["new-window", "-d", "-t", &format!("={POOL_SESSION}:"), "-c", path, "-n", path]);
    } else {
        run(&["new-session", "-d", "-s", POOL_SESSION, "-c", path, "-n", path]);
    }
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
        let args = chain(&commands);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        simple(&refs)?;
    }
    if inside { Ok(()) } else { Err(Fail::Attach(session.to_string())) }
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
    let mut out = match (spare, command) {
        // A session always starts with a window, so start with a cheap placeholder and swap in the spare shell.
        (Some(spare), None) => vec![
            cmd(&["new-session", "-d", "-s", name, "-c", path, "-n", "_placeholder", "cat"]),
            cmd(&["move-window", "-s", spare, "-t", &target]),
            cmd(&["kill-window", "-t", &format!("={name}:_placeholder")]),
            cmd(&["rename-window", "-t", spare, window]),
        ],
        _ => {
            let mut c = cmd(&["new-session", "-d", "-s", name, "-c", path, "-n", window]);
            c.extend(command.map(String::from));
            vec![c]
        }
    };
    out.extend(options);
    out
}

pub fn create_workspace(name: &str, path: &str, window: &str, command: Option<&str>, spare: Option<&str>) -> Res {
    match run_and_switch(name, create_commands(name, path, window, command, spare)) {
        Err(Fail::Error(_)) if spare.is_some() => run_and_switch(name, create_commands(name, path, window, command, None)),
        r => r,
    }
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

/// Open a review checkout as a workspace with Claude in it, the review prompt typed but not sent, and switch to it.
/// A checkout that already has a workspace is only switched to.
pub fn open_review(dir: &str, name: &str, prompt: &str) -> Result<String, String> {
    let real = canonical(dir);
    let open = list_all_panes()
        .into_iter()
        .filter(|p| !p.session.starts_with('_'))
        .find(|p| canonical(if p.workspace_path.is_empty() { &p.session_path } else { &p.workspace_path }) == real);
    if let Some(p) = open {
        let _ = switch_client(&p.session);
        return Ok(format!("Opened the review in {}", p.session));
    }
    let name = unique_session_name(name, &list_sessions());
    let args = chain(&create_commands(&name, dir, "claude", Some("claude"), None));
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    check(run(&refs))?;
    let _ = switch_client(&name);
    let pane = list_all_panes().into_iter().find(|p| p.session == name).map(|p| p.pane_id).ok_or("Claude exited at once in the review")?;
    if !wait_for_claude(&pane) {
        return Ok(format!("Opened {name}, but Claude took too long to start, so the review prompt is not typed in"));
    }
    type_text(&socket_path(), &pane, prompt)?;
    Ok(format!("Review ready in {name}: check the prompt, then Enter starts it"))
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

pub fn new_window(session: &str, cwd: &str, name: &str, command: Option<&str>, spare: Option<&str>) -> Res {
    if let (Some(spare), None) = (spare, command) {
        let r = run_and_switch(
            session,
            vec![
                cmd(&["move-window", "-s", spare, "-t", &format!("={session}:")]),
                cmd(&["rename-window", "-t", spare, name]),
                cmd(&["select-window", "-t", spare]),
            ],
        );
        if !matches!(r, Err(Fail::Error(_))) {
            return r;
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

/// Press a named key (Enter, Escape, ...) in a pane.
pub fn send_key(socket: &str, pane: &str, key: &str) -> Result<(), String> {
    on_socket(socket, &["send-keys", "-t", pane, key])
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
    fn chain_separates_commands() {
        assert_eq!(chain(&[cmd(&["a", "b"]), cmd(&["c"])]), ["a", "b", ";", "c"]);
    }
}
