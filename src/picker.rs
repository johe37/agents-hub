//! Pick a directory for a workspace: fuzzy search, or browse a typed path like a shell.

use std::path::{Path, PathBuf};

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::{expand, shorten};
use crate::fuzzy;
use crate::input::Input;
use crate::snapshot::Snapshot;
use crate::theme::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Tag {
    Open,
    Repo,
    Recent,
    Dir,
}

#[derive(Clone)]
pub struct Candidate {
    pub path: String,
    pub label: String,
    pub tag: Tag,
}

pub struct Choice {
    pub path: String,
    pub line: Line<'static>,
}

pub struct Picker {
    pub input: Input,
    pub candidates: Vec<Candidate>,
    pub options: Vec<Choice>,
    pub selected: usize,
    pub offset: usize,
}

fn is_path(text: &str) -> bool {
    text.starts_with(['/', '~', '.'])
}

/// Split typed text into the directory to list and the fragment typed after it.
fn split_path(text: &str) -> (PathBuf, String) {
    if matches!(text, "~" | "." | "..") || text.ends_with('/') {
        return (PathBuf::from(expand(text)), String::new());
    }
    let (parent, fragment) = text.rsplit_once('/').unwrap_or(("", text));
    let dir = if !parent.is_empty() { expand(parent) } else if text.starts_with('/') { "/".into() } else { ".".into() };
    (PathBuf::from(dir), fragment.to_string())
}

fn subdirs(parent: &Path, fragment: &str) -> Vec<String> {
    let show_hidden = fragment.starts_with('.');
    let mut names: Vec<String> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| show_hidden || !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn real(p: &Path) -> String {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).display().to_string()
}

/// (path, label, tag) for the picker: open workspaces, repos, recent dirs, other dirs.
pub fn candidates(snap: &Snapshot, roots: &[String]) -> Vec<Candidate> {
    let relative = |path: &str| {
        roots
            .iter()
            .find_map(|r| path.strip_prefix(&format!("{}/", r.trim_end_matches('/'))).map(String::from))
            .unwrap_or_else(|| shorten(path))
    };
    let mut found: Vec<Candidate> = vec![];
    let mut add = |path: &str, tag: Tag| {
        if !found.iter().any(|c| c.path == path) {
            found.push(Candidate { path: path.into(), label: relative(path), tag });
        }
    };
    for ws in &snap.workspaces {
        add(&ws.path, Tag::Open);
    }
    for (p, _) in snap.dirs.iter().filter(|(_, repo)| *repo) {
        add(p, Tag::Repo);
    }
    for d in &snap.recent_dirs {
        add(d, Tag::Recent);
    }
    for (p, _) in snap.dirs.iter() {
        add(p, Tag::Dir);
    }
    found
}

impl Picker {
    pub fn new(candidates: Vec<Candidate>, query: &str) -> Self {
        let mut p = Picker { input: Input::new(query), candidates, options: vec![], selected: 0, offset: 0 };
        p.update();
        p
    }

    pub fn selected_path(&self) -> Option<String> {
        self.options.get(self.selected).map(|o| o.path.clone())
    }

    pub fn update(&mut self) {
        let text = self.input.text.trim().to_string();
        self.options = if is_path(&text) { self.path_options(&text) } else { self.search_options(&text) };
        self.selected = 0;
        self.offset = 0;
    }

