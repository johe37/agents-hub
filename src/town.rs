//! `claude-hub town`: the hub's snapshot drawn as a pixel art town in the browser.
//!
//! A small HTTP server on 127.0.0.1 serves one page and pushes a JSON model of the town over
//! server-sent events. The page can answer agents (reply, approve, deny) through the same
//! functions the TUI uses. Every request but the static files needs the random token that is
//! only in the URL opened at start, and a Host (and for POST an Origin) of this server, so other
//! web pages in the browser cannot read the town or act on agents.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::app::Action;
use crate::config::{Config, shorten, state_dir};
use crate::git::{GitCache, GitStatus};
use crate::messages::{Message, resolve_target};
use crate::snapshot::{Agent, Cmd, Snapshot, Workspace, basename};
use crate::state::{Status, now};
use crate::theme::{colors, css, lerp};
use crate::ui::{Side, agent_side, reply_placeholder, window_lines, workspace_side};

const PAGE: &str = include_str!("town/town.html");
const SCRIPT: &str = include_str!("town/town.js");
const MAX_BODY: usize = 64 * 1024;
const MAX_REPLY: usize = 4000;
/// Text columns in the panel body (46em wide in town.html), for the chat cards.
const PANEL_COLUMNS: u16 = 72;
/// Lots per row of the town grid.
const COLUMNS: u64 = 4;
/// Messages younger than this are sent to the page; it plays each one once.
const MESSAGE_WINDOW: u64 = 600;

pub const USAGE: &str = "usage: claude-hub town [--port N] [--no-open] [--demo]

Serve the hub as a pixel art town on http://127.0.0.1 and open it in the browser.

  --port N    listen on port N instead of a free one
  --no-open   print the URL instead of opening the browser
  --demo      show a made-up town; replies and approvals are disabled";

struct Latest {
    version: u64,
    json: String,
    snap: Option<Snapshot>,
}

struct Shared {
    latest: Mutex<Latest>,
    changed: Condvar,
    token: String,
    port: u16,
    cmd: Option<Mutex<Sender<Cmd>>>,
}

impl Shared {
    fn publish(&self, model: &Value, snap: Option<Snapshot>) {
        let json = model.to_string();
        let mut l = self.latest.lock().unwrap();
        if l.json != json {
            l.version += 1;
            l.json = json;
        }
        l.snap = snap;
        self.changed.notify_all();
    }
}

pub fn run(cfg: Config, args: &[String]) -> std::process::ExitCode {
    use std::process::ExitCode;
    let mut port = 0u16;
    let (mut open, mut demo) = (true, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => match it.next().and_then(|p| p.parse().ok()) {
                Some(p) => port = p,
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            "--no-open" => open = false,
            "--demo" => demo = true,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("claude-hub: cannot listen on 127.0.0.1:{port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let Some(token) = random_token() else {
        eprintln!("claude-hub: cannot read /dev/urandom");
        return ExitCode::FAILURE;
    };

    let mut lots = Lots::load(!demo);
    let started = std::time::Instant::now();
    let shared = if demo {
        let shared = Arc::new(Shared::new(token, port, None));
        let s = shared.clone();
        std::thread::spawn(move || {
            let git = GitCache::default();
            loop {
                let (snap, messages) = demo_snapshot();
                s.publish(&model(&snap, &cfg, &mut lots, &git, &messages, true, started.elapsed().as_millis()), None);
                std::thread::sleep(Duration::from_secs(2));
            }
        });
        shared
    } else {
        let (cmd, snaps) = crate::snapshot::spawn(cfg.clone(), Default::default(), false);
        let shared = Arc::new(Shared::new(token, port, Some(cmd)));
        let s = shared.clone();
        std::thread::spawn(move || {
            let git = GitCache::default();
            let mut last: Option<Snapshot> = None;
            let mut seen = mtime(&crate::messages::dir());
            loop {
                // A new snapshot, or a new message between snapshots: rebuild the town.
                match snaps.recv_timeout(Duration::from_millis(300)) {
                    Ok(snap) => last = Some(snap),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        let m = mtime(&crate::messages::dir());
                        if m == seen {
                            continue;
                        }
                        seen = m;
                    }
                    Err(_) => return,
                }
                let Some(snap) = &last else { continue };
                let messages = crate::messages::recent(MESSAGE_WINDOW);
                let model = model(snap, &cfg, &mut lots, &git, &messages, false, started.elapsed().as_millis());
                s.publish(&model, Some(snap.clone()));
            }
        });
        shared
    };

    let url = format!("http://127.0.0.1:{port}/#{}", shared.token);
    println!("Town at {url}\nPress Ctrl+C to stop.");
    if open {
        let _ = std::process::Command::new("xdg-open")
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    for stream in listener.incoming().flatten() {
        let s = shared.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &s);
        });
    }
    ExitCode::SUCCESS
}

