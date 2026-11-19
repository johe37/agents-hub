//! Screen previews of terminals. tmux pipes each pane's output to a file; vt100 replays it into a screen.
//! capture-pane is not used: it crashes the tmux build on some machines (EL10's git snapshot).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroU16;
use std::path::PathBuf;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::state_dir;

const MAX_LOG: u64 = 512 * 1024;

pub fn screens_dir() -> PathBuf {
    state_dir().join("screens")
}

pub fn log_path(pane_id: &str) -> PathBuf {
    screens_dir().join(format!("{}.log", pane_id.trim_start_matches('%')))
}

fn color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

fn styled_lines(screen: &vt100::Screen) -> Vec<Line<'static>> {
    let (rows, cols) = screen.size();
    let (rows, cols) = (rows.get(), cols.get());
    let mut lines = vec![];
    for row in 0..rows {
        let mut spans: Vec<Span<'static>> = vec![];
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else { continue };
            if cell.is_wide_continuation() {
                continue;
            }
            let mut style = Style::default();
            if let Some(fg) = color(cell.fgcolor()) {
                style = style.fg(fg);
            }
            if let Some(bg) = color(cell.bgcolor()) {
                style = style.bg(bg);
            }
            for (on, m) in [(cell.bold(), Modifier::BOLD), (cell.italic(), Modifier::ITALIC), (cell.underline(), Modifier::UNDERLINED), (cell.inverse(), Modifier::REVERSED), (cell.dim(), Modifier::DIM)] {
                if on {
                    style = style.add_modifier(m);
                }
            }
            let text = if cell.has_contents() { cell.contents() } else { " " };
            match spans.last_mut() {
                Some(last) if last.style == style => last.content.to_mut().push_str(text),
                _ => spans.push(Span::styled(text.to_string(), style)),
            }
        }
        // Trailing unstyled blanks only cost width.
        if let Some(last) = spans.last_mut().filter(|s| s.style == Style::default()) {
            let trimmed = last.content.trim_end().to_string();
            last.content = trimmed.into();
        }
        lines.push(Line::from(spans));
    }
    while lines.last().is_some_and(|l| l.width() == 0) {
        lines.pop();
    }
    lines
}

#[derive(Default)]
pub struct ScreenCache {
    screens: HashMap<String, (vt100::Parser, u64)>,
}

impl ScreenCache {
    /// The pane's screen, or None when nothing is known yet.
    pub fn lines(&mut self, pane_id: &str, width: u16, height: u16) -> Option<Vec<Line<'static>>> {
        let path = log_path(pane_id);
        let size = std::fs::metadata(&path).ok()?.len();
        let (h, w) = (NonZeroU16::new(height).unwrap_or(NonZeroU16::MIN), NonZeroU16::new(width).unwrap_or(NonZeroU16::MIN));
        let (parser, offset) = self.screens.entry(pane_id.to_string()).or_insert_with(|| (vt100::Parser::new(h, w, 0), 0));
        if parser.screen().size() != (h, w) {
            parser.screen_mut().set_size(h, w);
        }
        if size < *offset {
            *offset = 0;
        }
        if size > *offset {
            let mut data = vec![];
            if let Ok(mut f) = std::fs::File::open(&path) {
                let _ = f.seek(SeekFrom::Start(*offset)).and_then(|_| f.take(size - *offset).read_to_end(&mut data));
            }
            parser.process(&data);
            *offset += data.len() as u64;
        }
        if size > MAX_LOG {
            // The screen state lives in memory; the file only needs what the next hub start replays.
            if std::fs::File::options().write(true).open(&path).and_then(|f| f.set_len(0)).is_ok() {
                *offset = 0;
            }
        }
        Some(styled_lines(parser.screen()))
    }

    /// Where the pane's cursor is, as (row, col), unless the program hides it.
    pub fn cursor(&self, pane_id: &str) -> Option<(u16, u16)> {
        let screen = self.screens.get(pane_id)?.0.screen();
        (!screen.hide_cursor()).then(|| screen.cursor_position())
    }

    /// Drop state and files of panes that no longer exist.
    pub fn forget(&mut self, live: &HashSet<String>) {
        self.screens.retain(|id, _| live.contains(id));
        for entry in std::fs::read_dir(screens_dir()).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(".log") {
                if !live.contains(&format!("%{stem}")) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}
