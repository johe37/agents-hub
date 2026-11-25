//! A merge request's commits and diff, for reviewing it in the page.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{Value, json};

use super::Hub;
use crate::gitlab::{self, FileDiff, Job, Mr, Pipeline};

/// Diffs by commit: a commit never changes, so they never go stale.
static DIFFS: LazyLock<Mutex<HashMap<String, Arc<Vec<FileDiff>>>>> = LazyLock::new(Mutex::default);

fn cached(key: String, fetch: impl FnOnce() -> Result<Vec<FileDiff>, String>) -> Result<Arc<Vec<FileDiff>>, String> {
    if let Some(d) = DIFFS.lock().unwrap().get(&key) {
        return Ok(Arc::clone(d));
    }
    let d = Arc::new(fetch()?);
    let mut map = DIFFS.lock().unwrap();
    if map.len() > 200 {
        map.clear();
    }
    map.insert(key, Arc::clone(&d));
    Ok(d)
}

fn files_json(files: &[FileDiff]) -> Vec<Value> {
    files
        .iter()
        .map(|f| json!({"old_path": f.old_path, "new_path": f.new_path, "diff": f.diff, "new_file": f.new_file, "deleted": f.deleted, "renamed": f.renamed, "too_large": f.too_large}))
        .collect()
}

fn reviewed_file() -> std::path::PathBuf {
    crate::config::state_dir().join("reviewed.json")
}