impl Shared {
    fn new(token: String, port: u16, cmd: Option<Sender<Cmd>>) -> Self {
        Shared {
            latest: Mutex::new(Latest { version: 0, json: String::new(), snap: None }),
            changed: Condvar::new(),
            token,
            port,
            cmd: cmd.map(Mutex::new),
        }
    }
}

fn mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn random_token() -> Option<String> {
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom").ok()?.read_exact(&mut buf).ok()?;
    Some(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compares in time independent of where the strings differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---------------------------------------------------------------- HTTP

struct Request {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut r = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = HashMap::new();
    let mut size = line.len();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 {
            return None;
        }
        size += h.len();
        if size > 16 * 1024 {
            return None;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let len: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return None;
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    let (path, query) = target.split_once('?').map_or((target.clone(), String::new()), |(p, q)| (p.into(), q.into()));
    Some(Request { method, path, query, headers, body })
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)
}

fn json_reply(stream: &mut TcpStream, status: &str, v: Value) -> std::io::Result<()> {
    respond(stream, status, "application/json", v.to_string().as_bytes())
}

fn handle(mut stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    if !stream.peer_addr()?.ip().is_loopback() {
        return Ok(());
    }
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let Some(req) = read_request(&stream) else { return respond(&mut stream, "400 Bad Request", "text/plain", b"bad request") };
    // A page on another site can point a name it controls at 127.0.0.1 (DNS rebinding); its requests carry that name.
    let hosts = [format!("127.0.0.1:{}", shared.port), format!("localhost:{}", shared.port)];
    if !req.headers.get("host").is_some_and(|h| hosts.contains(h)) {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"wrong host");
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/town.js") => respond(&mut stream, "200 OK", "text/javascript; charset=utf-8", SCRIPT.as_bytes()),
        ("GET", "/events") => {
            let token = req.query.split('&').find_map(|kv| kv.strip_prefix("t=")).unwrap_or("");
            if !same(token, &shared.token) {
                return respond(&mut stream, "403 Forbidden", "text/plain", b"bad token");
            }
            events(stream, shared)
        }
        ("POST", "/api/act") => {
            let origins = hosts.map(|h| format!("http://{h}"));
            let origin_ok = req.headers.get("origin").is_some_and(|o| origins.contains(o));
            let token_ok = req.headers.get("x-town-token").is_some_and(|t| same(t, &shared.token));
            if !origin_ok || !token_ok {
                return json_reply(&mut stream, "403 Forbidden", json!({"error": "Only the town page on this machine can do that."}));
            }
            let (status, v) = act(shared, &req.body);
            json_reply(&mut stream, status, v)
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// Server-sent events: the model now, then again whenever it changes.
fn events(mut stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
    )?;
    let mut seen = 0;
    loop {
        let msg = {
            let mut l = shared.latest.lock().unwrap();
            if l.version == seen {
                l = shared.changed.wait_timeout(l, Duration::from_secs(15)).unwrap().0;
            }
            if l.version != seen && !l.json.is_empty() {
                seen = l.version;
                format!("data: {}\n\n", l.json)
            } else {
                ": ping\n\n".into()
            }
        };
        stream.write_all(msg.as_bytes())?;
        stream.flush()?;
    }
}

/// Reply, approve or deny, checked against the newest snapshot.
fn act(shared: &Shared, body: &[u8]) -> (&'static str, Value) {
    let err = |status, msg: &str| (status, json!({"error": msg}));
    let Some(cmd) = &shared.cmd else { return err("403 Forbidden", "The demo town cannot act on agents.") };
    let Ok(req) = serde_json::from_slice::<Value>(body) else { return err("400 Bad Request", "Bad request.") };
    let agent = {
        let l = shared.latest.lock().unwrap();
        l.snap.as_ref().and_then(|s| s.agents.iter().find(|a| req["key"] == a.key.as_str()).cloned())
    };
    let Some(a) = agent else { return err("404 Not Found", "This agent is gone.") };
    // m in the hub: stop showing it as waiting. Needs no pane.
    if req["op"] == "done" {
        let Some(sess) = a.session.as_ref().filter(|s| s.status == Status::Waiting) else { return err("409 Conflict", "Nothing here is waiting for you.") };
        let _ = cmd.lock().unwrap().send(Cmd::Refresh);
        return match sess.mark_done() {
            Ok(()) => ("200 OK", json!({"ok": format!("Marked done → {}", a.title())})),
            Err(e) => err("500 Internal Server Error", &e.to_string()),
        };
    }
    if !a.status.alive() || a.target.is_none() {
        return err("409 Conflict", a.read_only.as_deref().unwrap_or("This agent does not take input."));
    }
    // The page shows one tool call; never answer a newer prompt the user has not seen.
    if a.needs_permission() && req["ts"].as_u64() != Some(a.timestamp()) {
        return err("409 Conflict", "The agent asks something new. Look again before you answer.");
    }
    if matches!(req["op"].as_str(), Some("approve" | "deny")) && !a.needs_permission() {
        return err("409 Conflict", "This agent does not ask for permission now.");
    }
    let result = match req["op"].as_str() {
        Some("approve") => a.answer(true),
        Some("deny") => a.answer(false),
        Some("reply") => {
            let text: String = req["text"].as_str().unwrap_or("").chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
            let text = text.trim();
            if text.is_empty() || text.chars().count() > MAX_REPLY {
                return err("400 Bad Request", "Write a reply (at most 4000 characters).");
            }
            a.respond(text)
        }
        _ => return err("400 Bad Request", "Unknown action."),
    };
    let _ = cmd.lock().unwrap().send(Cmd::Refresh);
    match result {
        Ok(what) => ("200 OK", json!({"ok": format!("{what} → {}", a.title())})),
        Err(e) => err("500 Internal Server Error", &e),
    }
}

// ---------------------------------------------------------------- Lots

/// Where each house stands. Saved, so a new house never pushes an old one to another lot.
struct Lots {
    lots: HashMap<String, u64>,
    save: bool,
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

impl Lots {
    fn path() -> std::path::PathBuf {
        state_dir().join("town.json")
    }

    fn load(save: bool) -> Self {
        let lots = save
            .then(|| std::fs::read(Self::path()).ok())
            .flatten()
            .and_then(|b| serde_json::from_slice::<HashMap<String, u64>>(&b).ok())
            .unwrap_or_default();
        Lots { lots, save }
    }

    /// Lot numbers for these paths: the saved one, else the first free lot from a hash of the path.
    fn assign(&mut self, paths: &[String]) -> HashMap<String, u64> {
        let mut sorted: Vec<&String> = paths.iter().collect();
        sorted.sort();
        sorted.dedup();
        // Room to spare, so hashes rarely collide, but dense enough to look like a town.
        let cap = (sorted.len() as u64 * 3 / 2 + 1).max(COLUMNS * 2).div_ceil(COLUMNS) * COLUMNS;
        let mut taken: HashMap<u64, &String> = HashMap::new();
        let mut out = HashMap::new();
        let mut changed = false;
        // Saved lots first, so older houses keep theirs.
        for p in &sorted {
            if let Some(&lot) = self.lots.get(*p) {
                if let std::collections::hash_map::Entry::Vacant(e) = taken.entry(lot) {
                    e.insert(p);
                    out.insert((*p).clone(), lot);
                }
            }
        }
        for p in &sorted {
            if out.contains_key(*p) {
                continue;
            }
            let mut lot = fnv(p) % cap;
            while taken.contains_key(&lot) {
                lot += 1;
            }
            taken.insert(lot, p);
            out.insert((*p).clone(), lot);
            self.lots.insert((*p).clone(), lot);
            changed = true;
        }
        // A house that is gone keeps its lot until a new house needs it.
        let present: HashSet<&String> = sorted.iter().copied().collect();
        let before = self.lots.len();
        self.lots.retain(|p, lot| present.contains(p) || taken.get(lot).is_none_or(|q| *q == p));
        changed |= before != self.lots.len();
        if changed && self.save {
            let _ = std::fs::create_dir_all(state_dir());
            let tmp = Self::path().with_extension("tmp");
            if std::fs::write(&tmp, serde_json::to_vec(&self.lots).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(tmp, Self::path());
            }
        }
        out
    }
}

// ---------------------------------------------------------------- Model

/// What the character does in the town.
fn state(a: &Agent) -> Option<&'static str> {
    Some(match a.status {
        _ if a.needs_permission() => "permission",
        Status::Waiting => "waiting",
        Status::Working => "working",
        Status::Idle | Status::Unknown | Status::Stale => "asleep",
        Status::Ended => return None,
    })
}

fn tool_kind(tool: &str) -> &'static str {
    match tool.split(':').next().unwrap_or("").trim() {
        "Bash" | "BashOutput" | "KillShell" => "bash",
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "edit",
        "Read" => "read",
        "Grep" | "Glob" | "WebSearch" | "ToolSearch" => "search",
        "WebFetch" => "web",
        "Task" | "Agent" => "agent",
        "TodoWrite" => "todo",
        _ => "other",
    }
}

fn terminal_kind(command: &str) -> &'static str {
    match command {
        "nvim" | "vim" | "vi" => "editor",
        "zsh" | "bash" | "fish" | "sh" | "dash" | "nu" => "shell",
        _ => "busy",
    }
}

fn git_json(g: &GitStatus) -> Value {
    json!({"branch": g.branch, "dirty": g.dirty, "ahead": g.ahead, "behind": g.behind, "added": g.added, "removed": g.removed})
}

// ---- The TUI's detail panel as JSON. The content comes from ui::agent_side / ui::workspace_side, the same
// functions the hub draws, so the town panel shows exactly what the hub shows.

fn color(c: Option<ratatui::style::Color>) -> Value {
    match c {
        Some(c @ ratatui::style::Color::Rgb(..)) => Value::String(css(c)),
        _ => Value::Null,
    }
}

fn line_json(l: &ratatui::text::Line) -> Value {
    use ratatui::style::Modifier;
    Value::Array(
        l.spans
            .iter()
            .map(|sp| {
                let st = l.style.patch(sp.style);
                let m = st.add_modifier;
                let flags = [Modifier::BOLD, Modifier::ITALIC, Modifier::UNDERLINED, Modifier::DIM].iter().enumerate().filter(|(_, f)| m.contains(**f)).map(|(i, _)| 1 << i).sum::<u8>();
                json!([sp.content, color(st.fg), color(st.bg), flags])
            })
            .collect(),
    )
}

fn lines_json(lines: &[ratatui::text::Line]) -> Value {
    Value::Array(lines.iter().map(line_json).collect())
}

fn action_name(a: &Action) -> &'static str {
    match a {
        Action::Approve => "approve",
        Action::Deny => "deny",
        Action::Reply => "reply",
        Action::MarkDone => "done",
        Action::Open => "resume",
        Action::Adopt => "adopt",
        _ => "other",
    }
}

