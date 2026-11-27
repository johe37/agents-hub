//! `jugglr web`: the hub as a web app on localhost.

mod actions;
mod conversation;
mod model;
mod review;
mod term;

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use serde_json::{Value, json};
use tokio::sync::{broadcast, watch};

use crate::config::{Config, state_dir};
use crate::snapshot::{Cmd, Snapshot};

#[derive(Embed)]
#[folder = "web/dist"]
struct Assets;

const COOKIE: &str = "jugglr";

pub struct Hub {
    snap: RwLock<Snapshot>,
    cmd: Mutex<Sender<Cmd>>,
    token: String,
    port: u16,
    state: watch::Sender<Arc<str>>,
    /// Toasts for every open page: notices from background jobs.
    events: broadcast::Sender<Arc<str>>,
    asked: AtomicU64,
    /// The RefreshFor number the published state answers.
    served: watch::Sender<u64>,
}

impl Hub {
    fn refresh(&self) {
        let _ = self.cmd.lock().unwrap().send(Cmd::Refresh);
    }

    /// Wait until the page has state that shows everything done so far, or a second passed.
    async fn fresh(&self) {
        let n = self.asked.fetch_add(1, Ordering::Relaxed) + 1;
        let mut served = self.served.subscribe();
        let _ = self.cmd.lock().unwrap().send(Cmd::RefreshFor(n));
        let _ = tokio::time::timeout(Duration::from_secs(1), served.wait_for(|s| *s >= n)).await;
    }

    fn snapshot(&self) -> std::sync::RwLockReadGuard<'_, Snapshot> {
        self.snap.read().unwrap()
    }
}

/// The token stays the same across restarts, so a bookmarked link keeps working.
fn token() -> String {
    let path = state_dir().join("web-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim();
        if t.len() >= 32 && t.chars().all(|c| c.is_ascii_hexdigit()) {
            return t.to_string();
        }
    }
    let mut bytes = [0u8; 24];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut bytes);
    }
    let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let _ = std::fs::create_dir_all(state_dir());
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path) {
        use std::io::Write;
        let _ = f.write_all(t.as_bytes());
    }
    t
}

