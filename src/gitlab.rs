//! Merge request status, unresolved review threads and assigned issues, through the glab CLI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

const TTL: Duration = Duration::from_secs(60);
/// While a pipeline runs, so its jobs move along on screen.
const RUNNING_TTL: Duration = Duration::from_secs(10);
/// MRs waiting for your review and your issues: one call per MR, so not too often.
const INBOX_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Default)]
pub struct Mr {
    pub iid: u64,
    pub project_id: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    /// success, failed, running, pending, canceled, ...
    pub pipeline: Option<String>,
    pub pipeline_url: Option<String>,
    /// GitLab's detailed_merge_status: mergeable, not_approved, draft_status, ...
    pub merge_status: Option<String>,
    pub threads: Vec<Thread>,
    /// The head pipeline's jobs, oldest first, which is also stage order.
    pub jobs: Vec<Job>,
    pub author: String,
    /// Full project path, `craton/auxiliary/craton-cli`.
    pub project: String,
    /// Last activity, seconds since the epoch.
    pub updated: u64,
    pub description: String,
    pub draft: bool,
    /// Usernames; only fetched for MRs you review.
    pub approved_by: Vec<String>,
    /// You are among approved_by.
    pub approved: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Issue {
    pub iid: u64,
    pub title: String,
    pub url: String,
    /// Full project path, `craton/auxiliary/craton-cli`.
    pub project: String,
    pub labels: Vec<String>,
    pub milestone: Option<String>,
    pub author: String,
    /// Last activity, seconds since the epoch.
    pub updated: u64,
    pub description: String,
}

impl Issue {
    pub fn repo(&self) -> &str {
        self.project.rsplit('/').next().unwrap_or(&self.project)
    }

