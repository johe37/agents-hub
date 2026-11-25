//! Merge request status, unresolved review threads and assigned issues, through the glab CLI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::config::Section;

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
    /// Full project path, `acme/tools/acme-cli`.
    pub project: String,
    /// Last activity, seconds since the epoch.
    pub updated: u64,
    pub description: String,
    pub draft: bool,
    /// The branch it merges.
    pub branch: String,
    /// The project that branch is in, when GraphQL told: another one for an MR from a fork.
    pub source_project: String,
    /// Usernames; not fetched for a workspace's MR.
    pub approved_by: Vec<String>,
    /// You are among approved_by.
    pub approved: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Issue {
    pub iid: u64,
    pub title: String,
    pub url: String,
    /// Full project path, `acme/tools/acme-cli`.
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
    /// The repo's own name, `acme-cli`.
    pub fn repo(&self) -> &str {
        self.project.rsplit('/').next().unwrap_or(&self.project)
    }

    /// Where its code is checked out for a review.
    pub fn review_dir(&self) -> PathBuf {
        reviews_dir().join(format!("{}-{}", self.repo(), self.iid))
    }

    /// Where the hub checks out the branch of your own MR.
    pub fn branch_dir(&self) -> PathBuf {
        branches_dir().join(format!("{}-{}", self.repo(), self.iid))
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
    /// The line is in the old version of the file: a removed line.
    pub old_line: bool,
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
    match out.status.code() {
        Some(0) => return Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Some(124) => return Err(format!("{program} timed out")),
        Some(127) => return Err(format!("{program} is not installed")),
        _ => {}
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
        branch: v["source_branch"].as_str().unwrap_or_default().to_string(),
        source_project: String::new(),
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
                old_line: pos["new_line"].as_u64().is_none() && pos["old_line"].as_u64().is_some(),
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

/// What a list of MRs asks GraphQL for. Thread texts are left out: they take most of the time, so
/// they come over REST, and only for the MRs that have unresolved threads.
const MR_FIELDS: &str = "iid title webUrl state draft updatedAt description detailedMergeStatus author { username } \
    sourceBranch sourceProject { fullPath } \
    project { id fullPath } approvedBy { nodes { username } } resolvableDiscussionsCount resolvedDiscussionsCount \
    headPipeline { status path jobs { nodes { id name status allowFailure duration webPath stage { name } } } }";

/// Your open MRs in one of GitLab's lists on the current user, such as `authoredMergeRequests`, newest activity first.
fn fetch_mrs(list: &str) -> Result<Vec<Mr>, String> {
    let home = crate::config::home().display().to_string();
    let query = format!("query {{ currentUser {{ username {list}(state: opened, first: 50) {{ nodes {{ {MR_FIELDS} }} }} }} }}");
    let v: Value = serde_json::from_str(&glab(&home, &["api", "graphql", "-f", &format!("query={query}")])?).map_err(|_| "glab gave an unexpected answer".to_string())?;
    if let Some(e) = v["errors"][0]["message"].as_str() {
        return Err(e.to_string());
    }
    let me = &v["data"]["currentUser"];
    let user = me["username"].as_str().unwrap_or_default();
    let nodes = me[list]["nodes"].as_array().ok_or("glab gave an unexpected answer")?;
    let mut mrs: Vec<(Mr, i64)> = nodes.iter().filter_map(|n| parse_graphql_mr(n, user)).collect();
    std::thread::scope(|s| {
        for (mr, _) in mrs.iter_mut().filter(|(_, open)| *open > 0) {
            let home = &home;
            s.spawn(move || {
                let path = format!("projects/{}/merge_requests/{}/discussions?per_page=100", mr.project_id, mr.iid);
                if let Ok(text) = glab(home, &["api", &path]) {
                    mr.threads = serde_json::from_str(&text).map(|v| parse_threads(&v)).unwrap_or_default();
                }
            });
        }
    });
    let mut mrs: Vec<Mr> = mrs.into_iter().map(|(m, _)| m).collect();
    mrs.sort_by_key(|m| std::cmp::Reverse(m.updated));
    Ok(mrs)
}

/// An MR from GraphQL and its number of unresolved threads; approved means user approved.
fn parse_graphql_mr(n: &Value, user: &str) -> Option<(Mr, i64)> {
    let url = n["webUrl"].as_str()?;
    // GraphQL gives paths; links need the host in front.
    let host = url.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let pipeline = &n["headPipeline"];
    let approved_by: Vec<String> = n["approvedBy"]["nodes"].as_array().into_iter().flatten().filter_map(|u| u["username"].as_str().map(String::from)).collect();
    let mut jobs: Vec<Job> = pipeline["jobs"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|j| {
            Some(Job {
                id: gid(&j["id"])?,
                name: j["name"].as_str()?.to_string(),
                stage: j["stage"]["name"].as_str()?.to_string(),
                status: match lower(&j["status"])?.as_str() {
                    "failed" if j["allowFailure"] == true => "warning".to_string(),
                    s => s.to_string(),
                },
                duration: j["duration"].as_f64(),
                url: j["webPath"].as_str().map(|p| format!("{host}{p}")).unwrap_or_default(),
            })
        })
        .collect();
    jobs.sort_by_key(|j| j.id);
    let open = n["resolvableDiscussionsCount"].as_i64().unwrap_or(0) - n["resolvedDiscussionsCount"].as_i64().unwrap_or(0);
    let mr = Mr {
        iid: n["iid"].as_str()?.parse().ok()?,
        project_id: gid(&n["project"]["id"])?,
        title: n["title"].as_str()?.to_string(),
        url: url.to_string(),
        state: n["state"].as_str().unwrap_or_default().to_string(),
        pipeline: lower(&pipeline["status"]),
        pipeline_url: pipeline["path"].as_str().map(|p| format!("{host}{p}")),
        merge_status: lower(&n["detailedMergeStatus"]),
        threads: vec![],
        jobs,
        author: n["author"]["username"].as_str().unwrap_or_default().to_string(),
        project: n["project"]["fullPath"].as_str().unwrap_or_default().to_string(),
        updated: n["updatedAt"].as_str().and_then(epoch).unwrap_or(0),
        description: n["description"].as_str().unwrap_or_default().to_string(),
        draft: n["draft"] == true,
        branch: n["sourceBranch"].as_str().unwrap_or_default().to_string(),
        source_project: n["sourceProject"]["fullPath"].as_str().unwrap_or_default().to_string(),
        approved: approved_by.iter().any(|u| u == user),
        approved_by,
    };
    Some((mr, open))
}

/// The number at the end of a GraphQL id, `gid://gitlab/Project/235`.
fn gid(v: &Value) -> Option<u64> {
    v.as_str()?.rsplit('/').next()?.parse().ok()
}

/// GraphQL's enums are REST's values in capitals: SUCCESS is success.
fn lower(v: &Value) -> Option<String> {
    v.as_str().map(str::to_lowercase)
}

/// Open issues assigned to you, newest activity first.
fn fetch_issues() -> Result<Vec<Issue>, String> {
    let home = crate::config::home().display().to_string();
    Ok(api_list(&home, "issues?scope=assigned_to_me&state=opened&order_by=updated_at&per_page=50")?.iter().filter_map(parse_issue).collect())
}

fn api_list(cwd: &str, path: &str) -> Result<Vec<Value>, String> {
    let text = glab(cwd, &["api", path])?;
    match serde_json::from_str(&text) {
        Ok(Value::Array(list)) => Ok(list),
        _ => Err("glab gave an unexpected answer".into()),
    }
}

/// One file of an MR's or a commit's diff.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    /// Hunks only, from the first `@@`; empty for a binary file.
    pub diff: String,
    pub new_file: bool,
    pub deleted: bool,
    pub renamed: bool,
    /// GitLab left the diff out because it is too large.
    pub too_large: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub title: String,
    pub message: String,
    pub author: String,
    pub created: u64,
}

/// The commits of the MR's latest diff version. Line comments must name all three.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Version {
    pub base: String,
    pub start: String,
    pub head: String,
}

fn parse_file_diffs(v: &Value) -> Vec<FileDiff> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(FileDiff {
                old_path: f["old_path"].as_str()?.to_string(),
                new_path: f["new_path"].as_str()?.to_string(),
                diff: f["diff"].as_str().unwrap_or_default().to_string(),
                new_file: f["new_file"] == true,
                deleted: f["deleted_file"] == true,
                renamed: f["renamed_file"] == true,
                too_large: f["too_large"] == true,
            })
        })
        .collect()
}

