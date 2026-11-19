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
    pub prewarm_claude: bool,
    pub hub_key: String,
    pub theme: String,
    /// The GitLab that XP and quests come from, e.g. "https://git.example.com". None: glab's default host.
    pub gitlab_url: Option<String>,
    /// Whose issues count. None: the user glab is logged in as.
    pub gitlab_username: Option<String>,
    pub xp_per_weight: u64,
    /// The weight of an issue without one.
    pub default_weight: u64,
    pub sync_minutes: f64,
    /// Flat XP for every merged MR you were assigned to, and for every merged MR you reviewed.
    pub xp_per_merged_mr: u64,
    pub xp_per_review: u64,
    /// Which closed issues count: "all" (every one, also those closed before claude-hub), "install"
    /// (closed since the progress file was made), or a date "2026-01-01".
    pub xp_since: String,
    /// Sounds in the town (level up, loot).
    pub sound: bool,
    pub drop_chances: DropChances,
}

/// Relative weights of the loot rarities; they need not add up to 100.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DropChances {
    pub poor: f64,
    pub common: f64,
    pub uncommon: f64,
    pub rare: f64,
    pub epic: f64,
    pub legendary: f64,
}

impl Default for DropChances {
    fn default() -> Self {
        DropChances { poor: 20.0, common: 40.0, uncommon: 25.0, rare: 10.0, epic: 4.0, legendary: 1.0 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            roots: vec!["~/repos".into()],
            mouse: true,
            refresh_seconds: 2.0,
            stale_minutes: 60.0,
            prewarm: true,
            prewarm_claude: true,
            hub_key: "M-q".into(),
            theme: "dark-modern".into(),
            gitlab_url: None,
            gitlab_username: None,
            xp_per_weight: 10,
            default_weight: 1,
            sync_minutes: 15.0,
            xp_per_merged_mr: 25,
            xp_per_review: 10,
            xp_since: "all".into(),
            sound: false,
            drop_chances: DropChances::default(),
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
    cfg.sync_minutes = cfg.sync_minutes.max(1.0);
    crate::gitlab::set_host(cfg.gitlab_url.as_deref());
    if let Err(e) = crate::theme::select(&cfg.theme) {
        eprintln!("claude-hub: {e}");
    }
    cfg
}