pub fn run(mut cfg: Config, port: u16, open: bool) -> ExitCode {
    cfg.passive = true;
    let screens = Arc::default();
    let (cmd, snaps) = crate::snapshot::spawn(cfg.clone(), screens);
    let (state_tx, _) = watch::channel::<Arc<str>>(Arc::from("null"));
    let (events, _) = broadcast::channel(64);

    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("jugglr: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("jugglr: cannot listen on 127.0.0.1:{port}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
        let hub = Arc::new(Hub { snap: RwLock::default(), cmd: Mutex::new(cmd), token: token(), port, state: state_tx, events, asked: AtomicU64::new(0), served: watch::channel(0).0 });

        let bridge = Arc::clone(&hub);
        std::thread::spawn(move || {
            let mut running: Option<HashMap<String, String>> = None;
            for snap in snaps {
                let state = model::state(&snap, &cfg);
                log_new_commands(&state, &mut running);
                for (text, error) in &snap.notices {
                    let _ = bridge.events.send(Arc::from(json!({"type": "notice", "text": text, "error": error}).to_string()));
                }
                let served = snap.served;
                *bridge.snap.write().unwrap() = snap;
                let text = json!({"type": "state", "data": state}).to_string();
                bridge.state.send_if_modified(|cur| {
                    let changed = **cur != *text;
                    if changed {
                        *cur = Arc::from(text);
                    }
                    changed
                });
                bridge.served.send_replace(served);
            }
        });

        let url = format!("http://127.0.0.1:{port}/?token={}", hub.token);
        println!("jugglr runs at {url}");
        println!("Press Ctrl+C to stop. Workspaces and agents keep running.");
        if open {
            let _ = std::process::Command::new("xdg-open").arg(&url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
        }

        let app = Router::new()
            .route("/api/events", get(events_ws))
            .route("/api/action", post(action))
            .route("/api/dirs", get(dirs))
            .route("/api/conversation", get(conversation))
            .route("/api/diff", get(diff))
            .route("/api/timeline", get(timeline))
            .route("/api/commands", get(commands))
            .route("/api/mr/changes", get(review::mr_changes))
            .route("/api/mr/drafts", get(review::mr_drafts))
            .route("/api/mr/job", get(review::mr_job))
            .route("/api/mr/pipelines", get(review::mr_pipelines))
            .route("/api/mr/pipeline", get(review::mr_pipeline))
            .route("/api/term", get(term::upgrade))
            .fallback(asset)
            .layer(middleware::from_fn_with_state(Arc::clone(&hub), guard))
            .with_state(hub);
        let shutdown = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
            eprintln!("jugglr: {e}");
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    })
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Every request must name this server as its host (against DNS rebinding) and carry the token cookie.
/// The link with ?token= sets the cookie. API writes also need a custom header, which a cross-site form cannot send.
async fn guard(State(hub): State<Arc<Hub>>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let allowed = [format!("127.0.0.1:{}", hub.port), format!("localhost:{}", hub.port), format!("[::1]:{}", hub.port)];
    if !allowed.iter().any(|a| a == host) {
        return (StatusCode::FORBIDDEN, "Unknown host").into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        if !allowed.iter().any(|a| origin == format!("http://{a}")) {
            return (StatusCode::FORBIDDEN, "Cross-origin request").into_response();
        }
    }
    // Chrome fetches the manifest and its icons without cookies when it installs the app. They hold no secrets.
    if is_install_asset(req.uri().path()) {
        return next.run(req).await;
    }
    let query: HashMap<String, String> = req.uri().query().map(|q| url_decode_query(q)).unwrap_or_default();
    if let Some(t) = query.get("token") {
        if !same_token(t, &hub.token) {
            return (StatusCode::UNAUTHORIZED, "Wrong token").into_response();
        }
        let cookie = format!("{COOKIE}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=31536000", hub.token);
        return Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(header::LOCATION, req.uri().path())
            .header(header::SET_COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
    }
    if !cookie_token(headers).is_some_and(|t| same_token(t, &hub.token)) {
        return (StatusCode::UNAUTHORIZED, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], LOCKED).into_response();
    }
    if req.method() == axum::http::Method::POST && headers.get("x-jugglr").is_none() {
        return (StatusCode::FORBIDDEN, "Missing header").into_response();
    }
    next.run(req).await
}

fn is_install_asset(path: &str) -> bool {
    let public = path == "/manifest.webmanifest" || path.strip_prefix("/icons/").is_some_and(|f| !f.is_empty() && !f.contains('/'));
    // Unknown paths fall back to index.html, which must stay behind the token.
    public && Assets::get(&path[1..]).is_some()
}

const LOCKED: &str = "<!doctype html><meta charset=utf-8><title>jugglr</title><body style=\"font:15px system-ui;background:#0b0d12;color:#c9d1e0;display:grid;place-items:center;height:100vh;margin:0\"><div style=\"max-width:28rem;text-align:center\"><h1 style=\"font-size:18px;color:#fff\">jugglr is locked</h1><p>Open the link that <code>jugglr web</code> printed in your terminal. It holds the key to this page.</p></div>";

fn url_decode_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|kv| kv.split_once('=').or(Some((kv, ""))))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(b)) => {
                out.push(b);
                i += 2;
            }
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, path) = match Assets::get(path) {
        Some(f) if !path.is_empty() => (f, path),
        // Client-side routes all load the app.
        _ => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => return (StatusCode::NOT_FOUND, "The web app was not built into this binary").into_response(),
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Vite puts a content hash in every asset name.
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap())
        .header(header::CACHE_CONTROL, cache)
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .body(Body::from(file.data.into_owned()))
        .unwrap()
}