fn parse_commits(v: &[Value]) -> Vec<Commit> {
    v.iter()
        .filter_map(|c| {
            Some(Commit {
                sha: c["id"].as_str()?.to_string(),
                short: c["short_id"].as_str().unwrap_or_default().to_string(),
                title: c["title"].as_str().unwrap_or_default().to_string(),
                message: c["message"].as_str().unwrap_or_default().to_string(),
                author: c["author_name"].as_str().unwrap_or_default().to_string(),
                created: c["created_at"].as_str().and_then(epoch).unwrap_or(0),
            })
        })
        .collect()
}

/// GitLab lists the versions newest first.
fn parse_version(v: &[Value]) -> Option<Version> {
    let l = v.first()?;
    Some(Version {
        base: l["base_commit_sha"].as_str()?.to_string(),
        start: l["start_commit_sha"].as_str()?.to_string(),
        head: l["head_commit_sha"].as_str()?.to_string(),
    })
}

/// Every page of a list, up to max pages of 100.
fn pages(path: &str, max: u32) -> Result<Vec<Value>, String> {
    let home = crate::config::home().display().to_string();
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut all = vec![];
    for page in 1..=max {
        let list = api_list(&home, &format!("{path}{sep}per_page=100&page={page}"))?;
        let last = list.len() < 100;
        all.extend(list);
        if last {
            break;
        }
    }
    Ok(all)
}

