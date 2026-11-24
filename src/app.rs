//! Hub state and input handling. Drawing lives in ui.rs.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use crate::config::{Config, Section, shorten};
use crate::input::Input;
use crate::picker::{self, Picker};
use crate::screens::ScreenCache;
use crate::snapshot::{Agent, Cmd, Snapshot, Workspace, basename};
use crate::state::Status;
use crate::tmux::{self, Fail, Res};

#[derive(Clone, Copy, PartialEq)]
pub enum Row {
    Gap,
    /// Section title and how many agents it holds.
    Header(&'static str, usize),
    Workspace(usize),
    Agent(usize),
    /// (workspace, window) indexes.
    Terminal(usize, usize),
    /// (workspace, thread) indexes: an unresolved review thread on the workspace's merge request.
    Thread(usize, usize),
    /// An open MR where you are a requested reviewer, by index into the snapshot's reviews.
    Review(usize),
    /// An open issue assigned to you, by index into the snapshot's issues.
    Issue(usize),
    /// An open MR you opened or are assigned to, by index into that list.
    Mr(Mine, usize),
    /// Stands in for the workspaces until there are any, so a new user sees how to open one.
    NewWorkspace,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mine {
    Authored,
    Assigned,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Open,
    NewWorkspace,
    NewWindow(&'static str),
    NewChat,
    Close,
    Rename,
    Reply,
    Approve,
    Deny,
    Filter,
    Help,
    Leave,
    JumpWindow(String, String),
    Tab(usize),
    Adopt,
    MarkDone,
    NextWaiting,
    Forward,
    Resolve,
    Publish,
    Confirm(bool),
    Fold,
    FoldAll,
    Type,
    Refresh,
}

pub enum Modal {
    Help,
    /// `done` is shown when `then` succeeds.
    Confirm { question: String, then: Box<dyn Fn() -> Res>, yes: bool, done: Option<String> },
    Rename { from: String, input: Input },
    Picker(Picker),
}

#[derive(PartialEq)]
pub enum Focus {
    List,
    Filter,
    Reply,
    /// Keys go to the selected terminal.
    Terminal,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
    /// Shown below the text as a link to Ctrl+click.
    pub link: Option<String>,
}

impl Toast {
    pub fn life(&self) -> Duration {
        Duration::from_secs(4)
    }
}

/// Screen areas from the last frame, for mouse hits.
#[derive(Default)]
pub struct Hits {
    pub list: Rect,
    pub detail: Rect,
    pub picker_list: Rect,
    pub last_selected: Option<usize>,
    pub buttons: Vec<(Rect, Action)>,
}

pub struct App {
    pub cfg: Config,
    pub snap: Snapshot,
    pub loaded: bool,
    cmd: Sender<Cmd>,
    pub rows: Vec<(String, Row)>,
    pub selected: usize,
    pub offset: usize,
    pub hover: Option<usize>,
    pub filter: Input,
    pub reply: Input,
    pub focus: Focus,
    /// The reply box resolves the selected review thread instead of only replying.
    pub resolving: bool,
    pub modal: Option<Modal>,
    pub toast: Option<Toast>,
    pub detail_scroll: u16,
    /// Text dragged over in the detail panel; copied when the mouse is released.
    pub sel: Option<Selection>,
    /// Prompts sent from the hub that the transcript does not show yet, by agent key.
    pub sent: HashMap<String, (String, Instant)>,
    /// Detail panel tab: chat or agents, changes, windows.
    pub tab: usize,
    pub started: Instant,
    pub hits: Hits,
    /// When each agent last changed status, for the row flash.
    pub changed: HashMap<String, Instant>,
    /// Names of workspaces folded to one row.
    pub folded: HashSet<String>,
    /// Foldable sections the user unfolded; they start folded.
    pub open_sections: HashSet<&'static str>,
    /// For each review, the workspace that holds its checkout, if one is open.
    pub review_ws: Vec<Option<usize>>,
    last_click: Option<(Instant, usize)>,
    prewarm_pending: Option<(Instant, String)>,
    /// A count typed before j/k, vim style, and when its last digit came.
    pub count: Option<(usize, Instant)>,
    /// The workspace selected last, and since when; its Claude is started once the selection rests.
    claude_target: Option<(Instant, String)>,
    claude_sent: Option<String>,
    rescanning: bool,
    quit: bool,
    pub attach_to: Option<String>,
    screens: Arc<Mutex<ScreenCache>>,
    /// The screen of the terminal being typed into, fresher than the snapshot's.
    pub live: Option<Live>,
}

pub struct Live {
    pub pane: String,
    pub lines: Vec<Line<'static>>,
    /// (row, column) on the pane's screen.
    pub cursor: Option<(u16, u16)>,
}

/// Positions are (row, column) in the panel's content, so they stay put when it scrolls.
#[derive(Clone, Copy)]
pub struct Selection {
    pub from: (u16, u16),
    pub to: (u16, u16),
    pub copy: bool,
}

pub const FLASH: Duration = Duration::from_millis(1500);
/// How long a workspace must stay selected before a Claude starts for it. Each start costs a few hundred MB,
/// so moving through the list must not start one for every row passed.
const CLAUDE_DWELL: Duration = Duration::from_millis(700);

const WINDOW_KINDS: [(&str, Option<&str>); 3] = [("claude", Some("claude")), ("shell", None), ("nvim", Some("nvim"))];

pub fn window_command(kind: &str) -> Option<&'static str> {
    WINDOW_KINDS.iter().find(|(k, _)| *k == kind).and_then(|(_, c)| *c)
}

impl App {
    pub fn new(cfg: Config, cmd: Sender<Cmd>, screens: Arc<Mutex<ScreenCache>>) -> Self {
        let warnings = cfg.warnings.join(" ");
        let mut app = App {
            cfg,
            snap: Snapshot::default(),
            loaded: false,
            cmd,
            rows: vec![],
            selected: 0,
            offset: 0,
            hover: None,
            filter: Input::default(),
            reply: Input::default(),
            focus: Focus::List,
            resolving: false,
            modal: None,
            toast: None,
            detail_scroll: 0,
            sel: None,
            sent: HashMap::new(),
            tab: 0,
            started: Instant::now(),
            hits: Hits::default(),
            changed: HashMap::new(),
            folded: HashSet::new(),
            open_sections: HashSet::from([WORKSPACES, OTHER]),
            review_ws: vec![],
            last_click: None,
            prewarm_pending: None,
            count: None,
            claude_target: None,
            claude_sent: None,
            rescanning: false,
            quit: false,
            attach_to: None,
            screens,
            live: None,
        };
        if !warnings.is_empty() {
            app.notify(warnings, true);
        }
        app
    }

    pub fn run(&mut self, term: &mut DefaultTerminal, snaps: Receiver<Snapshot>) -> std::io::Result<()> {
        let mut last_request_check = Instant::now();
        while !self.quit {
            term.draw(|f| crate::ui::draw(f, self))?;
            // Fast ticks only while something animates, faster still while typing so echo feels immediate.
            let timeout = if self.focus == Focus::Terminal || self.count.is_some() {
                Duration::from_millis(30)
            } else if self.animating() {
                Duration::from_millis(80)
            } else {
                Duration::from_millis(250)
            };
            if event::poll(timeout)? {
                // Drain queued input before the next frame, so held keys never lag behind.
                loop {
                    match event::read()? {
                        Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
                        Event::Mouse(m) => self.on_mouse(m),
                        _ => {}
                    }
                    if self.quit || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            while let Ok(snap) = snaps.try_recv() {
                self.apply(snap);
            }
            if last_request_check.elapsed() > Duration::from_millis(300) {
                last_request_check = Instant::now();
                self.check_request();
            }
            if self.count.is_some_and(|(_, at)| at.elapsed() > COUNT_TIMEOUT) {
                self.flush_count();
            }
            if let Some((at, path)) = &self.prewarm_pending {
                if at.elapsed() > Duration::from_millis(150) {
                    let _ = self.cmd.send(Cmd::Prewarm(Some(path.clone())));
                    self.prewarm_pending = None;
                }
            }
            self.prewarm_claude();
            self.update_live();
            if self.toast.as_ref().is_some_and(|t| t.at.elapsed() > t.life()) {
                self.toast = None;
            }
        }
        Ok(())
    }

    /// While typing, read the terminal's screen every tick instead of waiting for the next snapshot.
    fn update_live(&mut self) {
        let pane = match (self.focus == Focus::Terminal, self.current_terminal()) {
            (true, Some((_, w))) => (w.pane_id.clone(), w.width, w.height),
            (true, None) => {
                self.focus = Focus::List;
                self.live = None;
                return;
            }
            _ => {
                self.live = None;
                return;
            }
        };
        let mut screens = self.screens.lock().unwrap();
        let lines = screens.lines(&pane.0, pane.1, pane.2);
        let cursor = screens.cursor(&pane.0);
        drop(screens);
        self.live = Some(Live { pane: pane.0, lines: lines.unwrap_or_default(), cursor });
    }

    /// Start a Claude for the selected workspace, or the directory highlighted in the picker,
    /// so c or Ctrl+O shows it at once.
    fn prewarm_claude(&mut self) {
        let path = match &self.modal {
            Some(Modal::Picker(p)) => p.selected_path(),
            _ => self.current_workspace().map(|w| w.path.clone()),
        };
        if let Some(path) = path {
            if self.claude_target.as_ref().is_none_or(|(_, p)| *p != path) {
                self.claude_target = Some((Instant::now(), path));
            }
        }
        let Some((at, path)) = &self.claude_target else { return };
        if at.elapsed() > CLAUDE_DWELL && self.claude_sent.as_ref() != Some(path) {
            let _ = self.cmd.send(Cmd::PrewarmClaude(path.clone()));
            self.claude_sent = Some(path.clone());
        }
    }

    fn apply(&mut self, snap: Snapshot) {
        let dirs_changed = !std::sync::Arc::ptr_eq(&snap.dirs, &self.snap.dirs);
        let now = Instant::now();
        for a in &snap.agents {
            let old = self.snap.agents.iter().find(|o| o.key == a.key);
            if self.loaded && old.is_some_and(|o| o.status != a.status) {
                self.changed.insert(a.key.clone(), now);
            }
        }
        self.changed.retain(|_, t| t.elapsed() < FLASH);
        // Drop a sent prompt once the transcript has it, or when it never arrives.
        self.sent.retain(|key, (text, at)| {
            let shown = snap.agents.iter().find(|a| a.key == *key).and_then(|a| a.transcript.as_ref()?.last_prompt.as_deref()) == Some(&crate::transcript::flatten(text));
            !shown && at.elapsed() < Duration::from_secs(30)
        });
        for (text, error) in &snap.notices {
            self.notify(text.clone(), *error);
        }
        self.snap = snap;
        self.loaded = true;
        // A row that goes away leaves the selection at the same place, on the row that takes its spot.
        self.rebuild_rows(None);
        if dirs_changed {
            if let Some(Modal::Picker(p)) = &mut self.modal {
                p.candidates = picker::candidates(&self.snap, &self.cfg.roots);
                p.update();
            }
            if self.rescanning {
                self.rescanning = false;
                let n = self.snap.dirs.iter().filter(|(_, r)| *r).count();
                self.notify(format!("Found {n} repos"), false);
            }
        }
    }

    fn animating(&self) -> bool {
        !self.loaded
            || self.snap.inbox_loading
            || !self.snap.inbox_busy.is_empty()
            || self.toast.is_some()
            || self.changed.values().any(|t| t.elapsed() < FLASH)
            || self.snap.agents.iter().any(|a| matches!(a.status, Status::Working | Status::Waiting))
    }

    pub fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast { text: text.into(), error, at: Instant::now(), link: None });
    }

    fn refresh(&self) {
        let _ = self.cmd.send(Cmd::Refresh);
    }

    // ---- rows ----

    fn matches(&self, fields: &[Option<&str>]) -> bool {
        let needle = self.filter.text.trim().to_lowercase();
        needle.is_empty() || fields.iter().flatten().any(|f| f.to_lowercase().contains(&needle))
    }

    fn agent_matches(&self, a: &Agent) -> bool {
        let title = a.title();
        let name = a.name();
        self.matches(&[
            Some(&title),
            Some(&name),
            Some(&a.path),
            a.git.as_ref().and_then(|g| g.branch.as_deref()),
            a.session.as_ref().and_then(|s| s.message.as_deref()),
            a.transcript.as_ref().and_then(|t| t.last_prompt.as_deref()),
        ])
    }

    pub fn agents_of(&self, ws: &Workspace) -> Vec<usize> {
        (0..self.snap.agents.len()).filter(|&i| self.snap.agents[i].workspace.as_deref() == Some(&ws.name)).collect()
    }

    pub fn worst(&self, agents: &[usize]) -> Option<Status> {
        agents.iter().map(|&i| self.snap.agents[i].status).filter(|s| s.alive()).min()
    }

    pub fn build_rows(&mut self) {
        let owner = self.owner();
        self.rebuild_rows(owner);
    }

    /// The row key that takes the selection when a fold hides the selected row: its workspace or section.
    fn owner(&self) -> Option<String> {
        if !matches!(self.current(), Some(Row::Review(_))) {
            if let Some(ri) = self.current_workspace().and_then(|ws| self.review_of(&ws.name)) {
                return Some(review_key(&self.snap.reviews[ri]));
            }
        }
        match self.current() {
            Some(Row::Review(i)) if self.in_approved(i) && self.section_open(REVIEWS) => Some(section_key(APPROVED)),
            Some(Row::Review(_)) => Some(section_key(REVIEWS)),
            Some(Row::Issue(_)) => Some(section_key(ISSUES)),
            Some(Row::Mr(Mine::Authored, _)) => Some(section_key(MRS)),
            Some(Row::Mr(Mine::Assigned, _)) => Some(section_key(ASSIGNED)),
            Some(Row::Workspace(_) | Row::Terminal(..) | Row::Thread(..)) if !self.section_open(WORKSPACES) => Some(section_key(WORKSPACES)),
            Some(Row::Agent(i)) if self.snap.agents[i].workspace.is_some() && !self.section_open(WORKSPACES) => Some(section_key(WORKSPACES)),
            Some(Row::Agent(i)) if self.snap.agents[i].workspace.is_none() => Some(section_key(if self.snap.agents[i].status.alive() { OTHER } else { CLOSED })),
            _ => self.current_workspace().map(|w| format!("ws:{}", w.name)),
        }
    }

    fn rebuild_rows(&mut self, owner: Option<String>) {
        let current = self.rows.get(self.selected).map(|(k, _)| k.clone());
        let paths: Vec<String> = self.snap.workspaces.iter().map(|w| tmux::canonical(&w.path)).collect();
        self.review_ws = self
            .snap
            .reviews
            .iter()
            .map(|m| {
                let dir = tmux::canonical(&m.review_dir().display().to_string());
                paths.iter().position(|p| *p == dir)
            })
            .collect();
        let mut sections: Vec<(usize, Vec<usize>, Vec<usize>, Vec<usize>)> = vec![];
        for (wi, ws) in self.snap.workspaces.iter().enumerate() {
            // A review's workspace shows under its review instead.
            if self.review_ws.contains(&Some(wi)) {
                continue;
            }
            let agents = self.agents_of(ws);
            let terminals: Vec<usize> = (0..ws.windows.len()).filter(|&i| !ws.windows[i].is_claude()).collect();
            let threads: Vec<usize> = (0..ws.mr.as_ref().map_or(0, |m| m.threads.len())).collect();
            let branch = ws.git.as_ref().and_then(|g| g.branch.as_deref());
            if self.matches(&[Some(&ws.name), Some(&ws.path), branch]) {
                sections.push((wi, agents, terminals, threads));
            } else {
                let agents: Vec<usize> = agents.into_iter().filter(|&i| self.agent_matches(&self.snap.agents[i])).collect();
                let terminals: Vec<usize> = terminals
                    .into_iter()
                    .filter(|&i| self.matches(&[Some(&ws.windows[i].name), Some(&ws.windows[i].command)]))
                    .collect();
                let threads: Vec<usize> = threads
                    .into_iter()
                    .filter(|&i| {
                        let th = &ws.mr.as_ref().unwrap().threads[i];
                        let first = th.notes.first();
                        self.matches(&[th.path.as_deref(), first.map(|n| n.author.as_str()), first.map(|n| n.body.as_str())])
                    })
                    .collect();
                if !agents.is_empty() || !terminals.is_empty() || !threads.is_empty() {
                    sections.push((wi, agents, terminals, threads));
                }
            }
        }

        let mut ws_rows = vec![];
        if !sections.is_empty() {
            ws_rows.push((section_key(WORKSPACES), Row::Header(WORKSPACES, sections.len())));
        } else if self.snap.workspaces.is_empty() && self.filter.text.is_empty() {
            ws_rows.push((section_key(WORKSPACES), Row::Header(WORKSPACES, 0)));
            if self.section_open(WORKSPACES) {
                ws_rows.push(("new-workspace".into(), Row::NewWorkspace));
            }
        }
        if !self.section_open(WORKSPACES) {
            sections.clear();
        }
        for (wi, mut agents, mut terminals, mut threads) in sections {
            // A filter shows every match. Agents that wait for you show even when folded.
            if self.filter.text.is_empty() && self.folded.contains(&self.snap.workspaces[wi].name) {
                agents.retain(|&i| self.snap.agents[i].status == Status::Waiting);
                terminals.clear();
                threads.clear();
            }
            ws_rows.push((format!("ws:{}", self.snap.workspaces[wi].name), Row::Workspace(wi)));
            for ai in agents {
                ws_rows.push((self.snap.agents[ai].key.clone(), Row::Agent(ai)));
            }
            for ti in terminals {
                ws_rows.push((format!("term:{}", self.snap.workspaces[wi].windows[ti].window_id), Row::Terminal(wi, ti)));
            }
            for ti in threads {
                ws_rows.push((format!("thread:{}", self.snap.workspaces[wi].mr.as_ref().unwrap().threads[ti].id), Row::Thread(wi, ti)));
            }
        }
        let section = |alive: bool| -> Vec<usize> {
            let mut v: Vec<usize> = (0..self.snap.agents.len())
                .filter(|&i| {
                    let a = &self.snap.agents[i];
                    a.workspace.is_none() && a.status.alive() == alive && self.agent_matches(a)
                })
                .collect();
            if !alive {
                v.sort_by_key(|&i| std::cmp::Reverse(self.snap.agents[i].timestamp()));
            }
            v
        };
        let mut reviews: Vec<(usize, Vec<usize>, Vec<usize>)> = vec![];
        for (i, m) in self.snap.reviews.iter().enumerate() {
            let ws = self.review_ws[i].map(|wi| &self.snap.workspaces[wi]);
            let mut agents = ws.map(|ws| self.agents_of(ws)).unwrap_or_default();
            let mut terminals: Vec<usize> = ws.map(|ws| (0..ws.windows.len()).filter(|&t| !ws.windows[t].is_claude()).collect()).unwrap_or_default();
            if !self.matches(&[Some(&m.title), Some(&m.project), Some(&m.author)]) {
                agents.retain(|&a| self.agent_matches(&self.snap.agents[a]));
                terminals.retain(|&t| {
                    let w = &ws.unwrap().windows[t];
                    self.matches(&[Some(&w.name), Some(&w.command)])
                });
                if agents.is_empty() && terminals.is_empty() {
                    continue;
                }
            }
            if self.filter.text.is_empty() && ws.is_some_and(|ws| self.folded.contains(&ws.name)) {
                agents.retain(|&a| self.snap.agents[a].status == Status::Waiting);
                terminals.clear();
            }
            reviews.push((i, agents, terminals));
        }
        let mut review_rows = vec![];
        if !reviews.is_empty() {
            // A review with a workspace is work in progress, approved or not, so it stays on top.
            let (approved, mut todo): (Vec<_>, Vec<_>) = reviews.into_iter().partition(|r| self.in_approved(r.0));
            todo.sort_by_key(|r| self.review_ws[r.0].is_none());
            review_rows.push((section_key(REVIEWS), Row::Header(REVIEWS, todo.len())));
            let open = self.section_open(REVIEWS);
            self.push_reviews(&mut review_rows, todo, open);
            if open && !approved.is_empty() {
                review_rows.push((section_key(APPROVED), Row::Header(APPROVED, approved.len())));
                self.push_reviews(&mut review_rows, approved, self.section_open(APPROVED));
            }
        } else if self.placeholder(REVIEWS) {
            review_rows.push((section_key(REVIEWS), Row::Header(REVIEWS, 0)));
        }
        let mine_rows = |list: Mine, title: &'static str| -> Vec<(String, Row)> {
            let mrs = self.mine(list);
            let shown: Vec<usize> = (0..mrs.len())
                .filter(|&i| {
                    let m = &mrs[i];
                    // An MR you opened and are assigned to shows once, with the ones you opened.
                    let yours = list == Mine::Assigned && self.snap.authored.iter().any(|a| a.project_id == m.project_id && a.iid == m.iid);
                    !yours && self.matches(&[Some(&m.title), Some(&m.project), Some(&m.author)])
                })
                .collect();
            let mut rows = vec![];
            if !shown.is_empty() {
                rows.push((section_key(title), Row::Header(title, shown.len())));
                if self.section_open(title) {
                    rows.extend(shown.into_iter().map(|i| (format!("mr:{title}:{}!{}", mrs[i].project, mrs[i].iid), Row::Mr(list, i))));
                }
            } else if self.placeholder(title) {
                rows.push((section_key(title), Row::Header(title, 0)));
            }
            rows
        };
        let mut authored_rows = mine_rows(Mine::Authored, MRS);
        let mut assigned_rows = mine_rows(Mine::Assigned, ASSIGNED);
        let issues: Vec<usize> = (0..self.snap.issues.len())
            .filter(|&i| {
                let is = &self.snap.issues[i];
                self.matches(&[Some(&is.title), Some(&is.project), Some(&is.labels.join(" "))])
            })
            .collect();
        let mut issue_rows = vec![];
        if !issues.is_empty() {
            issue_rows.push((section_key(ISSUES), Row::Header(ISSUES, issues.len())));
            if self.section_open(ISSUES) {
                issue_rows.extend(issues.into_iter().map(|i| (format!("issue:{}#{}", self.snap.issues[i].project, self.snap.issues[i].iid), Row::Issue(i))));
            }
        } else if self.placeholder(ISSUES) {
            issue_rows.push((section_key(ISSUES), Row::Header(ISSUES, 0)));
        }
        let (outside, closed) = (section(true), section(false));
        let mut other_rows = vec![];
        if !outside.is_empty() {
            other_rows.push((section_key(OTHER), Row::Header(OTHER, outside.len())));
            if self.section_open(OTHER) {
                other_rows.extend(outside.into_iter().map(|i| (self.snap.agents[i].key.clone(), Row::Agent(i))));
            }
        }
        let mut closed_rows = vec![];
        if !closed.is_empty() {
            closed_rows.push((section_key(CLOSED), Row::Header(CLOSED, closed.len())));
            if self.section_open(CLOSED) {
                closed_rows.extend(closed.into_iter().map(|i| (self.snap.agents[i].key.clone(), Row::Agent(i))));
            }
        }
        let mut rows = vec![];
        for section in &self.cfg.sections {
            let block = std::mem::take(match section {
                Section::Workspaces => &mut ws_rows,
                Section::GitlabReviews => &mut review_rows,
                Section::GitlabMergeRequests => &mut authored_rows,
                Section::GitlabAssignedMergeRequests => &mut assigned_rows,
                Section::GitlabIssues => &mut issue_rows,
                Section::OtherSessions => &mut other_rows,
                Section::ClosedSessions => &mut closed_rows,
            });
            if !block.is_empty() && !rows.is_empty() {
                rows.push((String::new(), Row::Gap));
            }
            rows.extend(block);
        }
        self.rows = rows;
        let fallback = owner.and_then(|k| self.rows.iter().position(|(key, _)| *key == k));
        let prev = self.selected;
        let at = prev.min(self.rows.len().saturating_sub(1));
        let near = (at..self.rows.len()).chain((0..at).rev()).find(|&i| selectable(&self.rows[i].1)).unwrap_or(0);
        self.selected = current.and_then(|k| self.rows.iter().position(|(key, _)| *key == k)).or(fallback).unwrap_or(near);
        if self.selected != prev {
            self.detail_scroll = 0;
            self.sel = None;
        }
    }

    pub fn current(&self) -> Option<Row> {
        self.rows.get(self.selected).map(|(_, r)| *r).filter(selectable)
    }

    pub fn current_agent(&self) -> Option<&Agent> {
        match self.current() {
            Some(Row::Agent(i)) => self.snap.agents.get(i),
            _ => None,
        }
    }

    pub fn current_thread(&self) -> Option<(&Workspace, &crate::gitlab::Mr, &crate::gitlab::Thread)> {
        let Some(Row::Thread(wi, ti)) = self.current() else { return None };
        let ws = self.snap.workspaces.get(wi)?;
        let mr = ws.mr.as_ref()?;
        Some((ws, mr, mr.threads.get(ti)?))
    }

    pub fn current_review(&self) -> Option<&crate::gitlab::Mr> {
        let Some(Row::Review(i)) = self.current() else { return None };
        self.snap.reviews.get(i)
    }

    /// Check out the selected MR in its own worktree and open it with a window of kind; Claude gets the review prompt.
    fn start_review(&mut self, kind: &'static str) {
        let Some(mr) = self.current_review().cloned() else { return };
        let Some(repo) = crate::gitlab::find_clone(&self.snap.dirs, &mr.project) else {
            let roots = self.cfg.roots.iter().map(|r| shorten(r)).collect::<Vec<_>>().join(", ");
            return self.notify(format!("No clone of {} under {roots}. Clone it there, then Ctrl+R in the n picker.", mr.project), true);
        };
        self.notify(format!("Checking out !{} from {}…", mr.iid, shorten(&repo)), false);
        let _ = self.cmd.send(Cmd::StartReview { mr: Box::new(mr), repo, kind });
    }

    pub fn mine(&self, list: Mine) -> &[crate::gitlab::Mr] {
        match list {
            Mine::Authored => &self.snap.authored,
            Mine::Assigned => &self.snap.assigned,
        }
    }

    pub fn current_mr(&self) -> Option<&crate::gitlab::Mr> {
        let Some(Row::Mr(list, i)) = self.current() else { return None };
        self.mine(list).get(i)
    }

    /// The workspace whose branch has the MR.
    pub fn mr_workspace(&self, m: &crate::gitlab::Mr) -> Option<&Workspace> {
        self.snap.workspaces.iter().find(|w| w.mr.as_ref().is_some_and(|wm| wm.project_id == m.project_id && wm.iid == m.iid))
    }

    pub fn current_issue(&self) -> Option<&crate::gitlab::Issue> {
        let Some(Row::Issue(i)) = self.current() else { return None };
        self.snap.issues.get(i)
    }

    /// Open Claude in the workspace of the issue's local clone, with the issue typed into its prompt.
    fn start_issue(&mut self) {
        let Some(issue) = self.current_issue().cloned() else { return };
        let Some(repo) = crate::gitlab::find_clone(&self.snap.dirs, &issue.project) else {
            let roots = self.cfg.roots.iter().map(|r| shorten(r)).collect::<Vec<_>>().join(", ");
            return self.notify(format!("No clone of {} under {roots}. Clone it there, then Ctrl+R in the n picker.", issue.project), true);
        };
        self.notify(format!("Opening #{} in {}…", issue.iid, shorten(&repo)), false);
        let _ = self.cmd.send(Cmd::StartIssue { issue: Box::new(issue), repo });
    }

    pub fn current_terminal(&self) -> Option<(&Workspace, &crate::snapshot::Window)> {
        let Some(Row::Terminal(wi, ti)) = self.current() else { return None };
        let ws = self.snap.workspaces.get(wi)?;
        Some((ws, ws.windows.get(ti)?))
    }

    /// Approved by you and no workspace open: done, so it goes in the APPROVED group.
    pub fn in_approved(&self, review: usize) -> bool {
        self.snap.reviews[review].approved && self.review_ws.get(review).copied().flatten().is_none()
    }

    fn push_reviews(&self, rows: &mut Vec<(String, Row)>, reviews: Vec<(usize, Vec<usize>, Vec<usize>)>, open: bool) {
        for (i, agents, terminals) in reviews {
            let Some(wi) = self.review_ws[i] else {
                if open {
                    rows.push((review_key(&self.snap.reviews[i]), Row::Review(i)));
                }
                continue;
            };
            rows.push((review_key(&self.snap.reviews[i]), Row::Review(i)));
            rows.extend(agents.into_iter().map(|a| (self.snap.agents[a].key.clone(), Row::Agent(a))));
            rows.extend(terminals.into_iter().map(|t| (format!("term:{}", self.snap.workspaces[wi].windows[t].window_id), Row::Terminal(wi, t))));
        }
    }

    /// The review whose checkout the named workspace holds.
    pub fn review_of(&self, ws: &str) -> Option<usize> {
        self.review_ws.iter().position(|w| w.is_some_and(|wi| self.snap.workspaces[wi].name == ws))
    }

    pub fn review_workspace(&self, review: usize) -> Option<&Workspace> {
        self.review_ws.get(review).copied().flatten().and_then(|wi| self.snap.workspaces.get(wi))
    }

    fn current_workspace(&self) -> Option<&Workspace> {
        let name = match self.current()? {
            Row::Workspace(i) | Row::Terminal(i, _) | Row::Thread(i, _) => return self.snap.workspaces.get(i),
            Row::Review(i) => return self.review_workspace(i),
            Row::Mr(list, i) => return self.mr_workspace(self.mine(list).get(i)?),
            Row::Agent(i) => self.snap.agents[i].workspace.clone()?,
            _ => return None,
        };
        self.snap.workspaces.iter().find(|w| w.name == name)
    }

    fn select(&mut self, index: usize) {
        if index < self.rows.len() && selectable(&self.rows[index].1) && index != self.selected {
            self.selected = index;
            self.detail_scroll = 0;
            self.sel = None;
        }
    }

    /// A lone 1–4 that no j/k followed switches the panel tab, as it did before counts.
    fn flush_count(&mut self) {
        if let Some((n @ 1..=4, _)) = self.count.take() {
            self.act(Action::Tab(n - 1));
        }
    }

    /// Move n selectable rows, so a count skips gaps and never lands between sections.
    fn move_steps(&mut self, dir: isize, n: usize) {
        for _ in 0..n {
            self.move_by(dir);
        }
    }

    /// Jump to the next or previous workspace, review or section title.
    fn jump_group(&mut self, dir: isize) {
        let n = self.rows.len() as isize;
        let mut i = self.selected as isize + dir;
        while (0..n).contains(&i) {
            let r = &self.rows[i as usize].1;
            if matches!(r, Row::Workspace(_) | Row::Review(_) | Row::Header(..)) && selectable(r) {
                return self.select(i as usize);
            }
            i += dir;
        }
    }

    fn move_by(&mut self, delta: isize) {
        let n = self.rows.len() as isize;
        let mut i = (self.selected as isize + delta).clamp(0, (n - 1).max(0));
        while i >= 0 && i < n && !selectable(&self.rows[i as usize].1) {
            i += delta.signum();
        }
        if i >= 0 && i < n {
            self.select(i as usize);
        }
    }

    // ---- actions ----

    /// Footer buttons. Row-specific actions live in the side panel, so they only
    /// show here when the side panel is hidden. The rest are in help.
    pub fn actions(&self, side_visible: bool) -> Vec<(&'static str, &'static str, Action)> {
        let mut v = vec![];
        if !side_visible && self.current_review().is_some() {
            v.push(("↵", "open in GitLab", Action::Open));
            v.push(("c", "review with Claude", Action::NewWindow("claude")));
        }
        if !side_visible && matches!(self.current(), Some(Row::Header(title, _)) if gitlab_section(title).is_some()) {
            v.push(("^R", "refresh", Action::Refresh));
        }
        if !side_visible && let Some(m) = self.current_mr() {
            v.push(("↵", if self.mr_workspace(m).is_some() { "go to workspace" } else { "open in GitLab" }, Action::Open));
        }
        if !side_visible && self.current_issue().is_some() {
            v.push(("↵", "open in GitLab", Action::Open));
            v.push(("c", "work on it with Claude", Action::NewWindow("claude")));
        }
        if !side_visible && self.current_thread().is_some() {
            v.push(("r", "reply", Action::Reply));
            v.push(("v", "resolve", Action::Resolve));
            v.push(("f", "to claude", Action::Forward));
        }
        if !side_visible && let Some(a) = self.current_agent() {
            if a.status == Status::Waiting && a.target.is_some() {
                if a.needs_permission() {
                    v.push(("y", "approve", Action::Approve));
                    v.push(("d", "deny", Action::Deny));
                }
                v.push(("r", "reply", Action::Reply));
            }
            if a.status == Status::Waiting {
                v.push(("m", "done", Action::MarkDone));
            }
            if !a.status.alive() && a.session_id().is_some() {
                v.push(("↵", "resume", Action::Open));
            }
            if a.workspace.is_none() && a.status.alive() && a.pid.is_some() {
                v.push(("a", "adopt", Action::Adopt));
            }
        }
        if self.snap.agents.iter().any(|a| a.status == Status::Waiting) {
            v.push(("w", "next waiting", Action::NextWaiting));
        }
        v.push(("C", "chat", Action::NewChat));
        v.push(("?", "help", Action::Help));
        v.push(("q", "leave", Action::Leave));
        v
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Open => self.open(),
            Action::Refresh => self.refresh_mr(),
            Action::NewWorkspace => self.open_picker(""),
            Action::NewWindow("claude") if self.current_review().is_some() => self.start_review("claude"),
            Action::NewWindow(kind) if self.current_review().is_some() && self.current_workspace().is_none() => self.start_review(kind),
            Action::NewWindow("claude") if self.current_issue().is_some() => self.start_issue(),
            Action::NewWindow(kind) => self.new_window(kind),
            Action::NewChat => self.new_chat(),
            Action::Close => self.close(),
            Action::Rename => {
                if let Some(ws) = self.current_workspace() {
                    let from = ws.name.clone();
                    self.modal = Some(Modal::Rename { input: Input::new(&from), from });
                }
            }
            Action::Forward => self.forward(),
            Action::Fold => self.toggle_fold(),
            Action::FoldAll => self.toggle_fold_all(),
            Action::Resolve => {
                let Some((ws, _, _)) = self.current_thread() else { return };
                let text = crate::gitlab::head(&ws.path).map_or(String::new(), |sha| format!("Fixed in {sha}."));
                self.reply.set(&text);
                self.resolving = true;
                self.focus = Focus::Reply;
            }
            Action::Publish => self.publish(),
            Action::Reply => {
                if self.current_agent().is_some_and(|a| a.target.is_some()) || self.current_thread().is_some() {
                    self.focus = Focus::Reply;
                }
            }
            Action::Approve => self.answer(true),
            Action::Deny => self.answer(false),
            Action::Filter => self.focus = Focus::Filter,
            Action::Type => {
                if self.current_terminal().is_some() {
                    self.focus = Focus::Terminal;
                    self.update_live();
                }
            }
            Action::Help => self.modal = Some(Modal::Help),
            Action::Leave => {
                if tmux::inside_hub_server() {
                    tmux::detach();
                } else {
                    self.quit = true;
                }
            }
            Action::Adopt => self.adopt(),
            Action::MarkDone => self.mark_done(),
            Action::NextWaiting => self.next_waiting(),
            Action::Tab(i) => {
                self.tab = i;
                self.detail_scroll = 0;
                self.sel = None;
            }
            Action::JumpWindow(session, window) => self.finish(tmux::jump_to_window(&session, &window)),
            Action::Confirm(yes) => {
                if let Some(Modal::Confirm { then, done, .. }) = self.modal.take() {
                    if yes {
                        let r = then();
                        if let (Ok(()), Some(text)) = (&r, done) {
                            self.notify(text, false);
                        }
                        self.finish(r);
                    }
                }
            }
        }
    }

