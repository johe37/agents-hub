//! XP from GitLab: closed issues assigned to you give XP, open ones are your quests.
//!
//! Everything goes through glab (`glab api`), so claude-hub never sees your token: glab reads it
//! from its own config, the keyring or GITLAB_TOKEN. A failed sync changes nothing but the error
//! and when to try again; the town keeps working from the progress file.

use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;

use crate::config::Config;
use crate::progress::{self, Gains, Issue, MrRole, Progress, Rules, Store, format_time, level_for, parse_time};

/// Closed issues this much older than the newest one seen are fetched again next time, in case
/// GitLab's clocks or our cursor are a little off. Counted ones are skipped by id.
const OVERLAP: u64 = 600;

/// One GitLab REST call: the path after /api/v4/, all pages. Returns the response body.
pub trait Api {
    fn get(&self, path: &str) -> Result<String, String>;
}

/// `glab api --paginate`, on the configured host.
pub struct Glab {
    pub host: Option<String>,
}

impl Glab {
    pub fn new(cfg: &Config) -> Self {
        Glab { host: cfg.gitlab_url.as_deref().map(host_of) }
    }
}

impl Api for Glab {
    fn get(&self, path: &str) -> Result<String, String> {
        let mut args = vec!["api"];
        if let Some(h) = &self.host {
            args.extend(["--hostname", h]);
        }
        args.extend(["--paginate", path]);
        let home = crate::config::home().display().to_string();
        crate::gitlab::run(&home, 90, "glab", &args)
    }
}

/// "https://git.example.com/" → "git.example.com".
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest).to_string()
}

/// Every element of every page. `glab api --paginate` prints the pages one after another, each a JSON array.
pub fn parse_pages(text: &str) -> Result<Vec<Value>, String> {
    let mut out = vec![];
    for page in serde_json::Deserializer::from_str(text).into_iter::<Value>() {
        match page.map_err(|e| format!("GitLab sent something unexpected: {e}"))? {
            Value::Array(items) => out.extend(items),
            other => return Err(format!("GitLab sent something unexpected: {}", other.to_string().chars().take(120).collect::<String>())),
        }
    }
    Ok(out)
}

