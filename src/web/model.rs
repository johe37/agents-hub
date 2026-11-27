//! The JSON the page renders, built from a Snapshot.

use std::collections::HashSet;
use std::process::Command;

use serde_json::{Value, json};

use crate::config::{Config, shorten};
use crate::git::GitStatus;
use crate::gitlab::{Issue, Job, Mr, Todo};
use crate::snapshot::{Agent, Snapshot, Workspace};
use crate::state::Status;

pub fn status(s: Status) -> &'static str {
    crate::history::name(s)
}

fn git(g: &GitStatus) -> Value {
    json!({
        "branch": g.branch,
        "commit": g.commit.as_deref().map(|c| &c[..c.len().min(8)]),
        "dirty": g.dirty,
        "ahead": g.ahead,
        "behind": g.behind,
        "upstream": g.upstream,
        "remotes": g.remotes,
        "push_remote": g.push_remote(),
        "added": g.added,
        "removed": g.removed,
        // An object with a key, so the page's reconcile matches each file to its own row.
        "files": g.files.iter().map(|(code, path)| json!({"key": path, "code": code, "path": path})).collect::<Vec<_>>(),
    })
}

/// The key the page knows an MR by: unique across projects.
pub fn job_json(j: &Job) -> Value {
    json!({"key": j.id, "id": j.id, "name": j.name, "stage": j.stage, "status": j.status, "duration": j.duration, "url": j.url})
}

pub fn mr_key(m: &Mr) -> String {
    format!("{}!{}", m.project_id, m.iid)
}

pub fn issue_key(i: &Issue) -> String {
    format!("{}#{}", i.project, i.iid)
}

/// The workspace that works on the MR: it has the MR's branch, or is its review or branch checkout.
pub fn mr_workspace<'a>(m: &Mr, snap: &'a Snapshot) -> Option<&'a Workspace> {
    let dirs = [m.review_dir(), m.branch_dir()].map(|d| crate::tmux::canonical(&d.display().to_string()));
    snap.workspaces
        .iter()
        .find(|w| w.mr.as_ref().is_some_and(|wm| wm.project_id == m.project_id && wm.iid == m.iid))
        .or_else(|| snap.workspaces.iter().find(|w| dirs.contains(&crate::tmux::canonical(&w.path))))
}

fn mr(m: &Mr, ws: Option<&Workspace>) -> Value {
    let threads: Vec<Value> = m
        .threads
        .iter()
        .map(|t| {
            json!({
                "key": t.id,
                "id": t.id,
                "place": t.place(),
                "path": t.path,
                "line": t.line,
                "old_line": t.old_line,
                "notes": t.notes.iter().map(|n| json!({"id": n.id, "author": n.author, "body": n.body})).collect::<Vec<_>>(),
                "prompt": t.as_prompt(m),
            })
        })
        .collect();
    let jobs: Vec<Value> = m.jobs.iter().map(job_json).collect();
    json!({
        "key": mr_key(m),
        "project_id": m.project_id,
        "iid": m.iid,
        "title": m.title,
        "url": m.url,
        "state": m.state,
        "pipeline": m.pipeline,
        "pipeline_url": m.pipeline_url,
        "merge_status": m.merge_status,
        "draft": m.draft,
        "author": m.author,
        "project": m.project,
        "branch": m.branch,
        "updated": m.updated,
        "description": m.description,
        "approved": m.approved,
        "approved_by": m.approved_by,
        "threads": threads,
        "jobs": jobs,
        "review_prompt": m.review_prompt(),
        "workspace": ws.map(|w| w.name.clone()),
    })
}

/// A list's MR, with the fresher copy a workspace keeps of it when there is one. Only the list has approvals.
fn listed_mr(m: &Mr, snap: &Snapshot) -> Value {
    let ws = mr_workspace(m, snap);
    match ws.and_then(|w| w.mr.as_ref()).filter(|wm| wm.iid == m.iid && wm.project_id == m.project_id) {
        Some(fresh) => mr(&Mr { approved_by: m.approved_by.clone(), approved: m.approved, ..fresh.clone() }, ws),
        None => mr(m, ws),
    }
}

