use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::cache_dir;

const STATUS_TTL: Duration = Duration::from_secs(5);

#[derive(Clone, Default, Debug)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub dirty: usize,
    pub ahead: u32,
    pub behind: u32,
    /// Like origin/main; a branch made from origin/main tracks it until its first push.
    pub upstream: Option<String>,
    pub remotes: Vec<String>,
    pub added: u32,
    pub removed: u32,
    /// (XY status code or ??, path)
    pub files: Vec<(String, String)>,
}

fn git(cwd: &str, args: &[&str]) -> Option<String> {
    // coreutils timeout, so a hung git (network fs, lock) cannot stall the collector.
    let out = Command::new("timeout").args(["5", "git", "-C", cwd]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Directories under roots as (path, is_repo). Stops descending at a repo, which keeps the scan fast.
pub fn scan_dirs(roots: &[String]) -> Vec<(String, bool)> {
    fn walk(path: &Path, is_root: bool, found: &mut Vec<(String, bool)>) {
        let Ok(rd) = std::fs::read_dir(path) else { return };
        let entries: Vec<_> = rd.flatten().collect();
        // .git is a directory in a normal repo and a file in worktrees and submodules.
        if entries.iter().any(|e| e.file_name() == ".git") {
            found.push((path.display().to_string(), true));
            return;
        }
        if !is_root {
            found.push((path.display().to_string(), false));
        }
        let mut dirs: Vec<_> = entries
            .into_iter()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.') && e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for d in dirs {
            walk(&d, false, found);
        }
    }
    let mut found = vec![];
    for root in roots {
        walk(Path::new(root), true, &mut found);
    }
    found
}

pub fn load_dir_cache() -> Option<Vec<(String, bool)>> {
    let text = std::fs::read_to_string(cache_dir().join("dirs.tsv")).ok()?;
    Some(text.lines().filter_map(|l| l.split_once('\t')).map(|(kind, path)| (path.to_string(), kind == "repo")).collect())
}

pub fn save_dir_cache(dirs: &[(String, bool)]) {
    let text: String = dirs.iter().map(|(p, repo)| format!("{}\t{p}\n", if *repo { "repo" } else { "dir" })).collect();
    let _ = std::fs::create_dir_all(cache_dir());
    let _ = std::fs::write(cache_dir().join("dirs.tsv"), text);
}

/// (status, path) from a porcelain v2 line.
fn changed_file(line: &str) -> (String, String) {
    let field = |n: usize| line.splitn(n + 1, ' ').collect::<Vec<_>>();
    match line.as_bytes()[0] {
        b'?' => ("??".into(), line[2..].into()),
        b'2' => {
            let f = field(9);
            (f[1].into(), f.get(9).map(|p| p.split('\t').next().unwrap_or(p)).unwrap_or(&"").to_string())
        }
        b'u' => {
            let f = field(10);
            (f[1].into(), f.get(10).unwrap_or(&"").to_string())
        }
        _ => {
            let f = field(8);
            (f.get(1).unwrap_or(&"").to_string(), f.get(8).unwrap_or(&"").to_string())
        }
    }
}

fn parse_status(out: &str) -> GitStatus {
    let mut st = GitStatus::default();
    for line in out.lines().filter(|l| !l.is_empty()) {
        if let Some(oid) = line.strip_prefix("# branch.oid ") {
            st.commit = Some(oid.into());
        } else if let Some(head) = line.strip_prefix("# branch.head ") {
            st.branch = Some(head.into());
        } else if let Some(up) = line.strip_prefix("# branch.upstream ") {
            st.upstream = Some(up.into());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut it = ab.split(' ').map(|n| n.trim_start_matches(['+', '-']).parse().unwrap_or(0));
            st.ahead = it.next().unwrap_or(0);
            st.behind = it.next().unwrap_or(0);
        } else if !line.starts_with('#') {
            st.dirty += 1;
            st.files.push(changed_file(line));
        }
    }
    st
}

impl GitStatus {
    /// The remote to push to without asking: the upstream's, else the only one.
    pub fn push_remote(&self) -> Option<&str> {
        let upstream = self.upstream.as_deref().and_then(|u| self.remotes.iter().find(|r| u.starts_with(&format!("{r}/"))));
        upstream.or(match self.remotes.as_slice() {
            [one] => Some(one),
            _ => None,
        })
        .map(String::as_str)
    }
}

/// Caches for repo-root lookups and rate-limited git status, safe to share between threads.
#[derive(Default)]
pub struct GitCache {
    roots: Mutex<HashMap<String, Option<String>>>,
    status: Mutex<HashMap<String, (Instant, GitStatus)>>,
    /// Repos with a status refresh running in the background.
    busy: Mutex<HashSet<String>>,
    /// Bumped by forget, so a refresh that started before it cannot store its older status.
    gens: Mutex<HashMap<String, u64>>,
}

impl GitCache {
    /// Top level of the checkout holding path; a worktree is its own checkout, with its own branch and changes.
    pub fn repo_root(&self, path: &str) -> Option<String> {
        if let Some(r) = self.roots.lock().unwrap().get(path) {
            return r.clone();
        }
        let root = git(path, &["rev-parse", "--show-toplevel"]).map(|out| out.trim().to_string()).filter(|r| !r.is_empty());
        self.roots.lock().unwrap().insert(path.to_string(), root.clone());
        root
    }

    /// The cached status, refreshed in the background once it is old, so a slow repo never holds up a snapshot.
    pub fn status(self: &Arc<Self>, repo: &str) -> GitStatus {
        let cached = self.status.lock().unwrap().get(repo).cloned();
        let Some((at, st)) = cached else { return self.refresh(repo) };
        if at.elapsed() >= STATUS_TTL && self.busy.lock().unwrap().insert(repo.to_string()) {
            let (cache, repo) = (Arc::clone(self), repo.to_string());
            std::thread::spawn(move || {
                cache.refresh(&repo);
                cache.busy.lock().unwrap().remove(&repo);
            });
        }
        st
    }

    /// Drop the cached status, so the next lookup runs git status at once.
    pub fn forget(&self, repo: &str) {
        *self.gens.lock().unwrap().entry(repo.to_string()).or_default() += 1;
        self.status.lock().unwrap().remove(repo);
    }

    fn refresh(&self, repo: &str) -> GitStatus {
        let generation = self.gens.lock().unwrap().get(repo).copied();
        let mut st = git(repo, &["status", "--porcelain=v2", "--branch"]).map(|o| parse_status(&o)).unwrap_or_default();
        st.remotes = git(repo, &["remote"]).unwrap_or_default().lines().map(String::from).collect();
        for line in git(repo, &["diff", "--numstat", "HEAD"]).unwrap_or_default().lines() {
            let mut f = line.split('\t');
            if let (Some(Ok(a)), Some(Ok(r))) = (f.next().map(str::parse::<u32>), f.next().map(str::parse::<u32>)) {
                st.added += a;
                st.removed += r;
            }
        }
        if self.gens.lock().unwrap().get(repo).copied() == generation {
            self.status.lock().unwrap().insert(repo.to_string(), (Instant::now(), st.clone()));
        }
        st
    }
}

/// Run a program in cwd. On failure, the line of its error output that says what went wrong.
pub fn run(cwd: &str, secs: u32, program: &str, args: &[&str]) -> Result<String, String> {
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
pub fn failure(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // git push starts with "To <remote>"; the useful part is the error or rejection further down.
    let line = lines.iter().find(|l| l.starts_with("error:") || l.starts_with("fatal:") || l.starts_with("! [")).or(lines.first());
    line.map(|l| l.to_string())
}

/// Push the branch checked out at cwd to a branch of the same name on the remote, and track it.
/// The same name, since an upstream like origin/master (the branch was made from it) makes a plain push fail.
pub fn push(cwd: &str, remote: &str) -> Result<String, String> {
    let branch = run(cwd, 5, "git", &["branch", "--show-current"])?.trim().to_string();
    if branch.is_empty() {
        return Err("This checkout is not on a branch".into());
    }
    run(cwd, 120, "git", &["push", "-u", remote, "HEAD"])?;
    Ok(format!("Pushed {branch} to {remote}"))
}

/// Commit what is staged, or every change when nothing is.
pub fn commit(cwd: &str, message: &str) -> Result<String, String> {
    // --quiet exits 1 when something is staged.
    if run(cwd, 5, "git", &["diff", "--cached", "--quiet"]).is_ok() {
        run(cwd, 30, "git", &["add", "-A"])?;
    }
    // Long: a pre-commit hook may lint or test.
    run(cwd, 300, "git", &["commit", "-q", "-m", message])?;
    Ok(format!("Committed {}", head(cwd).unwrap_or_default()))
}

/// The short id of the commit checked out in cwd.
pub fn head(cwd: &str) -> Option<String> {
    run(cwd, 5, "git", &["rev-parse", "--short", "HEAD"]).ok().map(|s| s.trim().to_string())
}

/// Run git in the repo for a change the user asked for; the error is git's own message.
fn git_change(root: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new("timeout")
        .args(["30", "git", "-C", root])
        .args(args)
        // Paths come from git status: a name with * or [ is a file, not a pattern.
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("git failed").trim().to_string())
}

/// Status entries of the files as (XY code, path, path before a rename).
fn entries(root: &str, files: &[String]) -> Vec<(String, String, Option<String>)> {
    let out = git(root, &["status", "--porcelain=v2"]).unwrap_or_default();
    out.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (code, path) = changed_file(l);
            let orig = (l.as_bytes()[0] == b'2').then(|| l.rsplit('\t').next().map(String::from)).flatten();
            (code, path, orig)
        })
        .filter(|(_, p, _)| files.contains(p))
        .collect()
}