    fn finish(&mut self, result: Res) {
        match result {
            Ok(()) => {}
            Err(Fail::Attach(session)) => {
                self.attach_to = Some(session);
                self.quit = true;
            }
            Err(Fail::Error(e)) => self.notify(e, true),
        }
        self.refresh();
    }

    fn open(&mut self) {
        let r = match self.current() {
            Some(Row::Header(..)) => return self.toggle_fold(),
            Some(Row::NewWorkspace) => return self.act(Action::NewWorkspace),
            Some(Row::Agent(i)) => {
                let a = &self.snap.agents[i];
                match (&a.workspace, &a.window_id) {
                    (Some(ws), Some(w)) => tmux::jump_to_window(ws, w),
                    (Some(ws), None) if a.background && a.status.alive() => {
                        let Some(id) = a.session_id().filter(|id| crate::hooks::valid_id(id)) else { return };
                        let name = format!("{}{}", crate::snapshot::ATTACH_PREFIX, &id[..8.min(id.len())]);
                        let open = self.snap.workspaces.iter().find(|w| &w.name == ws).and_then(|w| w.windows.iter().find(|win| win.name == name));
                        match open {
                            Some(win) => tmux::jump_to_window(ws, &win.window_id),
                            // Background agents go by the first 8 characters of their session id.
                            None => tmux::new_window(ws, &a.path, &name, Some(&format!("claude attach {}", &id[..8.min(id.len())])), None),
                        }
                    }
                    _ if !a.status.alive() => match a.session_id() {
                        Some(id) => tmux::resume(id, &a.path),
                        None => return,
                    },
                    _ => {
                        let s = a.session.as_ref().and_then(|s| s.tmux_session.as_ref());
                        let where_ = s.map_or("its own terminal".into(), |s| format!("tmux session '{s}'"));
                        self.notify(format!("This agent runs outside Claude Hub, in {where_}. Switch to it there."), false);
                        return;
                    }
                }
            }
            Some(Row::Workspace(i)) => tmux::switch_client(&self.snap.workspaces[i].name),
            // A review is tried out in a shell; Claude has a row of its own.
            Some(Row::Review(i)) => match self.review_workspace(i) {
                Some(ws) => match ws.windows.iter().find(|w| !w.is_claude() && is_shell(&w.command)) {
                    Some(w) => tmux::jump_to_window(&ws.name, &w.window_id),
                    None => tmux::new_window(&ws.name, &ws.path, "shell", None, tmux::spare_for(&ws.path, None).as_deref()),
                },
                None => {
                    let Some(url) = self.current_review().map(|m| m.url.clone()) else { return };
                    return self.open_url(&url);
                }
            },
            Some(Row::Issue(_)) => {
                let Some(url) = self.current_issue().map(|is| is.url.clone()) else { return };
                return self.open_url(&url);
            }
            Some(Row::Mr(..)) => {
                let Some(m) = self.current_mr() else { return };
                match self.mr_workspace(m) {
                    Some(ws) => tmux::switch_client(&ws.name),
                    None => {
                        let url = m.url.clone();
                        return self.open_url(&url);
                    }
                }
            }
            Some(Row::Thread(..)) => {
                let Some((_, mr, th)) = self.current_thread() else { return };
                let url = format!("{}#note_{}", mr.url, th.notes.first().map_or(0, |n| n.id));
                return self.open_url(&url);
            }
            Some(Row::Terminal(wi, ti)) => {
                let ws = &self.snap.workspaces[wi];
                tmux::jump_to_window(&ws.name, &ws.windows[ti].window_id)
            }
            _ => return,
        };
        self.finish(r);
    }