fn issue(i: &Issue) -> Value {
    json!({
        "key": issue_key(i),
        "iid": i.iid,
        "title": i.title,
        "url": i.url,
        "project": i.project,
        "labels": i.labels,
        "milestone": i.milestone,
        "author": i.author,
        "updated": i.updated,
        "description": i.description,
        "prompt": i.prompt(),
    })
}

fn todo(t: &Todo) -> Value {
    // The key of the MR or issue it is about, as the page knows them.
    let target = match (t.target_type.as_str(), t.project_id, t.iid) {
        ("MergeRequest", Some(p), Some(iid)) => Some(format!("{p}!{iid}")),
        ("Issue", _, Some(iid)) => Some(format!("{}#{iid}", t.project)),
        _ => None,
    };
    json!({
        "key": format!("todo:{}", t.id),
        "id": t.id,
        "action": t.action,
        "target_type": t.target_type,
        "target": target,
        "iid": t.iid,
        "project": t.project,
        "title": t.title,
        "url": t.url,
        "author": t.author,
        "body": t.body,
        "created": t.created,
    })
}

/// The command line of what runs in the foreground of a shell's terminal, when it is not the shell itself.
fn running(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Fields after the command name, which is in parentheses and can hold spaces: state is first, tpgid sixth.
    let tpgid: u32 = stat.rsplit_once(')')?.1.split_whitespace().nth(5)?.parse().ok()?;
    if tpgid == 0 || tpgid == pid {
        return None;
    }
    let raw = std::fs::read(format!("/proc/{tpgid}/cmdline")).ok()?;
    let mut args = raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned());
    let program = args.next()?;
    let program = program.rsplit('/').next().unwrap_or(&program).to_string();
    let line = std::iter::once(program).chain(args).collect::<Vec<_>>().join(" ");
    Some(if line.chars().count() > 60 { format!("{}…", line.chars().take(60).collect::<String>()) } else { line })
}

fn workspace(ws: &Workspace, snap: &Snapshot) -> Value {
    let windows: Vec<Value> = ws
        .windows
        .iter()
        .map(|w| {
            let agent = snap.agents.iter().find(|a| a.window_id.as_deref() == Some(&w.window_id)).map(|a| a.key.clone());
            json!({
                "key": w.window_id,
                "id": w.window_id,
                "index": w.index,
                "name": w.name,
                "pane": w.pane_id,
                "command": w.command,
                "kind": w.kind(),
                "running": (w.kind() != "agent").then(|| running(w.pid)).flatten(),
                "agent": agent,
            })
        })
        .collect();
    json!({
        "key": ws.name,
        "name": ws.name,
        "path": ws.path,
        "short_path": shorten(&ws.path),
        "git": ws.git.as_ref().map(git),
        "mr": ws.mr.as_ref().map(|m| mr(m, Some(ws))),
        "windows": windows,
    })
}

