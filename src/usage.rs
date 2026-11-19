//! Plan usage (the 5-hour and weekly limits), passed on by the Claude Code status line.

use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::state_dir;
use crate::state::now;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// Percent of the limit used, 0 to 100.
    pub used: f64,
    /// Unix seconds.
    pub resets_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
}

fn path() -> std::path::PathBuf {
    state_dir().join("usage.json")
}

fn window(v: &Value) -> Option<Window> {
    Some(Window { used: v["used_percentage"].as_f64()?, resets_at: v["resets_at"].as_u64()? })
}

/// Only claude.ai subscribers get `rate_limits`, and only after the session's first response.
fn parse(input: &Value) -> Option<Usage> {
    let r = &input["rate_limits"];
    let u = Usage { five_hour: window(&r["five_hour"]), seven_day: window(&r["seven_day"]) };
    (u.five_hour.is_some() || u.seven_day.is_some()).then_some(u)
}

/// Windows that have reset no longer say anything.
pub fn load() -> Option<Usage> {
    let mut u: Usage = serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()?;
    let t = now();
    u.five_hour = u.five_hour.filter(|w| w.resets_at > t);
    u.seven_day = u.seven_day.filter(|w| w.resets_at > t);
    (u.five_hour.is_some() || u.seven_day.is_some()).then_some(u)
}

pub fn short(u: &Usage) -> String {
    [("5h", u.five_hour), ("7d", u.seven_day)]
        .iter()
        .filter_map(|(label, w)| w.map(|w| format!("{label} {:.0}%", w.used)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Status line command: store the limits for the hub and show them in Claude Code too.
pub fn statusline() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let Some(u) = serde_json::from_str::<Value>(&input).ok().as_ref().and_then(parse) else { return };
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join(format!(".usage.{}", std::process::id()));
    if std::fs::write(&tmp, serde_json::to_string(&u).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, path());
    }
    println!("{}", short(&u));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limits() {
        let v: Value = serde_json::from_str(r#"{"rate_limits":{"five_hour":{"used_percentage":23.5,"resets_at":100},"seven_day":{"used_percentage":41.2}}}"#).unwrap();
        let u = parse(&v).unwrap();
        assert_eq!(u.five_hour, Some(Window { used: 23.5, resets_at: 100 }));
        assert_eq!(u.seven_day, None);
        assert_eq!(short(&u), "5h 24%");
        assert_eq!(parse(&serde_json::json!({"model": {}})), None);
    }
}