    /// Open url in a browser on this machine; without a display (over SSH) point to the clickable link instead.
    fn open_url(&mut self, url: &str) {
        let display = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if !(display && std::process::Command::new("xdg-open").arg(url).spawn().is_ok()) {
            self.notify("No browser on this machine. Ctrl+click to open it on yours:", false);
            if let Some(t) = &mut self.toast {
                t.link = Some(url.to_string());
            }
        }
    }

    fn new_window(&mut self, kind: &'static str) {
        let Some(ws) = self.current_workspace() else {
            self.notify("Select a workspace first, or press n for a new one", true);
            return;
        };
        let command = window_command(kind);
        let spare = tmux::spare_for(&ws.path, command);
        let r = tmux::new_window(&ws.name, &ws.path, kind, command, spare.as_deref());
        self.finish(r);
    }

    /// Open Claude in the chats workspace, for questions that belong to no repo.
    fn new_chat(&mut self) {
        let path = self.cfg.chat_dir.clone();
        if let Err(e) = std::fs::create_dir_all(&path) {
            return self.notify(format!("Could not create {}: {e}", shorten(&path)), true);
        }
        let spare = tmux::spare_for(&path, Some("claude"));
        let real = tmux::canonical(&path);
        let r = match self.snap.workspaces.iter().find(|w| tmux::canonical(&w.path) == real) {
            Some(ws) => tmux::new_window(&ws.name, &path, "claude", Some("claude"), spare.as_deref()),
            None => {
                let taken: Vec<String> = self.snap.workspaces.iter().map(|w| w.name.clone()).collect();
                let name = tmux::unique_session_name(&basename(&path), &taken);
                tmux::create_workspace(&name, &path, "claude", Some("claude"), spare.as_deref())
            }
        };
        self.finish(r);
    }

