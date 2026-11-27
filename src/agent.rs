//! The coding agent jugglr starts: Claude Code, or another CLI set as `agent` in config.toml.
//! Only Claude Code reports through hooks; other agents get their status from their pane's output.

use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

pub const DEFAULT: &str = "claude";
/// Offered in the settings; any command works.
pub const KNOWN: [&str; 5] = ["claude", "codex", "gemini", "aider", "opencode"];

static CACHE: Mutex<Option<(Option<SystemTime>, String)>> = Mutex::new(None);

/// The agent command, read again when config.toml changes, so the hub and the web server agree on it.
pub fn command() -> String {
    let path = crate::config::config_path();
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut cache = CACHE.lock().unwrap();
    if let Some((at, cmd)) = cache.as_ref()
        && *at == mtime
    {
        return cmd.clone();
    }
    let cmd = std::fs::read_to_string(&path).ok().as_deref().and_then(from_config).unwrap_or_else(|| DEFAULT.into());
    *cache = Some((mtime, cmd.clone()));
    cmd
}

fn from_config(text: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    let cmd = table.get("agent")?.as_str()?.trim();
    (!cmd.is_empty()).then(|| cmd.to_string())
}

/// The program a command runs: `codex` for `OPENAI_LOG=1 /usr/bin/codex --full-auto`.
pub fn program(command: &str) -> Option<String> {
    let word = command.split_whitespace().find(|w| !w.contains('='))?;
    Some(Path::new(word).file_name()?.to_string_lossy().into_owned())
}

/// The agent's window name, e.g. `codex`.
pub fn name() -> String {
    program(&command()).unwrap_or_else(|| DEFAULT.into())
}

/// Claude Code, with its hooks, transcripts and pre-started pool.
pub fn is_claude() -> bool {
    name() == "claude"
}

/// Codex rather than codex, for buttons and messages.
pub fn label() -> String {
    let n = name();
    let mut c = n.chars();
    c.next().map_or(n.clone(), |f| f.to_uppercase().chain(c).collect())
}

fn installed(command: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(word) = command.split_whitespace().find(|w| !w.contains('=')) else { return true };
    let runnable = |p: &Path| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if word.contains('/') {
        return runnable(Path::new(&crate::config::expand(word)));
    }
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| runnable(&dir.join(word)))
}

/// Whether the agent is in PATH. Windows get the hub's PATH, as the hub runs on the tmux server.
pub fn agent_installed() -> bool {
    installed(&command())
}

/// A window for a missing program closes at once and shows no error, so check first.
pub fn require(command: Option<&str>) -> Result<(), String> {
    match command {
        Some(c) if !installed(c) => {
            let p = program(c).unwrap_or_default();
            Err(format!("{p} is not installed: there is no {p} in PATH"))
        }
        _ => Ok(()),
    }
}

/// Save the agent command in config.toml, keeping the rest of the file as it is.
pub fn set(command: &str) -> Result<(), String> {
    let command = command.trim();
    if program(command).is_none() {
        return Err("Give a command to start the agent".into());
    }
    let path = crate::config::config_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let new = with_agent(&text, command);
    if from_config(&new).as_deref() != Some(command) {
        return Err(format!("Could not set the agent in {}", path.display()));
    }
    let dir = crate::config::config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".config.toml.{}", std::process::id()));
    std::fs::write(&tmp, new).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

fn with_agent(text: &str, command: &str) -> String {
    let line = format!("agent = {}", toml::Value::String(command.into()));
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    // Only top-level keys come before the first table header.
    let top = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let is_agent = |l: &String| l.trim_start().strip_prefix("agent").is_some_and(|r| r.trim_start().starts_with('='));
    match lines[..top].iter().position(is_agent) {
        Some(i) => lines[i] = line,
        None => lines.insert(0, line),
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_program() {
        assert_eq!(program("claude").as_deref(), Some("claude"));
        assert_eq!(program("FOO=1 /opt/bin/codex --full-auto").as_deref(), Some("codex"));
        assert_eq!(program("  "), None);
    }

    #[test]
    fn sets_the_agent_line_only() {
        let text = "roots = [\"~/repos\"]\n# agent = \"x\"\n\n[gitlab]\nagent = \"not this\"\n";
        let new = with_agent(text, "codex");
        assert_eq!(from_config(&new).as_deref(), Some("codex"));
        assert!(new.contains("# agent = \"x\"") && new.contains("agent = \"not this\""));
        let again = with_agent(&new, "aider --model \"x\"");
        assert_eq!(from_config(&again).as_deref(), Some("aider --model \"x\""));
        assert_eq!(again.matches("agent = ").count(), new.matches("agent = ").count());
    }

    #[test]
    fn checks_the_program_of_a_command() {
        assert!(require(Some("sh -c true")).is_ok());
        assert!(require(None).is_ok());
        assert_eq!(require(Some("no-such-agent-cli --x")).unwrap_err(), "no-such-agent-cli is not installed: there is no no-such-agent-cli in PATH");
    }
}
