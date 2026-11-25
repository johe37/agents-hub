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

/// Caches for repo-root lookups and rate-limited git status, safe to share between threads.
#[derive(Default)]
pub struct GitCache {
    roots: Mutex<HashMap<String, Option<String>>>,
    status: Mutex<HashMap<String, (Instant, GitStatus)>>,
    /// Repos with a status refresh running in the background.
    busy: Mutex<HashSet<String>>,
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

    fn refresh(&self, repo: &str) -> GitStatus {
        let mut st = git(repo, &["status", "--porcelain=v2", "--branch"]).map(|o| parse_status(&o)).unwrap_or_default();
        for line in git(repo, &["diff", "--numstat", "HEAD"]).unwrap_or_default().lines() {
            let mut f = line.split('\t');
            if let (Some(Ok(a)), Some(Ok(r))) = (f.next().map(str::parse::<u32>), f.next().map(str::parse::<u32>)) {
                st.added += a;
                st.removed += r;
            }
        }
        self.status.lock().unwrap().insert(repo.to_string(), (Instant::now(), st.clone()));
        st
    }
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
        assert_eq!(st.files[0], (".M".into(), "src/x.rs".into()));
        assert_eq!(st.files[1], ("R.".into(), "new name.rs".into()));
        assert_eq!(st.files[2], ("??".into(), "untracked file".into()));
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
}