fn with_paths<'a>(args: &[&'a str], paths: &'a [String]) -> Vec<&'a str> {
    let mut v = args.to_vec();
    v.push("--");
    v.extend(paths.iter().map(String::as_str));
    v
}

pub fn stage(root: &str, files: &[String]) -> Result<(), String> {
    git_change(root, &with_paths(&["add", "-A"], files))
}

pub fn unstage(root: &str, files: &[String]) -> Result<(), String> {
    let mut paths = files.to_vec();
    // A staged rename leaves its old path deleted in the index unless that is restored too.
    paths.extend(entries(root, files).into_iter().filter_map(|(_, _, orig)| orig));
    if git(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_none() {
        return git_change(root, &with_paths(&["rm", "--cached", "-r", "-q"], &paths));
    }
    git_change(root, &with_paths(&["restore", "--staged"], &paths))
}

/// Throw away the changes that are not staged: tracked files go back to the index, untracked files are deleted.
pub fn discard(root: &str, files: &[String]) -> Result<(), String> {
    let found = entries(root, files);
    let pick = |untracked: bool| -> Vec<String> {
        found.iter().filter(|(c, _, _)| (c == "??") == untracked && (untracked || !c.ends_with('.'))).map(|(_, p, _)| p.clone()).collect()
    };
    let (tracked, untracked) = (pick(false), pick(true));
    if !tracked.is_empty() {
        git_change(root, &with_paths(&["restore", "--worktree"], &tracked))?;
    }
    if !untracked.is_empty() {
        git_change(root, &with_paths(&["clean", "-f", "-d", "-q"], &untracked))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let out = "# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -3\n\
                   1 .M N... 100644 100644 100644 a b src/x.rs\n\
                   2 R. N... 100644 100644 100644 a b R100 new name.rs\told.rs\n\
                   ? untracked file\n";
        let st = parse_status(out);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.commit.as_deref(), Some("abc"));
        assert_eq!((st.ahead, st.behind, st.dirty), (2, 3, 3));
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!(st.files[0], (".M".into(), "src/x.rs".into()));
        assert_eq!(st.files[1], ("R.".into(), "new name.rs".into()));
        assert_eq!(st.files[2], ("??".into(), "untracked file".into()));
    }

    #[test]
    fn push_remote_is_the_upstreams_or_the_only_one() {
        let st = |up: Option<&str>, remotes: &[&str]| GitStatus { upstream: up.map(String::from), remotes: remotes.iter().map(|r| r.to_string()).collect(), ..GitStatus::default() };
        assert_eq!(st(Some("fork/main"), &["origin", "fork"]).push_remote(), Some("fork"));
        assert_eq!(st(None, &["origin"]).push_remote(), Some("origin"));
        assert_eq!(st(None, &["origin", "fork"]).push_remote(), None, "two remotes and no upstream: ask");
        assert_eq!(st(None, &[]).push_remote(), None);
    }

    #[test]
    fn worktree_is_its_own_root() {
        let dir = std::env::temp_dir().join(format!("jugglr-git-{}", std::process::id()));
        let (main, wt) = (dir.join("main"), dir.join("wt"));
        std::fs::create_dir_all(&main).unwrap();
        let sh = |cwd: &Path, args: &[&str]| assert!(Command::new("git").current_dir(cwd).args(args).output().unwrap().status.success());
        sh(&main, &["init", "-q"]);
        sh(&main, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]);
        sh(&main, &["worktree", "add", "-q", "-b", "feature", wt.to_str().unwrap()]);
        let cache = Arc::new(GitCache::default());
        let root = cache.repo_root(wt.to_str().unwrap()).unwrap();
        assert_eq!(std::fs::canonicalize(&root).unwrap(), std::fs::canonicalize(&wt).unwrap());
        assert_eq!(cache.status(&root).branch.as_deref(), Some("feature"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stages_unstages_and_discards() {
        let dir = std::env::temp_dir().join(format!("jugglr-stage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.to_str().unwrap();
        let run = |args: &[&str]| git_change(p, args).unwrap();
        let files = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let codes = || entries(p, &files(&["a", "b", "c", "d", "new/"])).into_iter().map(|(c, f, _)| format!("{c} {f}")).collect::<Vec<_>>();
        run(&["init", "-q"]);
        run(&["config", "user.name", "t"]);
        run(&["config", "user.email", "t@t"]);
        for f in ["a", "b", "c"] {
            std::fs::write(dir.join(f), f).unwrap();
        }
        run(&["add", "-A"]);
        run(&["commit", "-qm", "init"]);

        std::fs::write(dir.join("a"), "a2").unwrap();
        std::fs::write(dir.join("b"), "b2").unwrap();
        std::fs::create_dir_all(dir.join("new")).unwrap();
        std::fs::write(dir.join("new/x"), "x").unwrap();
        run(&["mv", "c", "d"]);
        stage(p, &files(&["a"])).unwrap();
        assert_eq!(codes(), ["M. a", ".M b", "R. d", "?? new/"]);

        std::fs::write(dir.join("a"), "a3").unwrap();
        discard(p, &files(&["a", "b", "new/"])).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a")).unwrap(), "a2");
        assert_eq!(std::fs::read_to_string(dir.join("b")).unwrap(), "b");
        assert!(!dir.join("new").exists());

        unstage(p, &files(&["a", "d"])).unwrap();
        assert_eq!(codes(), [".M a", ".D c", "?? d"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