    pub fn prompt(&self) -> String {
        format!(
            "Work on issue #{} in {}: \"{}\". Read it with `glab issue view {} -R {} --comments`. Create a branch for it first, then plan the change before you edit any code.",
            self.iid, self.project, self.title, self.iid, self.project
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub stage: String,
    /// GitLab's job status, except a failure that `allow_failure` permits is "warning", as GitLab shows it.
    pub status: String,
    /// Seconds, once the job has finished.
    pub duration: Option<f64>,
    pub url: String,
}

impl Mr {
    /// The repo's own name, `craton-cli`.
    pub fn repo(&self) -> &str {
        self.project.rsplit('/').next().unwrap_or(&self.project)
    }

    /// Where its code is checked out for a review.
    pub fn review_dir(&self) -> PathBuf {
        reviews_dir().join(format!("{}-{}", self.repo(), self.iid))
    }

    /// The prompt that starts a review; one line, so typing it does not send it.
    pub fn review_prompt(&self) -> String {
        format!(
            "Review merge request !{} in {}: \"{}\" by {}. Its code is checked out in this directory; see the change with `glab mr diff {} -R {}`. Look for bugs, risky changes and missing tests, and list each finding with its file and line. Don't post anything on GitLab.",
            self.iid, self.project, self.title, self.author, self.iid, self.project
        )
    }

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
    run(cwd, 20, "glab", args)
}

/// Run a program in cwd. On failure, the line of its error output that says what went wrong.
fn run(cwd: &str, secs: u32, program: &str, args: &[&str]) -> Result<String, String> {
    // coreutils timeout, so a slow GitLab or remote cannot hang the caller.
    let out = Command::new("timeout")
        .arg(secs.to_string())
        .arg(program)
        .args(args)
        .current_dir(cwd)
        // Never wait for a password prompt nobody can see.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(failure(&String::from_utf8_lossy(&out.stderr)).unwrap_or(format!("{program} failed")))
}

/// The line of error output that says what went wrong.
fn failure(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // git push starts with "To <remote>"; the useful part is the error or rejection further down.
    let line = lines.iter().find(|l| l.starts_with("error:") || l.starts_with("fatal:") || l.starts_with("! [")).or(lines.first());
    line.map(|l| l.to_string())
}

fn parse_mr(v: &Value) -> Option<Mr> {
    Some(Mr {
        iid: v["iid"].as_u64()?,
        project_id: v["project_id"].as_u64()?,
        title: v["title"].as_str()?.to_string(),
        url: v["web_url"].as_str().unwrap_or_default().to_string(),
        state: v["state"].as_str().unwrap_or_default().to_string(),
        pipeline: v["head_pipeline"]["status"].as_str().map(String::from),
        pipeline_url: v["head_pipeline"]["web_url"].as_str().map(String::from),
        merge_status: v["detailed_merge_status"].as_str().map(String::from),
        threads: vec![],
        jobs: vec![],
        author: v["author"]["username"].as_str().unwrap_or_default().to_string(),
        project: v["references"]["full"].as_str().and_then(|r| r.split('!').next()).unwrap_or_default().to_string(),
        updated: v["updated_at"].as_str().and_then(epoch).unwrap_or(0),
        description: v["description"].as_str().unwrap_or_default().to_string(),
        draft: v["draft"] == true,
        approved_by: vec![],
        approved: false,
    })
}

fn parse_issue(v: &Value) -> Option<Issue> {
    Some(Issue {
        iid: v["iid"].as_u64()?,
        title: v["title"].as_str()?.to_string(),
        url: v["web_url"].as_str().unwrap_or_default().to_string(),
        project: v["references"]["full"].as_str().and_then(|r| r.split('#').next()).unwrap_or_default().to_string(),
        labels: v["labels"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect(),
        milestone: v["milestone"]["title"].as_str().map(String::from),
        author: v["author"]["username"].as_str().unwrap_or_default().to_string(),
        updated: v["updated_at"].as_str().and_then(epoch).unwrap_or(0),
        description: v["description"].as_str().unwrap_or_default().to_string(),
    })
}

/// Seconds since the epoch for GitLab's `2026-11-18T13:42:24.086+01:00`.
fn epoch(s: &str) -> Option<u64> {
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let secs = num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?;
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    let zone = &s[19..];
    let tz = zone.rfind(['+', '-']).filter(|_| !zone.ends_with('Z')).and_then(|i| {
        let sign = if zone.as_bytes()[i] == b'-' { -1 } else { 1 };
        let h: i64 = zone.get(i + 1..i + 3)?.parse().ok()?;
        let m: i64 = zone.get(i + 4..i + 6)?.parse().ok()?;
        Some(sign * (h * 3600 + m * 60))
    });
    u64::try_from(days * 86400 + secs - tz.unwrap_or(0)).ok()
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
                status: match j["status"].as_str()? {
                    "failed" if j["allow_failure"] == true => "warning".to_string(),
                    s => s.to_string(),
                },
                duration: j["duration"].as_f64(),
                url: j["web_url"].as_str().unwrap_or_default().to_string(),
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
    fill(cwd, &mut mr, &v);
    Some(mr)
}

/// Add the unresolved threads and the pipeline's jobs to an MR fetched as v.
fn fill(cwd: &str, mr: &mut Mr, v: &Value) {
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
}

/// Open MRs where user is a requested reviewer, newest activity first.
fn fetch_reviews(user: &str) -> Option<Vec<Mr>> {
    let home = crate::config::home().display().to_string();
    let list: Value = serde_json::from_str(&glab(&home, &["api", &format!("merge_requests?scope=all&state=opened&reviewer_username={user}&per_page=50")]).ok()?).ok()?;
    let ids: Vec<(u64, u64)> = list.as_array()?.iter().filter_map(|m| Some((m["project_id"].as_u64()?, m["iid"].as_u64()?))).collect();
    // The list leaves out the pipeline, so fetch each MR; in parallel, since they are independent.
    let mut mrs: Vec<Mr> = std::thread::scope(|s| {
        let handles: Vec<_> = ids
            .iter()
            .map(|&(project, iid)| {
                let home = &home;
                s.spawn(move || {
                    let v: Value = serde_json::from_str(&glab(home, &["api", &format!("projects/{project}/merge_requests/{iid}")]).ok()?).ok()?;
                    let mut mr = parse_mr(&v)?;
                    fill(home, &mut mr, &v);
                    if let Ok(text) = glab(home, &["api", &format!("projects/{project}/merge_requests/{iid}/approvals")]) {
                        let a: Value = serde_json::from_str(&text).unwrap_or_default();
                        mr.approved_by = a["approved_by"].as_array().into_iter().flatten().filter_map(|u| u["user"]["username"].as_str().map(String::from)).collect();
                        mr.approved = mr.approved_by.iter().any(|u| u == user);
                    }
                    Some(mr)
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    mrs.sort_by_key(|m| std::cmp::Reverse(m.updated));
    Some(mrs)
}

/// Open issues assigned to you, newest activity first.
fn fetch_issues() -> Option<Vec<Issue>> {
    let home = crate::config::home().display().to_string();
    let list: Value = serde_json::from_str(&glab(&home, &["api", "issues?scope=assigned_to_me&state=opened&order_by=updated_at&per_page=50"]).ok()?).ok()?;
    Some(list.as_array()?.iter().filter_map(parse_issue).collect())
}

pub fn reviews_dir() -> PathBuf {
    crate::config::cache_dir().join("reviews")
}

/// The local clone of project among the scanned dirs: one whose remote is the project, else one with its name.
pub fn find_clone(dirs: &[(String, bool)], project: &str) -> Option<String> {
    let name = project.rsplit('/').next()?;
    let clones: Vec<&String> = dirs.iter().filter(|(p, repo)| *repo && Path::new(p).file_name().is_some_and(|n| n == name)).map(|(p, _)| p).collect();
    let is_project = |p: &&&String| run(p, 5, "git", &["remote", "-v"]).is_ok_and(|r| r.contains(&format!("{project}.git")) || r.contains(&format!("{project} ")));
    clones.iter().find(is_project).or(clones.first()).map(|p| p.to_string())
}

/// The clone URL of project on the same host and protocol as remote, `git@host:a/b.git` or `https://host/a/b.git`.
fn project_url(remote: &str, project: &str) -> Option<String> {
    match remote.split_once("://") {
        Some((scheme, rest)) => Some(format!("{scheme}://{}/{project}.git", rest.split('/').next()?)),
        None => Some(format!("{}:{project}.git", remote.split_once(':')?.0)),
    }
}

/// Check out the MR's latest code in its own worktree of the clone at repo, and return that directory.
/// Fetching from the MR's project works for MRs from forks too: GitLab keeps every MR's head there.
pub fn checkout(repo: &str, mr: &Mr) -> Result<String, String> {
    let remote = run(repo, 5, "git", &["remote"])?.lines().next().ok_or("The clone has no remote")?.to_string();
    let remote_url = run(repo, 5, "git", &["remote", "get-url", &remote])?;
    let url = project_url(remote_url.trim(), &mr.project).ok_or("Can't make out the clone's remote URL")?;
    run(repo, 120, "git", &["fetch", &url, &format!("refs/merge-requests/{}/head", mr.iid)])?;
    let sha = run(repo, 5, "git", &["rev-parse", "FETCH_HEAD"])?.trim().to_string();
    let dir = mr.review_dir();
    let d = dir.display().to_string();
    if dir.exists() {
        run(&d, 30, "git", &["checkout", "--detach", &sha])?;
    } else {
        std::fs::create_dir_all(reviews_dir()).map_err(|e| e.to_string())?;
        run(repo, 60, "git", &["worktree", "add", "--detach", &d, &sha])?;
    }
    Ok(d)
}

/// Delete a review checkout, including anything changed in it.
pub fn remove_checkout(dir: &str) -> Result<(), String> {
    run(dir, 30, "git", &["worktree", "remove", "--force", dir]).map(|_| ())
}

#[derive(Clone, Default)]
pub struct Inbox {
    pub reviews: Vec<Mr>,
    pub issues: Vec<Issue>,
}

/// MRs to review and issues assigned to you, refreshed in the background like `MrCache`.
#[derive(Default)]
pub struct InboxCache {
    state: Mutex<(Option<Instant>, Inbox)>,
    busy: Mutex<bool>,
    user: Mutex<Option<String>>,
}

impl InboxCache {
    pub fn get(self: &Arc<Self>, refresh: bool) -> Inbox {
        let (at, inbox) = self.state.lock().unwrap().clone();
        if refresh && at.is_none_or(|at| at.elapsed() > INBOX_TTL) && !std::mem::replace(&mut *self.busy.lock().unwrap(), true) {
            let me = Arc::clone(self);
            std::thread::spawn(move || {
                let home = crate::config::home().display().to_string();
                let user = me.user.lock().unwrap().clone().or_else(|| {
                    let v: Value = serde_json::from_str(&glab(&home, &["api", "user"]).ok()?).ok()?;
                    v["username"].as_str().map(String::from)
                });
                *me.user.lock().unwrap() = user.clone();
                let reviews = user.and_then(|u| fetch_reviews(&u));
                let issues = fetch_issues();
                let mut state = me.state.lock().unwrap();
                // Keep the old lists when GitLab is unreachable; try again after the TTL either way.
                let old = std::mem::take(&mut state.1);
                *state = (Some(Instant::now()), Inbox { reviews: reviews.unwrap_or(old.reviews), issues: issues.unwrap_or(old.issues) });
                *me.busy.lock().unwrap() = false;
            });
        }
        inbox
    }

    pub fn forget(&self) {
        self.state.lock().unwrap().0 = None;
    }
}

pub fn reply(cwd: &str, mr: &Mr, thread: &str, body: &str) -> Result<(), String> {
    let path = format!("projects/{}/merge_requests/{}/discussions/{thread}/notes", mr.project_id, mr.iid);
    glab(cwd, &["api", "-X", "POST", &path, "-f", &format!("body={body}")]).map(|_| ())
}

/// Mark a review thread resolved.
pub fn resolve(cwd: &str, mr: &Mr, thread: &str) -> Result<(), String> {
    let path = format!("projects/{}/merge_requests/{}/discussions/{thread}", mr.project_id, mr.iid);
    glab(cwd, &["api", "-X", "PUT", &path, "-f", "resolved=true"]).map(|_| ())
}

/// The short id of the commit checked out in cwd.
pub fn head(cwd: &str) -> Option<String> {
    run(cwd, 5, "git", &["rev-parse", "--short", "HEAD"]).ok().map(|s| s.trim().to_string())
}

/// Push the checked-out branch, setting its upstream on the first push, then open a draft MR when asked.
pub fn publish(cwd: &str, open_mr: bool) -> Result<String, String> {
    let branch = run(cwd, 5, "git", &["branch", "--show-current"])?.trim().to_string();
    if run(cwd, 5, "git", &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).is_ok() {
        run(cwd, 120, "git", &["push"])?;
    } else {
        let remotes = run(cwd, 5, "git", &["remote"])?;
        let remote = remotes.lines().find(|r| *r == "origin").or(remotes.lines().next()).ok_or("This repo has no remote to push to")?.to_string();
        run(cwd, 120, "git", &["push", "-u", &remote, "HEAD"])?;
    }
    if !open_mr {
        return Ok(format!("Pushed {branch}"));
    }
    let out = glab(cwd, &["mr", "create", "--draft", "--fill", "--yes"])?;
    let url = out.lines().map(str::trim).find(|l| l.contains("/merge_requests/")).unwrap_or_default();
    Ok(format!("Pushed {branch} and opened a draft merge request {url}"))
}


/// MRs per (workspace path, branch). Lookups never wait for the network: a stale entry starts a fetch in the background.
#[derive(Default)]
pub struct MrCache {
    items: Mutex<HashMap<(String, String), (Instant, Option<Mr>)>>,
    busy: Mutex<HashSet<(String, String)>>,
}

impl MrCache {
    /// The cached MR. Fetches it again in the background when stale and refresh is set.
    pub fn get(self: &Arc<Self>, cwd: &str, branch: &str, refresh: bool) -> Option<Mr> {
        // shortcut: assumes MRs never come from the default branch; skips a glab call per repo and minute.
        // A detached checkout, like a review, is "(detached)": no branch to look up.
        if matches!(branch, "main" | "master") || branch.starts_with('(') {
            return None;
        }
        let key = (cwd.to_string(), branch.to_string());
        let cached = self.items.lock().unwrap().get(&key).cloned();
        let stale = |(at, mr): &(Instant, Option<Mr>)| at.elapsed() > if mr.as_ref().is_some_and(Mr::running) { RUNNING_TTL } else { TTL };
        if refresh && cached.as_ref().is_none_or(stale) && self.busy.lock().unwrap().insert(key.clone()) {
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
        let mr = Mr { iid: 7, ..mr_stub() };
        assert_eq!(threads[0].as_prompt(&mr), "Review comment on merge request !7 at src/x.rs:12. vilu: Rename this | me: ok. Please address it.");
    }

    #[test]
    fn groups_jobs_into_stages_in_order() {
        let v: Value = serde_json::from_str(
            r#"[{"id":3,"name":"e2e","stage":"test","status":"running","duration":null},
                {"id":4,"name":"lint","stage":"test","status":"failed","allow_failure":true,"duration":5.0},
                {"id":1,"name":"build","stage":"build","status":"success","duration":61.5},
                {"id":2,"name":"unit","stage":"test","status":"success","duration":30.0}]"#,
        )
        .unwrap();
        let mut mr = Mr { pipeline: Some("running".into()), jobs: parse_jobs(&v), ..mr_stub() };
        let stages: Vec<(&str, usize)> = mr.stages().iter().map(|(s, j)| (*s, j.len())).collect();
        assert_eq!(stages, [("build", 1), ("test", 3)]);
        assert_eq!(mr.jobs.iter().find(|j| j.name == "lint").unwrap().status, "warning");
        assert!(mr.running());
        mr.pipeline = Some("success".into());
        assert!(!mr.running());
    }

    #[test]
    fn failure_skips_the_push_preamble() {
        let push = "To git.basalt.se:lubo/x.git\n ! [rejected]        b -> b (fetch first)\nerror: failed to push some refs\n";
        assert_eq!(failure(push).as_deref(), Some("! [rejected]        b -> b (fetch first)"));
        assert_eq!(failure("\n  404 Not Found\n").as_deref(), Some("404 Not Found"));
        assert_eq!(failure(""), None);
    }

    fn mr_stub() -> Mr {
        parse_mr(&serde_json::json!({"iid": 1, "project_id": 1, "title": ""})).unwrap()
    }

    #[test]
    fn parses_gitlab_times() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2026-11-18T13:42:24.086+01:00"), epoch("2026-11-18T12:42:24Z"));
        assert_eq!(epoch("2000-03-01T00:00:00.000Z"), Some(951868800));
    }

    #[test]
    fn builds_project_urls_like_the_remote() {
        assert_eq!(project_url("git@git.basalt.se:lubo/craton-cli.git", "craton/auxiliary/craton-cli").as_deref(), Some("git@git.basalt.se:craton/auxiliary/craton-cli.git"));
        assert_eq!(project_url("https://git.basalt.se/lubo/x.git", "a/b").as_deref(), Some("https://git.basalt.se/a/b.git"));
        assert_eq!(project_url("ssh://git@host:2222/lubo/x.git", "a/b").as_deref(), Some("ssh://git@host:2222/a/b.git"));
    }

    #[test]
    fn reads_review_fields() {
        let mr = parse_mr(&serde_json::json!({"iid": 140, "project_id": 2954, "title": "T", "author": {"username": "vilu"},
            "references": {"full": "craton/auxiliary/craton-cli!140"}, "draft": true})).unwrap();
        assert_eq!((mr.project.as_str(), mr.repo(), mr.author.as_str(), mr.draft), ("craton/auxiliary/craton-cli", "craton-cli", "vilu", true));
    }

    #[test]
    fn reads_issue_fields() {
        let issue = parse_issue(&serde_json::json!({"iid": 12, "title": "T", "labels": ["bug", "backend"],
            "milestone": {"title": "v2"}, "references": {"full": "craton/auxiliary/craton-cli#12"}})).unwrap();
        assert_eq!((issue.project.as_str(), issue.repo(), issue.labels.len(), issue.milestone.as_deref()), ("craton/auxiliary/craton-cli", "craton-cli", 2, Some("v2")));
    }
}