/// Last change of the transcript in milliseconds, so the page knows when to load the conversation again.
fn activity(a: &Agent) -> u64 {
    a.transcript_file()
        .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

fn agent(a: &Agent) -> Value {
    let s = a.session.as_ref();
    let t = a.transcript.as_ref();
    let home = format!("{}/", crate::config::home().display());
    let tool = s.and_then(|s| s.tool.clone()).or_else(|| t.and_then(|t| t.last_tool.clone())).map(|t| t.replace(&home, "~/"));
    json!({
        "key": a.key,
        "id": a.session_id(),
        "title": a.title(),
        "name": a.name(),
        "status": status(a.status),
        "workspace": a.workspace,
        "window": a.window_id,
        "path": a.path,
        "short_path": shorten(&a.path),
        "branch": a.git.as_ref().and_then(|g| g.branch.clone()),
        "dirty": a.git.as_ref().map_or(0, |g| g.dirty),
        "timestamp": a.timestamp(),
        "message": s.and_then(|s| s.message.clone()),
        "notification": s.and_then(|s| s.notification_type.clone()),
        "permission_mode": s.and_then(|s| s.permission_mode.clone()),
        "pending_tool": s.and_then(|s| s.pending_tool.clone()),
        "needs_permission": a.needs_permission(),
        "tool": tool,
        "prompt": t.and_then(|t| t.last_prompt.clone()),
        "reply": t.and_then(|t| t.last_text.clone()),
        "model": t.and_then(|t| t.model.clone()),
        "turns": t.map_or(0, |t| t.turns),
        "cost": t.and_then(|t| t.cost_usd),
        "background": a.background,
        "reachable": a.target.is_some(),
        "adoptable": a.workspace.is_none() && a.status.alive() && a.pid.is_some() && !a.background,
        "activity": activity(a),
    })
}

pub fn state(snap: &Snapshot, cfg: &Config) -> Value {
    let usage = snap.usage.as_ref().map(|u| json!({"five_hour": u.five_hour, "seven_day": u.seven_day}));
    json!({
        "workspaces": snap.workspaces.iter().map(|w| workspace(w, snap)).collect::<Vec<_>>(),
        "agents": snap.agents.iter().map(agent).collect::<Vec<_>>(),
        "usage": usage,
        "agent": {
            "command": crate::agent::command(),
            "name": crate::agent::name(),
            "label": crate::agent::label(),
            "claude": crate::agent::is_claude(),
            "installed": snap.agent_installed,
            "known": crate::agent::KNOWN,
        },
        "gitlab": {
            "enabled": cfg.gitlab(),
            "loading": snap.inbox_loading,
            "errors": snap.inbox_errors.iter().map(|(s, e)| json!([s.title(), e])).collect::<Vec<_>>(),
            "reviews": snap.reviews.iter().map(|m| listed_mr(m, snap)).collect::<Vec<_>>(),
            "authored": snap.authored.iter().map(|m| listed_mr(m, snap)).collect::<Vec<_>>(),
            "assigned": snap.assigned.iter().map(|m| listed_mr(m, snap)).collect::<Vec<_>>(),
            "issues": snap.issues.iter().map(issue).collect::<Vec<_>>(),
            "todos": snap.todos.iter().map(todo).collect::<Vec<_>>(),
        },
        "closed": crate::closed::list().iter().map(|c| json!({
            "key": c.id,
            "id": c.id,
            "name": c.name,
            "path": c.path,
            "short_path": shorten(&c.path),
            "closed_at": c.closed_at,
            "windows": c.windows.iter().map(|w| json!({"kind": w.kind, "name": w.name, "resume": w.session_id.is_some()})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "chat_dir": crate::tmux::canonical(&cfg.chat_dir),
        "web_theme": cfg.web_theme,
        "home": crate::config::home(),
        "warnings": cfg.warnings,
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("JUGGLR_COMMIT"),
    })
}

const MAX_DIFF: usize = 1024 * 1024;

fn git_out(cwd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("timeout").args(["5", "git", "-C", cwd]).args(args).output().ok()?;
    // `git diff --no-index` exits 1 when the files differ.
    matches!(out.status.code(), Some(0 | 1)).then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(super) fn toplevel(path: &str) -> Option<String> {
    git_out(path, &["rev-parse", "--show-toplevel"]).map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
}

pub struct LogQuery<'a> {
    pub rev: Option<&'a str>,
    /// The default branch's commits that the branch lacks, instead of the branch's history.
    pub behind: bool,
    pub search: Option<&'a str>,
    pub skip: usize,
    pub limit: usize,
}

/// The branch that a branch's own commits are counted from. Upstream first: in a fork, that is the repo the branch is meant for.
fn base_branch(root: &str) -> Option<&'static str> {
    ["upstream/HEAD", "upstream/main", "upstream/master", "origin/HEAD", "origin/main", "origin/master", "main", "master"]
        .into_iter()
        .find(|b| is_rev(root, b))
}

fn is_rev(root: &str, rev: &str) -> bool {
    !rev.starts_with('-') && git_out(root, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]).is_some_and(|s| !s.trim().is_empty())
}

/// The branches and tags in a `%D` decoration, as [kind, name]: head, local, remote or tag.
fn decorations(d: &str) -> Vec<Value> {
    d.split(", ")
        .filter_map(|r| {
            let (r, head) = r.strip_prefix("HEAD -> ").map_or((r, false), |b| (b, true));
            let (kind, name) = if r == "HEAD" {
                ("head", "HEAD")
            } else if let Some(t) = r.strip_prefix("tag: refs/tags/") {
                ("tag", t)
            } else if let Some(b) = r.strip_prefix("refs/heads/") {
                (if head { "head" } else { "local" }, b)
            } else {
                let b = r.strip_prefix("refs/remotes/")?;
                if b.ends_with("/HEAD") {
                    return None;
                }
                ("remote", b)
            };
            Some(json!([kind, name]))
        })
        .collect()
}

/// A page of a branch's commits, newest first, on its first-parent line.
pub fn commits(path: &str, q: &LogQuery) -> Value {
    let Some(root) = toplevel(path) else { return json!({"commits": [], "more": false}) };
    let rev = q.rev.unwrap_or("HEAD");
    if !is_rev(&root, rev) {
        return json!({"commits": [], "more": false, "error": "Unknown branch"});
    }
    // git ANDs --grep with --author, so a search filters here: in the message, the author or the sha.
    let search = q.search.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let log = |range: &str| {
        let mut args = vec!["log", "--no-color", "--first-parent", "--decorate=full", "--format=%x1e%H%x1f%h%x1f%s%x1f%b%x1f%an%x1f%ct%x1f%D"];
        let (skip, n) = (format!("--skip={}", q.skip), format!("-n{}", q.limit + 1));
        if search.is_none() {
            args.extend([skip.as_str(), n.as_str()]);
        }
        args.extend(["--end-of-options", range]);
        let out = git_out(&root, &args).unwrap_or_default();
        let mut list: Vec<Value> = out
            .split('\x1e')
            .filter_map(|rec| {
                let f: Vec<&str> = rec.split('\x1f').collect();
                let [sha, short, title, body, author, ct, refs] = f[..] else { return None };
                if let Some(s) = &search {
                    if ![title, body, author].iter().any(|t| t.to_lowercase().contains(s.as_str())) && !sha.starts_with(s.as_str()) {
                        return None;
                    }
                }
                Some(json!({"sha": sha, "short": short, "title": title, "message": body.trim(), "author": author, "created": ct.parse::<u64>().unwrap_or(0), "refs": decorations(refs.trim())}))
            })
            .skip(if search.is_some() { q.skip } else { 0 })
            .take(q.limit + 1)
            .collect();
        let more = list.len() > q.limit;
        list.truncate(q.limit);
        (list, more)
    };
    // Counted on the first-parent line, as the list is: on the default branch, a merged MR is one commit.
    let base = base_branch(&root);
    let shas = |range: &str| git_out(&root, &["rev-list", "--first-parent", "-n", "1000", "--end-of-options", range]).unwrap_or_default();
    let count = |range: &str| git_out(&root, &["rev-list", "--first-parent", "--count", "--end-of-options", range]).and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    if q.behind {
        let Some(b) = base else { return json!({"commits": [], "more": false}) };
        let (list, more) = log(&format!("{rev}..{b}"));
        return json!({"commits": list, "more": more, "base": b});
    }
    let (mut list, more) = log(rev);
    let own = base.map(|b| shas(&format!("{b}..{rev}"))).unwrap_or_default();
    let own: HashSet<&str> = own.lines().collect();
    let unpushed = shas(&format!("{rev}@{{upstream}}..{rev}"));
    let unpushed: HashSet<&str> = unpushed.lines().collect();
    for c in &mut list {
        let sha = c["sha"].as_str().unwrap_or_default().to_string();
        c["own"] = json!(own.contains(sha.as_str()));
        c["unpushed"] = json!(unpushed.contains(sha.as_str()));
    }
    let (ahead, behind) = base.map_or((0, 0), |b| (count(&format!("{b}..{rev}")), count(&format!("{rev}..{b}"))));
    json!({"commits": list, "more": more, "base": base, "ahead": ahead, "behind": behind})
}

/// The files one commit changes, as [status letter, path], against its first parent.
pub fn commit_files(path: &str, sha: &str) -> Value {
    let Some(root) = toplevel(path) else { return json!([]) };
    let out = git_out(&root, &["show", "--no-color", "--format=", "--name-status", "--first-parent", sha]).unwrap_or_default();
    // "M\tpath", or "R100\told\tnew" for a rename.
    out.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(code, p)| json!([&code[..1], p.rsplit('\t').next().unwrap_or(p)]))
        .collect()
}

/// Local and remote branches as [kind, name, full ref], most recently committed first.
pub fn branches(path: &str) -> Value {
    let Some(root) = toplevel(path) else { return json!({"current": null, "branches": []}) };
    let out = git_out(&root, &["for-each-ref", "--sort=-committerdate", "--format=%(refname)", "refs/heads", "refs/remotes"]).unwrap_or_default();
    let list: Vec<Value> = out
        .lines()
        .filter(|r| !r.ends_with("/HEAD"))
        .filter_map(|r| {
            let (kind, name) = r.strip_prefix("refs/heads/").map(|n| ("local", n)).or_else(|| r.strip_prefix("refs/remotes/").map(|n| ("remote", n)))?;
            Some(json!([kind, name, r]))
        })
        .collect();
    let current = git_out(&root, &["symbolic-ref", "-q", "HEAD"]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    json!({"current": current, "branches": list})
}

/// Which side of the index a working-tree diff shows.
#[derive(Clone, Copy, PartialEq)]
pub enum Side {
    /// HEAD against the working tree.
    All,
    /// HEAD against the index.
    Staged,
    /// The index against the working tree, with untracked files.
    Unstaged,
}

/// The change of one file, or of the whole checkout, on one side of the index, or what one commit changed. New files show in full.
pub fn diff(path: &str, file: Option<&str>, commit: Option<&str>, side: Side) -> String {
    let Some(root) = toplevel(path) else {
        return String::new();
    };
    let base: &[&str] = match side {
        Side::All => &["diff", "HEAD", "--no-color"],
        Side::Staged => &["diff", "--cached", "--no-color"],
        Side::Unstaged => &["diff", "--no-color"],
    };
    let new_file = |f: &str| git_out(&root, &["diff", "--no-color", "--no-index", "--", "/dev/null", f]).unwrap_or_default();
    let mut text = match (commit, file) {
        (Some(sha), f) => {
            let mut args = vec!["show", "--no-color", "--format=", "--first-parent", sha];
            args.extend(f.map(|f| ["--", f]).into_iter().flatten());
            git_out(&root, &args).unwrap_or_default()
        }
        (None, Some(f)) => {
            let tracked = git_out(&root, &[base, &["--", f]].concat()).unwrap_or_default();
            let untracked = if side == Side::Staged { String::new() } else { git_out(&root, &["ls-files", "--others", "--exclude-standard", "--", f]).unwrap_or_default() };
            if tracked.is_empty() && !untracked.trim().is_empty() { new_file(f) } else { tracked }
        }
        (None, None) => {
            let mut all = git_out(&root, base).unwrap_or_default();
            if side != Side::Staged {
                let untracked = git_out(&root, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
                for f in untracked.lines().take(50) {
                    all.push_str(&new_file(f));
                }
            }
            all
        }
    };
    if text.len() > MAX_DIFF {
        let mut cut = MAX_DIFF;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n… diff cut off at 1 MB\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_commits_and_their_diff() {
        let dir = std::env::temp_dir().join(format!("jugglr-commits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.to_str().unwrap();
        let sh = |args: &[&str]| assert!(Command::new("git").current_dir(&dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap().status.success());
        sh(&["init", "-q", "-b", "main"]);
        sh(&["commit", "-q", "--allow-empty", "-m", "base"]);
        let q = |skip, limit| LogQuery { rev: None, behind: false, search: None, skip, limit };
        let titles = |v: &Value| v["commits"].as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let v = commits(p, &q(0, 50));
        assert_eq!((titles(&v), &v["ahead"], &v["behind"], &v["commits"][0]["own"]), (vec!["base".into()], &json!(0), &json!(0), &json!(false)), "on the default branch");
        sh(&["checkout", "-q", "-b", "feature"]);
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        sh(&["add", "a.txt"]);
        sh(&["commit", "-q", "-m", "add a", "-m", "why"]);
        sh(&["commit", "-q", "--allow-empty", "-m", "second"]);
        let v = commits(p, &q(0, 1));
        let c = &v["commits"][0];
        assert_eq!((c["title"].as_str(), v["more"].as_bool()), (Some("second"), Some(true)));
        assert_eq!(c["refs"], json!([["head", "feature"]]));
        assert_eq!(titles(&commits(p, &q(1, 1))), ["add a"]);
        let v = commits(p, &q(0, 50));
        assert_eq!(titles(&v), ["second", "add a", "base"]);
        let own: Vec<_> = v["commits"].as_array().unwrap().iter().map(|c| c["own"].as_bool().unwrap()).collect();
        assert_eq!((own, &v["ahead"], &v["behind"], &v["base"]), (vec![true, true, false], &json!(2), &json!(0), &json!("main")));
        sh(&["checkout", "-q", "main"]);
        sh(&["commit", "-q", "--allow-empty", "-m", "on main"]);
        sh(&["checkout", "-q", "feature"]);
        assert_eq!(commits(p, &q(0, 50))["behind"], json!(1));
        assert_eq!(titles(&commits(p, &LogQuery { behind: true, ..q(0, 50) })), ["on main"]);
        let found = commits(p, &LogQuery { search: Some("ADD"), ..q(0, 50) });
        assert_eq!(found["commits"].as_array().unwrap().len(), 1, "search ignores case");
        assert_eq!(commits(p, &LogQuery { search: Some("T"), ..q(0, 50) })["commits"].as_array().unwrap().len(), 3, "the author matches");
        let sha = commits(p, &q(1, 1))["commits"][0]["sha"].as_str().unwrap().to_string();
        assert_eq!(commit_files(p, &sha), json!([["A", "a.txt"]]));
        let d = diff(p, None, Some(&sha), Side::All);
        assert!(d.contains("+hello") && d.starts_with("diff --git"), "{d}");
        assert_eq!(commits(p, &LogQuery { rev: Some("main"), ..q(0, 50) })["commits"].as_array().unwrap().len(), 2);
        assert_eq!(commits(p, &LogQuery { rev: Some("--all"), ..q(0, 50) })["error"], json!("Unknown branch"));
        assert_eq!(branches(p), json!({"current": "refs/heads/feature", "branches": [["local", "feature", "refs/heads/feature"], ["local", "main", "refs/heads/main"]]}));
        sh(&["update-ref", "refs/remotes/upstream/master", "HEAD"]);
        assert_eq!(commits(p, &q(0, 50))["base"], json!("upstream/master"), "upstream wins over main");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
