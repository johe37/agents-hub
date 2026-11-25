mod app;
mod config;
mod fuzzy;
mod git;
mod gitlab;
mod hooks;
mod input;
mod picker;
mod screens;
mod snapshot;
mod state;
mod theme;
mod tmux;
mod transcript;
mod ui;
mod usage;
mod web;

use std::io::Write;
use std::process::ExitCode;

use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

const USAGE: &str = "usage: claude-hub [<dir> | <query>] | ui [--no-mouse] | web [--port N] [--no-open] | restart | stop [-y] | report | statusline | install-hooks [-y]

Open the claude-hub workspace manager. Give a directory to open a workspace there,
or a search query to open the picker with it.

  ui              run the hub UI in the current terminal, without the tmux engine
  web             run the hub as a web app on 127.0.0.1 (default port 7420) and open it
  restart         restart a frozen hub UI; workspaces and agents keep running
  stop            stop the tmux engine and close all workspaces and agents
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

fn restart() -> ExitCode {
    if !tmux::server_running() {
        eprintln!("claude-hub: the hub is not running");
        return ExitCode::FAILURE;
    }
    // Without a hub session, open_hub starts a new one.
    if tmux::list_sessions().iter().any(|s| s == tmux::HUB_SESSION) {
        if let Err(e) = tmux::restart_hub(&exe()) {
            eprintln!("claude-hub: {e}");
            return ExitCode::FAILURE;
        }
    }
    open_hub(&config::load(), None)
}

fn stop(yes: bool) -> ExitCode {
    if !tmux::server_running() {
        println!("The hub is not running.");
        return ExitCode::SUCCESS;
    }
    if !yes {
        let workspaces = tmux::list_sessions().iter().filter(|s| !s.starts_with('_')).count();
        print!("Stop the hub and close {workspaces} workspace(s) with all their agents and terminals? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if answer.trim().to_lowercase() != "y" {
            println!("Aborted.");
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = tmux::kill_server() {
        eprintln!("claude-hub: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_ui(cfg: config::Config) -> ExitCode {
    if tmux::inside_hub_server() {
        tmux::hide_hub_status();
    }
    let screens = std::sync::Arc::default();
    let (cmd, snaps) = snapshot::spawn(cfg.clone(), std::sync::Arc::clone(&screens));
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

const WEB_PORT: u16 = 7420;

fn run_web(args: &[String]) -> ExitCode {
    let port = match args.iter().position(|a| a == "--port").map(|i| args.get(i + 1).and_then(|p| p.parse::<u16>().ok())) {
        Some(Some(p)) => p,
        Some(None) => {
            eprintln!("claude-hub: --port needs a number");
            return ExitCode::FAILURE;
        }
        None => WEB_PORT,
    };
    // SAFETY: no other thread runs yet. Without these, tmux calls would act on the terminal the server started in.
    unsafe {
        std::env::remove_var("TMUX");
        std::env::remove_var("TMUX_PANE");
    }
    let cfg = config::load();
    // The hub UI keeps the pools of ready shells and Claudes; the web app only uses them.
    // A running hub keeps its binary and config, so the web app never restarts it.
    if !tmux::server_running() {
        let exe = exe();
        let conf = tmux::write_config(&exe, &cfg.hub_key);
        tmux::ensure_hub(&exe, &conf);
    }
    web::run(cfg, port, !args.iter().any(|a| a == "--no-open"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    match first {
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
        Some("restart") => restart(),
        Some("web") => run_web(&args[1..]),
        Some("stop") => stop(args.iter().any(|a| a == "-y" || a == "--yes")),
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