    /// The section waits for its first fetch from GitLab.
    pub fn loading(&self, title: &str) -> bool {
        gitlab_section(title).is_some() && self.snap.inbox_loading && self.filter.text.is_empty()
    }

    /// Why the section's last fetch failed.
    pub fn fetch_error(&self, title: &str) -> Option<&str> {
        let section = gitlab_section(title)?;
        self.snap.inbox_errors.iter().find(|(s, _)| *s == section).map(|(_, e)| e.as_str())
    }

    /// An empty section still shows its title while it loads or when its fetch fails, so you see why it is empty.
    fn placeholder(&self, title: &str) -> bool {
        self.filter.text.is_empty() && (self.loading(title) || self.fetch_error(title).is_some())
    }

    /// A filter shows every match, folded or not.
    pub fn section_open(&self, title: &str) -> bool {
        !foldable(title) || !self.filter.text.is_empty() || self.open_sections.contains(title)
    }

    fn toggle_fold(&mut self) {
        if let Some(Row::Header(title, _)) = self.current() {
            if !self.open_sections.remove(title) {
                self.open_sections.insert(title);
            }
            return self.build_rows();
        }
        if matches!(self.current(), Some(Row::Mr(..))) {
            return;
        }
        let Some(name) = self.current_workspace().map(|w| w.name.clone()) else { return };
        if !self.folded.remove(&name) {
            self.folded.insert(name);
        }
        self.build_rows();
    }