/// The panel in all four tabs; `side(tab)` builds it, `windows` is the Windows tab when there is a workspace.
fn panel_json(side: impl Fn(usize) -> Side, windows: Option<Vec<ratatui::text::Line<'static>>>, reply: Option<String>) -> Value {
    let first = side(0);
    let c = colors();
    let bodies: Vec<Value> = (0..4)
        .map(|tab| match (&windows, tab) {
            (Some(w), 2) => lines_json(w),
            (_, 0) => lines_json(&first.body),
            _ => lines_json(&side(tab).body),
        })
        .collect();
    json!({
        "title": first.title,
        "meta": line_json(&first.meta),
        "color": css(first.color),
        "banner_bg": css(lerp(c.bg, first.color, 0.12)),
        "banner": lines_json(&first.banner),
        "buttons": first.buttons.iter().map(|(key, label, col, action)| json!({
            "key": key, "label": label, "bg": css(*col),
            "fg": css(if *col == c.input { c.fg } else { c.bg }),
            "action": action_name(action),
        })).collect::<Vec<_>>(),
        "tabs": first.tabs.iter().map(|(l, n)| json!([l, n])).collect::<Vec<_>>(),
        "bodies": bodies,
        "reply": reply,
    })
}

fn theme_json() -> Value {
    let c = colors();
    json!({
        "bg": css(c.bg), "bar": css(c.bar), "border": css(c.border), "fg": css(c.fg), "bright": css(c.bright),
        "muted": css(c.muted), "dim": css(c.dim), "accent": css(c.accent), "panel": css(c.panel), "input": css(c.input),
        "warning": css(c.warning), "error": css(c.error), "success": css(c.success),
    })
}