pub fn parse_issue(v: &Value) -> Option<Issue> {
    let full = v["references"]["full"].as_str().unwrap_or_default();
    Some(Issue {
        id: v["id"].as_u64()?,
        project_id: v["project_id"].as_u64().unwrap_or(0),
        // "group/app#17" → "group/app"
        project: full.rsplit_once('#').map_or(full, |(p, _)| p).to_string(),
        iid: v["iid"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        url: v["web_url"].as_str().unwrap_or_default().to_string(),
        weight: v["weight"].as_u64(),
        closed_at: v["closed_at"].as_str().and_then(parse_time),
        updated_at: v["updated_at"].as_str().and_then(parse_time),
        moved: !v["moved_to_id"].is_null(),
        assignees: v["assignees"].as_array().map(|a| a.iter().filter_map(|u| u["username"].as_str().map(String::from)).collect()),
        due_date: v["due_date"].as_str().map(String::from),
        labels: v["labels"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect(),
    })
}

fn parse_issues(text: &str) -> Result<Vec<Issue>, String> {
    Ok(parse_pages(text)?.iter().filter_map(parse_issue).collect())
}

/// A merged MR, in the shape counting uses: `closed_at` is when it was merged, `assignees` the users
/// in the role asked about (its assignees or its reviewers).
pub fn parse_mr(v: &Value, role: MrRole) -> Option<Issue> {
    let mut m = parse_issue(v)?;
    m.closed_at = v["merged_at"].as_str().and_then(parse_time);
    m.moved = false;
    let field = match role {
        MrRole::Assignee => "assignees",
        MrRole::Reviewer => "reviewers",
    };
    m.assignees = v[field].as_array().map(|a| a.iter().filter_map(|u| u["username"].as_str().map(String::from)).collect());
    // "group/app!17" → "group/app"
    let full = v["references"]["full"].as_str().unwrap_or_default();
    m.project = full.rsplit_once('!').map_or(full, |(p, _)| p).to_string();
    Some(m)
}

fn parse_mrs(text: &str, role: MrRole) -> Result<Vec<Issue>, String> {
    Ok(parse_pages(text)?.iter().filter_map(|v| parse_mr(v, role)).collect())
}

/// What one sync fetched.
struct Fetched {
    user: String,
    closed: Vec<Issue>,
    open: Vec<Issue>,
    merged: Vec<Issue>,
    reviewed: Vec<Issue>,
    start: u64,
}

/// When counting starts, from `xp_since`: the progress file's birth, the beginning of time, or a date.
pub fn since(cfg: &Config, p: &Progress) -> u64 {
    match cfg.xp_since.trim() {
        "all" => 0,
        "install" | "" => p.started_at,
        date => parse_time(date).unwrap_or(p.started_at),
    }
}

/// Seconds until the next background sync may run.
fn wait(cfg: &Config, p: &Progress) -> u64 {
    let interval = (cfg.sync_minutes * 60.0) as u64;
    let backoff = match p.sync.failures {
        0 => interval,
        n => (60u64 << (n - 1).min(10)).min(interval),
    };
    // GitLab asked us to slow down: give it at least five minutes.
    let backoff = if p.sync.last_error.as_deref().is_some_and(|e| e.contains("429")) { backoff.max(300) } else { backoff };
    p.sync.last_attempt.map_or(0, |at| (at + backoff).saturating_sub(crate::state::now()))
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub gains: Gains,
    pub quests: usize,
    pub level: u32,
    pub total_xp: u64,
}

/// Fetch and count. With `force` false it does nothing until the next sync is due, so the hub, the
/// town and several of each can all call it and GitLab is asked once per `sync_minutes`.
pub fn sync(cfg: &Config, store: &Store, api: &dyn Api, force: bool) -> Result<Option<Report>, String> {
    let now = crate::state::now();
    // Claim this round under the lock, so two processes do not both fetch.
    let claimed = store
        .update(|p| {
            if !force && wait(cfg, p) > 0 {
                return None;
            }
            p.sync.last_attempt = Some(now);
            Some(p.clone())
        })
        .map_err(|e| e.to_string())?;
    let Some(p) = claimed else { return Ok(None) };

    let fetched = fetch(cfg, &p, api);
    let result = store
        .update(|p| match fetched {
            Ok(f) => {
                let rules = Rules {
                    username: f.user,
                    xp_per_weight: cfg.xp_per_weight,
                    default_weight: cfg.default_weight,
                    since: f.start,
                    chances: cfg.drop_chances.clone(),
                    xp_per_merged_mr: cfg.xp_per_merged_mr,
                    xp_per_review: cfg.xp_per_review,
                };
                let mut gains = p.count_closed(&f.closed, &rules, now);
                for (list, role) in [(&f.merged, MrRole::Assignee), (&f.reviewed, MrRole::Reviewer)] {
                    let g = p.count_merged(list, role, &rules, now);
                    gains.mrs.extend(g.mrs);
                    gains.xp += g.xp;
                    gains.levels.extend(g.levels);
                }
                // A gentler level curve can owe you levels for XP you already had.
                let trigger = p.latest_trigger();
                p.catch_up(trigger, &mut gains, &rules.chances);
                p.set_quests(&f.open, &rules);
                if let Some(newest) = f.closed.iter().filter_map(|i| i.updated_at).max() {
                    p.sync.cursor = Some(newest.saturating_sub(OVERLAP).max(p.sync.cursor.unwrap_or(0)));
                }
                if let Some(newest) = f.merged.iter().chain(&f.reviewed).filter_map(|i| i.updated_at).max() {
                    p.sync.mr_cursor = Some(newest.saturating_sub(OVERLAP).max(p.sync.mr_cursor.unwrap_or(0)));
                }
                p.sync.cursor_since = Some(f.start);
                p.sync.mr_cursor_since = Some(f.start);
                p.sync.last_ok = Some(now);
                p.sync.failures = 0;
                p.sync.last_error = None;
                Ok(Report { gains, quests: p.quests.len(), level: p.level, total_xp: p.total_xp })
            }
            Err(e) => {
                p.sync.failures += 1;
                p.sync.last_error = Some(e.clone());
                Err(e)
            }
        })
        .map_err(|e| e.to_string())?;
    result.map(Some)
}

/// The GitLab user, the closed issues to look at, the open ones, merged MRs you were assigned to and
/// reviewed, and when counting starts.
fn fetch(cfg: &Config, p: &Progress, api: &dyn Api) -> Result<Fetched, String> {
    let user = match &cfg.gitlab_username {
        Some(u) => u.clone(),
        None => {
            let v: Value = serde_json::from_str(&api.get("user")?).map_err(|e| format!("GitLab sent something unexpected: {e}"))?;
            v["username"].as_str().ok_or("GitLab did not say who you are")?.to_string()
        }
    };
    if !user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err(format!("Not a GitLab username: {user}"));
    }
    let start = since(cfg, p);
    // A new start (xp_since changed) fetches everything from it again; otherwise from the cursor.
    // Labels missing from older counted issues (they decide your class) also fetch everything once.
    let cursor = p.sync.cursor.filter(|_| p.sync.cursor_since == Some(start) && !p.missing_labels());
    // An issue closed after the start was updated after it too, so the start limits the fetch as well.
    let after = cursor.unwrap_or(0).max(start);
    let base = format!("issues?scope=all&assignee_username={user}&per_page=100");
    let mut closed_q = format!("{base}&state=closed&order_by=updated_at&sort=asc");
    if after > 0 {
        closed_q.push_str(&format!("&updated_after={}", format_time(after)));
    }
    let closed = parse_issues(&api.get(&closed_q)?)?;
    let open = parse_issues(&api.get(&format!("{base}&state=opened"))?)?;
    // Merged MRs, the same way: from their own cursor, or everything from the start the first time.
    let mr_after = p.sync.mr_cursor.filter(|_| p.sync.mr_cursor_since == Some(start)).unwrap_or(0).max(start);
    let mr_query = |who: &str| {
        let mut q = format!("merge_requests?scope=all&state=merged&{who}_username={user}&per_page=100&order_by=updated_at&sort=asc");
        if mr_after > 0 {
            q.push_str(&format!("&updated_after={}", format_time(mr_after)));
        }
        q
    };
    let merged = parse_mrs(&api.get(&mr_query("assignee"))?, MrRole::Assignee)?;
    let reviewed = parse_mrs(&api.get(&mr_query("reviewer"))?, MrRole::Reviewer)?;
    Ok(Fetched { user, closed, open, merged, reviewed, start })
}

/// Sync in the background whenever it is due. Only when a GitLab is configured, so the hub does not
/// call glab for people who do not use XP; `claude-hub xp sync` works either way.
pub fn spawn_background(cfg: Config) {
    if cfg.gitlab_url.is_none() && cfg.gitlab_username.is_none() {
        return;
    }
    std::thread::spawn(move || {
        let store = Store::default();
        let api = Glab::new(&cfg);
        loop {
            let _ = sync(&cfg, &store, &api, false);
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

// ---------------------------------------------------------------- claude-hub xp

pub const USAGE: &str = "usage: claude-hub xp [sync]

Show your level and XP, or with sync, fetch closed GitLab issues now.";

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub fn run(cfg: Config, args: &[String]) -> ExitCode {
    let store = Store::default();
    match args.first().map(String::as_str) {
        None => {
            match store.load() {
                Some(p) => print!("{}", summary(&cfg, &p)),
                None => println!("No progress yet. Run `claude-hub xp sync` after setting gitlab_url in the config."),
            }
            ExitCode::SUCCESS
        }
        Some("sync") => match sync(&cfg, &store, &Glab::new(&cfg), true) {
            Ok(Some(r)) => {
                let g = &r.gains;
                let reviews = g.mrs.iter().filter(|k| k.starts_with("review:")).count();
                let merged = g.mrs.len() - reviews;
                if g.xp == 0 {
                    println!("Nothing new closed or merged.");
                } else {
                    println!("+{} XP: {} closed issue{}, {} merged MR{}, {} review{}.", g.xp, g.issues.len(), plural(g.issues.len()), merged, plural(merged), reviews, plural(reviews));
                }
                for l in &g.levels {
                    let item = store.load().and_then(|p| p.items.iter().find(|i| i.dropped_at_level == *l).map(|i| format!("{} ({:?})", i.name, i.rarity)));
                    println!("Level {l}! Loot: {}", item.unwrap_or_default());
                }
                let (level, into, need) = level_for(r.total_xp);
                println!("Level {level}, {into}/{need} XP. {} open quest{}.", r.quests, if r.quests == 1 { "" } else { "s" });
                ExitCode::SUCCESS
            }
            Ok(None) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("claude-hub: sync failed: {e}");
                if cfg.gitlab_url.is_none() {
                    eprintln!("Set gitlab_url in ~/.config/claude-hub/config.toml if your GitLab is not glab's default host.");
                }
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn summary(cfg: &Config, p: &Progress) -> String {
    let (level, into, need) = level_for(p.total_xp);
    let class = crate::class::class_of(p).map_or(crate::class::NO_CLASS.0, |c| c.name);
    let mut out = format!("{class}, level {level}  {into}/{need} XP  ({} total from {} issues and {} merged MRs and reviews)\n", p.total_xp, p.issues.len(), p.mrs.len());
    out += &format!("Counting issues closed since {}\n", match since(cfg, p) {
        0 => "the beginning".into(),
        t => format_time(t)[..10].to_string(),
    });
    out += &format!("{} items, {} open quests\n", p.items.len(), p.quests.len());
    match (p.sync.last_ok, &p.sync.last_error) {
        (_, Some(e)) => out += &format!("Last sync failed: {e}\n"),
        (Some(t), None) => out += &format!("Last sync {}\n", format_time(t)),
        (None, None) => out += "Never synced\n",
    }
    out
}

/// A short "Verminbane · Lv 6 · 340/520 XP" for the hub's footer.
pub fn badge(p: &Progress) -> String {
    let (level, into, need) = progress::level_for(p.total_xp);
    match crate::class::class_of(p) {
        Some(c) => format!("{} · Lv {level} · {into}/{need} XP", c.name),
        None => format!("Lv {level} · {into}/{need} XP"),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// A GitLab that answers from a table of path prefixes, and remembers what it was asked.
    #[derive(Default)]
    pub struct MockGitLab {
        pub answers: RefCell<HashMap<&'static str, Result<String, String>>>,
        pub asked: RefCell<Vec<String>>,
    }

    impl Api for MockGitLab {
        fn get(&self, path: &str) -> Result<String, String> {
            self.asked.borrow_mut().push(path.to_string());
            let answers = self.answers.borrow();
            // "user" is a whole path; every other key matches any path containing it.
            let key = answers.keys().filter(|k| if **k == "user" { path == "user" } else { path.contains(**k) }).max_by_key(|k| k.len()).copied();
            match key {
                Some(k) => answers[k].clone(),
                // Tests about issues do not care about MRs: none merged.
                None if path.starts_with("merge_requests") => Ok("[]".into()),
                None => Err(format!("mock: nothing for {path}")),
            }
        }
    }

    fn gl_issue(id: u64, iid: u64, weight: Option<u64>, closed: Option<&str>, updated: &str) -> Value {
        serde_json::json!({
            "id": id, "iid": iid, "project_id": 7, "title": format!("Fix thing {iid}"),
            "state": if closed.is_some() { "closed" } else { "opened" },
            "web_url": format!("https://git.example.com/group/app/-/issues/{iid}"),
            "references": {"full": format!("group/app#{iid}")},
            "weight": weight, "closed_at": closed, "updated_at": updated, "moved_to_id": null,
            "assignees": [{"username": "osbo"}], "labels": ["backend"], "due_date": null,
        })
    }

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("claude-hub-xp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store { path: dir.join("progress.json") }
    }

    fn cfg() -> Config {
        Config { gitlab_url: Some("https://git.example.com".into()), xp_since: "all".into(), ..Config::default() }
    }

    fn pages(pages: &[Vec<Value>]) -> String {
        // glab --paginate prints the pages back to back.
        pages.iter().map(|p| Value::Array(p.clone()).to_string()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn sync_counts_closed_issues_over_pages_and_makes_quests() {
        let store = temp_store("pages");
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("user", Ok(r#"{"username":"osbo"}"#.into()));
        let mut moved = gl_issue(4, 4, Some(9), Some("2026-11-03T00:00:00Z"), "2026-11-03T00:00:00Z");
        moved["moved_to_id"] = 99.into();
        gl.answers.borrow_mut().insert(
            "state=closed",
            Ok(pages(&[
                vec![gl_issue(1, 1, Some(3), Some("2026-11-01T10:00:00Z"), "2026-11-01T10:00:00Z"), gl_issue(2, 2, None, Some("2026-11-02T10:00:00Z"), "2026-11-02T10:00:00Z")],
                // The page boundary repeats an issue, as offset paging can when issues change meanwhile.
                vec![gl_issue(2, 2, None, Some("2026-11-02T10:00:00Z"), "2026-11-02T10:00:00Z"), moved],
            ])),
        );
        gl.answers.borrow_mut().insert("state=opened", Ok(pages(&[vec![gl_issue(5, 5, Some(8), None, "2026-11-04T00:00:00Z"), gl_issue(6, 6, None, None, "2026-11-04T00:00:00Z")]])));
        let r = sync(&cfg(), &store, &gl, true).unwrap().unwrap();
        assert_eq!(r.gains.issues, vec![1, 2]);
        assert_eq!(r.total_xp, 40);
        assert_eq!(r.quests, 2);
        let p = store.load().unwrap();
        assert_eq!(p.quests[0].xp, 80, "the biggest reward first");
        assert_eq!(p.quests[0].project, "group/app");
        assert_eq!(p.issues[&1].project, "group/app");
        assert_eq!(p.sync.cursor, Some(parse_time("2026-11-03T00:00:00Z").unwrap() - OVERLAP));
        assert!(gl.asked.borrow().iter().any(|q| q.starts_with("issues?scope=all&assignee_username=osbo&per_page=100&state=closed")));

        // The next sync asks only for what changed since the cursor, and counts nothing twice.
        gl.asked.borrow_mut().clear();
        let r = sync(&cfg(), &store, &gl, true).unwrap().unwrap();
        assert_eq!(r.gains, Gains::default());
        assert_eq!(r.total_xp, 40);
        assert!(gl.asked.borrow().iter().any(|q| q.contains("updated_after=2026-11-02T23:50:00Z")), "{:?}", gl.asked.borrow());
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_failed_sync_keeps_progress_and_backs_off() {
        let store = temp_store("fail");
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("user", Ok(r#"{"username":"osbo"}"#.into()));
        gl.answers.borrow_mut().insert("state=closed", Ok(pages(&[vec![gl_issue(1, 1, Some(2), Some("2026-11-01T10:00:00Z"), "2026-11-01T10:00:00Z")]])));
        gl.answers.borrow_mut().insert("state=opened", Ok("[]".into()));
        sync(&cfg(), &store, &gl, true).unwrap();
        assert_eq!(store.load().unwrap().total_xp, 20);

        gl.answers.borrow_mut().insert("state=closed", Err("glab: 429 Too Many Requests".into()));
        assert!(sync(&cfg(), &store, &gl, true).is_err());
        let p = store.load().unwrap();
        assert_eq!(p.total_xp, 20, "the town keeps the last known XP");
        assert_eq!(p.sync.failures, 1);
        assert!(p.sync.last_error.as_deref().unwrap().contains("429"));
        // Not due again for at least five minutes after a rate limit.
        assert!(wait(&cfg(), &p) >= 290);
        assert_eq!(sync(&cfg(), &store, &gl, false), Ok(None));

        // Back online: the error clears.
        gl.answers.borrow_mut().insert("state=closed", Ok("[]".into()));
        sync(&cfg(), &store, &gl, true).unwrap();
        let p = store.load().unwrap();
        assert_eq!((p.sync.failures, p.sync.last_error.clone()), (0, None));
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn install_date_start_skips_older_issues_and_a_new_start_refetches() {
        let store = temp_store("since");
        store.update(|p| p.started_at = parse_time("2026-11-05").unwrap()).unwrap();
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("user", Ok(r#"{"username":"osbo"}"#.into()));
        gl.answers.borrow_mut().insert(
            "state=closed",
            Ok(pages(&[vec![gl_issue(1, 1, Some(3), Some("2026-11-01T00:00:00Z"), "2026-11-06T00:00:00Z"), gl_issue(2, 2, Some(1), Some("2026-11-06T00:00:00Z"), "2026-11-06T00:00:00Z")]])),
        );
        gl.answers.borrow_mut().insert("state=opened", Ok("[]".into()));
        let mut c = cfg();
        c.xp_since = "install".into();
        sync(&c, &store, &gl, true).unwrap();
        assert_eq!(store.load().unwrap().total_xp, 10, "closed before install: commented on later, still not counted");
        assert!(gl.asked.borrow().iter().any(|q| q.contains("updated_after=2026-11-05T00:00:00Z")));

        // Switching to "all" fetches from the beginning again and counts the older one.
        gl.asked.borrow_mut().clear();
        c.xp_since = "all".into();
        sync(&c, &store, &gl, true).unwrap();
        assert_eq!(store.load().unwrap().total_xp, 40);
        assert!(!gl.asked.borrow().iter().any(|q| q.contains("updated_after")), "{:?}", gl.asked.borrow());
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn by_default_old_issues_count_and_no_weight_is_weight_one() {
        let store = temp_store("defaults");
        store.update(|p| p.started_at = parse_time("2026-11-19").unwrap()).unwrap();
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("user", Ok(r#"{"username":"osbo"}"#.into()));
        gl.answers.borrow_mut().insert("state=closed", Ok(pages(&[vec![gl_issue(1, 1, None, Some("2025-03-01T00:00:00Z"), "2025-03-01T00:00:00Z"), gl_issue(2, 2, Some(4), Some("2024-06-01T00:00:00Z"), "2024-06-01T00:00:00Z")]])));
        gl.answers.borrow_mut().insert("state=opened", Ok("[]".into()));
        let c = Config { gitlab_url: Some("https://git.example.com".into()), ..Config::default() };
        assert_eq!(c.xp_since, "all");
        assert_eq!(c.default_weight, 1);
        let r = sync(&c, &store, &gl, true).unwrap().unwrap();
        assert_eq!(r.gains.issues, vec![2, 1], "closed long before install, oldest first");
        assert_eq!(r.total_xp, 50, "4 × 10, plus 1 × 10 for the issue without a weight");
        assert!(!gl.asked.borrow().iter().any(|q| q.contains("updated_after")), "the whole history is fetched");
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn labels_missing_from_an_older_file_are_fetched_once_without_changing_xp() {
        let store = temp_store("labels");
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("user", Ok(r#"{"username":"osbo"}"#.into()));
        let mut bug = gl_issue(1, 1, Some(3), Some("2026-11-01T00:00:00Z"), "2026-11-01T00:00:00Z");
        bug["labels"] = serde_json::json!(["Type::Bug", "Team::Core"]);
        gl.answers.borrow_mut().insert("state=closed", Ok(pages(&[vec![bug]])));
        gl.answers.borrow_mut().insert("state=opened", Ok("[]".into()));
        sync(&cfg(), &store, &gl, true).unwrap();
        // As a file from before labels were kept: labels gone, cursor set.
        store.update(|p| p.issues.values_mut().for_each(|c| c.labels = None)).unwrap();
        assert!(crate::class::class_of(&store.load().unwrap()).is_none());
        gl.asked.borrow_mut().clear();
        sync(&cfg(), &store, &gl, true).unwrap();
        assert!(!gl.asked.borrow().iter().any(|q| q.contains("updated_after")), "fetched everything once");
        let p = store.load().unwrap();
        assert_eq!(p.total_xp, 30);
        assert_eq!(p.issues[&1].labels.as_deref(), Some(&["Type::Bug".to_string(), "Team::Core".to_string()][..]));
        assert_eq!(crate::class::class_of(&p).unwrap().name, "Verminbane");
        // And from then on incremental again.
        gl.asked.borrow_mut().clear();
        sync(&cfg(), &store, &gl, true).unwrap();
        assert!(gl.asked.borrow().iter().any(|q| q.contains("updated_after")));
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn merged_mrs_and_reviews_sync_with_their_own_cursor() {
        let store = temp_store("mrs");
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("user", Ok(r#"{"username":"osbo"}"#.into()));
        gl.answers.borrow_mut().insert("state=closed&order_by", Ok("[]".into()));
        gl.answers.borrow_mut().insert("state=opened", Ok("[]".into()));
        let mr = |id: u64, merged: &str, assignee: &str, reviewer: &str| serde_json::json!({
            "id": id, "iid": id, "project_id": 7, "title": format!("MR {id}"), "state": "merged",
            "web_url": format!("https://git.example.com/group/app/-/merge_requests/{id}"),
            "references": {"full": format!("group/app!{id}")}, "merged_at": merged, "updated_at": merged,
            "assignees": [{"username": assignee}], "reviewers": [{"username": reviewer}],
        });
        gl.answers.borrow_mut().insert("assignee_username=osbo&per_page=100&order_by", Ok(pages(&[vec![mr(1, "2026-10-01T00:00:00Z", "osbo", "vilu")]])));
        gl.answers.borrow_mut().insert("reviewer_username=osbo", Ok(pages(&[vec![mr(2, "2026-10-02T00:00:00Z", "vilu", "osbo"), mr(3, "2026-10-03T00:00:00Z", "vilu", "osbo")]])));
        let r = sync(&cfg(), &store, &gl, true).unwrap().unwrap();
        assert_eq!(r.total_xp, 25 + 10 + 10);
        let p = store.load().unwrap();
        assert_eq!(p.mrs["mr:1"].project, "group/app");
        assert_eq!(p.mrs.len(), 3);
        assert!(gl.asked.borrow().iter().any(|q| q.starts_with("merge_requests?scope=all&state=merged&assignee_username=osbo")));
        // The next sync asks only for what changed, and counts nothing twice.
        gl.asked.borrow_mut().clear();
        let r = sync(&cfg(), &store, &gl, true).unwrap().unwrap();
        assert_eq!(r.total_xp, 45);
        assert!(gl.asked.borrow().iter().filter(|q| q.starts_with("merge_requests")).all(|q| q.contains("updated_after=2026-10-02T23:50:00Z")), "{:?}", gl.asked.borrow());
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn configured_username_skips_the_user_lookup() {
        let store = temp_store("user");
        let gl = MockGitLab::default();
        gl.answers.borrow_mut().insert("state=", Ok("[]".into()));
        let mut c = cfg();
        c.gitlab_username = Some("someone".into());
        sync(&c, &store, &gl, true).unwrap();
        assert!(gl.asked.borrow().iter().all(|q| q != "user"));
        assert!(gl.asked.borrow()[0].contains("assignee_username=someone"));
        c.gitlab_username = Some("x&scope=all".into());
        assert!(sync(&c, &store, &gl, true).unwrap_err().contains("Not a GitLab username"));
        std::fs::remove_dir_all(store.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn pages_and_hosts_parse() {
        assert_eq!(parse_pages("[1,2]\n[3]").unwrap().len(), 3);
        assert_eq!(parse_pages("[]").unwrap().len(), 0);
        assert!(parse_pages(r#"{"message":"401 Unauthorized"}"#).is_err());
        assert_eq!(host_of("https://git.example.com/"), "git.example.com");
        assert_eq!(host_of("git.example.com"), "git.example.com");
    }
}