    /// Fold every workspace and section, or unfold them all when all are folded.
    fn toggle_fold_all(&mut self) {
        let names: Vec<String> = self.snap.workspaces.iter().map(|w| w.name.clone()).collect();
        // The workspaces section stays open so that the folded workspaces still show.
        let sections = [REVIEWS, APPROVED, MRS, ASSIGNED, ISSUES, OTHER, CLOSED];
        if sections.iter().all(|t| !self.open_sections.contains(t)) && names.iter().all(|n| self.folded.contains(n)) {
            self.folded.clear();
            self.open_sections.extend(sections);
        } else {
            self.folded.extend(names);
            self.open_sections.retain(|t| !sections.contains(t));
        }
        self.build_rows();
    }

    /// Clear "waiting for you" on the selected agent, or on every agent of the selected workspace.
    fn mark_done(&mut self) {
        let agents = match self.current() {
            Some(Row::Agent(i)) => vec![i],
            Some(Row::Workspace(wi)) => self.agents_of(&self.snap.workspaces[wi]),
            Some(Row::Review(i)) => self.review_workspace(i).map(|ws| self.agents_of(ws)).unwrap_or_default(),
            _ => vec![],
        };
        let sessions: Vec<_> = agents.iter().filter_map(|&i| self.snap.agents[i].session.as_ref()).filter(|s| s.status == Status::Waiting).collect();
        if sessions.is_empty() {
            self.notify("Nothing here is waiting for you", false);
            return;
        }
        let failed = sessions.iter().filter(|s| s.mark_done().is_err()).count();
        if failed > 0 {
            self.notify(format!("Could not update {failed} agent(s)"), true);
        }
        self.refresh();
    }

