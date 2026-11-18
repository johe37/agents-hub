//! Gathers tmux, hook, git and transcript data into a Snapshot, on a background thread.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant, SystemTime};

use ratatui::text::Line;

use crate::config::Config;
use crate::git::{GitCache, GitStatus, load_dir_cache, save_dir_cache, scan_dirs};
use crate::hooks::events_dir;
use crate::screens::{ScreenCache, log_path, screens_dir};
use crate::state::{Live, Session, Status, live_claudes, load_sessions, transcript_for};
use crate::theme::{colors, css};
use crate::tmux::{self, Pane, POOL_SESSION};
use crate::transcript::{Transcript, TranscriptCache};

/// Window name prefix for `claude attach` windows.
pub const ATTACH_PREFIX: &str = "attach:";

#[derive(Clone)]
pub struct Window {
    pub index: u32,
    pub name: String,
    pub window_id: String,
    pub pane_id: String,
    pub command: String,
    pub claude: Option<Session>,
    pub width: u16,
    pub height: u16,
    pub screen: Option<Vec<Line<'static>>>,
}

impl Window {
    pub fn is_claude(&self) -> bool {
        // An attach window shows a background agent that already has its own row.
        (self.claude.is_some() || self.command == "claude") && !self.name.starts_with(ATTACH_PREFIX)
    }
}

#[derive(Clone)]
pub struct Workspace {
    pub name: String,
    pub path: String,
    pub windows: Vec<Window>,
    pub git: Option<GitStatus>,
    pub mr: Option<crate::gitlab::Mr>,
}

/// One Claude session: in a hub workspace, outside the hub, or a claude window without hook data.
#[derive(Clone)]
pub struct Agent {
    pub key: String,
    pub status: Status,
    pub session: Option<Session>,
    /// Workspace name and window id, for agents inside the hub.
    pub workspace: Option<String>,
    pub window_id: Option<String>,
    window_name: Option<String>,
    pub path: String,
    pub transcript: Option<Transcript>,
    pub git: Option<GitStatus>,
    /// (tmux socket, pane id) where the agent reads its input.
    pub target: Option<(String, String)>,
    /// The Claude process, from Claude Code's registry; needed to adopt it.
    pub pid: Option<u32>,
    /// For agents without hook data, found through the registry.
    transcript_path: Option<String>,
    /// Lives in Claude Code's background daemon; reached with `claude attach`.
    pub background: bool,
}

impl Agent {
    pub fn title(&self) -> String {
        if let Some(t) = &self.transcript {
            if let Some(s) = t.title.as_ref().or(t.first_prompt.as_ref()) {
                return s.clone();
            }
        }
        match &self.window_name {
            Some(n) if n != "claude" => n.clone(),
            _ => "Claude".into(),
        }
    }

    pub fn name(&self) -> String {
        self.workspace.clone().unwrap_or_else(|| basename(&self.path))
    }

    pub fn session_id(&self) -> Option<&str> {
        self.key.strip_prefix("agent:")
    }

    pub fn timestamp(&self) -> u64 {
        self.session.as_ref().map_or(0, |s| s.timestamp)
    }

    pub fn needs_permission(&self) -> bool {
        self.status == Status::Waiting
            && self.session.as_ref().is_some_and(|s| s.notification_type.as_deref() == Some("permission_prompt"))
    }
}

pub fn basename(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.into())
}

const MAX_CLOSED: usize = 15;

#[derive(Clone, Default)]
pub struct Snapshot {
    pub workspaces: Vec<Workspace>,
    pub agents: Vec<Agent>,
    pub recent_dirs: Vec<String>,
    pub spares: HashMap<String, String>,
    pub dirs: Arc<Vec<(String, bool)>>,
    /// Results of background jobs since the last snapshot, as (text, is_error), for toasts.
    pub notices: Vec<(String, bool)>,
}

pub enum Cmd {
    Refresh,
    Rescan,
    /// Keep a shell ready for the directory highlighted in the picker.
    Prewarm(Option<String>),
    /// Fetch the merge request of the workspace at this path again.
    ForgetMr(String),
    /// Push the branch checked out at path, opening a draft merge request when asked.
    Publish { path: String, open_mr: bool },
}

fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    std::thread::scope(|s| {
        let handles: Vec<_> = items.iter().map(|it| s.spawn(|| f(it))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

struct Collector {
    cfg: Config,
    git: GitCache,
    mrs: Arc<crate::gitlab::MrCache>,
    notices: Arc<Mutex<Vec<(String, bool)>>>,
    transcripts: TranscriptCache,
    screens: ScreenCache,
    hub_socket: String,
    speculative: Option<String>,
    dirs: Arc<Vec<(String, bool)>>,
    need_scan: bool,
}

/// Start the collector thread. It sends a Snapshot whenever data may have changed.
pub fn spawn(cfg: Config) -> (Sender<Cmd>, Receiver<Snapshot>) {
    let (cmd_tx, cmd_rx) = channel();
    let (snap_tx, snap_rx) = channel();
    std::thread::spawn(move || {
        let cached = load_dir_cache();
        let mut c = Collector {
            hub_socket: tmux::socket_path(),
            need_scan: cached.is_none(),
            dirs: Arc::new(cached.unwrap_or_default()),
            speculative: None,
            git: GitCache::default(),
            mrs: Default::default(),
            notices: Default::default(),
            transcripts: TranscriptCache::default(),
            screens: ScreenCache::default(),
            cfg,
        };
        c.run(cmd_rx, snap_tx);
    });
    (cmd_tx, snap_rx)
}

fn mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Collector {
    fn run(&mut self, rx: Receiver<Cmd>, tx: Sender<Snapshot>) {
        let interval = Duration::from_secs_f64(self.cfg.refresh_seconds);
        let events = events_dir();
        let mut events_mtime = mtime(&events);
        let mut last = Instant::now() - interval;
        loop {
            let mut due = last.elapsed() >= interval;
            // Hooks write by rename, so the directory mtime changes on every event: status shows up at once.
            let m = mtime(&events);
            if m != events_mtime {
                events_mtime = m;
                due = true;
            }
            if due {
                if tx.send(self.collect()).is_err() {
                    return;
                }
                last = Instant::now();
            }
            match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(Cmd::Refresh) => last = Instant::now() - interval,
                Ok(Cmd::Rescan) => {
                    self.rescan();
                    last = Instant::now() - interval;
                }
                Ok(Cmd::Prewarm(path)) => self.prewarm(path),
                Ok(Cmd::ForgetMr(path)) => {
                    self.mrs.forget(&path);
                    last = Instant::now() - interval;
                }
                // Its own thread: a push can take a while, and status updates must keep flowing meanwhile.
                Ok(Cmd::Publish { path, open_mr }) => {
                    let (mrs, notices) = (Arc::clone(&self.mrs), Arc::clone(&self.notices));
                    std::thread::spawn(move || {
                        let r = crate::gitlab::publish(&path, open_mr);
                        mrs.forget(&path);
                        notices.lock().unwrap().push(r.map_or_else(|e| (format!("Push failed: {e}"), true), |m| (m, false)));
                    });
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn rescan(&mut self) {
        let dirs = scan_dirs(&self.cfg.roots);
        save_dir_cache(&dirs);
        self.dirs = Arc::new(dirs);
    }

    fn collect(&mut self) -> Snapshot {
        let all_panes = tmux::list_all_panes();
        let panes: Vec<&Pane> = all_panes.iter().filter(|p| !p.session.starts_with('_')).collect();
        let hub_panes: Vec<(String, String)> = all_panes.iter().map(|p| (p.pane_id.clone(), p.session.clone())).collect();
        let hub_socket = self.hub_socket.clone();
        let live = |socket: &str| if tmux::canonical(socket) == hub_socket { hub_panes.clone() } else { tmux::live_panes(socket) };
        let sessions = load_sessions(live, self.cfg.stale_minutes);

        let on_hub = |s: &Session| s.tmux_socket.as_deref().is_some_and(|sock| tmux::canonical(sock) == hub_socket);
        let in_hub: HashMap<String, Session> = sessions
            .iter()
            .filter(|s| s.status.alive() && on_hub(s))
            .filter_map(|s| Some((s.tmux_pane.clone()?, s.clone())))
            .collect();
        let mut closed: Vec<Session> = sessions.iter().filter(|s| !s.status.alive() && !s.cwd.is_empty()).cloned().collect();
        closed.sort_by_key(|s| std::cmp::Reverse(s.timestamp));
        // Pane ids are only unique per tmux server, so check the socket too.
        let elsewhere: Vec<Session> = sessions
            .iter()
            .filter(|s| s.status.alive() && !(on_hub(s) && s.tmux_pane.as_ref().is_some_and(|p| in_hub.contains_key(p))))
            .cloned()
            .collect();

        let mut workspaces = self.workspaces(&panes, &in_hub);
        self.previews(&mut workspaces, &all_panes);

        let git = &self.git;
        let statuses = par_map(&workspaces, |ws| git.repo_root(&ws.path).map(|r| git.status(&r)));
        for (ws, st) in workspaces.iter_mut().zip(statuses) {
            ws.mr = st.as_ref().and_then(|g| g.branch.as_deref()).and_then(|b| self.mrs.get(&ws.path, b));
            ws.git = st;
        }
        workspaces.sort_by_key(|w| w.name.to_lowercase());
        let agents = self.agents(&workspaces, elsewhere, live_claudes(), closed);
        write_summary(&sessions);

        if self.need_scan {
            self.need_scan = false;
            self.rescan();
        }
        let mut recent = vec![];
        for s in &sessions {
            if !s.cwd.is_empty() && !recent.contains(&s.cwd) {
                recent.push(s.cwd.clone());
            }
        }
        if self.cfg.prewarm {
            let paths: HashSet<String> = workspaces.iter().map(|w| w.path.clone()).collect();
            self.fill_pool(paths, &all_panes);
        }
        Snapshot {
            workspaces,
            agents,
            recent_dirs: recent,
            spares: tmux::spares(&tmux::list_all_panes()).into_iter().collect(),
            dirs: self.dirs.clone(),
            notices: std::mem::take(&mut *self.notices.lock().unwrap()),
        }
    }

    fn workspaces(&self, panes: &[&Pane], claude_by_pane: &HashMap<String, Session>) -> Vec<Workspace> {
        let mut by_window: Vec<(String, Vec<&Pane>)> = vec![];
        for p in panes {
            match by_window.iter_mut().find(|(id, _)| *id == p.window_id) {
                Some((_, v)) => v.push(p),
                None => by_window.push((p.window_id.clone(), vec![p])),
            }
        }
        let mut result: Vec<Workspace> = vec![];
        for (_, wp) in by_window {
            let active = wp.iter().find(|p| p.active).unwrap_or(&wp[0]);
            let claude = wp.iter().find_map(|p| claude_by_pane.get(&p.pane_id)).cloned();
            // In a split window, replies must reach Claude's pane even when a shell next to it has focus.
            let main = wp
                .iter()
                .find(|p| claude_by_pane.contains_key(&p.pane_id))
                .or_else(|| wp.iter().find(|p| p.command == "claude"))
                .unwrap_or(active);
            let w = Window {
                index: active.window_index,
                name: active.window_name.clone(),
                window_id: active.window_id.clone(),
                pane_id: main.pane_id.clone(),
                command: main.command.clone(),
                claude,
                width: main.width,
                height: main.height,
                screen: None,
            };
            match result.iter_mut().find(|ws| ws.name == active.session) {
                Some(ws) => ws.windows.push(w),
                None => {
                    let path = if active.workspace_path.is_empty() { &active.session_path } else { &active.workspace_path };
                    result.push(Workspace { name: active.session.clone(), path: path.clone(), windows: vec![w], git: None, mr: None });
                }
            }
        }
        for ws in &mut result {
            ws.windows.sort_by_key(|w| w.index);
        }
        result
    }

    /// Keep every terminal pane piped to a log and replay the logs into screen previews.
    fn previews(&mut self, workspaces: &mut [Workspace], all_panes: &[Pane]) {
        let _ = std::fs::create_dir_all(screens_dir());
        let piped: HashSet<&str> = all_panes.iter().filter(|p| p.piped).map(|p| p.pane_id.as_str()).collect();
        for w in workspaces.iter_mut().flat_map(|ws| ws.windows.iter_mut()).filter(|w| !w.is_claude()) {
            if !piped.contains(w.pane_id.as_str()) {
                tmux::pipe_pane(&w.pane_id, &log_path(&w.pane_id).display().to_string());
            }
            w.screen = self.screens.lines(&w.pane_id, w.width, w.height);
        }
        self.screens.forget(&all_panes.iter().map(|p| p.pane_id.clone()).collect());
    }

    fn agents(&self, workspaces: &[Workspace], elsewhere: Vec<Session>, mut live: Vec<Live>, closed: Vec<Session>) -> Vec<Agent> {
        let hub = tmux::socket_path();
        let in_pane = |l: &Live, socket: &str, pane: &str| l.tmux.as_ref().is_some_and(|(s, p)| p == pane && tmux::canonical(s) == tmux::canonical(socket));
        let mut agents = vec![];
        for ws in workspaces {
            for w in ws.windows.iter().filter(|w| w.is_claude()) {
                let l = live.iter().position(|l| in_pane(l, &hub, &w.pane_id)).map(|i| live.swap_remove(i));
                let status = w.claude.as_ref().map(|s| s.status).or(l.as_ref().map(|l| l.status)).unwrap_or(Status::Unknown);
                agents.push(Agent {
                    pid: l.as_ref().map(|l| l.pid),
                    transcript_path: l.as_ref().and_then(|l| transcript_for(&l.cwd, &l.session_id)),
                    key: w.claude.as_ref().map_or(format!("window:{}", w.window_id), |s| format!("agent:{}", s.session_id)),
                    status,
                    session: w.claude.clone(),
                    workspace: Some(ws.name.clone()),
                    window_id: Some(w.window_id.clone()),
                    window_name: Some(w.name.clone()),
                    path: ws.path.clone(),
                    transcript: None,
                    background: false,
                    git: ws.git.clone(),
                    target: Some((hub.clone(), w.pane_id.clone())),
                });
            }
        }
        let git = &self.git;
        let statuses = par_map(&elsewhere, |s| (!s.cwd.is_empty()).then(|| git.repo_root(&s.cwd)).flatten().map(|r| git.status(&r)));
        for (s, st) in elsewhere.into_iter().zip(statuses) {
            // Same session, or the same pane when the registry lags behind a /clear.
            let l = live.iter().position(|l| l.session_id == s.session_id || s.tmux_socket.as_deref().zip(s.tmux_pane.as_deref()).is_some_and(|(sock, p)| in_pane(l, sock, p))).map(|i| live.swap_remove(i));
            agents.push(Agent {
                background: l.as_ref().is_some_and(|l| l.background),
                pid: l.map(|l| l.pid),
                transcript_path: None,
                key: format!("agent:{}", s.session_id),
                status: s.status,
                target: s.tmux_socket.clone().zip(s.tmux_pane.clone()),
                path: s.cwd.clone(),
                session: Some(s),
                workspace: None,
                window_id: None,
                window_name: None,
                transcript: None,
                git: st,
            });
        }
        // Running Claudes that never reported through the hooks.
        for l in live.into_iter().filter(|l| !l.tmux.as_ref().is_some_and(|(s, _)| tmux::canonical(s) == hub)) {
            agents.push(Agent {
                key: format!("agent:{}", l.session_id),
                status: l.status,
                session: None,
                workspace: None,
                window_id: None,
                window_name: None,
                git: (!l.cwd.is_empty()).then(|| git.repo_root(&l.cwd)).flatten().map(|r| git.status(&r)),
                transcript_path: transcript_for(&l.cwd, &l.session_id),
                target: l.tmux,
                pid: Some(l.pid),
                background: l.background,
                path: l.cwd,
                transcript: None,
            });
        }
        // A background session has no pane to place it by, so it goes to the workspace holding its directory.
        for a in agents.iter_mut().filter(|a| a.background && a.workspace.is_none()) {
            a.workspace = workspaces.iter().filter(|w| a.path == w.path || a.path.starts_with(&format!("{}/", w.path))).max_by_key(|w| w.path.len()).map(|w| w.name.clone());
        }
        // Recently closed sessions, so they can be resumed; skip ones already running again.
        let closed: Vec<Session> = closed
            .into_iter()
            // No transcript means it never got a prompt: nothing to resume.
            .filter(|s| s.transcript_path.as_deref().is_some_and(|p| std::path::Path::new(p).exists()))
            .filter(|s| !agents.iter().any(|a| a.session_id() == Some(&s.session_id)))
            .take(MAX_CLOSED)
            .collect();
        let statuses = par_map(&closed, |s| git.repo_root(&s.cwd).map(|r| git.status(&r)));
        for (s, st) in closed.into_iter().zip(statuses) {
            agents.push(Agent {
                key: format!("agent:{}", s.session_id),
                status: s.status,
                // The pane is gone or runs something else now, so never send it input.
                target: None,
                pid: None,
                transcript_path: None,
                path: s.cwd.clone(),
                session: Some(s),
                workspace: None,
                window_id: None,
                window_name: None,
                transcript: None,
                background: false,
                git: st,
            });
        }
        let tc = &self.transcripts;
        let transcripts =
            par_map(&agents, |a| a.session.as_ref().and_then(|s| s.transcript_path.as_deref()).or(a.transcript_path.as_deref()).and_then(|p| tc.get(p)));
        for (a, t) in agents.iter_mut().zip(transcripts) {
            a.transcript = t;
        }
        // Fixed order so rows never jump: workspace agents keep their window order, the rest go by path.
        agents.sort_by_cached_key(|a| a.workspace.is_none().then(|| (a.path.clone(), a.key.clone())));
        agents
    }

    /// Keep one started shell ready per workspace, so a new terminal shows its prompt at once.
    fn fill_pool(&self, mut wanted: HashSet<String>, all_panes: &[Pane]) {
        wanted.extend(self.speculative.clone());
        let mut seen = HashSet::new();
        for p in all_panes.iter().filter(|p| p.session == POOL_SESSION) {
            if seen.contains(&p.window_name) || !wanted.contains(&p.window_name) {
                let _ = tmux::kill_window(&p.window_id);
            }
            seen.insert(p.window_name.clone());
        }
        let mut pool_exists = !seen.is_empty();
        let mut missing: Vec<&String> = wanted.iter().filter(|p| !seen.contains(*p)).collect();
        missing.sort();
        // A few per round, so a hub start with many workspaces does not spawn a burst of shells.
        for path in missing.into_iter().take(3) {
            if std::path::Path::new(path).is_dir() {
                tmux::spawn_spare(path, pool_exists);
                pool_exists = true;
            }
        }
    }

    fn prewarm(&mut self, path: Option<String>) {
        self.speculative = path.clone();
        let Some(path) = path.filter(|p| self.cfg.prewarm && std::path::Path::new(p).is_dir()) else { return };
        let current = tmux::spares(&tmux::list_all_panes());
        if !current.iter().any(|(p, _)| *p == path) {
            tmux::spawn_spare(&path, !current.is_empty());
        }
    }
}

/// One-line summary shown in the tmux status bar of every workspace.
fn write_summary(sessions: &[Session]) {
    let waiting = sessions.iter().filter(|s| s.status == Status::Waiting).count();
    let text = if waiting > 0 {
        let c = colors();
        let (warning, bg, bar, fg) = (css(c.warning), css(c.bg), css(c.bar), css(c.fg));
        format!("#[bg={warning},fg={bg},bold] ⏳ {waiting} waiting #[bg={bar},fg={fg},nobold]")
    } else {
        String::new()
    };
    let path = tmux::summary_file();
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        let _ = std::fs::write(path, text);
    }
}
