//! What the chat's prompt box needs: slash commands and files to complete, and pasted images.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde_json::{Value, json};

use super::Hub;
use crate::config::{home, state_dir};

const MAX_FILES: usize = 30_000;
const KEEP_UPLOADS: Duration = Duration::from_secs(7 * 24 * 3600);

/// Built-in commands that make sense to type from the page.
const BUILTIN: &[(&str, &str)] = &[
    ("clear", "Clear the conversation and start over"),
    ("compact", "Summarize the conversation to free context"),
    ("context", "Show how the context window is used"),
    ("cost", "Show the cost of this session"),
    ("init", "Write a CLAUDE.md for this project"),
    ("memory", "Edit Claude's memory files"),
    ("model", "Change the model"),
    ("review", "Review a pull request"),
    ("security-review", "Review the pending changes for security issues"),
    ("status", "Show the session status"),
    ("todos", "Show the todo list"),
    ("usage", "Show plan usage"),
    ("rewind", "Go back to an earlier point in the conversation"),
    ("export", "Export the conversation"),
];

fn agent_path(hub: &Hub, key: Option<&String>) -> Option<String> {
    let snap = hub.snapshot();
    snap.agents.iter().find(|a| Some(&a.key) == key).map(|a| a.path.clone())
}

pub async fn complete(State(hub): State<Arc<Hub>>, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let Some(path) = agent_path(&hub, q.get("agent")) else { return (StatusCode::NOT_FOUND, "Unknown agent").into_response() };
    let r = tokio::task::spawn_blocking(move || json!({"commands": commands(Path::new(&path)), "files": files(&path)})).await;
    Json(r.unwrap_or_default()).into_response()
}

const MAX_ENTRIES: usize = 2_000;

/// The entries of one directory, for `@~/…`, `@/…` and `@../…`. A relative directory is the agent's.
pub async fn entries(State(hub): State<Arc<Hub>>, Query(q): Query<std::collections::HashMap<String, String>>) -> Response {
    let Some(path) = agent_path(&hub, q.get("agent")) else { return (StatusCode::NOT_FOUND, "Unknown agent").into_response() };
    let dir = resolve(&path, q.get("dir").map(String::as_str).unwrap_or(""));
    let r = tokio::task::spawn_blocking(move || list(&dir)).await;
    Json(json!({"entries": r.unwrap_or_default()})).into_response()
}

fn resolve(base: &str, dir: &str) -> PathBuf {
    if dir == "~" {
        home()
    } else if let Some(rest) = dir.strip_prefix("~/") {
        home().join(rest)
    } else {
        Path::new(base).join(dir)
    }
}

fn list(dir: &Path) -> Vec<Value> {
    let mut out: Vec<(bool, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .take(MAX_ENTRIES)
        .map(|e| (e.path().is_dir(), e.file_name().to_string_lossy().into_owned()))
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    out.into_iter().map(|(d, n)| json!({"name": n, "dir": d})).collect()
}

fn commands(path: &Path) -> Vec<Value> {
    let mut out: Vec<(String, String)> = BUILTIN.iter().map(|(n, d)| (n.to_string(), d.to_string())).collect();
    let roots = [home().join(".claude"), path.join(".claude")];
    for root in &roots {
        command_files(&root.join("commands"), "", &mut out);
        for dir in std::fs::read_dir(root.join("skills")).into_iter().flatten().flatten() {
            let file = dir.path().join("SKILL.md");
            if let Ok(text) = std::fs::read_to_string(&file) {
                out.push((dir.file_name().to_string_lossy().into_owned(), description(&text)));
            }
        }
    }
    // A project command with the same name as a personal one wins.
    let mut seen = std::collections::HashSet::new();
    let mut list: Vec<Value> = out.into_iter().rev().filter(|(n, _)| seen.insert(n.clone())).map(|(n, d)| json!({"name": n, "description": d})).collect();
    list.reverse();
    list
}

/// A command in a subdirectory gets the directory as a prefix, as in `/frontend:test`.
fn command_files(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            command_files(&p, &format!("{prefix}{name}:"), out);
        } else if let Some(stem) = name.strip_suffix(".md") {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            out.push((format!("{prefix}{stem}"), description(&text)));
        }
    }
}

/// The front matter's description, else the first line of text.
fn description(text: &str) -> String {
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---") {
        if let Some((front, after)) = rest.split_once("\n---") {
            if let Some(d) = front.lines().find_map(|l| l.trim().strip_prefix("description:")) {
                return d.trim().trim_matches('"').trim_matches('\'').to_string();
            }
            body = after;
        }
    }
    let line = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    line.trim_start_matches('#').trim().chars().take(120).collect()
}

fn files(path: &str) -> Vec<String> {
    let out = Command::new("git").args(["-C", path, "ls-files", "-z", "--cached", "--others", "--exclude-standard"]).output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else { return vec![] };
    out.stdout.split(|&b: &u8| b == 0).filter(|f: &&[u8]| !f.is_empty()).take(MAX_FILES).map(|f| String::from_utf8_lossy(f).into_owned()).collect()
}

/// Save a pasted image where Claude can read it; the prompt names the file.
pub async fn upload(headers: HeaderMap, body: Bytes) -> Response {
    let kind = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let ext = match kind {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Only PNG, JPEG, GIF and WebP images").into_response(),
    };
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "Empty image").into_response();
    }
    let r = tokio::task::spawn_blocking(move || save(&body, ext)).await;
    match r {
        Ok(Ok(p)) => Json(json!({"path": p})).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn save(data: &[u8], ext: &str) -> Result<String, String> {
    let dir = state_dir().join("uploads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    forget_old(&dir);
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path: PathBuf = dir.join(format!("paste-{ms}.{ext}"));
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

fn forget_old(dir: &Path) {
    let now = SystemTime::now();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > KEEP_UPLOADS);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_typed_directories() {
        assert_eq!(resolve("/repo", "~/chats/"), home().join("chats/"));
        assert_eq!(resolve("/repo", "~"), home());
        assert_eq!(resolve("/repo", "/etc/"), PathBuf::from("/etc/"));
        assert_eq!(resolve("/repo", "../x/"), PathBuf::from("/repo/../x/"));
    }

    #[test]
    fn reads_a_description() {
        assert_eq!(description("---\nname: x\ndescription: \"Run the tests\"\n---\nBody"), "Run the tests");
        assert_eq!(description("---\nname: x\n---\n\n# Deploy the app\n"), "Deploy the app");
        assert_eq!(description("Plain text"), "Plain text");
    }
}
