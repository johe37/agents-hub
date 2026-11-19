//! Gathers tmux, hook, git and transcript data into a Snapshot, on a background thread.

use std::cell::RefCell;
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
use crate::tmux::{self, Pane, PaneTty};
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
    /// Where the agent reads its input, when the hub can type into it.
    pub target: Option<Input>,
    /// Why a running agent cannot be sent input, when it cannot.
    pub read_only: Option<String>,
    /// (tmux socket, pane) the agent reported, verified or not.
    pub tmux: Option<(String, String)>,
    /// The Claude process, from Claude Code's registry; needed to adopt it.
    pub pid: Option<u32>,
    /// For agents without hook data, found through the registry.
    transcript_path: Option<String>,
    /// Lives in Claude Code's background daemon; reached with `claude attach`.
    pub background: bool,
}

/// Where an agent reads its input.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A tmux pane that is Claude's own terminal: keys go in with send-keys.
    Tmux { socket: String, pane: String },
    /// An nvim :terminal: text goes to the terminal over nvim's RPC socket, whatever nvim shows.
    Nvim { socket: String, pid: u32 },
}

impl Input {
    /// Type text without pressing Enter.
    pub fn type_text(&self, text: &str) -> Result<(), String> {
        match self {
            Input::Tmux { socket, pane } => tmux::type_text(socket, pane, text),
            Input::Nvim { socket, pid } => crate::nvim::send(socket, *pid, text),
        }
    }

    /// Type text and press Enter, as if the user wrote it.
    pub fn send_text(&self, text: &str) -> Result<(), String> {
        self.type_text(text)?;
        // Claude Code reads a burst of text as a paste and turns an Enter inside it into a newline; let the paste end first.
        std::thread::sleep(std::time::Duration::from_millis(150));
        self.send_key("Enter")
    }

    /// Press Enter or Escape.
    pub fn send_key(&self, key: &str) -> Result<(), String> {
        match self {
            Input::Tmux { socket, pane } => tmux::send_key(socket, pane, key),
            Input::Nvim { socket, pid } => crate::nvim::send(socket, *pid, if key == "Escape" { "\x1b" } else { "\r" }),
        }
    }
}

impl std::fmt::Display for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Input::Tmux { socket, pane } => write!(f, "pane {pane} on {socket}"),
            Input::Nvim { socket, .. } => write!(f, "its nvim terminal, through {socket}"),
        }
    }
}

impl Agent {
    fn transcript_file(&self) -> Option<&str> {
        self.session.as_ref().and_then(|s| s.transcript_path.as_deref()).or(self.transcript_path.as_deref())
    }

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

    fn input(&self) -> Result<&Input, String> {
        self.target
            .as_ref()
            .ok_or_else(|| self.read_only.clone().unwrap_or_else(|| "The hub does not know where this agent reads its input.".into()))
    }

    /// Answer Claude Code's permission dialog: Enter takes the highlighted "Yes", Escape declines.
    /// Returns what was done, for a confirmation message.
    pub fn answer(&self, approve: bool) -> Result<&'static str, String> {
        if !self.needs_permission() {
            return Err("This agent does not ask for permission now.".into());
        }
        let (what, key) = if approve { ("Approved", "Enter") } else { ("Denied", "Escape") };
        self.input()?.send_key(key).map(|_| what)
    }

    /// Type a message to the agent and send it, never as an answer to a permission prompt.
    pub fn send_message(&self, text: &str) -> Result<(), String> {
        if self.needs_permission() {
            return Err(format!("{} is waiting for a permission answer; answer that first.", self.title()));
        }
        self.input()?.send_text(text)
    }

    /// A made-up agent, for the demo town.
    pub fn demo(key: &str, status: Status, session: Option<Session>, workspace: Option<&str>, path: &str, transcript: Transcript, git: Option<GitStatus>) -> Agent {
        Agent {
            key: key.into(),
            status,
            session,
            workspace: workspace.map(String::from),
            window_id: None,
            window_name: None,
            path: path.into(),
            transcript: Some(transcript),
            git,
            target: None,
            read_only: Some("This is the demo town.".into()),
            tmux: None,
            pid: None,
            transcript_path: None,
            background: false,
        }
    }

    /// y/n answers a permission prompt; anything else is typed to the agent and sent.
    pub fn respond(&self, text: &str) -> Result<&'static str, String> {
        let lower = text.to_lowercase();
        if self.needs_permission() && matches!(lower.as_str(), "y" | "yes") {
            self.answer(true)
        } else if self.needs_permission() && matches!(lower.as_str(), "n" | "no" | "d") {
            self.answer(false)
        } else {
            self.input()?.send_text(text).map(|_| "Sent")
        }
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
    pub claude_spares: HashMap<String, String>,
    pub dirs: Arc<Vec<(String, bool)>>,
    /// Results of background jobs since the last snapshot, as (text, is_error), for toasts.
    pub notices: Vec<(String, bool)>,
    /// Open MRs where you are a requested reviewer.
    pub reviews: Vec<crate::gitlab::Mr>,
    pub usage: Option<crate::usage::Usage>,
    /// Your XP, level, loot and quests, if you have any.
    pub progress: Option<Arc<crate::progress::Progress>>,
}

