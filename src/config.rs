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

/// A section of the hub's list. The GitLab sections are named for their host, so GitHub can join later.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Section {
    Workspaces,
    GitlabReviews,
    GitlabMergeRequests,
    GitlabAssignedMergeRequests,
    GitlabIssues,
    OtherSessions,
    ClosedSessions,
}

impl Section {
    /// The name without its host, in capitals: `gitlab-issues` is ISSUES.
    pub const fn title(self) -> &'static str {
        match self {
            Section::Workspaces => "WORKSPACES",
            Section::GitlabReviews => "REVIEWS",
            Section::GitlabMergeRequests => "MERGE REQUESTS",
            Section::GitlabAssignedMergeRequests => "ASSIGNED MERGE REQUESTS",
            Section::GitlabIssues => "ISSUES",
            Section::OtherSessions => "OTHER SESSIONS",
            Section::ClosedSessions => "CLOSED SESSIONS",
        }
    }

    pub fn gitlab(self) -> bool {
        matches!(self, Section::GitlabReviews | Section::GitlabMergeRequests | Section::GitlabAssignedMergeRequests | Section::GitlabIssues)
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
    pub chat_dir: String,
    /// The sections to show, in this order.
    #[serde(skip)]
    pub sections: Vec<Section>,
    /// Problems in the config file, for the hub to show.
    #[serde(skip)]
    pub warnings: Vec<String>,
    /// Only read state; leave pools and pane logs to the hub UI. Set by the web server.
    #[serde(skip)]
    pub passive: bool,
}

const SECTIONS: [(&str, Section); 7] = [
    ("workspaces", Section::Workspaces),
    ("gitlab-reviews", Section::GitlabReviews),
    ("gitlab-merge-requests", Section::GitlabMergeRequests),
    ("gitlab-assigned-merge-requests", Section::GitlabAssignedMergeRequests),
    ("gitlab-issues", Section::GitlabIssues),
    ("other-sessions", Section::OtherSessions),
    ("closed-sessions", Section::ClosedSessions),
];

impl Config {
    pub fn gitlab(&self) -> bool {
        self.sections.iter().any(|s| s.gitlab())
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
            chat_dir: "~/chats".into(),
            sections: SECTIONS.iter().map(|(_, s)| *s).collect(),
            warnings: vec![],
            passive: false,
        }
    }
}

pub fn load() -> Config {
    let mut cfg = match std::fs::read_to_string(config_path()) {
        Ok(text) => parse(&text),
        Err(_) => Config::default(),
    };
    cfg.roots = cfg.roots.iter().map(|r| expand(r)).collect();
    cfg.chat_dir = expand(&cfg.chat_dir);
    cfg.refresh_seconds = cfg.refresh_seconds.max(0.2);
    if let Err(e) = crate::theme::select(&cfg.theme) {
        cfg.warnings.push(e);
    }
    cfg
}

/// The config in text. A bad `sections` entry is skipped on its own, so a typo there keeps the other keys.
fn parse(text: &str) -> Config {
    let file = config_path().display().to_string();
    let mut table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return Config { warnings: vec![format!("Ignoring invalid {file}: {e}")], ..Config::default() },
    };
    let sections = table.remove("sections");
    let mut cfg: Config = toml::Value::Table(table).try_into().unwrap_or_else(|e| Config { warnings: vec![format!("Ignoring invalid {file}: {e}")], ..Config::default() });
    if let Some(v) = sections {
        let Some(list) = v.as_array() else {
            cfg.warnings.push("`sections` must be a list of names".into());
            return cfg;
        };
        cfg.sections = vec![];
        for item in list {
            match SECTIONS.iter().find(|(n, _)| Some(*n) == item.as_str()) {
                Some((_, s)) => cfg.sections.push(*s),
                None => {
                    let names: Vec<&str> = SECTIONS.iter().map(|(n, _)| *n).collect();
                    cfg.warnings.push(format!("Unknown section {item} in {file}. Use one of: {}", names.join(", ")));
                }
            }
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sections_in_order() {
        let cfg = parse(r#"sections = ["gitlab-reviews", "workspaces"]"#);
        assert_eq!(cfg.sections, [Section::GitlabReviews, Section::Workspaces]);
        assert!(cfg.gitlab() && cfg.warnings.is_empty());
        assert!(!parse(r#"sections = ["workspaces"]"#).gitlab());
    }

    #[test]
    fn titles_follow_the_names() {
        for (name, section) in SECTIONS {
            let name = name.strip_prefix("gitlab-").unwrap_or(name);
            assert_eq!(section.title(), name.to_uppercase().replace('-', " "));
        }
    }

    #[test]
    fn skips_an_unknown_section_but_keeps_the_rest() {
        let cfg = parse("mouse = false\nsections = [\"reviews\", \"workspaces\"]");
        assert!(!cfg.mouse);
        assert_eq!(cfg.sections, [Section::Workspaces]);
        assert_eq!(cfg.warnings.len(), 1);
        assert!(cfg.warnings[0].contains("\"reviews\""));
    }
}