pub fn mr_version(mr: &Mr) -> Result<Version, String> {
    let list = pages(&format!("projects/{}/merge_requests/{}/versions", mr.project_id, mr.iid), 1)?;
    parse_version(&list).ok_or_else(|| "The merge request has no diff yet".to_string())
}

/// Newest first, as GitLab shows them.
pub fn mr_commits(mr: &Mr) -> Result<Vec<Commit>, String> {
    Ok(parse_commits(&pages(&format!("projects/{}/merge_requests/{}/commits", mr.project_id, mr.iid), 10)?))
}

pub fn mr_diff(mr: &Mr) -> Result<Vec<FileDiff>, String> {
    match pages(&format!("projects/{}/merge_requests/{}/diffs", mr.project_id, mr.iid), 30) {
        Ok(list) => Ok(parse_file_diffs(&Value::Array(list))),
        // GitLab before 15.7 has only the older changes endpoint.
        Err(e) if e.contains("404") => {
            let home = crate::config::home().display().to_string();
            let text = glab(&home, &["api", &format!("projects/{}/merge_requests/{}/changes", mr.project_id, mr.iid)])?;
            let v: Value = serde_json::from_str(&text).map_err(|_| "glab gave an unexpected answer".to_string())?;
            Ok(parse_file_diffs(&v["changes"]))
        }
        Err(e) => Err(e),
    }
}

/// The diff of one commit. An MR's commits are in the target project, also from a fork.
pub fn commit_diff(mr: &Mr, sha: &str) -> Result<Vec<FileDiff>, String> {
    let list = pages(&format!("projects/{}/repository/commits/{sha}/diff", mr.project_id), 30)?;
    Ok(parse_file_diffs(&Value::Array(list)))
}

/// The diff between two commits of the MR's project, such as your last review and the head now.
pub fn compare_diff(mr: &Mr, from: &str, to: &str) -> Result<Vec<FileDiff>, String> {
    let home = crate::config::home().display().to_string();
    let text = glab(&home, &["api", &format!("projects/{}/repository/compare?from={from}&to={to}", mr.project_id)])?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "glab gave an unexpected answer".to_string())?;
    Ok(parse_file_diffs(&v["diffs"]))
}

/// A comment of your review that GitLab keeps unpublished until you finish the review.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftNote {
    pub id: u64,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub old_line: bool,
}

fn parse_drafts(v: &[Value]) -> Vec<DraftNote> {
    v.iter()
        .filter_map(|d| {
            let pos = &d["position"];
            let new_line = pos["new_line"].as_u64();
            Some(DraftNote {
                id: d["id"].as_u64()?,
                body: d["note"].as_str().unwrap_or_default().to_string(),
                path: pos["new_path"].as_str().or(pos["old_path"].as_str()).map(String::from),
                line: new_line.or(pos["old_line"].as_u64()),
                old_line: new_line.is_none() && pos["old_line"].as_u64().is_some(),
            })
        })
        .collect()
}

/// A request with a JSON body: --field would send a nested object, such as a position, as a string.
fn glab_json(method: &str, path: &str, body: &Value) -> Result<String, String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = crate::config::state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("glab-body-{}-{}.json", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&file, body.to_string()).map_err(|e| format!("{}: {e}", file.display()))?;
    let home = crate::config::home().display().to_string();
    let r = glab(&home, &["api", "-X", method, path, "--input", &file.display().to_string(), "-H", "Content-Type: application/json"]);
    let _ = std::fs::remove_file(&file);
    r
}

