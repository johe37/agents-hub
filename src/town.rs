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

use crate::config::{Config, shorten, state_dir};
use crate::git::{GitCache, GitStatus};
use crate::snapshot::{Agent, Cmd, Snapshot, basename};
use crate::state::{Status, now};

const PAGE: &str = include_str!("town/town.html");
const SCRIPT: &str = include_str!("town/town.js");
const MAX_BODY: usize = 64 * 1024;
const MAX_REPLY: usize = 4000;
const MAX_CALL: usize = 4000;
const MAX_FILES: usize = 40;
/// Lots per row of the town grid.
const COLUMNS: u64 = 5;

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
    let shared = if demo {
        let shared = Arc::new(Shared::new(token, port, None));
        let s = shared.clone();
        std::thread::spawn(move || {
            loop {
                s.publish(&demo_model(&mut lots), None);
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
            for snap in snaps {
                let model = model(&snap, &cfg, &mut lots, &git);
                s.publish(&model, Some(snap));
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
    if !a.status.alive() || a.target.is_none() {
        return err("409 Conflict", "This agent does not take input.");
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
fn state(a: &Agent, stale_minutes: f64) -> Option<&'static str> {
    let old = a.timestamp() > 0 && now().saturating_sub(a.timestamp()) as f64 > stale_minutes * 60.0;
    Some(match a.status {
        _ if a.needs_permission() => "permission",
        Status::Waiting => "waiting",
        Status::Working => "working",
        Status::Idle if old => "asleep",
        Status::Idle => "idle",
        Status::Unknown | Status::Stale => "asleep",
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
    json!({
        "branch": g.branch,
        "dirty": g.dirty,
        "ahead": g.ahead,
        "behind": g.behind,
        "added": g.added,
        "removed": g.removed,
        "files": g.files.iter().take(MAX_FILES).map(|(st, p)| json!([st, p])).collect::<Vec<_>>(),
        "more_files": g.files.len().saturating_sub(MAX_FILES),
    })
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n).collect::<String>() + "…" }
}

/// The exact tool call behind a permission prompt.
fn call_json(a: &Agent) -> Value {
    let Some((name, input)) = a.transcript.as_ref().and_then(|t| t.last_tool_call.as_ref()) else { return Value::Null };
    let text = match input["command"].as_str() {
        Some(cmd) if name == "Bash" => cmd.to_string(),
        _ => serde_json::to_string_pretty(input).unwrap_or_default(),
    };
    json!({"name": name, "input": clip(&text, MAX_CALL)})
}

fn agent_json(a: &Agent, state: &str, house: Option<&String>, camp: bool) -> Value {
    let t = a.transcript.as_ref();
    let s = a.session.as_ref();
    let tool = if state == "working" { s.and_then(|s| s.tool.clone()).or_else(|| t.and_then(|t| t.last_tool.clone())) } else { None };
    let (headline, detail) = crate::ui::headline(a);
    json!({
        "key": a.key,
        "title": clip(&a.title(), 200),
        "state": state,
        "house": house,
        "camp": camp,
        "workspace": a.workspace,
        "path": shorten(&a.path),
        "tool": tool,
        "tool_kind": tool.as_deref().map(tool_kind),
        "headline": headline,
        "detail": detail,
        "message": s.and_then(|s| s.message.clone()),
        "call": if a.needs_permission() { call_json(a) } else { Value::Null },
        "ts": a.timestamp(),
        "prompt": t.and_then(|t| t.last_prompt.as_deref()).map(|p| clip(p, 2000)),
        "reply": t.and_then(|t| t.last_text.clone()),
        "model": t.and_then(|t| t.model.clone()),
        "cost": t.and_then(|t| t.cost_usd),
        "turns": t.map_or(0, |t| t.turns),
        "mode": s.and_then(|s| s.permission_mode.clone()),
        "session": a.session_id().map(|id| id.chars().take(8).collect::<String>()),
        "can_reply": a.status.alive() && a.target.is_some(),
        "background": a.background,
    })
}

struct House {
    name: String,
    git: Option<GitStatus>,
    hub: bool,
    terminals: Vec<Value>,
}

fn model(snap: &Snapshot, cfg: &Config, lots: &mut Lots, git: &GitCache) -> Value {
    let mut houses: Vec<(String, House)> = vec![];
    let mut by_name: HashMap<String, String> = HashMap::new();
    for ws in &snap.workspaces {
        by_name.insert(ws.name.clone(), ws.path.clone());
        let terminals = ws
            .windows
            .iter()
            .filter(|w| !w.is_claude())
            .map(|w| json!({"name": w.name, "command": w.command, "kind": terminal_kind(&w.command)}))
            .collect();
        houses.push((ws.path.clone(), House { name: ws.name.clone(), git: ws.git.clone(), hub: true, terminals }));
    }
    // The house that holds a directory: the deepest one around it.
    let holder = |houses: &[(String, House)], path: &str| {
        houses.iter().map(|(p, _)| p).filter(|p| path == *p || path.starts_with(&format!("{p}/"))).max_by_key(|p| p.len()).cloned()
    };

    let mut agents = vec![];
    let mut placed: Vec<(&Agent, &'static str)> = snap.agents.iter().filter_map(|a| Some((a, state(a, cfg.stale_minutes)?))).collect();
    // Live agents first: their repos get a house, closed ones only move into houses that exist.
    placed.sort_by_key(|(a, _)| !a.status.alive());
    for (a, st) in placed {
        let outside = a.workspace.is_none();
        let mut house = a.workspace.as_ref().and_then(|n| by_name.get(n).cloned()).or_else(|| holder(&houses, &a.path));
        if house.is_none() && a.status.alive() && !a.path.is_empty() {
            let root = git.repo_root(&a.path).unwrap_or_else(|| a.path.clone());
            houses.push((root.clone(), House { name: basename(&root), git: a.git.clone(), hub: false, terminals: vec![] }));
            house = Some(root);
        }
        // Agents outside the hub camp at the edge of town; so do closed ones without a house.
        let camp = (outside && a.status.alive()) || house.is_none();
        agents.push(agent_json(a, st, house.as_ref(), camp));
    }

    let paths: Vec<String> = houses.iter().map(|(p, _)| p.clone()).collect();
    let lot = lots.assign(&paths);
    let houses: Vec<Value> = houses
        .into_iter()
        .map(|(path, h)| {
            json!({
                "id": path,
                "name": h.name,
                "path": shorten(&path),
                "lot": lot.get(&path),
                "hub": h.hub,
                "git": h.git.as_ref().map(git_json),
                "terminals": h.terminals,
            })
        })
        .collect();
    json!({"columns": COLUMNS, "demo": false, "houses": houses, "agents": agents, "time": now()})
}

// ---------------------------------------------------------------- Demo

/// A made-up town whose agents change state now and then, for trying the page without real sessions.
fn demo_model(lots: &mut Lots) -> Value {
    let phase = (now() / 8) % 4;
    let pick = |states: [&'static str; 4]| states[phase as usize];
    let git = |branch: &str, dirty: usize, ahead: u32, behind: u32| {
        let files: Vec<(String, String)> = (0..dirty).map(|i| (if i % 3 == 0 { "??" } else { ".M" }.into(), format!("src/file_{i}.rs"))).collect();
        git_json(&GitStatus { branch: Some(branch.into()), dirty, ahead, behind, added: dirty as u32 * 7, removed: dirty as u32 * 2, files })
    };
    let term = |name: &str, command: &str| json!({"name": name, "command": command, "kind": terminal_kind(command)});
    let houses = [
        ("/home/demo/repos/agents-hub", "agents-hub", Some(git("osbo/town", 3, 2, 0)), vec![term("zsh", "zsh"), term("nvim", "nvim")]),
        ("/home/demo/repos/webshop", "webshop", Some(git("main", 0, 0, 1)), vec![term("server", "cargo"), term("nvim", "nvim")]),
        ("/home/demo/notes", "notes", None, vec![]),
        ("/home/demo/repos/infra", "infra", Some(git("fix/dns", 12, 0, 0)), vec![term("zsh", "zsh")]),
        ("/home/demo/repos/website", "website", Some(git("main", 0, 1, 2)), vec![]),
    ];
    let paths: Vec<String> = houses.iter().map(|h| h.0.to_string()).collect();
    let lot = lots.assign(&paths);
    let houses: Vec<Value> = houses
        .into_iter()
        .map(|(path, name, git, terminals)| {
            json!({"id": path, "name": name, "path": path.replace("/home/demo", "~"), "lot": lot.get(path), "hub": true, "git": git, "terminals": terminals})
        })
        .collect();
    let agent = |key: &str, title: &str, house: Option<&str>, camp: bool, state: &str, tool: Option<&str>| {
        let (headline, detail) = match state {
            "permission" => ("Needs your permission", tool),
            "waiting" => ("Waiting for your input", None),
            "working" => ("Working", tool),
            "idle" => ("Done with its turn", Some("Enter opens it to continue.")),
            _ => ("Closed", None),
        };
        let call = (state == "permission").then(|| json!({"name": "Bash", "input": "cargo test --all -- --nocapture"}));
        json!({
            "key": key, "title": title, "state": state, "house": house, "camp": camp, "workspace": house.map(basename),
            "path": house.unwrap_or("~/scratch").replace("/home/demo", "~"),
            "tool": if state == "working" { tool } else { None }, "tool_kind": tool.filter(|_| state == "working").map(tool_kind),
            "headline": headline, "detail": detail, "message": (state == "permission").then_some("Claude needs your permission to use Bash"),
            "call": call, "ts": now(), "prompt": format!("Please {}.", title.to_lowercase()),
            "reply": "I looked at the code and made the change. The tests pass.\n\nWant me to **commit** it?",
            "model": "claude-opus-5-5", "cost": 1.23, "turns": 4, "mode": "default", "session": key.get(6..14),
            "can_reply": false, "background": false,
        })
    };
    let hub = "/home/demo/repos/agents-hub";
    let shop = "/home/demo/repos/webshop";
    let agents = vec![
        agent("agent:aaaaaaaa-1", "Add the town view", Some(hub), false, pick(["working", "working", "permission", "idle"]), Some("Edit: src/town.rs")),
        agent("agent:bbbbbbbb-2", "Run the test suite", Some(hub), false, pick(["permission", "working", "working", "working"]), Some("Bash: cargo test")),
        agent("agent:cccccccc-3", "Find the slow query", Some(shop), false, pick(["waiting", "waiting", "working", "idle"]), Some("Grep: SELECT")),
        agent("agent:dddddddd-4", "Write release notes", Some("/home/demo/notes"), false, "asleep", None),
        agent("agent:eeeeeeee-5", "Bump the DNS records", Some("/home/demo/repos/infra"), false, pick(["idle", "working", "working", "waiting"]), Some("WebFetch: https://example.com")),
        agent("agent:ffffffff-6", "Read the website copy", Some("/home/demo/repos/website"), true, pick(["working", "idle", "permission", "working"]), Some("Read: index.md")),
        agent("agent:99999999-7", "Old experiment", None, true, "asleep", None),
    ];
    json!({"columns": COLUMNS, "demo": true, "houses": houses, "agents": agents, "time": now()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lots_stay_put_when_houses_come_and_go() {
        let mut lots = Lots { lots: HashMap::new(), save: false };
        let a = lots.assign(&["/a".into(), "/b".into()]);
        let b = lots.assign(&["/a".into(), "/b".into(), "/c".into(), "/0".into()]);
        assert_eq!(a["/a"], b["/a"]);
        assert_eq!(a["/b"], b["/b"]);
        let distinct: HashSet<u64> = b.values().copied().collect();
        assert_eq!(distinct.len(), 4);
        // /b leaves and comes back to its lot.
        lots.assign(&["/a".into(), "/c".into()]);
        assert_eq!(lots.assign(&["/a".into(), "/b".into(), "/c".into()])["/b"], a["/b"]);
    }

    #[test]
    fn same_is_exact() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }
}
