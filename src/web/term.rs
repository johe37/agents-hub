//! A live terminal in the page: a tmux client in a pty, bridged to a WebSocket.
//! Each page terminal attaches to its own grouped session, so it can show another window
//! than your terminal client shows, and closing it never closes the workspace.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;

use super::Hub;
use crate::tmux;

static NEXT: AtomicU32 = AtomicU32::new(1);

pub async fn upgrade(ws: WebSocketUpgrade, State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(window) = q.get("window").cloned() else { return (StatusCode::BAD_REQUEST, "window missing").into_response() };
    let size = |k: &str, d: u16| q.get(k).and_then(|v| v.parse::<u16>().ok()).unwrap_or(d).clamp(4, 1000);
    let (cols, rows) = (size("cols", 120), size("rows", 32));
    let session = hub.snapshot().workspaces.iter().find(|w| w.windows.iter().any(|x| x.window_id == window)).map(|w| w.name.clone());
    let Some(session) = session else { return (StatusCode::NOT_FOUND, "This window is gone").into_response() };
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = serve(socket, &session, &window, cols, rows).await {
            eprintln!("claude-hub: terminal {window}: {e}");
        }
    })
}

#[derive(Deserialize)]
struct Control {
    cols: u16,
    rows: u16,
}

struct Grouped(String);

impl Drop for Grouped {
    fn drop(&mut self) {
        let _ = tmux::call(&["kill-session", "-t", &format!("={}", self.0)]);
    }
}

async fn serve(socket: WebSocket, session: &str, window: &str, cols: u16, rows: u16) -> Result<(), String> {
    let name = format!("_web{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    tmux::call(&["new-session", "-d", "-s", &name, "-t", &format!("={session}")])?;
    let grouped = Grouped(name.clone());
    // The page has its own window tabs; tmux's bar would only repeat them.
    tmux::call(&["set-option", "-t", &format!("={name}:"), "status", "off"])?;
    tmux::call(&["select-window", "-t", &format!("={name}:{window}")])?;

    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new("tmux");
    cmd.args(["-L", tmux::SOCKET_NAME, "attach-session", "-t", &format!("={name}")]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("TMUX");
    cmd.cwd(crate::config::home());
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let master = pair.master;
    let mut reader = master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = master.take_writer().map_err(|e| e.to_string())?;

    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
            }
        }
    });
    // Writes can block while tmux is busy; keep them off the async workers.
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for data in in_rx {
            if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                return;
            }
        }
    });

    let (mut ws_tx, mut ws_rx) = socket.split();
    loop {
        tokio::select! {
            out = out_rx.recv() => match out {
                Some(data) => {
                    if ws_tx.send(Message::Binary(data.into())).await.is_err() {
                        break;
                    }
                }
                // tmux exited: the window or the workspace closed.
                None => {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    break;
                }
            },
            msg = ws_rx.next() => match msg {
                Some(Ok(Message::Binary(data))) => {
                    let _ = in_tx.send(data.to_vec());
                }
                Some(Ok(Message::Text(text))) => {
                    if let Ok(c) = serde_json::from_str::<Control>(text.as_str()) {
                        let _ = master.resize(PtySize { rows: c.rows.clamp(4, 1000), cols: c.cols.clamp(4, 1000), pixel_width: 0, pixel_height: 0 });
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    drop(grouped);
    Ok(())
}
