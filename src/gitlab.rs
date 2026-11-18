//! Merge request status and unresolved review threads, through the glab CLI.

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

const TTL: Duration = Duration::from_secs(60);
/// While a pipeline runs, so its jobs move along on screen.
const RUNNING_TTL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct Mr {
    pub iid: u64,
    pub project_id: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    /// success, failed, running, pending, canceled, ...
    pub pipeline: Option<String>,
    /// GitLab's detailed_merge_status: mergeable, not_approved, draft_status, ...
    pub merge_status: Option<String>,
    pub threads: Vec<Thread>,
    /// The head pipeline's jobs, oldest first, which is also stage order.
    pub jobs: Vec<Job>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub stage: String,
    pub status: String,
    /// Seconds, once the job has finished.
    pub duration: Option<f64>,
}

impl Mr {
    pub fn running(&self) -> bool {
        matches!(self.pipeline.as_deref(), Some("running" | "pending" | "created" | "preparing" | "waiting_for_resource"))
    }

    /// Stages in pipeline order, each with its jobs.
    pub fn stages(&self) -> Vec<(&str, Vec<&Job>)> {
        let mut out: Vec<(&str, Vec<&Job>)> = vec![];
        for j in &self.jobs {
            match out.iter_mut().find(|(s, _)| *s == j.stage) {
                Some((_, jobs)) => jobs.push(j),
                None => out.push((&j.stage, vec![j])),
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u64,
    pub author: String,
    pub body: String,
}

impl Thread {
    /// `src/app.rs:120`, or "General" for comments on the whole MR.
    pub fn place(&self) -> String {
        match (&self.path, self.line) {
            (Some(p), Some(l)) => format!("{p}:{l}"),
            (Some(p), None) => p.clone(),
            _ => "General".into(),
        }
    }

    /// The whole thread on one line, since a newline typed into Claude Code would send the prompt early.
    pub fn as_prompt(&self, mr: &Mr) -> String {
        let notes: Vec<String> = self.notes.iter().map(|n| format!("{}: {}", n.author, n.body.split_whitespace().collect::<Vec<_>>().join(" "))).collect();
        format!("Review comment on merge request !{} at {}. {}. Please address it.", mr.iid, self.place(), notes.join(" | "))
    }
}

fn glab(cwd: &str, args: &[&str]) -> Result<String, String> {
    // coreutils timeout, so a slow GitLab cannot hang the caller.
    let out = Command::new("timeout").arg("20").arg("glab").args(args).current_dir(cwd).output().map_err(|e| format!("glab: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("glab failed").trim().to_string())
    }
}

fn parse_mr(v: &Value) -> Option<Mr> {
    Some(Mr {
        iid: v["iid"].as_u64()?,
        project_id: v["project_id"].as_u64()?,
        title: v["title"].as_str()?.to_string(),
        url: v["web_url"].as_str().unwrap_or_default().to_string(),
        state: v["state"].as_str().unwrap_or_default().to_string(),
        pipeline: v["head_pipeline"]["status"].as_str().map(String::from),
        merge_status: v["detailed_merge_status"].as_str().map(String::from),
        threads: vec![],
        jobs: vec![],
    })
}

/// Jobs sorted by id: GitLab creates them stage by stage, so this is also stage order.
fn parse_jobs(v: &Value) -> Vec<Job> {
    let mut jobs: Vec<Job> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|j| {
            Some(Job {
                id: j["id"].as_u64()?,
                name: j["name"].as_str()?.to_string(),
                stage: j["stage"].as_str()?.to_string(),
                status: j["status"].as_str()?.to_string(),
                duration: j["duration"].as_f64(),
            })
        })
        .collect();
    jobs.sort_by_key(|j| j.id);
    jobs
}

/// Unresolved review threads; system notes ("added 2 commits") and resolved threads are left out.
fn parse_threads(v: &Value) -> Vec<Thread> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let notes = d["notes"].as_array()?;
            let first = notes.first()?;
            if first["system"] == true || first["resolvable"] != true || first["resolved"] == true {
                return None;
            }
            let pos = &first["position"];
            Some(Thread {
                id: d["id"].as_str()?.to_string(),
                path: pos["new_path"].as_str().or(pos["old_path"].as_str()).map(String::from),
                line: pos["new_line"].as_u64().or(pos["old_line"].as_u64()),
                notes: notes
                    .iter()
                    .filter(|n| n["system"] != true)
                    .filter_map(|n| Some(Note { id: n["id"].as_u64()?, author: n["author"]["username"].as_str()?.to_string(), body: n["body"].as_str()?.to_string() }))
                    .collect(),
            })
        })
        .collect()
}