fn mr_path(mr: &Mr, rest: &str) -> String {
    format!("projects/{}/merge_requests/{}/{rest}", mr.project_id, mr.iid)
}

pub fn drafts(mr: &Mr) -> Result<Vec<DraftNote>, String> {
    Ok(parse_drafts(&pages(&mr_path(mr, "draft_notes"), 5)?))
}

/// A comment on the MR, on a diff line when position is given. A draft waits for publish_drafts.
pub fn add_note(mr: &Mr, body: &str, position: Option<&Value>, draft: bool) -> Result<(), String> {
    let (path, mut v) = match (draft, position) {
        (true, _) => (mr_path(mr, "draft_notes"), serde_json::json!({"note": body})),
        (false, Some(_)) => (mr_path(mr, "discussions"), serde_json::json!({"body": body})),
        (false, None) => (mr_path(mr, "notes"), serde_json::json!({"body": body})),
    };
    if let Some(p) = position {
        v["position"] = p.clone();
    }
    glab_json("POST", &path, &v).map(|_| ())
}

pub fn edit_draft(mr: &Mr, id: u64, body: &str) -> Result<(), String> {
    glab_json("PUT", &mr_path(mr, &format!("draft_notes/{id}")), &serde_json::json!({"note": body})).map(|_| ())
}

pub fn delete_draft(mr: &Mr, id: u64) -> Result<(), String> {
    let home = crate::config::home().display().to_string();
    glab(&home, &["api", "-X", "DELETE", &mr_path(mr, &format!("draft_notes/{id}"))]).map(|_| ())
}

pub fn publish_drafts(mr: &Mr) -> Result<(), String> {
    let home = crate::config::home().display().to_string();
    glab(&home, &["api", "-X", "POST", &mr_path(mr, "draft_notes/bulk_publish")]).map(|_| ())
}

pub fn approve(mr: &Mr, on: bool) -> Result<(), String> {
    let home = crate::config::home().display().to_string();
    glab(&home, &["api", "-X", "POST", &mr_path(mr, if on { "approve" } else { "unapprove" })]).map(|_| ())
}

/// The project of a job from its web URL, `https://host/acme/api/-/jobs/12`, URL-encoded for the API.
/// A fork's pipeline runs in the fork, so the MR's project would be the wrong one.
fn job_project(url: &str) -> Option<String> {
    let path = url.split("://").nth(1)?.split_once('/')?.1.split("/-/jobs/").next()?;
    (!path.is_empty()).then(|| path.replace('/', "%2F"))
}

/// The end of a job's log: the error is almost always there.
pub fn job_log(url: &str, id: u64, max: usize) -> Result<(String, bool), String> {
    let project = job_project(url).ok_or("The job has no project")?;
    let home = crate::config::home().display().to_string();
    let log = glab(&home, &["api", &format!("projects/{project}/jobs/{id}/trace")])?;
    if log.len() <= max {
        return Ok((log, false));
    }
    let mut cut = log.len() - max;
    while !log.is_char_boundary(cut) {
        cut += 1;
    }
    let start = log[cut..].find('\n').map_or(cut, |i| cut + i + 1);
    Ok((log[start..].to_string(), true))
}

pub fn retry_job(url: &str, id: u64) -> Result<(), String> {
    let project = job_project(url).ok_or("The job has no project")?;
    let home = crate::config::home().display().to_string();
    glab(&home, &["api", "-X", "POST", &format!("projects/{project}/jobs/{id}/retry")]).map(|_| ())
}

pub fn reviews_dir() -> PathBuf {
    crate::config::cache_dir().join("reviews")
}

