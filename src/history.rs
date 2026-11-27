//! Each session's status changes over time, one "<unix time> <status>" line per change.

use std::io::Write;
use std::path::PathBuf;

use crate::config::state_dir;
use crate::state::Status;

const KEEP_SECS: u64 = 7 * 24 * 3600;

pub fn dir() -> PathBuf {
    state_dir().join("history")
}

pub fn name(s: Status) -> &'static str {
    match s {
        Status::Waiting => "waiting",
        Status::Working => "working",
        Status::Polling => "polling",
        Status::Idle => "idle",
        Status::Unknown => "unknown",
        Status::Ended => "ended",
        Status::Stale => "stale",
    }
}

fn parse(s: &str) -> Option<Status> {
    Some(match s {
        "waiting" => Status::Waiting,
        "working" => Status::Working,
        "polling" => Status::Polling,
        "idle" => Status::Idle,
        "unknown" => Status::Unknown,
        "ended" => Status::Ended,
        "stale" => Status::Stale,
        _ => return None,
    })
}

/// Runs in the hook, so it must stay cheap and silent. An append of one short line is atomic.
pub fn append(id: &str, time: u64, status: Status) {
    let dir = dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{id}.log"))) {
        let _ = f.write_all(format!("{time} {}\n", name(status)).as_bytes());
    }
}

pub fn parse_log(text: &str) -> Vec<(u64, Status)> {
    text.lines()
        .filter_map(|l| {
            let (t, s) = l.split_once(' ')?;
            Some((t.parse().ok()?, parse(s.trim())?))
        })
        .collect()
}

/// Every session's changes, oldest first, for the sessions with a change since `from`.
/// Also removes logs that have not changed for a week.
pub fn load(from: u64) -> Vec<(String, Vec<(u64, Status)>)> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return vec![] };
    let now = crate::state::now();
    let mut out = vec![];
    for e in rd.flatten() {
        let path = e.path();
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).filter(|_| path.extension().is_some_and(|x| x == "log")) else { continue };
        let modified = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        if now.saturating_sub(modified) > KEEP_SECS {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let changes = std::fs::read_to_string(&path).map(|t| parse_log(&t)).unwrap_or_default();
        // A log untouched since before the window still counts while its last status lasts into it.
        if modified < from && changes.last().is_none_or(|(_, s)| !s.alive()) {
            continue;
        }
        out.push((id.to_string(), changes));
    }
    out
}

/// Spans of (start, end, status) inside [from, until]. The last change lasts until `until` while the
/// session lives, else it ends where it was logged.
pub fn spans(changes: &[(u64, Status)], from: u64, until: u64, alive: bool) -> Vec<(u64, u64, Status)> {
    let mut out: Vec<(u64, u64, Status)> = vec![];
    for (i, &(t, s)) in changes.iter().enumerate() {
        if !s.alive() {
            continue;
        }
        let end = match changes.get(i + 1) {
            Some(&(next, _)) => next,
            None if alive => until,
            None => t,
        };
        let (start, end) = (t.max(from), end.min(until));
        if end <= start {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.2 == s && last.1 == start => last.1 = end,
            _ => out.push((start, end, s)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_skips_junk() {
        let log = "10 working\n20 waiting\nbad\n30 nope\n40 idle\n";
        assert_eq!(parse_log(log), vec![(10, Status::Working), (20, Status::Waiting), (40, Status::Idle)]);
    }

    #[test]
    fn spans_are_clipped_to_the_window() {
        let c = [(10, Status::Working), (20, Status::Waiting), (30, Status::Idle)];
        assert_eq!(spans(&c, 15, 50, true), vec![(15, 20, Status::Working), (20, 30, Status::Waiting), (30, 50, Status::Idle)]);
        assert_eq!(spans(&c, 0, 25, true), vec![(10, 20, Status::Working), (20, 25, Status::Waiting)]);
    }

    #[test]
    fn a_dead_session_stops_at_its_last_change() {
        let c = [(10, Status::Working), (20, Status::Ended)];
        assert_eq!(spans(&c, 0, 100, false), vec![(10, 20, Status::Working)]);
        let c = [(10, Status::Working), (20, Status::Idle)];
        assert_eq!(spans(&c, 0, 100, false), vec![(10, 20, Status::Working)]);
    }

    #[test]
    fn repeated_statuses_merge() {
        let c = [(10, Status::Working), (20, Status::Working), (30, Status::Idle)];
        assert_eq!(spans(&c, 0, 40, true), vec![(10, 30, Status::Working), (30, 40, Status::Idle)]);
    }
}