fn fetch(cwd: &str, branch: &str) -> Option<Mr> {
    let v: Value = serde_json::from_str(&glab(cwd, &["mr", "view", branch, "-F", "json"]).ok()?).ok()?;
    let mut mr = parse_mr(&v)?;
    // The MR's own project id, so MRs from forks resolve to the right project.
    let path = format!("projects/{}/merge_requests/{}/discussions?per_page=100", mr.project_id, mr.iid);
    if let Ok(text) = glab(cwd, &["api", &path]) {
        mr.threads = serde_json::from_str(&text).map(|v| parse_threads(&v)).unwrap_or_default();
    }
    // A fork's pipeline runs in the fork, so ask the pipeline's own project.
    let p = &v["head_pipeline"];
    if let (Some(project), Some(id)) = (p["project_id"].as_u64(), p["id"].as_u64()) {
        if let Ok(text) = glab(cwd, &["api", &format!("projects/{project}/pipelines/{id}/jobs?per_page=100")]) {
            mr.jobs = serde_json::from_str(&text).map(|v| parse_jobs(&v)).unwrap_or_default();
        }
    }
    Some(mr)
}

pub fn reply(cwd: &str, mr: &Mr, thread: &str, body: &str) -> Result<(), String> {
    let path = format!("projects/{}/merge_requests/{}/discussions/{thread}/notes", mr.project_id, mr.iid);
    glab(cwd, &["api", "-X", "POST", &path, "-f", &format!("body={body}")]).map(|_| ())
}

pub fn open(url: &str) -> Result<(), String> {
    Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| format!("xdg-open: {e}"))
}

/// MRs per (workspace path, branch). Lookups never wait for the network: a stale entry starts a fetch in the background.
#[derive(Default)]
pub struct MrCache {
    items: Mutex<HashMap<(String, String), (Instant, Option<Mr>)>>,
    busy: Mutex<HashSet<(String, String)>>,
}

impl MrCache {
    pub fn get(self: &Arc<Self>, cwd: &str, branch: &str) -> Option<Mr> {
        // shortcut: assumes MRs never come from the default branch; skips a glab call per repo and minute.
        if matches!(branch, "main" | "master") {
            return None;
        }
        let key = (cwd.to_string(), branch.to_string());
        let cached = self.items.lock().unwrap().get(&key).cloned();
        let stale = |(at, mr): &(Instant, Option<Mr>)| at.elapsed() > if mr.as_ref().is_some_and(Mr::running) { RUNNING_TTL } else { TTL };
        if cached.as_ref().is_none_or(stale) && self.busy.lock().unwrap().insert(key.clone()) {
            let me = Arc::clone(self);
            std::thread::spawn(move || {
                let mr = fetch(&key.0, &key.1);
                me.items.lock().unwrap().insert(key.clone(), (Instant::now(), mr));
                me.busy.lock().unwrap().remove(&key);
            });
        }
        cached.and_then(|(_, mr)| mr)
    }

    /// Fetch this workspace's MR again on the next lookup, after a reply changed it. Keeps showing the old one until then.
    pub fn forget(&self, cwd: &str) {
        let Some(stale) = Instant::now().checked_sub(TTL * 2) else { return };
        for ((path, _), (at, _)) in self.items.lock().unwrap().iter_mut() {
            if path == cwd {
                *at = stale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_unresolved_review_threads() {
        let v: Value = serde_json::from_str(
            r#"[
            {"id":"sys","notes":[{"id":1,"system":true,"resolvable":false,"author":{"username":"bot"},"body":"added 1 commit"}]},
            {"id":"done","notes":[{"id":2,"system":false,"resolvable":true,"resolved":true,"author":{"username":"a"},"body":"fixed"}]},
            {"id":"open","notes":[
                {"id":3,"system":false,"resolvable":true,"resolved":false,"author":{"username":"vilu"},"body":"Rename\nthis","position":{"new_path":"src/x.rs","new_line":12}},
                {"id":4,"system":false,"resolvable":true,"resolved":false,"author":{"username":"me"},"body":"ok"}]}
        ]"#,
        )
        .unwrap();
        let threads = parse_threads(&v);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].place(), "src/x.rs:12");
        assert_eq!(threads[0].notes.len(), 2);
        let mr = Mr { iid: 7, project_id: 1, title: String::new(), url: String::new(), state: String::new(), pipeline: None, merge_status: None, threads: vec![], jobs: vec![] };
        assert_eq!(threads[0].as_prompt(&mr), "Review comment on merge request !7 at src/x.rs:12. vilu: Rename this | me: ok. Please address it.");
    }

    #[test]
    fn groups_jobs_into_stages_in_order() {
        let v: Value = serde_json::from_str(
            r#"[{"id":3,"name":"e2e","stage":"test","status":"running","duration":null},
                {"id":1,"name":"build","stage":"build","status":"success","duration":61.5},
                {"id":2,"name":"unit","stage":"test","status":"success","duration":30.0}]"#,
        )
        .unwrap();
        let mut mr = Mr { iid: 1, project_id: 1, title: String::new(), url: String::new(), state: String::new(), pipeline: Some("running".into()), merge_status: None, threads: vec![], jobs: parse_jobs(&v) };
        let stages: Vec<(&str, usize)> = mr.stages().iter().map(|(s, j)| (*s, j.len())).collect();
        assert_eq!(stages, [("build", 1), ("test", 2)]);
        assert!(mr.running());
        mr.pipeline = Some("success".into());
        assert!(!mr.running());
    }
}