    /// Select the next agent waiting for you, after the selected row, wrapping around.
    fn next_waiting(&mut self) {
        let n = self.rows.len();
        let waiting = |i: usize| matches!(self.rows[i].1, Row::Agent(a) if self.snap.agents[a].status == Status::Waiting);
        match (1..=n).map(|d| (self.selected + d) % n).find(|&i| waiting(i)) {
            Some(i) if i != self.selected => self.select(i),
            Some(_) => {}
            None => self.notify("Nothing is waiting for you", false),
        }
    }

    fn adopt(&mut self) {
        let Some(a) = self.current_agent().filter(|a| a.workspace.is_none() && a.status.alive()) else {
            self.notify("Only agents outside Claude Hub can be adopted", true);
            return;
        };
        let (Some(pid), Some(id)) = (a.pid, a.session_id().map(String::from)) else {
            self.notify("Can't find this agent's process. Adopting needs a recent Claude Code.", true);
            return;
        };
        let path = a.path.clone();
        let cut = if matches!(a.status, Status::Working | Status::Waiting) { " It is in the middle of a turn, which gets cut off." } else { "" };
        let question = format!("Adopt '{}'? It stops where it runs now and continues in a Claude Hub workspace for {}.{cut}", a.title(), shorten(&path));
        self.modal = Some(Modal::Confirm { question, then: Box::new(move || tmux::adopt(pid, &id, &path)), yes: false, done: None });
    }

    fn close(&mut self) {
        let row = match self.current() {
            Some(Row::Review(i)) => self.review_ws.get(i).copied().flatten().map(Row::Workspace),
            r => r,
        };
        let (question, then): (String, Box<dyn Fn() -> Res>) = match row {
            Some(Row::Agent(i)) => {
                let a = &self.snap.agents[i];
                if a.background && a.status.alive() {
                    let Some(id) = a.session_id().filter(|id| crate::hooks::valid_id(id)).map(|id| id[..8.min(id.len())].to_string()) else { return };
                    let stop = move || match std::process::Command::new("claude").args(["stop", &id]).output() {
                        Ok(o) if o.status.success() => Ok(()),
                        Ok(o) => Err(Fail::Error(format!("claude stop: {}", String::from_utf8_lossy(&o.stderr).trim()))),
                        Err(e) => Err(Fail::Error(format!("claude stop: {e}"))),
                    };
                    self.modal = Some(Modal::Confirm { question: format!("Stop background agent '{}'? This ends its Claude session.", a.title()), then: Box::new(stop), yes: false, done: None });
                    return;
                }
                let (Some(ws), Some(w)) = (a.workspace.clone(), a.window_id.clone()) else {
                    self.notify("This agent runs outside Claude Hub. Close it in its own terminal, or press a to adopt it.", true);
                    return;
                };
                (format!("Close agent '{}' in '{ws}'? This ends its Claude session.", a.title()), Box::new(move || tmux::kill_window(&w)))
            }
            Some(Row::Workspace(i)) => {
                let ws = &self.snap.workspaces[i];
                let (name, path) = (ws.name.clone(), ws.path.clone());
                if std::path::Path::new(&path).starts_with(crate::gitlab::reviews_dir()) {
                    let close = move || {
                        tmux::kill_session(&name)?;
                        crate::gitlab::remove_checkout(&path).map_err(Fail::Error)
                    };
                    (format!("Close review workspace '{}' and delete its checkout of the merge request?", ws.name), Box::new(close))
                } else {
                    (format!("Close workspace '{name}' and all its {} window(s)?", ws.windows.len()), Box::new(move || tmux::kill_session(&name)))
                }
            }
            Some(Row::Terminal(wi, ti)) => {
                let ws = &self.snap.workspaces[wi];
                let w = ws.windows[ti].window_id.clone();
                (format!("Close terminal '{}' in '{}'?", ws.windows[ti].name, ws.name), Box::new(move || tmux::kill_window(&w)))
            }
            _ => return,
        };
        // Start on No, so a stray Enter never closes anything.
        self.modal = Some(Modal::Confirm { question, then, yes: false, done: None });
    }

    /// Keys that answer Claude Code's permission dialog: Enter takes the highlighted "Yes", Escape declines.
    fn answer(&mut self, approve: bool) {
        let Some(a) = self.current_agent().filter(|a| a.needs_permission()) else { return };
        let (what, key) = if approve { ("Approved", "Enter") } else { ("Denied", "Escape") };
        self.send(a.clone(), what, |sock, pane| tmux::send_key(sock, pane, key));
    }

    fn send(&mut self, a: Agent, what: &str, f: impl Fn(&str, &str) -> Result<(), String>) {
        let Some((sock, pane)) = &a.target else {
            self.notify("The hub does not know where this agent reads its input.", true);
            return;
        };
        match f(sock, pane) {
            Ok(()) => self.notify(format!("{what} → {}", a.title()), false),
            Err(e) => self.notify(e, true),
        }
        self.refresh();
    }

    /// y/n answers a permission prompt; anything else is typed to the agent.
    fn send_reply(&mut self, text: String) {
        if self.current_thread().is_some() {
            return self.thread_note(text, false);
        }
        let Some(a) = self.current_agent().cloned() else { return };
        let lower = text.to_lowercase();
        if a.needs_permission() && matches!(lower.as_str(), "y" | "yes") {
            self.answer(true);
        } else if a.needs_permission() && matches!(lower.as_str(), "n" | "no" | "d") {
            self.answer(false);
        } else {
            self.sent.insert(a.key.clone(), (text.clone(), Instant::now()));
            self.send(a, "Sent", |sock, pane| tmux::send_text(sock, pane, &text));
        }
    }

    /// Post the text in the selected thread, if any, and mark the thread resolved when asked. GitLab is slow, so it runs on the collector side.
    fn thread_note(&mut self, text: String, resolve: bool) {
        let Some((ws, mr, th)) = self.current_thread() else { return };
        let done = if resolve { format!("Resolved the thread at {} on !{}", th.place(), mr.iid) } else { format!("Replied on !{} at {}", mr.iid, th.place()) };
        let cmd = Cmd::ThreadNote { path: ws.path.clone(), mr: Box::new(mr.clone()), thread: th.id.clone(), text, resolve, done };
        let _ = self.cmd.send(cmd);
        self.notify(if resolve { "Resolving the thread…" } else { "Posting the reply…" }, false);
    }

    /// Push the workspace's branch after asking, and open a draft merge request when it has none.
    fn publish(&mut self) {
        let Some(ws) = self.current_workspace() else { return };
        let Some(branch) = ws.git.as_ref().and_then(|g| g.branch.clone()) else {
            self.notify("This workspace is not on a git branch", true);
            return;
        };
        let open_mr = ws.mr.is_none() && !matches!(branch.as_str(), "main" | "master");
        let what = if open_mr { " and open a draft merge request" } else { "" };
        let (cmd, path) = (self.cmd.clone(), ws.path.clone());
        let then = move || {
            let _ = cmd.send(Cmd::Publish { path: path.clone(), open_mr });
            Ok(())
        };
        self.modal = Some(Modal::Confirm { question: format!("Push {branch}{what}?"), then: Box::new(then), yes: false, done: Some(format!("Pushing {branch}…")) });
    }

    /// Fetch the selected workspace's merge request now instead of waiting for the next round.
    fn refresh_mr(&mut self) {
        let section = match self.current() {
            Some(Row::Review(_) | Row::Header(APPROVED, _)) => Some(Section::GitlabReviews),
            Some(Row::Issue(_)) => Some(Section::GitlabIssues),
            Some(Row::Mr(Mine::Authored, _)) => Some(Section::GitlabMergeRequests),
            Some(Row::Mr(Mine::Assigned, _)) => Some(Section::GitlabAssignedMergeRequests),
            Some(Row::Header(title, _)) => gitlab_section(title),
            _ => None,
        };
        if let Some(section) = section {
            let _ = self.cmd.send(Cmd::ForgetInbox(section));
            return self.notify(format!("Refreshing {}…", section.title()), false);
        }
        let Some(ws) = self.current_workspace() else { return };
        let (path, text) = (ws.path.clone(), format!("Refreshing the merge request of {}…", ws.name));
        let _ = self.cmd.send(Cmd::ForgetMr(path));
        self.notify(text, false);
    }

