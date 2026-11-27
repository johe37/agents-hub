//! A log of the commands that run: Claude's tool calls (from the hook) and new foreground
//! commands in terminals (from the web app). One JSON line per command, one file per day.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use serde_json::{Value, json};

const KEEP_DAYS: u64 = 7;

fn file(day: u64) -> PathBuf {
    crate::history::dir().join(format!("commands-{day}.jsonl"))
}

/// Runs in the hook, so it must stay cheap and silent. An append of one short line is atomic.
pub fn append(time: u64, mut entry: Value) {
    if let Some(text) = entry["text"].as_str() {
        entry["text"] = redact(text).into();
    }
    entry["time"] = time.into();
    let path = file(time / 86400);
    let _ = std::fs::create_dir_all(crate::history::dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path) {
        let _ = f.write_all(format!("{entry}\n").as_bytes());
    }
}

pub fn claude(time: u64, session: &str, cwd: &str, text: &str) {
    append(time, json!({"source": "claude", "session": session, "cwd": cwd, "text": text}));
}

pub fn shell(time: u64, workspace: &str, window: &str, window_name: &str, text: &str) {
    append(time, json!({"source": "shell", "workspace": workspace, "window": window, "window_name": window_name, "text": text}));
}

/// The newest `limit` entries, newest first. Also removes files older than a week.
pub fn recent(limit: usize) -> Vec<Value> {
    let today = crate::state::now() / 86400;
    if let Ok(rd) = std::fs::read_dir(crate::history::dir()) {
        for e in rd.flatten() {
            let name = e.file_name();
            let day = name.to_str().and_then(|n| n.strip_prefix("commands-")?.strip_suffix(".jsonl")?.parse::<u64>().ok());
            if day.is_some_and(|d| d + KEEP_DAYS < today) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut out = vec![];
    for day in (today.saturating_sub(KEEP_DAYS)..=today).rev() {
        let Ok(text) = std::fs::read_to_string(file(day)) else { continue };
        for line in text.lines().rev() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                out.push(v);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out
}

const SECRET_KEYS: [&str; 8] = ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "API_KEY", "APIKEY", "CREDENTIAL", "PRIVATE_KEY"];
const SECRET_FLAGS: [&str; 6] = ["--password", "--token", "--secret", "--api-key", "--private-token", "--client-secret"];
const SECRET_PREFIXES: [&str; 9] = ["glpat-", "ghp_", "gho_", "ghs_", "github_pat_", "sk-", "xoxb-", "xoxp-", "AKIA"];

/// Hides values that look like secrets. A best effort: it knows common shapes only.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut hide_next = false;
    for (i, part) in text.split_inclusive(char::is_whitespace).enumerate() {
        let word = part.trim_end();
        let gap = &part[word.len()..];
        if i > 0 && hide_next && !word.is_empty() {
            out.push_str(&word.replacen(word.trim_matches(|c| c == '"' || c == '\''), "***", 1));
            out.push_str(gap);
            hide_next = false;
            continue;
        }
        let bare = word.trim_matches(|c| c == '"' || c == '\'');
        let lower = bare.to_ascii_lowercase();
        hide_next = lower == "bearer" || lower == "basic" || SECRET_FLAGS.contains(&lower.as_str());
        out.push_str(&redact_word(word));
        out.push_str(gap);
    }
    out
}

fn redact_word(word: &str) -> String {
    let bare = word.trim_matches(|c| c == '"' || c == '\'');
    let assignment = bare.split_once('=').map(|(k, v)| (k, v.trim_matches(|c| c == '"' || c == '\'')));
    if let Some((key, value)) = assignment.filter(|(_, v)| !v.is_empty()) {
        let k = key.trim_start_matches('-').to_ascii_uppercase().replace('-', "_");
        if SECRET_KEYS.iter().any(|s| k.contains(s)) {
            return word.replacen(value, "***", 1);
        }
    }
    if SECRET_PREFIXES.iter().any(|p| bare.starts_with(p)) && bare.len() > 12 {
        return word.replacen(bare, "***", 1);
    }
    // user:password@ in a URL.
    if let Some(at) = bare.find("://").map(|s| s + 3) {
        let rest = &bare[at..];
        let host_end = rest.find('/').unwrap_or(rest.len());
        if let Some(cred) = rest[..host_end].rfind('@').map(|a| &rest[..a]).filter(|c| c.contains(':')) {
            return word.replacen(cred, "***", 1);
        }
    }
    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn hides_secret_assignments() {
        assert_eq!(redact("GITLAB_TOKEN=abc123 glab mr list"), "GITLAB_TOKEN=*** glab mr list");
        assert_eq!(redact("export DB_PASSWORD='hunter2'"), "export DB_PASSWORD='***'");
        assert_eq!(redact("--api-key=xyz run"), "--api-key=*** run");
    }

    #[test]
    fn hides_the_word_after_bearer_and_secret_flags() {
        assert_eq!(redact(r#"curl -H "Authorization: Bearer abc.def" x"#), r#"curl -H "Authorization: Bearer ***" x"#);
        assert_eq!(redact("mysql --password hunter2 db"), "mysql --password *** db");
    }

    #[test]
    fn hides_known_token_shapes_and_url_credentials() {
        assert_eq!(redact("echo glpat-0123456789abcdef"), "echo ***");
        assert_eq!(redact("git clone https://me:pw@host/repo.git"), "git clone https://***@host/repo.git");
    }

    #[test]
    fn keeps_ordinary_commands() {
        for c in ["cargo test --release", "git push origin HEAD", "Edit: ~/code/src/token.rs", "ls a=b"] {
            assert_eq!(redact(c), c);
        }
    }
}
