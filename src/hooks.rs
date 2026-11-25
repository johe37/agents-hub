//! The Claude Code hook (`jugglr report`) and its installer.

use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::config::{home, state_dir};

const HOOK_EVENTS: [&str; 7] =
    ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop", "SessionEnd"];

pub fn events_dir() -> std::path::PathBuf {
    state_dir().join("events")
}

/// Stores the latest event per session; all parsing is left to the hub. Never fails or prints:
/// hook stdout can end up in Claude's context, and a failing PreToolUse hook blocks the tool call.
pub fn report() {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let Ok(event) = serde_json::from_str::<Value>(&input) else { return };
    let Some(id) = event["session_id"].as_str() else { return };
    if !valid_id(id) {
        return;
    }
    // Sent to every open session when another one finishes; it would hide this session's real state.
    if event["notification_type"] == "agent_completed" {
        return;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let tmux = std::env::var("TMUX").unwrap_or_default();
    let dir = events_dir();
    let mut record = json!({
        "time": now,
        "tmux": tmux.split(',').next().unwrap_or(""),
        "tmux_pane": std::env::var("TMUX_PANE").unwrap_or_default(),
        "event": event,
    });
    // A permission prompt names no tool; the PreToolUse event before it holds what Claude asks to run.
    if event["hook_event_name"] == "Notification" {
        let previous = std::fs::read(dir.join(format!("{id}.json"))).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        if let Some(prev) = previous.filter(|p| p["event"]["hook_event_name"] == "PreToolUse") {
            record["pending_tool"] = json!({"name": prev["event"]["tool_name"], "input": prev["event"]["tool_input"]});
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    // Write and rename: the rename bumps the directory mtime, which the hub watches.
    let tmp = dir.join(format!(".{id}.{}", std::process::id()));
    if std::fs::write(&tmp, record.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join(format!("{id}.json")));
    }
}

/// Session ids end up in file names and shell commands.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_ours(command: &str) -> bool {
    command.ends_with("jugglr report") || command.ends_with("claude-hub report") || command.ends_with("claude_hub/hook.sh")
}

pub fn install(yes: bool) -> i32 {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "jugglr".into());
    let command = format!("{exe} report");
    let path = home().join(".claude/settings.json");
    let mut settings: Value = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("jugglr: {} is not valid JSON: {e}", path.display());
                return 1;
            }
        },
        Err(_) => json!({}),
    };
    if !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }
    let mut changed = vec![];
    for event in HOOK_EVENTS {
        let groups = &mut settings["hooks"][event];
        if !groups.is_array() {
            *groups = json!([]);
        }
        let groups = groups.as_array_mut().unwrap();
        let hooks_of = |g: &Value| g["hooks"].as_array().cloned().unwrap_or_default();
        if groups.iter().any(|g| hooks_of(g).iter().any(|h| h["command"] == command.as_str())) {
            continue;
        }
        // Replace older hooks, from claude-hub or the Python hook.sh.
        for g in groups.iter_mut() {
            let kept: Vec<Value> =
                hooks_of(g).into_iter().filter(|h| !is_ours(h["command"].as_str().unwrap_or(""))).collect();
            g["hooks"] = Value::Array(kept);
        }
        groups.retain(|g| !hooks_of(g).is_empty());
        let mut group = json!({"hooks": [{"type": "command", "command": command}]});
        if event.ends_with("ToolUse") {
            group["matcher"] = json!("*");
        }
        groups.push(group);
        changed.push(event);
    }
    // The status line is the only place Claude Code passes on plan usage. Keep one the user set up.
    let status_command = format!("{exe} statusline");
    let current = settings["statusLine"]["command"].as_str().unwrap_or("").to_string();
    let set_status = current != status_command && (current.is_empty() || current.ends_with("jugglr statusline") || current.ends_with("claude-hub statusline"));
    if set_status {
        settings["statusLine"] = json!({"type": "command", "command": status_command});
    } else if current != status_command {
        println!("You have your own status line, so the hub shows no plan usage.");
        println!("To fix that, pipe its JSON input to '{status_command}' too.");
    }
    if changed.is_empty() && !set_status {
        println!("Hooks already installed.");
        return 0;
    }
    if !changed.is_empty() {
        println!("Will set '{command}' as the jugglr hook for these events in {}:", path.display());
        println!("  {}", changed.join(", "));
    }
    if set_status {
        println!("Will set '{status_command}' as the status line in {}, to show plan usage.", path.display());
    }
    if !yes {
        print!("Continue? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if answer.trim().to_lowercase() != "y" {
            println!("Aborted.");
            return 1;
        }
    }
    if path.exists() && std::fs::copy(&path, path.with_extension("json.bak")).is_err() {
        eprintln!("jugglr: could not back up {}", path.display());
        return 1;
    }
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let text = serde_json::to_string_pretty(&settings).unwrap() + "\n";
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!("jugglr: {e}");
        return 1;
    }
    println!("Done. Backup saved as settings.json.bak. New Claude sessions will report status.");
    0
}