    /// Type the selected review thread into the workspace's Claude, preferring one that is free, and go there.
    /// It does not press Enter: you edit the prompt or send it yourself.
    fn forward(&mut self) {
        let Some((ws, mr, th)) = self.current_thread() else { return };
        let prompt = th.as_prompt(mr);
        let agents = self.agents_of(ws);
        let pick = agents.iter().map(|&i| &self.snap.agents[i]).filter(|a| a.target.is_some() && a.status.alive()).min_by_key(|a| a.status != Status::Idle).cloned();
        match pick {
            Some(a) => {
                let (Some((sock, pane)), Some(ws), Some(w)) = (&a.target, &a.workspace, &a.window_id) else { return };
                match tmux::type_text(sock, pane, &prompt) {
                    Ok(()) => self.finish(tmux::jump_to_window(ws, w)),
                    Err(e) => self.notify(e, true),
                }
            }
            None => self.notify("No Claude in this workspace. Press c to start one, then f again.", true),
        }
    }

    fn open_picker(&mut self, query: &str) {
        self.modal = Some(Modal::Picker(Picker::new(picker::candidates(&self.snap, &self.cfg.roots), query)));
        self.queue_prewarm();
    }

    fn queue_prewarm(&mut self) {
        if let Some(Modal::Picker(p)) = &self.modal {
            // Start a shell for the highlighted directory while the user decides, so Enter opens it at once.
            self.prewarm_pending = p.selected_path().map(|path| (Instant::now(), path));
        }
    }

    fn choose(&mut self, kind: &'static str) {
        let Some(Modal::Picker(p)) = &self.modal else { return };
        let Some(path) = p.selected_path() else { return };
        self.modal = None;
        self.prewarm_pending = None;
        let _ = self.cmd.send(Cmd::Prewarm(None));
        let real = tmux::canonical(&path);
        if let Some(ws) = self.snap.workspaces.iter().find(|w| tmux::canonical(&w.path) == real) {
            let r = tmux::switch_client(&ws.name);
            return self.finish(r);
        }
        let command = window_command(kind);
        let spare = tmux::spare_for(&path, command);
        let taken: Vec<String> = self.snap.workspaces.iter().map(|w| w.name.clone()).collect();
        let name = tmux::unique_session_name(&basename(&path), &taken);
        let r = tmux::create_workspace(&name, &path, kind, command, spare.as_deref());
        self.finish(r);
    }

    /// Pick up a request from `claude-hub <query>` run in another terminal.
    fn check_request(&mut self) {
        let path = tmux::request_file();
        let Ok(query) = std::fs::read_to_string(&path) else { return };
        let _ = std::fs::remove_file(&path);
        if self.modal.is_none() {
            self.open_picker(query.trim());
        }
    }

    // ---- input ----