struct Shop {
    name: String,
    git: Option<GitStatus>,
    /// The hub workspaces in this repo.
    workspaces: Vec<usize>,
    agents: Vec<usize>,
}

/// The repo root (worktrees resolve to their main repo), or the directory itself when it is not in a repo.
fn shop_of(git: &GitCache, path: &str) -> String {
    git.repo_root(path).unwrap_or_else(|| path.to_string())
}

fn model(snap: &Snapshot, cfg: &Config, lots: &mut Lots, git: &GitCache, messages: &[Message], demo: bool, t: u128) -> Value {
    let _ = cfg;
    let mut shops: Vec<(String, Shop)> = vec![];
    fn shop<'a>(shops: &'a mut Vec<(String, Shop)>, root: &str, git: Option<&GitStatus>) -> &'a mut Shop {
        let i = match shops.iter().position(|(r, _)| r == root) {
            Some(i) => i,
            None => {
                shops.push((root.to_string(), Shop { name: basename(root), git: git.cloned(), workspaces: vec![], agents: vec![] }));
                shops.len() - 1
            }
        };
        &mut shops[i].1
    }
    for (wi, ws) in snap.workspaces.iter().enumerate() {
        let root = shop_of(git, &ws.path);
        shop(&mut shops, &root, ws.git.as_ref()).workspaces.push(wi);
    }
    // Every running agent works in the shop of its repo, wherever its session runs.
    let mut placed: Vec<(usize, &'static str, String)> = vec![];
    for (i, a) in snap.agents.iter().enumerate() {
        let Some(st) = state(a) else { continue };
        let ws = a.workspace.as_ref().and_then(|n| snap.workspaces.iter().find(|w| &w.name == n));
        let root = ws.map_or_else(|| shop_of(git, &a.path), |w| shop_of(git, &w.path));
        if a.path.is_empty() && ws.is_none() {
            continue;
        }
        // Closed sessions only doze in shops that are open anyway.
        if !a.status.alive() && !shops.iter().any(|(r, _)| *r == root) {
            continue;
        }
        if a.status.alive() {
            shop(&mut shops, &root, a.git.as_ref()).agents.push(i);
        } else if let Some((_, s)) = shops.iter_mut().find(|(r, _)| *r == root) {
            s.agents.push(i);
        }
        placed.push((i, st, root));
    }

    let paths: Vec<String> = shops.iter().map(|(p, _)| p.clone()).collect();
    let lot = lots.assign(&paths);
    let shops_json: Vec<Value> = shops
        .iter()
        .map(|(root, sh)| {
            let wss: Vec<&Workspace> = sh.workspaces.iter().map(|&i| &snap.workspaces[i]).collect();
            // One hub workspace: the panel is exactly the hub's. Several in one repo: their windows together.
            let ws = match wss.as_slice() {
                [one] => (*one).clone(),
                _ => Workspace { name: sh.name.clone(), path: root.clone(), windows: wss.iter().flat_map(|w| w.windows.clone()).collect(), git: sh.git.clone(), mr: wss.iter().find_map(|w| w.mr.clone()) },
            };
            let agents: Vec<&Agent> = sh.agents.iter().map(|&i| &snap.agents[i]).collect();
            let terminals: Vec<Value> = ws
                .windows
                .iter()
                .filter(|w| !w.is_claude())
                .map(|w| json!({"name": w.name, "command": w.command, "kind": terminal_kind(&w.command)}))
                .collect();
            json!({
                "id": root,
                "name": sh.name,
                "path": shorten(root),
                "lot": lot.get(root),
                "git": sh.git.as_ref().map(git_json),
                "terminals": terminals,
                "panel": panel_json(|tab| workspace_side(&agents, &ws, tab, t, PANEL_COLUMNS), Some(window_lines(&ws, None)), None),
            })
        })
        .collect();

    let agents_json: Vec<Value> = placed
        .iter()
        .map(|(i, st, root)| {
            let a = &snap.agents[*i];
            let ws = a.workspace.as_ref().and_then(|n| snap.workspaces.iter().find(|w| &w.name == n));
            let tool = (*st == "working").then(|| a.session.as_ref().and_then(|s| s.tool.clone()).or_else(|| a.transcript.as_ref().and_then(|t| t.last_tool.clone()))).flatten();
            let reply = a.target.as_ref().map(|_| reply_placeholder(a));
            json!({
                "key": a.key,
                "shop": root,
                "state": st,
                "title": a.title(),
                "tool_kind": tool.as_deref().map(tool_kind),
                "ts": a.timestamp(),
                "panel": panel_json(|tab| agent_side(a, ws, tab, t, None, PANEL_COLUMNS), ws.map(|w| window_lines(w, a.window_id.as_deref())), reply),
            })
        })
        .collect();

    // Messages between agents that are both in town; a tmux send-keys target is resolved to its agent here.
    let in_town = |key: &str| placed.iter().any(|(i, _, _)| snap.agents[*i].key == key);
    let messages_json: Vec<Value> = messages
        .iter()
        .filter_map(|m| {
            let from = m.from.as_deref().filter(|k| in_town(k))?;
            let to = match &m.to {
                Some(to) => to.clone(),
                None => {
                    let sender = snap.agents.iter().find(|a| a.key == from);
                    resolve_target(snap, sender, m.target.as_deref()?, m.socket.as_deref())?.key.clone()
                }
            };
            (to != from && in_town(&to)).then(|| json!({"id": m.id, "time": m.time, "from": from, "to": to, "preview": m.preview, "via": m.via}))
        })
        .collect();

    json!({
        "columns": COLUMNS,
        "demo": demo,
        "theme": theme_json(),
        "shops": shops_json,
        "agents": agents_json,
        "messages": messages_json,
        "time": now(),
    })
}

