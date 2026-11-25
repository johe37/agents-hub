//! The JSON the page renders, built from a Snapshot.

use std::process::Command;

use serde_json::{Value, json};

use crate::config::{Config, shorten};
use crate::git::GitStatus;
use crate::gitlab::{Issue, Job, Mr};
use crate::snapshot::{Agent, Snapshot, Workspace};
use crate::state::Status;

pub fn status(s: Status) -> &'static str {
    match s {
        Status::Waiting => "waiting",
        Status::Working => "working",
        Status::Polling => "polling",
        Status::Idle => "idle",
        Status::Unknown => "unknown",
        Status::Ended => "ended",
        Status::Stale => "stale",
    }
}

fn git(g: &GitStatus) -> Value {
    json!({
        "branch": g.branch,
        "commit": g.commit.as_deref().map(|c| &c[..c.len().min(8)]),
        "dirty": g.dirty,
        "ahead": g.ahead,
        "behind": g.behind,
        "added": g.added,
        "removed": g.removed,
        "files": g.files.iter().map(|(code, path)| json!([code, path])).collect::<Vec<_>>(),
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
        "gitlab": {
            "enabled": cfg.gitlab(),
            "loading": snap.inbox_loading,
            "errors": snap.inbox_errors.iter().map(|(s, e)| json!([s.title(), e])).collect::<Vec<_>>(),
            "reviews": snap.reviews.iter().map(|m| listed_mr(m, snap)).collect::<Vec<_>>(),
            "authored": snap.authored.iter().map(|m| listed_mr(m, snap)).collect::<Vec<_>>(),
            "assigned": snap.assigned.iter().map(|m| listed_mr(m, snap)).collect::<Vec<_>>(),
            "issues": snap.issues.iter().map(issue).collect::<Vec<_>>(),
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

/// The change of one file against HEAD, or of the whole checkout. New files show in full.
pub fn diff(path: &str, file: Option<&str>) -> String {
    let Some(root) = git_out(path, &["rev-parse", "--show-toplevel"]).map(|r| r.trim().to_string()).filter(|r| !r.is_empty()) else {
        return String::new();
    };
    let mut text = match file {
        Some(f) => {
            let tracked = git_out(&root, &["diff", "HEAD", "--no-color", "--", f]).unwrap_or_default();
            let untracked = git_out(&root, &["ls-files", "--others", "--exclude-standard", "--", f]).unwrap_or_default();
            if tracked.is_empty() && !untracked.trim().is_empty() {
                git_out(&root, &["diff", "--no-color", "--no-index", "--", "/dev/null", f]).unwrap_or_default()
            } else {
                tracked
            }
        }
        None => {
            let mut all = git_out(&root, &["diff", "HEAD", "--no-color"]).unwrap_or_default();
            let untracked = git_out(&root, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
            for f in untracked.lines().take(50) {
                all.push_str(&git_out(&root, &["diff", "--no-color", "--no-index", "--", "/dev/null", f]).unwrap_or_default());
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