    fn on_key(&mut self, k: KeyEvent) {
        if self.modal.is_some() {
            return self.on_modal_key(k);
        }
        match self.focus {
            Focus::Filter => {
                match k.code {
                    KeyCode::Esc => {
                        self.filter = Input::default();
                        self.focus = Focus::List;
                    }
                    KeyCode::Enter | KeyCode::Down | KeyCode::Up => self.focus = Focus::List,
                    _ => {}
                }
                if self.focus == Focus::Filter && self.filter.handle(k) {
                    self.build_rows();
                } else if self.focus == Focus::List {
                    self.build_rows();
                }
                return;
            }
            Focus::Reply => {
                match k.code {
                    KeyCode::Esc => {
                        self.focus = Focus::List;
                        if std::mem::take(&mut self.resolving) {
                            self.reply = Input::default();
                        }
                    }
                    KeyCode::Enter => {
                        let text = std::mem::take(&mut self.reply).text.trim().to_string();
                        self.focus = Focus::List;
                        if std::mem::take(&mut self.resolving) {
                            self.thread_note(text, true);
                        } else if !text.is_empty() {
                            self.send_reply(text);
                        }
                    }
                    _ => {
                        self.reply.handle(k);
                    }
                }
                return;
            }
            Focus::Terminal => return self.type_key(k),
            Focus::List => {}
        }
        let page = self.hits.list.height.max(2) as isize - 1;
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if let KeyCode::Char(c @ '0'..='9') = k.code {
            if plain && (c != '0' || self.count.is_some()) {
                let n = self.count.map_or(0, |(n, _)| n).saturating_mul(10).saturating_add(c as usize - '0' as usize);
                self.count = Some((n.min(9999), Instant::now()));
                return;
            }
        }
        let n = match k.code {
            KeyCode::Down | KeyCode::Up | KeyCode::Char('j' | 'k' | 'J' | 'K') => self.count.take().map_or(1, |(n, _)| n),
            _ => {
                self.flush_count();
                1
            }
        };
        match k.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_steps(1, n),
            KeyCode::Up | KeyCode::Char('k') => self.move_steps(-1, n),
            KeyCode::Char('J') => (0..n).for_each(|_| self.jump_group(1)),
            KeyCode::Char('K') => (0..n).for_each(|_| self.jump_group(-1)),
            KeyCode::Char('d') if k.modifiers.contains(KeyModifiers::CONTROL) => self.move_by(page / 2 + 1),
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => self.move_by(-(page / 2 + 1)),
            KeyCode::Char('g') | KeyCode::Home => self.move_by(-(self.rows.len() as isize)),
            KeyCode::Char('G') | KeyCode::End => self.move_by(self.rows.len() as isize),
            KeyCode::Char('f') if k.modifiers.contains(KeyModifiers::CONTROL) => self.move_by(page),
            KeyCode::Char('r') if k.modifiers.contains(KeyModifiers::CONTROL) => self.refresh_mr(),
            KeyCode::Char('b') if k.modifiers.contains(KeyModifiers::CONTROL) => self.move_by(-page),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(self.hits.detail.height.saturating_sub(2)),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(self.hits.detail.height.saturating_sub(2)),
            KeyCode::Esc if !self.filter.text.is_empty() => {
                self.filter = Input::default();
                self.build_rows();
            }
            KeyCode::Enter => self.act(Action::Open),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.act(Action::Tab((self.tab + 1) % 4)),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.act(Action::Tab((self.tab + 3) % 4)),
            KeyCode::Char('n') => self.act(Action::NewWorkspace),
            KeyCode::Char('c') => self.act(Action::NewWindow("claude")),
            KeyCode::Char('C') => self.act(Action::NewChat),
            KeyCode::Char('t') => self.act(Action::NewWindow("shell")),
            KeyCode::Char('e') => self.act(Action::NewWindow("nvim")),
            KeyCode::Char('x') => self.act(Action::Close),
            KeyCode::Char('R') => self.act(Action::Rename),
            KeyCode::Char('a') => self.act(Action::Adopt),
            KeyCode::Char('m') => self.act(Action::MarkDone),
            KeyCode::Char('w') => self.act(Action::NextWaiting),
            KeyCode::Char('f') => self.act(Action::Forward),
            KeyCode::Char('v') => self.act(Action::Resolve),
            KeyCode::Char('p') => self.act(Action::Publish),
            KeyCode::Char('r') => self.act(Action::Reply),
            KeyCode::Char('y') => self.act(Action::Approve),
            KeyCode::Char('d') => self.act(Action::Deny),
            KeyCode::Char(' ') => self.act(Action::Fold),
            KeyCode::Char('z') => self.act(Action::FoldAll),
            KeyCode::Char('/') => self.act(Action::Filter),
            KeyCode::Char('i') => self.act(Action::Type),
            KeyCode::Char('?') => self.act(Action::Help),
            KeyCode::Char('q') => self.act(Action::Leave),
            _ => {}
        }
    }

    /// Ctrl+] gives the keyboard back to the hub; every other key goes to the terminal.
    fn type_key(&mut self, k: KeyEvent) {
        // Legacy terminals report Ctrl+] as Ctrl+5.
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char(']' | '5')) {
            self.focus = Focus::List;
            return;
        }
        let Some(pane) = self.current_terminal().map(|(_, w)| w.pane_id.clone()) else { return };
        let Some(key) = tmux::key_for(k) else { return };
        let r = match key {
            Ok(text) => tmux::type_text(&tmux::socket_path(), &pane, &text),
            Err(name) => tmux::send_key(&tmux::socket_path(), &pane, &name),
        };
        if let Err(e) = r {
            self.notify(e, true);
        }
    }

    fn on_modal_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match self.modal.as_mut().unwrap() {
            Modal::Help => self.modal = None,
            Modal::Confirm { yes, .. } => match k.code {
                KeyCode::Char('y') => self.act(Action::Confirm(true)),
                KeyCode::Char('n') | KeyCode::Esc => self.act(Action::Confirm(false)),
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Char('l') => *yes = !*yes,
                KeyCode::Enter => {
                    let y = *yes;
                    self.act(Action::Confirm(y));
                }
                _ => {}
            },
            Modal::Rename { from, input } => match k.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter => {
                    let (from, new) = (from.clone(), input.text.trim().to_string());
                    self.modal = None;
                    if !new.is_empty() && new != from {
                        let new = tmux::unique_session_name(&new, &tmux::list_sessions());
                        if self.folded.remove(&from) {
                            self.folded.insert(new.clone());
                        }
                        let r = tmux::rename_session(&from, &new);
                        self.finish(r);
                    }
                }
                _ => {
                    input.handle(k);
                }
            },
            Modal::Picker(p) => {
                match k.code {
                    KeyCode::Esc => {
                        self.modal = None;
                        self.prewarm_pending = None;
                        let _ = self.cmd.send(Cmd::Prewarm(None));
                        return;
                    }
                    KeyCode::Enter => return self.choose("shell"),
                    KeyCode::Char('o') if ctrl => return self.choose("claude"),
                    KeyCode::Char('r') if ctrl => {
                        self.rescanning = true;
                        let _ = self.cmd.send(Cmd::Rescan);
                        self.notify("Scanning for repos…", false);
                        return;
                    }
                    KeyCode::Down => p.move_by(1),
                    KeyCode::Char('n' | 'j') if ctrl => p.move_by(1),
                    KeyCode::Up => p.move_by(-1),
                    KeyCode::Char('p' | 'k') if ctrl => p.move_by(-1),
                    KeyCode::PageDown => p.move_by(10),
                    KeyCode::PageUp => p.move_by(-10),
                    KeyCode::Tab => p.complete(),
                    KeyCode::Right if p.input.at_end() => {
                        p.accept_suggestion();
                    }
                    _ => {
                        let before = p.input.text.clone();
                        p.input.handle(k);
                        if p.input.text != before {
                            p.update();
                        }
                    }
                }
                self.queue_prewarm();
            }
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        if !self.cfg.mouse {
            return;
        }
        let pos = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Moved => {
                let l = self.hits.list;
                self.hover = (self.modal.is_none() && l.contains(pos)).then(|| self.offset + (m.row - l.y) as usize);
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                if let Some(Modal::Picker(p)) = &mut self.modal {
                    p.move_by(if down { 3 } else { -3 });
                    self.queue_prewarm();
                } else if self.hits.detail.contains(pos) {
                    self.detail_scroll = if down { self.detail_scroll.saturating_add(3) } else { self.detail_scroll.saturating_sub(3) };
                } else if self.hits.list.contains(pos) {
                    let max = self.rows.len().saturating_sub(self.hits.list.height as usize);
                    self.offset = if down { (self.offset + 3).min(max) } else { self.offset.saturating_sub(3) };
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let d = self.hits.detail;
                self.sel = (self.modal.is_none() && d.contains(pos)).then(|| {
                    let at = (pos.y - d.y + self.detail_scroll, pos.x - d.x);
                    Selection { from: at, to: at, copy: false }
                });
                self.on_click(pos);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let d = self.hits.detail;
                if let Some(sel) = &mut self.sel {
                    let y = pos.y.clamp(d.y, d.bottom().saturating_sub(1)) - d.y;
                    let x = pos.x.clamp(d.x, d.right().saturating_sub(1)) - d.x;
                    sel.to = (y + self.detail_scroll, x);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => match &mut self.sel {
                Some(sel) if sel.from != sel.to => sel.copy = true,
                _ => self.sel = None,
            },
            _ => {}
        }
    }

    fn on_click(&mut self, pos: Position) {
        // A click anywhere but the screen leaves typing; the screen's own button turns it back on.
        if self.focus == Focus::Terminal {
            self.focus = Focus::List;
        }
        if let Some((_, action)) = self.hits.buttons.iter().find(|(r, _)| r.contains(pos)).cloned() {
            return self.act(action);
        }
        // Click selects, a click on the selected row (or a double-click) opens it.
        let double = |last: Option<(Instant, usize)>, i: usize| last.is_some_and(|(t, j)| j == i && t.elapsed() < Duration::from_millis(400));
        match &mut self.modal {
            Some(Modal::Picker(p)) => {
                let l = self.hits.picker_list;
                if l.contains(pos) {
                    let i = p.offset + (pos.y - l.y) as usize;
                    if i < p.options.len() {
                        if i == p.selected || double(self.last_click, i) {
                            return self.choose("shell");
                        }
                        p.selected = i;
                        self.last_click = Some((Instant::now(), i));
                        self.queue_prewarm();
                    }
                }
            }
            Some(Modal::Help) => self.modal = None,
            Some(_) => {}
            None => {
                let l = self.hits.list;
                if l.contains(pos) {
                    self.focus = Focus::List;
                    let i = self.offset + (pos.y - l.y) as usize;
                    if i < self.rows.len() && selectable(&self.rows[i].1) {
                        // The ↻ at the right end of a GitLab title.
                        if matches!(self.rows[i].1, Row::Header(title, _) if gitlab_section(title).is_some()) && pos.x + 3 >= l.right() {
                            self.select(i);
                            return self.act(Action::Refresh);
                        }
                        let foldable_row = match self.rows[i].1 {
                            Row::Workspace(_) | Row::Header(..) => true,
                            Row::Review(r) => self.review_workspace(r).is_some(),
                            _ => false,
                        };
                        let indent = if matches!(self.rows[i].1, Row::Header(APPROVED, _)) { 2 } else { 0 };
                        if foldable_row && (l.x + 1 + indent..=l.x + 2 + indent).contains(&pos.x) {
                            self.select(i);
                            return self.act(Action::Fold);
                        }
                        if i == self.selected || double(self.last_click, i) || self.rows[i].1 == Row::NewWorkspace {
                            self.last_click = None;
                            self.select(i);
                            return self.act(Action::Open);
                        }
                        self.select(i);
                        self.last_click = Some((Instant::now(), i));
                    }
                }
            }
        }
    }
}

const COUNT_TIMEOUT: Duration = Duration::from_millis(400);
const WORKSPACES: &str = Section::Workspaces.title();
const REVIEWS: &str = Section::GitlabReviews.title();
pub const APPROVED: &str = "APPROVED";
const MRS: &str = Section::GitlabMergeRequests.title();
const ASSIGNED: &str = Section::GitlabAssignedMergeRequests.title();
const ISSUES: &str = Section::GitlabIssues.title();
const OTHER: &str = Section::OtherSessions.title();
const CLOSED: &str = Section::ClosedSessions.title();
const FOLDABLE: [&str; 8] = [WORKSPACES, REVIEWS, APPROVED, MRS, ASSIGNED, ISSUES, OTHER, CLOSED];
const GITLAB: [Section; 4] = [Section::GitlabReviews, Section::GitlabMergeRequests, Section::GitlabAssignedMergeRequests, Section::GitlabIssues];

fn is_shell(command: &str) -> bool {
    matches!(command, "bash" | "zsh" | "fish" | "sh" | "dash" | "ksh" | "nu")
}

pub fn gitlab_section(title: &str) -> Option<Section> {
    GITLAB.into_iter().find(|s| s.title() == title)
}

fn review_key(m: &crate::gitlab::Mr) -> String {
    format!("review:{}!{}", m.project, m.iid)
}

fn section_key(title: &str) -> String {
    format!("section:{title}")
}

pub fn foldable(title: &str) -> bool {
    FOLDABLE.contains(&title)
}

pub fn selectable(r: &Row) -> bool {
    match r {
        Row::Gap => false,
        Row::Header(title, _) => foldable(title),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{MouseButton, MouseEvent};

    #[test]
    fn a_click_on_the_refresh_mark_fetches_the_gitlab_lists_again() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(Config::default(), tx, Default::default());
        app.snap.inbox_errors = vec![(Section::GitlabIssues, "timed out".into())];
        app.build_rows();
        app.hits.list = Rect::new(1, 1, 60, 20);
        let y = app.rows.iter().position(|(_, r)| *r == Row::Header(ISSUES, 0)).unwrap() as u16 + 1;
        let click = |column| MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row: y, modifiers: KeyModifiers::NONE };
        app.on_mouse(click(30));
        assert!(rx.try_recv().is_err(), "a click on the title only selects it");
        app.on_mouse(click(59));
        assert!(matches!(rx.try_recv(), Ok(Cmd::ForgetInbox(Section::GitlabIssues))));
    }
}