async fn events_ws(ws: WebSocketUpgrade, State(hub): State<Arc<Hub>>) -> Response {
    ws.on_upgrade(move |socket| stream_events(socket, hub))
}

async fn stream_events(socket: WebSocket, hub: Arc<Hub>) {
    let (mut tx, mut rx) = socket.split();
    let mut state = hub.state.subscribe();
    let mut events = hub.events.subscribe();
    let first = state.borrow_and_update().clone();
    if tx.send(Message::Text((*first).into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                let text = state.borrow_and_update().clone();
                if tx.send(Message::Text((*text).into())).await.is_err() {
                    return;
                }
            }
            ev = events.recv() => {
                if let Ok(text) = ev {
                    if tx.send(Message::Text((*text).into())).await.is_err() {
                        return;
                    }
                }
            }
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                    // The page asks for a fresh look, e.g. when it becomes visible again.
                    Some(Ok(Message::Text(t))) if t.as_str() == "refresh" => hub.refresh(),
                    _ => {}
                }
            }
        }
    }
}

async fn action(State(hub): State<Arc<Hub>>, Json(body): Json<Value>) -> Response {
    let parsed: Result<actions::Action, _> = serde_json::from_value(body);
    let act = match parsed {
        Ok(a) => a,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": format!("Bad action: {e}")}))).into_response(),
    };
    let h = Arc::clone(&hub);
    let result = tokio::task::spawn_blocking(move || actions::run(&h, act)).await;
    // The page goes to what the action made as soon as it has the answer, so the state must show it by then.
    hub.fresh().await;
    match result {
        Ok(Ok(done)) => Json(json!({"ok": true, "message": done.message, "workspace": done.workspace, "window": done.window, "path": done.path})).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"ok": false, "error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}

async fn dirs(State(hub): State<Arc<Hub>>) -> Json<Value> {
    let snap = hub.snapshot();
    let dirs: Vec<Value> = snap.dirs.iter().map(|(p, repo)| json!([p, repo])).collect();
    // A review checkout is deleted when its workspace closes, but stays among the recent ones.
    let recent: Vec<&String> = snap.recent_dirs.iter().filter(|p| std::path::Path::new(p).is_dir()).collect();
    Json(json!({"dirs": dirs, "recent": recent, "home": crate::config::home()}))
}

async fn conversation(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let path = {
        let snap = hub.snapshot();
        q.get("agent").and_then(|k| snap.agents.iter().find(|a| &a.key == k)).and_then(|a| a.transcript_file().map(String::from))
    };
    let Some(path) = path else { return Json(json!({"items": [], "missing": true})).into_response() };
    let r = tokio::task::spawn_blocking(move || conversation::read(&path)).await.unwrap_or_default();
    Json(r).into_response()
}

async fn diff(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(path) = q.get("path").cloned() else { return (StatusCode::BAD_REQUEST, "path missing").into_response() };
    // Only directories the hub shows, so the page cannot read any repo on the machine.
    let known = {
        let snap = hub.snapshot();
        snap.workspaces.iter().any(|w| w.path == path) || snap.agents.iter().any(|a| a.path == path)
    };
    if !known {
        return (StatusCode::NOT_FOUND, "Unknown directory").into_response();
    }
    let file = q.get("file").cloned();
    let commit = q.get("commit").cloned();
    if commit.as_deref().is_some_and(|c| !review::is_sha(c)) {
        return (StatusCode::BAD_REQUEST, "Not a commit").into_response();
    }
    if q.contains_key("commits") {
        return Json(tokio::task::spawn_blocking(move || model::commits(&path)).await.unwrap_or_default()).into_response();
    }
    let text = tokio::task::spawn_blocking(move || model::diff(&path, file.as_deref(), commit.as_deref())).await.unwrap_or_default();
    Json(json!({"diff": text})).into_response()
}

/// Logs each command that starts in the foreground of a terminal. The first state only primes the
/// list, so a restart does not log what already ran.
fn log_new_commands(state: &Value, seen: &mut Option<HashMap<String, String>>) {
    let mut now_running = HashMap::new();
    let time = crate::state::now();
    for ws in state["workspaces"].as_array().into_iter().flatten() {
        for win in ws["windows"].as_array().into_iter().flatten() {
            let (Some(id), Some(cmd)) = (win["id"].as_str(), win["running"].as_str()) else { continue };
            if seen.as_ref().is_some_and(|s| s.get(id).map(String::as_str) != Some(cmd)) {
                crate::commands::shell(time, ws["name"].as_str().unwrap_or(""), id, win["name"].as_str().unwrap_or(""), cmd);
            }
            now_running.insert(id.to_string(), cmd.to_string());
        }
    }
    *seen = Some(now_running);
}

async fn commands(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let limit = q.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(100).clamp(1, 500);
    let mut entries = tokio::task::spawn_blocking(move || crate::commands::recent(limit)).await.unwrap_or_default();
    let snap = hub.snapshot();
    for e in &mut entries {
        let Some(id) = e["session"].as_str() else { continue };
        if let Some(a) = snap.agents.iter().find(|a| a.session_id() == Some(id)) {
            e["agent"] = a.key.clone().into();
            e["workspace"] = a.workspace.clone().into();
            e["window"] = a.window_id.clone().into();
            e["title"] = a.title().into();
        }
    }
    Json(Value::Array(entries))
}

async fn timeline(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let hours = q.get("hours").and_then(|h| h.parse::<u64>().ok()).unwrap_or(8).clamp(1, 168);
    let until = crate::state::now();
    let from = until - hours * 3600;
    let logs = tokio::task::spawn_blocking(move || crate::history::load(from)).await.unwrap_or_default();
    let snap = hub.snapshot();
    let mut lanes: Vec<Value> = logs
        .iter()
        .filter_map(|(id, changes)| {
            let agent = snap.agents.iter().find(|a| a.session_id() == Some(id));
            let alive = agent.is_some_and(|a| a.status.alive());
            let spans = crate::history::spans(changes, from, until, alive);
            let first = spans.first()?.0;
            Some(json!({
                "id": id,
                "agent": agent.map(|a| a.key.clone()),
                "title": agent.map_or_else(|| "Claude".to_string(), |a| a.title()),
                "name": agent.map(|a| a.name()),
                "status": agent.map(|a| model::status(a.status)),
                "first": first,
                "spans": spans.iter().map(|(s, e, st)| json!([s, e, model::status(*st)])).collect::<Vec<_>>(),
            }))
        })
        .collect();
    lanes.sort_by_key(|l| l["first"].as_u64());
    Json(json!({"from": from, "until": until, "lanes": lanes}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query_values() {
        let q = url_decode_query("window=%4012&cols=80&name=a+b&bad=%zz");
        assert_eq!(q["window"], "@12");
        assert_eq!(q["name"], "a b");
        assert_eq!(q["bad"], "%zz");
    }

    #[test]
    fn finds_the_token_among_cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("other=1; jugglr=abc; x=y"));
        assert_eq!(cookie_token(&h), Some("abc"));
        h.insert(header::COOKIE, HeaderValue::from_static("jugglrx=abc"));
        assert_eq!(cookie_token(&h), None);
    }

    #[test]
    fn opens_only_install_assets() {
        assert!(!is_install_asset("/"));
        assert!(!is_install_asset("/api/events"));
        assert!(!is_install_asset("/icons/"));
        assert!(!is_install_asset("/icons/missing.png"));
        assert!(!is_install_asset("/icons/../index.html"));
        assert_eq!(mime_guess::from_path("manifest.webmanifest").first_or_octet_stream(), "application/manifest+json");
    }

    #[test]
    fn compares_tokens_fully() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abc", "abd"));
        assert!(!same_token("abc", "ab"));
    }
}
