use std::path::PathBuf;

use serde::Deserialize;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| home().join(fallback))
}

pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("claude-hub")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("claude-hub")
}

fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("claude-hub/config.toml")
}

pub fn expand(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", home().display()),
        _ => path.to_string(),
    }
}

/// `~/x` for paths under home.
pub fn shorten(path: &str) -> String {
    let home = home().display().to_string();
    match path.strip_prefix(&home) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub roots: Vec<String>,
    pub mouse: bool,
    pub refresh_seconds: f64,
    pub stale_minutes: f64,
    pub prewarm: bool,
    pub hub_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            roots: vec!["~/repos".into()],
            mouse: true,
            refresh_seconds: 2.0,
            stale_minutes: 60.0,
            prewarm: true,
            hub_key: "C-q".into(),
        }
    }
}

pub fn load() -> Config {
    let mut cfg: Config = match std::fs::read_to_string(config_path()) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("claude-hub: ignoring invalid {}: {e}", config_path().display());
            Config::default()
        }),
        Err(_) => Config::default(),
    };
    cfg.roots = cfg.roots.iter().map(|r| expand(r)).collect();
    cfg.refresh_seconds = cfg.refresh_seconds.max(0.2);
    cfg
}
