mod app;
mod config;
mod debug;
mod fuzzy;
mod git;
mod gitlab;
mod hooks;
mod input;
mod messages;
mod nvim;
mod picker;
mod screens;
mod snapshot;
mod state;
mod theme;
mod tmux;
mod town;
mod transcript;
mod ui;
mod usage;

use std::process::ExitCode;

use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

const USAGE: &str = "usage: claude-hub [<dir> | <query>] | ui [--no-mouse] | town [--port N] [--no-open] [--demo] | send <agent> <message> | debug-sessions | report | statusline | install-hooks [-y]

Open the claude-hub workspace manager. Give a directory to open a workspace there,
or a search query to open the picker with it.

  ui              run the hub UI in the current terminal, without the tmux engine
  town            show your workspaces as a pixel art town in the browser
  send            send a message to an agent, the way the hub's reply does
  debug-sessions  show every Claude session and how it matches a tmux pane
  report          hook handler: read hook JSON from stdin
  statusline      status line command: store plan usage for the hub
  install-hooks   add claude-hub hooks and status line to ~/.claude/settings.json";

fn exe() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "claude-hub".into())
}

/// Start the engine if needed, then go to the hub, or straight to a workspace for a directory.
fn open_hub(cfg: &config::Config, target: Option<String>) -> ExitCode {
    let exe = exe();
    let conf = tmux::write_config(&exe, &cfg.hub_key);
    if tmux::server_running() {
        tmux::source_config(&conf, &cfg.hub_key);
    }
    tmux::ensure_hub(&exe, &conf);
    let mut session = tmux::HUB_SESSION.to_string();
    match target {
        Some(t) if std::path::Path::new(&t).is_dir() => match tmux::ensure_workspace(&tmux::canonical(&t)) {
            Ok(s) => session = s,
            Err(e) => {
                eprintln!("claude-hub: {e}");
                return ExitCode::FAILURE;
            }
        },
        Some(query) => {
            let _ = std::fs::write(tmux::request_file(), query);
        }
        None => {}
    }
    if tmux::inside_hub_server() {
        let _ = tmux::switch_client(&session);
        return ExitCode::SUCCESS;
    }
    eprintln!("claude-hub: {}", tmux::exec_attach(&conf, &session));
    ExitCode::FAILURE
}

fn run_ui(cfg: config::Config) -> ExitCode {
    if tmux::inside_hub_server() {
        tmux::hide_hub_status();
    }
    let screens = std::sync::Arc::default();
    let (cmd, snaps) = snapshot::spawn(cfg.clone(), std::sync::Arc::clone(&screens), true);
    let mouse = cfg.mouse;
    let mut app = app::App::new(cfg.clone(), cmd, screens);
    let mut term = ratatui::init();
    if mouse {
        let _ = execute!(std::io::stdout(), EnableMouseCapture);
    }
    let result = app.run(&mut term, snaps);
    if mouse {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    if let Err(e) = result {
        eprintln!("claude-hub: {e}");
        return ExitCode::FAILURE;
    }
    if let Some(session) = app.attach_to {
        let conf = tmux::write_config(&exe(), &cfg.hub_key);
        eprintln!("claude-hub: {}", tmux::exec_attach(&conf, &session));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    match first {
        Some("send") => messages::send(config::load(), &args[1..]),
        Some("debug-sessions") => {
            debug::run(config::load());
            ExitCode::SUCCESS
        }
        Some("report") => {
            hooks::report();
            ExitCode::SUCCESS
        }
        Some("statusline") => {
            usage::statusline();
            ExitCode::SUCCESS
        }
        Some("install-hooks") => ExitCode::from(hooks::install(args.iter().any(|a| a == "-y" || a == "--yes")) as u8),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("town") if args[1..].iter().any(|a| a == "-h" || a == "--help") => {
            println!("{}", town::USAGE);
            ExitCode::SUCCESS
        }
        Some("town") => town::run(config::load(), &args[1..]),
        Some("ui") => {
            let mut cfg = config::load();
            cfg.mouse &= !args.iter().any(|a| a == "--no-mouse");
            run_ui(cfg)
        }
        Some(a) if a.starts_with('-') && a != "--no-mouse" => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
        Some(a) if a != "--no-mouse" => open_hub(&config::load(), Some(args.join(" "))),
        _ => open_hub(&config::load(), None),
    }
}