/// The head commit of each MR when you last finished a review or approved it, by MR key.
fn reviewed_all() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(reviewed_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn reviewed(key: &str) -> Option<String> {
    reviewed_all().get(key).and_then(|v| v.as_str()).map(String::from)
}

pub fn mark_reviewed(key: &str, head: &str) {
    let mut all = reviewed_all();
    all.insert(key.to_string(), Value::from(head));
    let _ = std::fs::create_dir_all(crate::config::state_dir());
    let _ = std::fs::write(reviewed_file(), Value::Object(all).to_string());
}

pub fn is_sha(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn changes(key: &str, mr: &Mr, commit: Option<&str>, since: Option<&str>) -> Result<Value, String> {
    if let Some(sha) = commit {
        let files = cached(format!("commit:{}:{sha}", mr.project_id), || gitlab::commit_diff(mr, sha))?;
        return Ok(json!({"files": files_json(&files)}));
    }
    if let Some(from) = since {
        let to = gitlab::mr_version(mr)?.head;
        let files = cached(format!("compare:{}:{from}:{to}", mr.project_id), || gitlab::compare_diff(mr, from, &to))?;
        return Ok(json!({"files": files_json(&files)}));
    }
    let (version, commits) = std::thread::scope(|s| {
        let commits = s.spawn(|| gitlab::mr_commits(mr));
        (gitlab::mr_version(mr), commits.join().unwrap_or_else(|_| Err("glab failed".into())))
    });
    let (version, commits) = (version?, commits?);
    let files = cached(format!("mr:{}:{}:{}", mr.project_id, version.start, version.head), || gitlab::mr_diff(mr))?;
    Ok(json!({
        "version": {"base": version.base, "start": version.start, "head": version.head},
        "commits": commits.iter().map(|c| json!({"sha": c.sha, "short": c.short, "title": c.title, "message": c.message, "author": c.author, "created": c.created})).collect::<Vec<_>>(),
        "files": files_json(&files),
        "reviewed": reviewed(key).filter(|r| *r != version.head),
    }))
}

pub async fn mr_changes(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(key) = q.get("mr") else { return (StatusCode::BAD_REQUEST, "mr missing").into_response() };
    // Only MRs the hub lists, so the page cannot read any project.
    let mr = match super::actions::find_mr(&hub, key) {
        Ok(m) => m,
        Err(e) => return (StatusCode::NOT_FOUND, Json(json!({"error": e}))).into_response(),
    };
    let (commit, since) = (q.get("commit").cloned(), q.get("since").cloned());
    if [&commit, &since].iter().any(|c| c.as_deref().is_some_and(|c| !is_sha(c))) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "Not a commit"}))).into_response();
    }
    let key = key.clone();
    match tokio::task::spawn_blocking(move || changes(&key, &mr, commit.as_deref(), since.as_deref())).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn mr_drafts(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(key) = q.get("mr") else { return (StatusCode::BAD_REQUEST, "mr missing").into_response() };
    let mr = match super::actions::find_mr(&hub, key) {
        Ok(m) => m,
        Err(e) => return (StatusCode::NOT_FOUND, Json(json!({"error": e}))).into_response(),
    };
    match tokio::task::spawn_blocking(move || gitlab::drafts(&mr)).await {
        Ok(Ok(list)) => Json(json!({
            "drafts": list.iter().map(|d| json!({"id": d.id, "body": d.body, "path": d.path, "line": d.line, "old_line": d.old_line})).collect::<Vec<_>>(),
        }))
        .into_response(),
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

/// The pipelines last listed and the jobs of older pipelines last shown, by MR key.
/// A job or a pipeline must be in them before the page may act on it.
static PIPELINES: LazyLock<Mutex<HashMap<String, Vec<Pipeline>>>> = LazyLock::new(Mutex::default);
static OLD_JOBS: LazyLock<Mutex<HashMap<String, Vec<Job>>>> = LazyLock::new(Mutex::default);

fn remember<T>(map: &Mutex<HashMap<String, Vec<T>>>, key: &str, list: Vec<T>) {
    let mut map = map.lock().unwrap();
    if map.len() > 200 {
        map.clear();
    }
    map.insert(key.to_string(), list);
}

fn listed(key: &str, id: u64) -> Option<Pipeline> {
    PIPELINES.lock().unwrap().get(key)?.iter().find(|p| p.id == id).cloned()
}

/// A pipeline of the MR. A new one may not be listed yet, so ask GitLab again before giving up.
pub(super) fn pipeline_of(mr: &Mr, key: &str, id: u64) -> Result<Pipeline, String> {
    if let Some(p) = listed(key, id) {
        return Ok(p);
    }
    remember(&PIPELINES, key, gitlab::mr_pipelines(mr)?);
    listed(key, id).ok_or_else(|| "The pipeline is not in this merge request".to_string())
}

/// A job of the MR's head pipeline, or of the older pipeline last shown.
pub(super) fn job_of(mr: &Mr, key: &str, id: u64) -> Option<Job> {
    mr.jobs.iter().find(|j| j.id == id).cloned().or_else(|| OLD_JOBS.lock().unwrap().get(key)?.iter().find(|j| j.id == id).cloned())
}

/// The MR's pipelines, newest first, and which one is its head pipeline.
pub async fn mr_pipelines(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(key) = q.get("mr").cloned() else { return (StatusCode::BAD_REQUEST, "mr missing").into_response() };
    let mr = match super::actions::find_mr(&hub, &key) {
        Ok(m) => m,
        Err(e) => return (StatusCode::NOT_FOUND, Json(json!({"error": e}))).into_response(),
    };
    let head = mr.pipeline_url.as_deref().and_then(|u| u.rsplit('/').next()?.parse::<u64>().ok());
    match tokio::task::spawn_blocking(move || gitlab::mr_pipelines(&mr)).await {
        Ok(Ok(list)) => {
            let out: Vec<Value> = list
                .iter()
                .map(|p| {
                    json!({
                        "id": p.id, "status": p.status, "sha": p.sha, "title": p.title, "user": p.user,
                        "created": p.created, "duration": p.duration, "url": p.url,
                        "stages": p.stages.iter().map(|(name, status)| json!({"name": name, "status": status})).collect::<Vec<_>>(),
                    })
                })
                .collect();
            remember(&PIPELINES, &key, list);
            Json(json!({"pipelines": out, "head": head})).into_response()
        }
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

/// The jobs of one of the MR's pipelines.
pub async fn mr_pipeline(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let (Some(key), Some(id)) = (q.get("mr").cloned(), q.get("id").and_then(|j| j.parse::<u64>().ok())) else {
        return (StatusCode::BAD_REQUEST, "mr or id missing").into_response();
    };
    let mr = match super::actions::find_mr(&hub, &key) {
        Ok(m) => m,
        Err(e) => return (StatusCode::NOT_FOUND, Json(json!({"error": e}))).into_response(),
    };
    let jobs = tokio::task::spawn_blocking(move || {
        let jobs = gitlab::pipeline_jobs(&pipeline_of(&mr, &key, id)?)?;
        remember(&OLD_JOBS, &key, jobs.clone());
        Ok::<_, String>(jobs)
    });
    match jobs.await {
        Ok(Ok(jobs)) => Json(json!({"jobs": jobs.iter().map(super::model::job_json).collect::<Vec<_>>()})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

/// A job of one of the MR's pipelines: its log, the last 512 kB.
pub async fn mr_job(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let (Some(key), Some(id)) = (q.get("mr"), q.get("job").and_then(|j| j.parse::<u64>().ok())) else {
        return (StatusCode::BAD_REQUEST, "mr or job missing").into_response();
    };
    let job = super::actions::find_mr(&hub, key).ok().and_then(|m| job_of(&m, key, id));
    let Some(job) = job else { return (StatusCode::NOT_FOUND, Json(json!({"error": "The job is not in this merge request's pipelines"}))).into_response() };
    match tokio::task::spawn_blocking(move || gitlab::job_log(&job.url, job.id, 512 * 1024)).await {
        Ok(Ok((log, truncated))) => Json(json!({"log": log, "truncated": truncated})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}
