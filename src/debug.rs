//! `claude-hub debug-sessions`: every Claude session the hub finds, and whether the hub can type into it.
//! It runs one real collection round, so it shows what the hub and the town decide, not a separate guess.

use crate::config::Config;
use crate::hooks::events_dir;
use crate::state::{age, comm, parent, tty_of};

/// Names of the parents of pid, nearest first, up to the first tmux server.
fn ancestry(pid: u32) -> Vec<String> {
    let mut out = vec![];
    let mut pid = pid;
    while let Some(p) = parent(pid).filter(|_| out.len() < 12) {
        let name = comm(p);
        let server = name.starts_with("tmux");
        out.push(name);
        if server {
            break;
        }
        pid = p;
    }
    out
}

pub fn run(cfg: Config) {
    let hooks = events_dir().is_dir();
    println!("hub tmux server: {}", crate::tmux::socket_path());
    println!("hook events:     {} ({})", events_dir().display(), if hooks { "present" } else { "missing: no hook has reported yet, run `claude-hub install-hooks`" });
    let snap = crate::snapshot::collect_once(cfg);
    let alive: Vec<_> = snap.agents.iter().filter(|a| a.status.alive()).collect();
    println!("claude sessions: {} running\n", alive.len());
    for a in alive {
        println!("{}", a.session_id().unwrap_or(&a.key));
        println!("  pid         {}", a.pid.map_or("- (unknown)".into(), |p| p.to_string()));
        println!("  cwd         {}", a.path);
        println!("  tmux socket {}", a.tmux.as_ref().map_or("-", |t| t.0.as_str()));
        println!("  pane        {}", a.tmux.as_ref().map_or("-", |t| t.1.as_str()));
        match &a.session {
            Some(s) => println!("  hook        yes: {:?}, last event {} ago{}", s.status, age(s.timestamp), if s.pid.is_some() { "" } else { " (no pid recorded)" }),
            None => println!("  hook        no: status from Claude's registry only (no waiting, permission or messages)"),
        }
        if let Some(p) = a.pid {
            println!("  parents     {}", ancestry(p).join(" <- "));
            println!("  claude tty  {}", tty_of(p).unwrap_or_else(|| "-".into()));
        }
        println!("  status      {:?}", a.status);
        println!("  workspace   {}", a.workspace.as_deref().unwrap_or("- (outside the hub)"));
        match (&a.target, &a.read_only) {
            (Some(input), _) => println!("  verdict     MATCHED: input goes to {input}"),
            (None, Some(why)) => println!("  verdict     READ-ONLY: {why}"),
            (None, None) => println!("  verdict     READ-ONLY: no pane known"),
        }
        println!();
    }
}
