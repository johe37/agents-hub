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
use crate::progress::{Progress, level_for};
use crate::state::{Status, now};
use crate::theme::{colors, css, lerp};
use crate::ui::{Side, agent_side, reply_placeholder, window_lines, workspace_side};

const PAGE: &str = include_str!("town/town.html");
const SCRIPT: &str = include_str!("town/town.js");
const PLAYER_SCRIPT: &str = include_str!("town/player.js");
const MAX_BODY: usize = 64 * 1024;
const MAX_REPLY: usize = 4000;
/// Columns of the town's panel body (46em of monospace), for the chat cards and pipeline layout.
const PANEL_WIDTH: u16 = 80;
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
    /// The demo town's player, kept in memory so the demo never writes your progress file.
    demo_player: Option<Mutex<Progress>>,
    /// The shops (repo roots) in the newest model: the only directories the page may start Claude in.
    shop_ids: Mutex<HashSet<String>>,
    hub_key: String,
    /// Where to look for repos, for the place picker.
    roots: Vec<String>,
}

impl Shared {
    fn publish(&self, model: &Value, snap: Option<Snapshot>) {
        *self.shop_ids.lock().unwrap() = model["shops"].as_array().into_iter().flatten().filter_map(|s| s["id"].as_str().map(String::from)).collect();
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
        let mut sh = Shared::new(token, port, None);
        sh.demo_player = Some(Mutex::new(demo_progress(&cfg)));
        let shared = Arc::new(sh);
        let s = shared.clone();
        std::thread::spawn(move || {
            let git = GitCache::default();
            let mut remotes = Remotes::demo();
            loop {
                let (mut snap, messages) = demo_snapshot();
                snap.progress = s.demo_player.as_ref().map(|p| Arc::new(p.lock().unwrap().clone()));
                s.publish(&model(&snap, &cfg, &mut lots, &git, &mut remotes, &messages, true, started.elapsed().as_millis()), None);
                // Sooner after a change to the player, so equipping feels instant.
                let mut l = s.latest.lock().unwrap();
                l = s.changed.wait_timeout(l, Duration::from_secs(2)).unwrap().0;
                drop(l);
            }
        });
        shared
    } else {
        let (cmd, snaps) = crate::snapshot::spawn(cfg.clone(), Default::default(), false);
        crate::xp::spawn_background(cfg.clone());
        let mut sh = Shared::new(token, port, Some(cmd));
        sh.hub_key = cfg.hub_key.clone();
        sh.roots = cfg.roots.clone();
        let shared = Arc::new(sh);
        let s = shared.clone();
        std::thread::spawn(move || {
            let git = GitCache::default();
            let mut remotes = Remotes::default();
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
                let model = model(snap, &cfg, &mut lots, &git, &mut remotes, &messages, false, started.elapsed().as_millis());
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
            demo_player: None,
            shop_ids: Mutex::new(HashSet::new()),
            hub_key: "M-q".into(),
            roots: vec![],
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
        ("GET", "/player.js") => respond(&mut stream, "200 OK", "text/javascript; charset=utf-8", PLAYER_SCRIPT.as_bytes()),
        ("GET", "/events") => {
            let token = req.query.split('&').find_map(|kv| kv.strip_prefix("t=")).unwrap_or("");
            if !same(token, &shared.token) {
                return respond(&mut stream, "403 Forbidden", "text/plain", b"bad token");
            }
            events(stream, shared)
        }
        ("POST", "/api/player") => {
            let origins = hosts.map(|h| format!("http://{h}"));
            let origin_ok = req.headers.get("origin").is_some_and(|o| origins.contains(o));
            let token_ok = req.headers.get("x-town-token").is_some_and(|t| same(t, &shared.token));
            if !origin_ok || !token_ok {
                return json_reply(&mut stream, "403 Forbidden", json!({"error": "Only the town page on this machine can do that."}));
            }
            let (status, v) = player_act(shared, &req.body);
            json_reply(&mut stream, status, v)
        }
        ("POST", "/api/places") => {
            let origins = hosts.map(|h| format!("http://{h}"));
            let origin_ok = req.headers.get("origin").is_some_and(|o| origins.contains(o));
            let token_ok = req.headers.get("x-town-token").is_some_and(|t| same(t, &shared.token));
            if !origin_ok || !token_ok {
                return json_reply(&mut stream, "403 Forbidden", json!({"error": "Only the town page on this machine can do that."}));
            }
            let (status, v) = places(shared, &req.body);
            json_reply(&mut stream, status, v)
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
    // c on a shop, or a place from the picker: a new Claude there, with an optional first task.
    if req["op"] == "claude" {
        let shop = match (req["shop"].as_str(), req["path"].as_str()) {
            (Some(shop), _) if shared.shop_ids.lock().unwrap().contains(shop) => shop.to_string(),
            (Some(_), _) => return err("404 Not Found", "This shop is gone."),
            // Any directory, as the hub's n picker allows; resolved, so the workspace matches the real path.
            (None, Some(path)) => match std::fs::canonicalize(crate::config::expand(path.trim())) {
                Ok(p) if p.is_dir() => p.display().to_string(),
                _ => return err("404 Not Found", "No such directory."),
            },
            _ => return err("400 Bad Request", "Where should Claude start?"),
        };
        let task: String = req["text"].as_str().unwrap_or("").chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let task = task.trim().to_string();
        if task.chars().count() > MAX_REPLY {
            return err("400 Bad Request", "Write a shorter task (at most 4000 characters).");
        }
        let has_task = !task.is_empty();
        let trust = req["trust"] == true;
        let result = crate::tmux::start_claude(&shop, has_task.then_some(task), &shared.hub_key, trust);
        let _ = cmd.lock().unwrap().send(Cmd::Refresh);
        return match result {
            Ok(ws) => ("200 OK", json!({"ok": match (has_task, trust) {
                (true, _) => format!("Started Claude in {ws}; it gets your task as soon as it is ready"),
                (false, true) => format!("Started Claude in {ws}, and told it to trust the folder"),
                (false, false) => format!("Started Claude in {ws}"),
            }})),
            Err(crate::tmux::StartError::NeedsTrust) => ("409 Conflict", json!({
                "error": format!("Claude has not worked in {} before. If you trust it, Claude may read, edit and run files there.", shorten(&shop)),
                "needs_trust": true,
            })),
            Err(crate::tmux::StartError::Failed(e)) => err("500 Internal Server Error", &e),
        };
    }
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

/// The hub's picker, for the town: open workspaces, repos, recent and other directories matching the
/// query, or the directories under a typed path.
fn places(shared: &Shared, body: &[u8]) -> (&'static str, Value) {
    let Ok(req) = serde_json::from_slice::<Value>(body) else { return ("400 Bad Request", json!({"error": "Bad request."})) };
    let Some(snap) = shared.latest.lock().unwrap().snap.clone() else { return ("403 Forbidden", json!({"error": "The demo town cannot start agents."})) };
    let picker = crate::picker::Picker::new(crate::picker::candidates(&snap, &shared.roots), req["q"].as_str().unwrap_or(""));
    let options: Vec<Value> = picker
        .options
        .iter()
        .take(40)
        .map(|c| {
            let spans = &c.line.spans;
            let tag = spans.get(1).map(|t| t.content.trim().trim_start_matches('●').trim().to_string());
            json!({"path": c.path, "label": spans.first().map(|l| l.content.trim_end().to_string()), "tag": tag})
        })
        .collect();
    ("200 OK", json!({"options": options}))
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

#[allow(clippy::too_many_arguments)]
fn model(snap: &Snapshot, cfg: &Config, lots: &mut Lots, git: &GitCache, remotes: &mut Remotes, messages: &[Message], demo: bool, t: u128) -> Value {
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
    // Quests whose project is the remote of a shop: a scroll on that shop.
    // The demo's made-up remotes are on no real host, so it does not check the host.
    let host = cfg.gitlab_url.as_deref().map(crate::xp::host_of).filter(|_| !demo);
    let mut quest_shops: HashMap<u64, String> = HashMap::new();
    for q in snap.progress.iter().flat_map(|p| p.quests.iter()) {
        if let Some(root) = paths.iter().find(|root| remotes.project(root).is_some_and(|(h, path)| path.eq_ignore_ascii_case(&q.project) && host.as_ref().is_none_or(|x| x.eq_ignore_ascii_case(&h)))) {
            quest_shops.insert(q.id, root.clone());
        }
    }
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
                "quests": quest_shops.iter().filter(|(_, r)| *r == root).map(|(id, _)| *id).collect::<Vec<_>>(),
                "panel": panel_json(|tab| workspace_side(&agents, &ws, tab, t, PANEL_WIDTH), Some(window_lines(&ws, None)), None),
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
                "panel": panel_json(|tab| agent_side(a, ws, tab, t, None, PANEL_WIDTH), ws.map(|w| window_lines(w, a.window_id.as_deref())), reply),
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
        "player": player_json(snap.progress.as_deref(), cfg, &quest_shops),
        "time": now(),
    })
}

// ---------------------------------------------------------------- The player

/// The git remote of each shop, as (host, "group/project"); looked up once per shop.
#[derive(Default)]
struct Remotes(HashMap<String, Option<(String, String)>>);

impl Remotes {
    fn demo() -> Self {
        let r = |root: &str, path: &str| (root.to_string(), Some(("git.example.com".to_string(), path.to_string())));
        Remotes(HashMap::from([r("/home/demo/repos/agents-hub", "tools/agents-hub"), r("/home/demo/repos/webshop", "shop/webshop")]))
    }

    fn project(&mut self, root: &str) -> Option<(String, String)> {
        self.0
            .entry(root.to_string())
            .or_insert_with(|| {
                let out = std::process::Command::new("git").args(["-C", root, "remote", "get-url", "origin"]).stderr(std::process::Stdio::null()).output().ok()?;
                parse_remote(String::from_utf8_lossy(&out.stdout).trim())
            })
            .clone()
    }
}

/// ("git.example.com", "group/project") from an ssh or https remote URL.
fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        // https://host/path, ssh://git@host:2222/path
        let (auth_host, path) = rest.split_once('/')?;
        let host = auth_host.rsplit('@').next()?.split(':').next()?;
        (host, path)
    } else {
        // git@host:path
        let (auth_host, path) = url.split_once(':')?;
        (auth_host.rsplit('@').next()?, path)
    };
    (!host.is_empty() && path.contains('/')).then(|| (host.to_string(), path.to_string()))
}

fn item_json(p: &Progress, it: &crate::loot::Item) -> Value {
    let mut v = serde_json::to_value(it).unwrap_or_default();
    v["slot"] = serde_json::to_value(it.kind.slot()).unwrap_or_default();
    v["base"] = it.base().into();
    v["compare"] = serde_json::to_value(p.compare(it)).unwrap_or_default();
    v
}

/// You in the town: level and XP, quests, loot, what you wear and how to draw it.
fn player_json(p: Option<&Progress>, cfg: &Config, quest_shops: &HashMap<u64, String>) -> Value {
    let fresh = Progress::new(now());
    let p = p.unwrap_or(&fresh);
    let (level, into, need) = level_for(p.total_xp);
    let recent: Vec<Value> = p.recent(6).iter().map(|c| json!({"title": c.title, "project": c.project, "iid": c.iid, "weight": c.weight, "xp": c.xp, "url": c.url})).collect();
    let quests: Vec<Value> = p
        .quests
        .iter()
        .map(|q| json!({"id": q.id, "title": q.title, "project": q.project, "iid": q.iid, "weight": q.weight, "xp": q.xp, "due": q.due_date, "labels": q.labels, "url": q.url, "shop": quest_shops.get(&q.id)}))
        .collect();
    // Level-ups the town has not celebrated yet, each with its loot; at most ten, oldest first.
    let celebrate: Vec<Value> = ((p.celebrated_level.max(1) + 1)..=level).take(10).map(|l| json!({"level": l, "item": p.items.iter().find(|i| i.dropped_at_level == l).map(|i| i.id.clone())})).collect();
    let mut done: Vec<_> = p.issues.values().filter(|c| c.seq > p.acked_seq).collect();
    done.sort_by_key(|c| c.seq);
    let completed: Vec<Value> = done.iter().rev().take(5).rev().map(|c| json!({"seq": c.seq, "title": c.title, "project": c.project, "xp": c.xp})).collect();
    let class = crate::class::class_of(p);
    json!({
        "class": {
            "key": class.map(|c| c.key),
            "name": class.map_or(crate::class::NO_CLASS.0, |c| c.name),
            "calling": class.map(|c| c.calling),
            "flavor": class.map_or(crate::class::NO_CLASS.1, |c| c.flavor),
            "standings": crate::class::standings(p),
        },
        "appearance": p.appearance,
        "level": level,
        "xp": into,
        "need": need,
        "total": p.total_xp,
        "issues": p.issues.len(),
        "recent": recent,
        "quests": quests,
        "items": p.items.iter().map(|i| item_json(p, i)).collect::<Vec<_>>(),
        "equipped": p.equipped,
        "bag": p.bag().iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
        "layers": crate::gear::layers(p),
        "totals": p.totals(),
        "slots": crate::gear::Slot::ALL.iter().map(|s| json!({"slot": s, "kinds": s.kinds()})).collect::<Vec<_>>(),
        "celebrate": celebrate,
        "completed": completed,
        "completed_total": done.len(),
        "completed_xp": done.iter().map(|c| c.xp).sum::<u64>(),
        "last_seq": p.issues.values().map(|c| c.seq).max().unwrap_or(0),
        "sync": {"last_ok": p.sync.last_ok, "error": p.sync.last_error, "configured": cfg.gitlab_url.is_some() || cfg.gitlab_username.is_some()},
        "sound": cfg.sound,
    })
}

/// Equip, unequip, change your look, or say what the page has celebrated.
fn player_act(shared: &Shared, body: &[u8]) -> (&'static str, Value) {
    let err = |status, msg: &str| (status, json!({"error": msg}));
    let Ok(req) = serde_json::from_slice::<Value>(body) else { return err("400 Bad Request", "Bad request.") };
    let change = |p: &mut Progress| -> Result<String, String> {
        let slot = req["slot"].as_str().map(|s| crate::gear::Slot::parse(s).ok_or("No such slot."));
        let slot = slot.transpose()?;
        match req["op"].as_str() {
            Some("equip") => {
                let id = req["item"].as_str().ok_or("Which item?")?;
                p.equip(id, slot).map_err(|e| e.to_string())?;
                Ok(format!("Equipped {}", p.item(id).map_or("", |i| i.name.as_str())))
            }
            Some("unequip") => {
                let gone = match (slot, req["item"].as_str()) {
                    (Some(s), _) => p.unequip(s),
                    (None, Some(id)) => p.unequip_item(id).map(|_| id.to_string()),
                    _ => None,
                };
                gone.map(|id| format!("Unequipped {}", p.item(&id).map_or("", |i| i.name.as_str()))).ok_or_else(|| "Nothing to take off there.".into())
            }
            Some("appearance") => {
                let a: crate::progress::Appearance = serde_json::from_value(req["appearance"].clone()).map_err(|_| "Bad appearance.")?;
                p.appearance = Some(a.clamped());
                Ok("Saved your look".into())
            }
            Some("ack") => {
                if let Some(l) = req["level"].as_u64() {
                    p.celebrated_level = p.celebrated_level.max(l.min(p.level as u64) as u32);
                }
                if let Some(seq) = req["seq"].as_u64() {
                    p.acked_seq = p.acked_seq.max(seq);
                }
                Ok(String::new())
            }
            _ => Err("Unknown action.".into()),
        }
    };
    let result = match &shared.demo_player {
        Some(demo) => change(&mut demo.lock().unwrap()),
        None => crate::progress::Store::default().update(change).unwrap_or_else(|e| Err(format!("Cannot save your progress: {e}"))),
    };
    shared.changed.notify_all();
    if let Some(cmd) = &shared.cmd {
        let _ = cmd.lock().unwrap().send(Cmd::Refresh);
    }
    match result {
        Ok(msg) => ("200 OK", json!({"ok": msg})),
        Err(e) => err("409 Conflict", &e),
    }
}

/// A made-up player for the demo town: a few levels of real loot from a demo seed, and quests.
fn demo_progress(cfg: &Config) -> Progress {
    use crate::progress::{Issue, Rules};
    let mut p = Progress::new(now());
    p.username = Some("demo".into());
    // Generous XP, so the demo has a few levels of loot to show.
    let rules = Rules { username: "demo".into(), xp_per_weight: 60, default_weight: 1, since: 0, chances: cfg.drop_chances.clone() };
    let titles = ["Fix the flaky login test", "Speed up the search page", "Write the onboarding guide", "Upgrade the database driver", "Add dark mode to the dashboard", "Remove the old billing code", "Cache the product images", "Handle timeouts in the importer"];
    let closed: Vec<Issue> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| Issue {
            id: 900 + i as u64,
            project_id: 1,
            project: if i % 2 == 0 { "tools/agents-hub" } else { "shop/webshop" }.into(),
            iid: 40 + i as u64,
            title: (*t).into(),
            weight: [Some(3), Some(8), None, Some(5), Some(13), Some(2), Some(8), Some(20)][i],
            labels: vec![["Type::Bug", "Type::Enhancement", "Documentation", "Type::Bug", "Type::Enhancement", "Type::Tech Debt", "Type::Bug", "Severity::Critical"][i].to_string()],
            closed_at: Some(now() - 86400 * (10 - i as u64)),
            ..Default::default()
        })
        .collect();
    p.count_closed(&closed, &rules, now());
    let open = [("Draw the quest log", "tools/agents-hub", Some(5), Some("2026-12-01")), ("Make checkout faster", "shop/webshop", Some(8), None), ("Rotate the DNS keys", "ops/infra", None, None)];
    let open: Vec<Issue> = open
        .iter()
        .enumerate()
        .map(|(i, (t, proj, w, due))| Issue { id: 950 + i as u64, project: (*proj).into(), iid: 60 + i as u64, title: (*t).into(), weight: *w, due_date: due.map(String::from), labels: vec!["demo".into()], ..Default::default() })
        .collect();
    p.set_quests(&open, &rules);
    // And one legendary to show off, worn.
    let only = crate::config::DropChances { poor: 0.0, common: 0.0, uncommon: 0.0, rare: 0.0, epic: 0.0, legendary: 1.0 };
    let mut legend = crate::loot::generate(crate::loot::random_seed(), 1, &only, &[]);
    legend.id = "demo-legendary".into();
    p.items.push(legend);
    p.scale_legendaries();
    let _ = p.equip("demo-legendary", None);
    let ids: Vec<String> = p.items.iter().map(|i| i.id.clone()).collect();
    for id in ids.iter().take(2) {
        if p.item(id).is_some_and(|i| p.worn(i.kind.slot()).is_none()) {
            let _ = p.equip(id, None);
        }
    }
    // Show off the celebration once: the last level is not celebrated yet.
    p.celebrated_level = p.level.saturating_sub(1).max(1);
    p.acked_seq = p.issues.len() as u64 - 1;
    p
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
    fn remotes_parse_to_host_and_project() {
        let want = Some(("git.example.com".to_string(), "group/sub/app".to_string()));
        assert_eq!(parse_remote("git@git.example.com:group/sub/app.git"), want);
        assert_eq!(parse_remote("https://git.example.com/group/sub/app.git/"), want);
        assert_eq!(parse_remote("ssh://git@git.example.com:2222/group/sub/app"), want);
        assert_eq!(parse_remote("https://oauth2:secret@git.example.com/group/sub/app"), want);
        assert_eq!(parse_remote(""), None);
        assert_eq!(parse_remote("/local/path"), None);
    }

    #[test]
    fn demo_player_has_loot_quests_and_a_quest_shop() {
        crate::theme::select("dark-modern").ok();
        let (mut snap, messages) = demo_snapshot();
        snap.progress = Some(Arc::new(demo_progress(&Config::default())));
        let mut lots = Lots { lots: HashMap::new(), save: false };
        let cfg = Config { gitlab_url: Some("https://git.elsewhere.example".into()), ..Config::default() };
        let m = model(&snap, &cfg, &mut lots, &GitCache::default(), &mut Remotes::demo(), &messages, true, 0);
        let pl = &m["player"];
        assert_eq!(pl["class"]["name"], "Verminbane");
        assert!(pl["level"].as_u64().unwrap() >= 3, "{pl}");
        assert_eq!(pl["quests"].as_array().unwrap().len(), 3);
        assert_eq!(pl["quests"][0]["shop"], "/home/demo/repos/agents-hub", "the quest with a due date, in the shop with that remote");
        assert_eq!(pl["celebrate"].as_array().unwrap().len(), 1);
        let hub = m["shops"].as_array().unwrap().iter().find(|s| s["name"] == "agents-hub").unwrap();
        assert_eq!(hub["quests"].as_array().unwrap().len(), 1);
        assert_eq!(pl["layers"][0]["part"].as_str().map(|p| p == "back" || p == "body"), Some(true));
    }

    #[test]
    fn only_the_town_map_is_pinned_to_the_corner() {
        // A position rule on every canvas once pinned all item icons over the dock's strip.
        let css = &PAGE[PAGE.find("<style>").unwrap()..PAGE.find("</style>").unwrap()];
        for rule in css.split('}') {
            let (selector, body) = rule.split_once('{').unwrap_or(("", ""));
            if selector.trim() == "canvas" {
                assert!(!body.contains("position"), "a bare canvas rule must not position: {rule}");
            }
        }
        assert!(css.contains("#town { position: absolute"));
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
        let m = model(&snap, &Config::default(), &mut lots, &GitCache::default(), &mut Remotes::demo(), &messages, true, 0);
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