    fn tag_span(&self, path: &str, tag: Option<Tag>) -> Option<Span<'static>> {
        let open = tag == Some(Tag::Open) || self.candidates.iter().any(|c| c.tag == Tag::Open && c.path == path);
        if open {
            return Some(Span::styled("  ● open", Style::new().fg(C_SUCCESS)));
        }
        match tag {
            Some(Tag::Repo) => Some(Span::styled("  repo", Style::new().fg(C_PURPLE))),
            Some(Tag::Recent) => Some(Span::styled("  recent", Style::new().fg(C_DIM))),
            Some(Tag::Dir) => Some(Span::styled("  dir", Style::new().fg(C_DIM))),
            _ if Path::new(path).join(".git").exists() => Some(Span::styled("  repo", Style::new().fg(C_PURPLE))),
            _ => None,
        }
    }

    fn path_options(&self, text: &str) -> Vec<Choice> {
        let (parent, fragment) = split_path(text);
        let mut items = vec![];
        let typed = PathBuf::from(expand(text));
        if typed.is_dir() {
            let r = real(&typed);
            items.push(Choice { line: Line::from(Span::styled(format!("Open {}", shorten(&r)), Style::new().fg(C_BRIGHT).bold())), path: r });
        }
        for name in fuzzy::rank(&fragment, &subdirs(&parent, &fragment), |n| n).into_iter().take(300) {
            let full = real(&parent.join(&name));
            if items.iter().any(|c| c.path == full) {
                continue;
            }
            let mut spans = vec![Span::styled(format!("{}/", shorten(&full)), Style::new().fg(C_FG))];
            spans.extend(self.tag_span(&full, None));
            items.push(Choice { path: full, line: Line::from(spans) });
        }
        items
    }

    fn search_options(&self, text: &str) -> Vec<Choice> {
        fuzzy::rank(text, &self.candidates, |c| &c.label)
            .into_iter()
            .take(300)
            .map(|c| {
                let label = if c.tag == Tag::Dir { format!("{}/", c.label) } else { c.label.clone() };
                let mut spans = vec![Span::styled(label, Style::new().fg(C_FG))];
                spans.extend(self.tag_span(&c.path, Some(c.tag)));
                Choice { path: c.path, line: Line::from(spans) }
            })
            .collect()
    }

    /// Inline completion for a typed path, like a shell: the rest of the first matching directory name.
    pub fn suggestion(&self) -> Option<String> {
        let value = &self.input.text;
        if !is_path(value) || matches!(value.as_str(), "~" | ".") || !self.input.at_end() {
            return None;
        }
        let (parent, fragment) = split_path(value);
        if fragment.is_empty() {
            return None;
        }
        subdirs(&parent, &fragment)
            .into_iter()
            .find(|n| n.starts_with(&fragment) && *n != fragment)
            .map(|n| format!("{}/", &n[fragment.len()..]))
    }

    pub fn accept_suggestion(&mut self) -> bool {
        match self.suggestion() {
            Some(s) => {
                let text = format!("{}{s}", self.input.text);
                self.input.set(&text);
                self.update();
                true
            }
            None => false,
        }
    }

    /// Fill the input with the selected directory, so the list shows what is inside it.
    pub fn complete(&mut self) {
        if let Some(path) = self.selected_path() {
            self.input.set(&format!("{}/", shorten(&path).trim_end_matches('/')));
            self.update();
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if !self.options.is_empty() {
            self.selected = (self.selected as isize + delta).clamp(0, self.options.len() as isize - 1) as usize;
        }
    }

    pub fn hint() -> Line<'static> {
        let mut spans = vec![];
        for (key, what) in [("↵", "open with terminal"), ("^O", "open with Claude"), ("⇥", "complete"), ("→", "accept"), ("^R", "rescan"), ("esc", "close")] {
            spans.push(Span::styled(format!(" {key} "), Style::new().fg(C_FG).bg(C_INPUT)));
            spans.push(Span::styled(format!(" {what}  "), Style::new().fg(C_MUTED)));
        }
        Line::from(spans)
    }
}

pub fn highlight(line: &Line<'static>) -> Line<'static> {
    let mut l = line.clone();
    for s in &mut l.spans {
        s.style = s.style.add_modifier(Modifier::BOLD);
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_typed_paths() {
        assert_eq!(split_path("/usr/lo"), (PathBuf::from("/usr"), "lo".into()));
        assert_eq!(split_path("/us"), (PathBuf::from("/"), "us".into()));
        assert_eq!(split_path("/usr/"), (PathBuf::from("/usr/"), "".into()));
        assert_eq!(split_path("./x"), (PathBuf::from("."), "x".into()));
    }
}