pub fn branches_dir() -> PathBuf {
    crate::config::cache_dir().join("mrs")
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

/// A directory with the MR's own branch checked out, ready to commit and push: the one that has it
/// already, else a new worktree of the clone at repo in dir, such as `branch_dir`. The clone itself is never switched.
pub fn checkout_branch(repo: &str, mr: &Mr, dir: &Path) -> Result<String, String> {
    if mr.branch.is_empty() {
        return Err("GitLab gave no branch for it".into());
    }
    if let Some(dir) = worktree_with(&run(repo, 5, "git", &["worktree", "list", "--porcelain"])?, &mr.branch) {
        return Ok(dir);
    }
    let source = if mr.source_project.is_empty() { &mr.project } else { &mr.source_project };
    // The URLs as configured: `git remote -v` shows them after any insteadOf rewrite.
    let remotes = run(repo, 5, "git", &["config", "--get-regexp", r"^remote\..*\.url$"])?;
    let remote = remote_for(&remotes, source).ok_or_else(|| format!("{} has no remote for {source}. Add one with git remote add, then try again.", crate::config::shorten(repo)))?;
    run(repo, 120, "git", &["fetch", &remote, &mr.branch])?;
    if dir.exists() {
        return Err(format!("{} is in the way", crate::config::shorten(&dir.display().to_string())));
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let dir = dir.display().to_string();
    let local = run(repo, 5, "git", &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{}", mr.branch)]).is_ok();
    if local {
        run(repo, 60, "git", &["worktree", "add", &dir, &mr.branch])?;
    } else {
        run(repo, 60, "git", &["worktree", "add", "--track", "-b", &mr.branch, &dir, &format!("{remote}/{}", mr.branch)])?;
    }
    Ok(dir)
}

/// The worktree, in `git worktree list --porcelain` output, that has branch checked out.
fn worktree_with(porcelain: &str, branch: &str) -> Option<String> {
    let want = format!("branch refs/heads/{branch}");
    porcelain.split("\n\n").find(|w| w.lines().any(|l| l == want))?.lines().find_map(|l| l.strip_prefix("worktree ")).map(String::from)
}

/// The remote that points at project, from `remote.<name>.url <url>` lines.
fn remote_for(remotes: &str, project: &str) -> Option<String> {
    remotes.lines().find_map(|l| {
        let (key, url) = l.split_once(' ')?;
        let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
        let path = match url.split_once("://") {
            Some((_, rest)) => rest.split_once('/')?.1,
            None => url.split_once(':')?.1,
        };
        (path.trim_end_matches('/').trim_end_matches(".git") == project).then(|| name.to_string())
    })
}

/// Remove the worktree of your MR's branch. Git refuses while it has uncommitted changes; commits stay on the branch.
pub fn remove_branch_checkout(dir: &str) -> Result<(), String> {
    run(dir, 30, "git", &["worktree", "remove", dir]).map(|_| ())
}

/// Delete a review checkout, including anything changed in it.
pub fn remove_checkout(dir: &str) -> Result<(), String> {
    run(dir, 30, "git", &["worktree", "remove", "--force", dir]).map(|_| ())
}

#[derive(Clone, Default)]
pub struct Inbox {
    pub reviews: Vec<Mr>,
    /// Open MRs you opened.
    pub authored: Vec<Mr>,
    /// Open MRs assigned to you.
    pub assigned: Vec<Mr>,
    pub issues: Vec<Issue>,
    /// The first fetch has finished, whether it worked or not.
    pub loaded: bool,
    /// Why the last fetch of a section failed.
    pub errors: Vec<(Section, String)>,
    /// The sections a fetch runs for now.
    pub busy: Vec<Section>,
}

/// MRs to review, your own and assigned MRs, and issues assigned to you, refreshed in the background like `MrCache`.
#[derive(Default)]
pub struct InboxCache {
    state: Mutex<(Option<Instant>, Inbox)>,
    /// The sections a fetch runs for now.
    busy: Mutex<Vec<Section>>,
    /// Sections to fetch at once, out of turn.
    asked: Mutex<Vec<Section>>,
}

impl InboxCache {
    /// Every one of sections when the TTL is up, else only the ones asked for.
    pub fn get(self: &Arc<Self>, refresh: bool, sections: &[Section]) -> Inbox {
        let (at, inbox) = self.state.lock().unwrap().clone();
        let mut busy = self.busy.lock().unwrap();
        if busy.is_empty() {
            let due = refresh && at.is_none_or(|at| at.elapsed() > INBOX_TTL);
            let asked = std::mem::take(&mut *self.asked.lock().unwrap());
            let wanted: Vec<Section> = sections.iter().copied().filter(|s| s.gitlab() && (due || asked.contains(s))).collect();
            if !wanted.is_empty() {
                busy.clone_from(&wanted);
                let me = Arc::clone(self);
                std::thread::spawn(move || me.fetch(&wanted, due));
            }
        }
        Inbox { busy: busy.clone(), ..inbox }
    }

    fn fetch(&self, wanted: &[Section], all: bool) {
        let on = |s| wanted.contains(&s);
        let mrs = |section, list| on(section).then(|| fetch_mrs(list));
        let (reviews, authored, assigned, issues) = std::thread::scope(|s| {
            let reviews = s.spawn(|| mrs(Section::GitlabReviews, "reviewRequestedMergeRequests"));
            let authored = s.spawn(|| mrs(Section::GitlabMergeRequests, "authoredMergeRequests"));
            let assigned = s.spawn(|| mrs(Section::GitlabAssignedMergeRequests, "assignedMergeRequests"));
            let issues = on(Section::GitlabIssues).then(fetch_issues);
            (reviews.join().unwrap(), authored.join().unwrap(), assigned.join().unwrap(), issues)
        });
        let mut state = self.state.lock().unwrap();
        let inbox = &mut state.1;
        inbox.errors.retain(|(s, _)| !wanted.contains(s));
        put(&mut inbox.reviews, &mut inbox.errors, Section::GitlabReviews, reviews);
        put(&mut inbox.authored, &mut inbox.errors, Section::GitlabMergeRequests, authored);
        put(&mut inbox.assigned, &mut inbox.errors, Section::GitlabAssignedMergeRequests, assigned);
        put(&mut inbox.issues, &mut inbox.errors, Section::GitlabIssues, issues);
        inbox.loaded = true;
        // Try again after the TTL, whether it worked or not.
        if all {
            state.0 = Some(Instant::now());
        }
        self.busy.lock().unwrap().clear();
    }

    /// Fetch section on the next get, unless a fetch of it runs now.
    pub fn forget(&self, section: Section) {
        if !self.busy.lock().unwrap().contains(&section) {
            self.asked.lock().unwrap().push(section);
        }
    }
}

/// A fetched list replaces the old one; on an error the old one stays, so GitLab being away empties nothing.
fn put<T>(list: &mut Vec<T>, errors: &mut Vec<(Section, String)>, section: Section, new: Option<Result<Vec<T>, String>>) {
    match new {
        Some(Ok(new)) => *list = new,
        Some(Err(e)) => errors.push((section, e)),
        None => {}
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
                {"id":3,"system":false,"resolvable":true,"resolved":false,"author":{"username":"erik"},"body":"Rename\nthis","position":{"new_path":"src/x.rs","new_line":12}},
                {"id":4,"system":false,"resolvable":true,"resolved":false,"author":{"username":"me"},"body":"ok"}]}
        ]"#,
        )
        .unwrap();
        let threads = parse_threads(&v);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].place(), "src/x.rs:12");
        assert_eq!(threads[0].notes.len(), 2);
        let mr = Mr { iid: 7, ..mr_stub() };
        assert_eq!(threads[0].as_prompt(&mr), "Review comment on merge request !7 at src/x.rs:12. erik: Rename this | me: ok. Please address it.");
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
        let push = "To git.example.com:anna/x.git\n ! [rejected]        b -> b (fetch first)\nerror: failed to push some refs\n";
        assert_eq!(failure(push).as_deref(), Some("! [rejected]        b -> b (fetch first)"));
        assert_eq!(failure("\n  404 Not Found\n").as_deref(), Some("404 Not Found"));
        assert_eq!(failure(""), None);
    }

    #[test]
    fn names_a_missing_program() {
        assert_eq!(run("/", 5, "no-such-program-here", &[]), Err("no-such-program-here is not installed".into()));
    }

    #[test]
    fn reads_diffs_commits_and_the_latest_version() {
        let v: Value = serde_json::from_str(
            r#"[{"old_path":"a.rs","new_path":"b.rs","diff":"@@ -1 +1 @@\n-x\n+y\n","renamed_file":true},
                {"old_path":"big.json","new_path":"big.json","diff":"","too_large":true},
                {"new_path":"no-old-path"}]"#,
        )
        .unwrap();
        let files = parse_file_diffs(&v);
        assert_eq!(files.len(), 2);
        assert!(files[0].renamed && !files[0].new_file);
        assert!(files[1].too_large);

        let v: Vec<Value> = serde_json::from_str(r#"[{"id":"abc123","short_id":"abc","title":"Fix it","message":"Fix it\n\nBody","author_name":"Erik","created_at":"2026-11-18T13:42:24.000+01:00"}]"#).unwrap();
        let c = &parse_commits(&v)[0];
        assert_eq!((c.short.as_str(), c.author.as_str(), c.created), ("abc", "Erik", 1_795_005_744));

        let v: Vec<Value> = serde_json::from_str(r#"[{"base_commit_sha":"b2","start_commit_sha":"s2","head_commit_sha":"h2"},{"base_commit_sha":"b1","start_commit_sha":"s1","head_commit_sha":"h1"}]"#).unwrap();
        assert_eq!(parse_version(&v), Some(Version { base: "b2".into(), start: "s2".into(), head: "h2".into() }));
        assert_eq!(parse_version(&[]), None);
    }

    #[test]
    fn reads_draft_notes_on_old_and_new_lines() {
        let v: Vec<Value> = serde_json::from_str(
            r#"[{"id":1,"note":"Why?","position":{"new_path":"a.rs","old_path":"a.rs","new_line":null,"old_line":4}},
                {"id":2,"note":"Nice","position":{"new_path":"b.rs","new_line":9,"old_line":null}},
                {"id":3,"note":"Overall fine"}]"#,
        )
        .unwrap();
        let d = parse_drafts(&v);
        assert_eq!((d[0].line, d[0].old_line), (Some(4), true));
        assert_eq!((d[1].path.as_deref(), d[1].line, d[1].old_line), (Some("b.rs"), Some(9), false));
        assert_eq!((d[2].path.as_deref(), d[2].line), (None, None));
    }

    #[test]
    fn finds_the_project_of_a_job() {
        assert_eq!(job_project("https://git.example.com/acme/tools/api/-/jobs/12").as_deref(), Some("acme%2Ftools%2Fapi"));
        assert_eq!(job_project(""), None);
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
        assert_eq!(project_url("git@git.example.com:anna/acme-cli.git", "acme/tools/acme-cli").as_deref(), Some("git@git.example.com:acme/tools/acme-cli.git"));
        assert_eq!(project_url("https://git.example.com/anna/x.git", "a/b").as_deref(), Some("https://git.example.com/a/b.git"));
        assert_eq!(project_url("ssh://git@host:2222/anna/x.git", "a/b").as_deref(), Some("ssh://git@host:2222/a/b.git"));
    }

    #[test]
    fn reads_review_fields() {
        let mr = parse_mr(&serde_json::json!({"iid": 140, "project_id": 2954, "title": "T", "author": {"username": "erik"},
            "references": {"full": "acme/tools/acme-cli!140"}, "draft": true})).unwrap();
        assert_eq!((mr.project.as_str(), mr.repo(), mr.author.as_str(), mr.draft), ("acme/tools/acme-cli", "acme-cli", "erik", true));
    }

    #[test]
    fn reads_graphql_mrs() {
        let n: Value = serde_json::from_str(
            r#"{"iid": "170", "title": "Fix", "webUrl": "https://git.x/a/b/-/merge_requests/170", "state": "opened", "draft": true,
                "updatedAt": "2026-10-22T07:43:13+02:00", "description": "d", "detailedMergeStatus": "DISCUSSIONS_NOT_RESOLVED",
                "author": {"username": "anna"}, "project": {"id": "gid://gitlab/Project/7016", "fullPath": "a/b"},
                "approvedBy": {"nodes": [{"username": "johe"}]}, "resolvableDiscussionsCount": 5, "resolvedDiscussionsCount": 2,
                "headPipeline": {"status": "FAILED", "path": "/a/b/-/pipelines/9", "jobs": {"nodes": [
                    {"id": "gid://gitlab/Ci::Build/12", "name": "lint", "status": "FAILED", "allowFailure": true, "duration": 4, "webPath": "/a/b/-/jobs/12", "stage": {"name": "test"}},
                    {"id": "gid://gitlab/Ci::Build/11", "name": "build", "status": "SUCCESS", "allowFailure": false, "duration": null, "webPath": "/a/b/-/jobs/11", "stage": {"name": "build"}}]}}}"#,
        )
        .unwrap();
        let (mr, open) = parse_graphql_mr(&n, "johe").unwrap();
        assert_eq!(open, 3);
        assert_eq!((mr.iid, mr.project_id, mr.project.as_str()), (170, 7016, "a/b"));
        assert_eq!(mr.pipeline.as_deref(), Some("failed"));
        assert_eq!(mr.pipeline_url.as_deref(), Some("https://git.x/a/b/-/pipelines/9"));
        assert_eq!(mr.merge_status.as_deref(), Some("discussions_not_resolved"));
        assert!(mr.draft && mr.approved);
        let jobs: Vec<_> = mr.jobs.iter().map(|j| (j.name.as_str(), j.stage.as_str(), j.status.as_str(), j.duration, j.url.as_str())).collect();
        assert_eq!(jobs, [("build", "build", "success", None, "https://git.x/a/b/-/jobs/11"), ("lint", "test", "warning", Some(4.0), "https://git.x/a/b/-/jobs/12")]);
    }

    #[test]
    fn finds_the_worktree_and_remote_of_a_branch() {
        let porcelain = "worktree /r/app\nHEAD 1\nbranch refs/heads/master\n\nworktree /r/app-fix\nHEAD 2\nbranch refs/heads/fix/x\n";
        assert_eq!(worktree_with(porcelain, "fix/x").as_deref(), Some("/r/app-fix"));
        assert_eq!(worktree_with(porcelain, "fix"), None);
        let remotes = "remote.origin.url git@git.x:acme/app.git\nremote.fork.url https://git.x/johe/app\n";
        assert_eq!(remote_for(remotes, "acme/app").as_deref(), Some("origin"));
        assert_eq!(remote_for(remotes, "johe/app").as_deref(), Some("fork"));
        assert_eq!(remote_for(remotes, "anna/app"), None);
        assert_eq!(remote_for(remotes, "app"), None);
        assert_eq!(remote_for("remote.up.url ssh://git@git.x:22/other/acme/app.git\n", "acme/app"), None);
    }

    #[test]
    fn checks_out_the_branch_next_to_the_clone() {
        let dir = std::env::temp_dir().join(format!("jugglr-mine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sh = |cwd: &Path, args: &[&str]| {
            let out = Command::new("git").current_dir(cwd).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        let up = dir.join("up");
        std::fs::create_dir_all(&up).unwrap();
        sh(&up, &["init", "-q", "-b", "master"]);
        sh(&up, &["commit", "-q", "--allow-empty", "-m", "x"]);
        sh(&up, &["branch", "fix/refunds"]);
        sh(&dir, &["clone", "-q", up.to_str().unwrap(), "app"]);
        let app = dir.join("app");
        // The remote points at GitLab; insteadOf sends it to the local repo.
        sh(&app, &["remote", "set-url", "origin", "git@git.x:acme/app.git"]);
        sh(&app, &["config", &format!("url.{}.insteadOf", up.display()), "git@git.x:acme/app.git"]);
        let mr = Mr { project: "acme/app".into(), branch: "fix/refunds".into(), ..Default::default() };
        let repo = app.to_str().unwrap();
        let at = dir.join("cache/app-7");
        let wt = checkout_branch(repo, &mr, &at).unwrap();
        assert_eq!(wt, at.display().to_string());
        assert_eq!(run(&wt, 5, "git", &["branch", "--show-current"]).unwrap().trim(), "fix/refunds");
        assert_eq!(run(&wt, 5, "git", &["rev-parse", "--abbrev-ref", "@{u}"]).unwrap().trim(), "origin/fix/refunds");
        assert_eq!(run(repo, 5, "git", &["branch", "--show-current"]).unwrap().trim(), "master", "the clone is not switched");
        assert_eq!(checkout_branch(repo, &mr, &at).unwrap(), wt, "a second time finds the worktree");
        std::fs::write(Path::new(&wt).join("new.txt"), "x").unwrap();
        assert!(remove_branch_checkout(&wt).is_err(), "uncommitted work stops the removal");
        std::fs::remove_file(Path::new(&wt).join("new.txt")).unwrap();
        remove_branch_checkout(&wt).unwrap();
        assert!(!Path::new(&wt).exists());
        let fork = Mr { source_project: "johe/app".into(), ..mr };
        assert!(checkout_branch(repo, &Mr { branch: "other".into(), ..fork }, &at).unwrap_err().contains("no remote for johe/app"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_issue_fields() {
        let issue = parse_issue(&serde_json::json!({"iid": 12, "title": "T", "labels": ["bug", "backend"],
            "milestone": {"title": "v2"}, "references": {"full": "acme/tools/acme-cli#12"}})).unwrap();
        assert_eq!((issue.project.as_str(), issue.repo(), issue.labels.len(), issue.milestone.as_deref()), ("acme/tools/acme-cli", "acme-cli", 2, Some("v2")));
    }
}