const CLAUDE_MAX_AGE: Duration = Duration::from_secs(10 * 60);
const CLAUDE_PAUSE: Duration = Duration::from_secs(60);
/// Drop the pre-started Claude when nobody looked at the hub for this long.
const CLAUDE_UNSEEN: Duration = Duration::from_secs(15 * 60);

struct ClaudeSpare {
    window_id: String,
    path: String,
    started: Instant,
    branch: Option<String>,
}

pub enum Cmd {
    Refresh,
    Rescan,
    /// Keep a shell ready for the directory highlighted in the picker.
    Prewarm(Option<String>),
    /// Keep a Claude ready for the workspace at this path.
    PrewarmClaude(String),
    /// Fetch the merge request of the workspace at this path again.
    ForgetMr(String),
    /// Push the branch checked out at path, opening a draft merge request when asked.
    Publish { path: String, open_mr: bool },
    /// Fetch the MRs to review again.
    ForgetReviews,
    /// Check out an MR from the clone at repo and open it for a review with Claude.
    StartReview { mr: Box<crate::gitlab::Mr>, repo: String },
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
    reviews: Arc<crate::gitlab::ReviewCache>,
    notices: Arc<Mutex<Vec<(String, bool)>>>,
    transcripts: TranscriptCache,
    screens: Arc<Mutex<ScreenCache>>,
    hub_socket: String,
    speculative: Option<String>,
    claude_target: Option<String>,
    claude_spare: Option<ClaudeSpare>,
    /// Path and time of the last spare taken or lost.
    claude_pause: Option<(String, Instant)>,
    /// Last time a client showed the hub.
    hub_seen: Instant,
    dirs: Arc<Vec<(String, bool)>>,
    need_scan: bool,
    /// Transcripts of live agents. Claude writes its replies there without a hook event.
    watched: Vec<String>,
    /// Collect what only the TUI shows: terminal previews, merge requests and pooled shells.
    tui: bool,
    progress_store: crate::progress::Store,
    progress: (Option<SystemTime>, Option<Arc<crate::progress::Progress>>),
}

/// The panes of every tmux server asked so far in one collection round.
struct Servers {
    hub: String,
    panes: RefCell<HashMap<String, Result<Vec<PaneTty>, String>>>,
}

impl Servers {
    fn new(hub: &str, hub_panes: &[Pane]) -> Self {
        let panes = HashMap::from([(hub.to_string(), Ok(hub_panes.iter().map(PaneTty::from).collect()))]);
        Servers { hub: hub.to_string(), panes: RefCell::new(panes) }
    }

    /// Where keys reach this Claude, or why they cannot.
    fn target(&self, pid: Option<u32>, tmux: Option<(String, String)>) -> Result<(String, String), String> {
        let Some((socket, pane)) = tmux else { return Err("It runs in a terminal outside tmux, so the hub cannot type into it.".into()) };
        let key = tmux::canonical(&socket);
        let mut panes = self.panes.borrow_mut();
        let list = panes.entry(key).or_insert_with(|| tmux::pane_ttys(&socket));
        tmux::verify_pane(pid, &pane, list).map(|_| (socket, pane))
    }

    /// Where the hub can type to this Claude: its own tmux pane, else the nvim terminal it runs in.
    fn input(&self, pid: Option<u32>, tmux: Option<(String, String)>) -> Result<Input, String> {
        let pane = self.target(pid, tmux).map(|(socket, pane)| Input::Tmux { socket, pane });
        match (pane, pid.and_then(|p| crate::nvim::socket_of(p).map(|s| (s, p)))) {
            (Err(_), Some((socket, pid))) => Ok(Input::Nvim { socket, pid }),
            (pane, _) => pane,
        }
    }