// ---------------------------------------------------------------- Demo

/// A made-up snapshot whose agents change state now and then, and talk to each other every half minute.
fn demo_snapshot() -> (Snapshot, Vec<Message>) {
    use crate::snapshot::Window;
    use crate::state::Session;
    use crate::transcript::Transcript;
    let now = now();
    let phase = ((now / 8) % 4) as usize;
    let gitst = |branch: &str, dirty: usize, ahead: u32, behind: u32| {
        let files = (0..dirty).map(|i| (if i % 3 == 0 { "??" } else { ".M" }.to_string(), format!("src/file_{i}.rs"))).collect();
        Some(GitStatus { branch: Some(branch.into()), dirty, ahead, behind, added: dirty as u32 * 7, removed: dirty as u32 * 2, files })
    };
    let win = |i: u32, name: &str, cmd: &str| Window { index: i, name: name.into(), window_id: format!("@{i}{name}"), pane_id: format!("%{i}"), command: cmd.into(), claude: None, width: 80, height: 24, screen: None };
    let ws = |name: &str, path: &str, git: Option<GitStatus>, windows: Vec<Window>| Workspace { name: name.into(), path: path.into(), windows, git, mr: None };
    let workspaces = vec![
        ws("agents-hub", "/home/demo/repos/agents-hub", gitst("osbo/town", 3, 2, 0), vec![win(1, "claude", "claude"), win(2, "zsh", "zsh"), win(3, "nvim", "nvim")]),
        ws("webshop", "/home/demo/repos/webshop", gitst("main", 0, 0, 1), vec![win(1, "server", "cargo"), win(2, "nvim", "nvim")]),
        ws("infra", "/home/demo/repos/infra", gitst("fix/dns", 12, 0, 0), vec![win(1, "zsh", "zsh")]),
    ];
    let agent = |id: &str, title: &str, ws: Option<&str>, path: &str, states: [(&str, Option<&str>); 4], git: Option<GitStatus>| {
        let (st, tool) = states[phase];
        let status = match st {
            "permission" | "waiting" => Status::Waiting,
            "working" => Status::Working,
            _ => Status::Idle,
        };
        let kind = match st {
            "permission" => Some("permission_prompt"),
            "waiting" => Some("idle_prompt"),
            _ => None,
        };
        let msg = (st == "permission").then_some("Claude needs your permission to use Bash");
        let session = Session::demo(id, path, status, now - 40, tool, kind, msg);
        let mut t = Transcript::default();
        t.title = Some(title.into());
        t.last_prompt = Some(format!("Please {}.", title.to_lowercase()));
        t.last_text = Some("I looked at the code and made the change. The tests pass.\n\nWant me to **commit** it?".into());
        t.model = Some("claude-opus-5-5".into());
        t.cost_usd = Some(1.23);
        t.turns = 4;
        t.last_tool = Some("Bash: cargo test --all".into());
        t.last_tool_call = Some(("Bash".into(), json!({"command": "cargo test --all -- --nocapture"})));
        Agent::demo(&format!("agent:{id}"), status, Some(session), ws, path, t, git)
    };
    let hub = "/home/demo/repos/agents-hub";
    let agents = vec![
        agent("aaaaaaaa-1", "Add the town view", Some("agents-hub"), hub, [("working", Some("Edit: src/town.rs")), ("working", Some("Grep: walk")), ("permission", None), ("idle", None)], gitst("osbo/town", 3, 2, 0)),
        agent("bbbbbbbb-2", "Run the test suite", None, hub, [("permission", None), ("working", Some("Bash: cargo test")), ("working", Some("Read: Cargo.toml")), ("working", Some("Bash: cargo build"))], gitst("osbo/town", 3, 2, 0)),
        agent("cccccccc-3", "Find the slow query", Some("webshop"), "/home/demo/repos/webshop", [("waiting", None), ("waiting", None), ("working", Some("Grep: SELECT")), ("idle", None)], gitst("main", 0, 0, 1)),
        agent("dddddddd-4", "Write release notes", None, "/home/demo/notes", [("idle", None); 4], None),
        agent("eeeeeeee-5", "Bump the DNS records", Some("infra"), "/home/demo/repos/infra", [("idle", None), ("working", Some("WebFetch: https://example.com")), ("working", Some("Task: check zones")), ("waiting", None)], gitst("fix/dns", 12, 0, 0)),
    ];
    // Every 30 s the town-view agent asks the webshop agent something, and gets an answer 12 s later.
    let round = now / 30;
    let mut messages = vec![];
    for r in [round.saturating_sub(1), round] {
        let at = r * 30;
        messages.push(Message { id: format!("demo-q{r}"), time: at, from: Some("agent:aaaaaaaa-1".into()), to: Some("agent:cccccccc-3".into()), preview: "Which endpoint is slow on your side?".into(), via: "send".into(), target: None, socket: None });
        if now >= at + 12 {
            messages.push(Message { id: format!("demo-a{r}"), time: at + 12, from: Some("agent:cccccccc-3".into()), to: Some("agent:aaaaaaaa-1".into()), preview: "GET /orders, it scans the whole table.".into(), via: "send".into(), target: None, socket: None });
        }
    }
    let messages = messages.into_iter().filter(|m| m.time <= now).collect();
    (Snapshot { workspaces, agents, ..Default::default() }, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lots_stay_put_when_shops_come_and_go() {
        let mut lots = Lots { lots: HashMap::new(), save: false };
        let a = lots.assign(&["/a".into(), "/b".into()]);
        let b = lots.assign(&["/a".into(), "/b".into(), "/c".into(), "/0".into()]);
        assert_eq!(a["/a"], b["/a"]);
        assert_eq!(a["/b"], b["/b"]);
        let distinct: HashSet<u64> = b.values().copied().collect();
        assert_eq!(distinct.len(), 4);
        lots.assign(&["/a".into(), "/c".into()]);
        assert_eq!(lots.assign(&["/a".into(), "/b".into(), "/c".into()])["/b"], a["/b"]);
    }

    #[test]
    fn subdirectories_and_worktrees_share_the_repo_shop() {
        let dir = std::env::temp_dir().join(format!("claude-hub-shop-{}", std::process::id()));
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap().status.success(), "{args:?}");
        git(&["init", "-q"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]);
        git(&["worktree", "add", "-q", dir.join("wt").to_str().unwrap()]);
        let cache = GitCache::default();
        let root = shop_of(&cache, repo.to_str().unwrap());
        assert_eq!(shop_of(&cache, repo.join("src").to_str().unwrap()), root);
        assert_eq!(shop_of(&cache, dir.join("wt").to_str().unwrap()), root);
        // Not a repo: the directory is its own shop.
        assert_eq!(shop_of(&cache, dir.to_str().unwrap()), dir.to_str().unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_is_exact() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }

    #[test]
    fn demo_town_groups_by_repo_and_carries_the_hub_panel() {
        crate::theme::select("dark-modern").ok();
        let (snap, messages) = demo_snapshot();
        let mut lots = Lots { lots: HashMap::new(), save: false };
        let m = model(&snap, &Config::default(), &mut lots, &GitCache::default(), &messages, true, 0);
        let shops = m["shops"].as_array().unwrap();
        // agents-hub, webshop, infra, and notes, which has an agent but no hub workspace.
        assert_eq!(shops.len(), 4);
        let a2 = m["agents"].as_array().unwrap().iter().find(|a| a["key"] == "agent:bbbbbbbb-2").unwrap();
        assert_eq!(a2["shop"], "/home/demo/repos/agents-hub", "an agent outside the hub shares the shop of its repo");
        let agent = &m["agents"][0];
        let text: String = agent["panel"]["bodies"][0].as_array().unwrap().iter().flat_map(|l| l.as_array().unwrap().iter().map(|s| s[0].as_str().unwrap().to_string())).collect();
        assert!(text.contains("Please add the town view."), "{text}");
        assert!(!messages.is_empty() || now() % 30 < 1);
    }
}