    fn on_hub(&self, socket: &str) -> bool {
        tmux::canonical(socket) == self.hub
    }
}

/// Start the collector thread. It sends a Snapshot whenever data may have changed.
/// With `tui` false (the town) it skips terminal previews, merge requests and pooled shells.
pub fn spawn(cfg: Config, screens: Arc<Mutex<ScreenCache>>, tui: bool) -> (Sender<Cmd>, Receiver<Snapshot>) {
    let (cmd_tx, cmd_rx) = channel();
    let (snap_tx, snap_rx) = channel();
    std::thread::spawn(move || Collector::new(cfg, screens, tui).run(cmd_rx, snap_tx));
    (cmd_tx, snap_rx)
}

/// One snapshot, collected the way the town does it.
pub fn collect_once(cfg: Config) -> Snapshot {
    Collector::new(cfg, Default::default(), false).collect()
}

fn mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Collector {
    fn new(cfg: Config, screens: Arc<Mutex<ScreenCache>>, tui: bool) -> Self {
        let cached = load_dir_cache();
        Collector {
            hub_socket: tmux::socket_path(),
            need_scan: cached.is_none(),
            watched: vec![],
            dirs: Arc::new(cached.unwrap_or_default()),
            speculative: None,
            claude_target: None,
            claude_spare: None,
            claude_pause: None,
            hub_seen: Instant::now(),
            git: GitCache::default(),
            mrs: Default::default(),
            reviews: Default::default(),
            notices: Default::default(),
            transcripts: TranscriptCache::default(),
            screens,
            progress_store: Default::default(),
            progress: (None, None),
            cfg,
            tui,
        }
    }

    fn run(&mut self, rx: Receiver<Cmd>, tx: Sender<Snapshot>) {
        let interval = Duration::from_secs_f64(self.cfg.refresh_seconds);
        let events = events_dir();
        let mut events_mtime = mtime(&events);
        let mut last = Instant::now() - interval;
        let mut replies = vec![];
        loop {
            let mut due = last.elapsed() >= interval;
            let r: Vec<_> = self.watched.iter().map(|p| mtime(p.as_ref())).collect();
            if r != replies {
                replies = r;
                due = true;
            }
            // Hooks write by rename, so the directory mtime changes on every event: status shows up at once.
            let m = mtime(&events);
            if m != events_mtime {
                events_mtime = m;
                due = true;
            }
            // A sync or the town changed your progress: show it now.
            if self.progress_store.mtime() != self.progress.0 {
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
                Ok(Cmd::PrewarmClaude(path)) => {
                    self.claude_target = Some(path);
                    last = Instant::now() - interval;
                }
                Ok(Cmd::ForgetMr(path)) => {
                    self.mrs.forget(&path);
                    last = Instant::now() - interval;
                }
                Ok(Cmd::ForgetReviews) => {
                    self.reviews.forget();
                    last = Instant::now() - interval;
                }
                Ok(Cmd::StartReview { mr, repo }) => {
                    let notices = Arc::clone(&self.notices);
                    std::thread::spawn(move || {
                        let name = format!("review-{}-{}", mr.repo(), mr.iid);
                        let r = crate::gitlab::checkout(&repo, &mr).and_then(|dir| tmux::open_review(&dir, &name, &mr.review_prompt()));
                        notices.lock().unwrap().push(r.map_or_else(|e| (format!("Review of !{} failed: {e}", mr.iid), true), |m| (m, false)));
                    });
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
        let live_panes = |socket: &str| if tmux::canonical(socket) == hub_socket { hub_panes.clone() } else { tmux::live_panes(socket) };
        let live = live_claudes();
        let registry: HashMap<String, u32> = live.iter().map(|l| (l.session_id.clone(), l.pid)).collect();
        let sessions = load_sessions(live_panes, self.cfg.stale_minutes, &registry);
        let servers = Servers::new(&hub_socket, &all_panes);

        let on_hub = |s: &Session| s.tmux_socket.as_deref().is_some_and(|sock| servers.on_hub(sock));
        // Only sessions whose pane is Claude's own terminal belong to that hub window.
        let in_hub: HashMap<String, Session> = sessions
            .iter()
            .filter(|s| s.status.alive() && on_hub(s))
            .filter(|s| servers.target(s.pid, s.tmux_socket.clone().zip(s.tmux_pane.clone())).is_ok())
            .filter_map(|s| Some((s.tmux_pane.clone()?, s.clone())))
            .collect();
        let mut closed: Vec<Session> = sessions.iter().filter(|s| !s.status.alive() && !s.cwd.is_empty()).cloned().collect();
        closed.sort_by_key(|s| std::cmp::Reverse(s.timestamp));
        // Pane ids are only unique per tmux server, so check the socket too.
        let elsewhere: Vec<Session> =
            sessions.iter().filter(|s| s.status.alive() && !in_hub.values().any(|h| h.session_id == s.session_id)).cloned().collect();

        let mut workspaces = self.workspaces(&panes, &in_hub);
        if self.tui {
            self.previews(&mut workspaces, &all_panes);
        }

        let git = &self.git;
        let statuses = par_map(&workspaces, |ws| git.repo_root(&ws.path).map(|r| git.status(&r)));
        for (ws, st) in workspaces.iter_mut().zip(statuses) {
            if self.tui {
                ws.mr = st.as_ref().and_then(|g| g.branch.as_deref()).and_then(|b| self.mrs.get(&ws.path, b));
            }
            ws.git = st;
        }
        workspaces.sort_by_key(|w| w.name.to_lowercase());
        let agents = self.agents(&workspaces, elsewhere, live, closed, &servers);
        self.watched = agents.iter().filter(|a| a.status.alive()).filter_map(|a| a.transcript_file().map(String::from)).collect();
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
        if self.tui && self.cfg.prewarm {
            let paths: HashSet<String> = workspaces.iter().map(|w| w.path.clone()).collect();
            self.fill_pool(paths, &all_panes);
        }
        if self.tui {
            self.fill_claude(&all_panes);
        }
        let pool = tmux::list_all_panes();
        let progress = self.progress();
        Snapshot {
            workspaces,
            agents,
            recent_dirs: recent,
            spares: tmux::spares(&pool).into_iter().collect(),
            claude_spares: tmux::claude_spares(&pool).into_iter().collect(),
            dirs: self.dirs.clone(),
            notices: std::mem::take(&mut *self.notices.lock().unwrap()),
            reviews: self.reviews.get(),
            usage: crate::usage::load(),
            progress,
        }
    }

    /// The progress file, read again only when it changed.
    fn progress(&mut self) -> Option<Arc<crate::progress::Progress>> {
        let m = self.progress_store.mtime();
        if m != self.progress.0 {
            self.progress = (m, self.progress_store.load().map(Arc::new));
        }
        self.progress.1.clone()
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
        let mut screens = self.screens.lock().unwrap();
        for w in workspaces.iter_mut().flat_map(|ws| ws.windows.iter_mut()).filter(|w| !w.is_claude()) {
            if !piped.contains(w.pane_id.as_str()) {
                tmux::pipe_pane(&w.pane_id, &log_path(&w.pane_id).display().to_string());
            }
            w.screen = screens.lines(&w.pane_id, w.width, w.height);
        }
        screens.forget(&all_panes.iter().map(|p| p.pane_id.clone()).collect());
    }

    fn agents(&self, workspaces: &[Workspace], elsewhere: Vec<Session>, mut live: Vec<Live>, closed: Vec<Session>, servers: &Servers) -> Vec<Agent> {
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
                    target: Some(Input::Tmux { socket: hub.clone(), pane: w.pane_id.clone() }),
                    read_only: None,
                    tmux: Some((hub.clone(), w.pane_id.clone())),
                });
            }
        }
        let git = &self.git;
        let statuses = par_map(&elsewhere, |s| (!s.cwd.is_empty()).then(|| git.repo_root(&s.cwd)).flatten().map(|r| git.status(&r)));
        for (s, st) in elsewhere.into_iter().zip(statuses) {
            // Same session, or the same pane when the registry lags behind a /clear.
            // The same session; without a known pid, the same pane when the registry lags behind a /clear.
            let l = live
                .iter()
                .position(|l| l.session_id == s.session_id || (s.pid.is_none() && s.tmux_socket.as_deref().zip(s.tmux_pane.as_deref()).is_some_and(|(sock, p)| in_pane(l, sock, p))))
                .map(|i| live.swap_remove(i));
            let pid = s.pid.or(l.as_ref().map(|l| l.pid));
            let target = servers.input(pid, s.tmux_socket.clone().zip(s.tmux_pane.clone()));
            // Nested in a hub window (say, an nvim terminal): it still belongs to that workspace.
            let workspace = s.tmux_socket.as_deref().filter(|sock| servers.on_hub(sock)).and(s.tmux_session.clone()).filter(|n| !n.starts_with('_'));
            agents.push(Agent {
                background: l.as_ref().is_some_and(|l| l.background),
                pid,
                transcript_path: None,
                key: format!("agent:{}", s.session_id),
                status: s.status,
                read_only: target.as_ref().err().cloned(),
                target: target.ok(),
                tmux: s.tmux_socket.clone().zip(s.tmux_pane.clone()),
                path: s.cwd.clone(),
                session: Some(s),
                workspace,
                window_id: None,
                window_name: None,
                transcript: None,
                git: st,
            });
        }
        // Running Claudes that never reported through the hooks.
        for l in live.into_iter().filter(|l| !l.tmux.as_ref().is_some_and(|(s, _)| tmux::canonical(s) == hub)) {
            let target = servers.input(Some(l.pid), l.tmux.clone());
            agents.push(Agent {
                tmux: l.tmux.clone(),
                read_only: target.as_ref().err().cloned(),
                key: format!("agent:{}", l.session_id),
                status: l.status,
                session: None,
                workspace: None,
                window_id: None,
                window_name: None,
                git: (!l.cwd.is_empty()).then(|| git.repo_root(&l.cwd)).flatten().map(|r| git.status(&r)),
                transcript_path: transcript_for(&l.cwd, &l.session_id),
                target: target.ok(),
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
                read_only: None,
                tmux: None,
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
            par_map(&agents, |a| a.transcript_file().and_then(|p| tc.get(p)));
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
        for (path, id) in tmux::spares(all_panes) {
            if seen.contains(&path) || !wanted.contains(&path) {
                let _ = tmux::kill_window(&id);
            }
            seen.insert(path);
        }
        let mut missing: Vec<&String> = wanted.iter().filter(|p| !seen.contains(*p)).collect();
        missing.sort();
        // A few per round, so a hub start with many workspaces does not spawn a burst of shells.
        for path in missing.into_iter().take(3) {
            if std::path::Path::new(path).is_dir() {
                tmux::spawn_spare(path);
            }
        }
    }

    /// Keep one Claude started for the workspace selected in the hub, or the directory
    /// highlighted in the picker, so c or Ctrl+O shows it at once.
    /// Only one, as each takes a few hundred MB; replaced when old or on another branch,
    /// as Claude reads the git state and CLAUDE.md files when it starts.
    fn fill_claude(&mut self, all_panes: &[Pane]) {
        if tmux::hub_visible() {
            self.hub_seen = Instant::now();
        }
        let target = self
            .claude_target
            .clone()
            .filter(|p| self.cfg.prewarm_claude && self.hub_seen.elapsed() < CLAUDE_UNSEEN && std::path::Path::new(p).is_dir());
        let branch = target.as_deref().and_then(|p| self.git.repo_root(p)).and_then(|r| self.git.status(&r).branch);
        let spares = tmux::claude_spares(all_panes);
        // Taken into a workspace, or Claude exited: wait a while before the next one, so it does not compete with this one.
        if let Some(s) = self.claude_spare.take_if(|s| !spares.iter().any(|(_, id)| *id == s.window_id)) {
            self.claude_pause = Some((s.path, Instant::now()));
        }
        let mut kept = false;
        for (path, id) in spares {
            let fresh = self.claude_spare.as_ref().is_some_and(|s| s.window_id == id && s.started.elapsed() < CLAUDE_MAX_AGE && s.branch == branch);
            if !kept && fresh && target.as_ref() == Some(&path) {
                kept = true;
            } else {
                let _ = tmux::kill_window(&id);
                self.claude_spare.take_if(|s| s.window_id == id);
            }
        }
        let Some(path) = target.filter(|_| !kept) else { return };
        if self.claude_pause.as_ref().is_some_and(|(p, at)| *p == path && at.elapsed() < CLAUDE_PAUSE) {
            return;
        }
        self.claude_spare = tmux::spawn_claude_spare(&path).map(|window_id| ClaudeSpare {
            window_id,
            path: path.clone(),
            started: Instant::now(),
            branch,
        });
        if self.claude_spare.is_none() {
            self.claude_pause = Some((path, Instant::now()));
        }
    }

    fn prewarm(&mut self, path: Option<String>) {
        self.speculative = path.clone();
        let Some(path) = path.filter(|p| self.tui && self.cfg.prewarm && std::path::Path::new(p).is_dir()) else { return };
        let current = tmux::spares(&tmux::list_all_panes());
        if !current.iter().any(|(p, _)| *p == path) {
            tmux::spawn_spare(&path);
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
