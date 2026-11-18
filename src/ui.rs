//! Drawing the hub.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::app::{Action, App, Focus, Modal, Row};
use crate::config::shorten;
use crate::git::GitStatus;
use crate::input::Input;
use crate::picker::{self, Picker};
use crate::snapshot::{Agent, Workspace};
use crate::state::{Status, age};
use crate::theme::*;
use crate::tmux::key_label;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_FILES: usize = 20;
const MAX_MESSAGE_LINES: usize = 40;
const C_HOVER: Color = hex("#2A2D2E");

fn s(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

fn fg(c: Color) -> Style {
    Style::new().fg(c)
}

/// Milliseconds since start, for animations.
fn tick(app: &App) -> u128 {
    app.started.elapsed().as_millis()
}

fn status_symbol(status: Status, t: u128) -> Span<'static> {
    match status {
        // Pulses, so an agent that needs you catches the eye.
        Status::Waiting => s("◉", fg(if (t / 600) % 2 == 0 { C_WARNING } else { hex("#7A6400") }).bold()),
        Status::Working => s(SPINNER[(t / 80) as usize % SPINNER.len()], fg(C_INFO)),
        Status::Idle => s("○", fg(C_SUCCESS)),
        Status::Unknown => s("·", fg(C_DIM)),
        Status::Ended => s("✗", fg(C_DIM)),
        Status::Stale => s("?", fg(C_DIM)),
    }
}

fn waiting_reason(kind: Option<&str>) -> Option<&'static str> {
    match kind? {
        "permission_prompt" => Some("Needs your permission"),
        "idle_prompt" => Some("Waiting for your input"),
        "elicitation_dialog" => Some("Asks you a question"),
        _ => None,
    }
}

fn git_spans(g: Option<&GitStatus>) -> Vec<Span<'static>> {
    let Some(g) = g else { return vec![] };
    let mut v = vec![s(format!("⎇ {}", g.branch.as_deref().unwrap_or("-")), fg(C_PURPLE))];
    for (n, prefix, color) in [
        (g.dirty as u32, " ●", C_MODIFIED),
        (g.added, " +", C_SUCCESS),
        (g.removed, " −", C_ERROR),
        (g.ahead, " ↑", C_INFO),
        (g.behind, " ↓", C_INFO),
    ] {
        if n > 0 {
            v.push(s(format!("{prefix}{n}"), fg(color)));
        }
    }
    v
}

/// Cut spans to width cells, ending in … when cut; pad with spaces when pad is set.
fn fit(spans: Vec<Span<'static>>, width: usize, pad: bool) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| s.width()).sum();
    if total <= width {
        let mut v = spans;
        if pad && total < width {
            v.push(Span::raw(" ".repeat(width - total)));
        }
        return v;
    }
    let mut out = vec![];
    let mut used = 0;
    let budget = width.saturating_sub(1);
    'outer: for sp in spans {
        let mut text = String::new();
        for c in sp.content.chars() {
            let w = c.width().unwrap_or(0);
            if used + w > budget {
                out.push(Span::styled(text, sp.style));
                out.push(Span::styled("…", sp.style));
                used += 1;
                break 'outer;
            }
            used += w;
            text.push(c);
        }
        out.push(Span::styled(text, sp.style));
    }
    if pad && used < width {
        out.push(Span::raw(" ".repeat(width - used)));
    }
    out
}

fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.width()).sum()
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(C_BG).fg(C_FG)), area);
    app.hits.buttons.clear();
    let [header, main, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(area);
    draw_header(f, app, header);
    if area.width >= 100 {
        let [list, side] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(main);
        draw_list(f, app, list);
        draw_side(f, app, side);
    } else {
        app.hits.detail = Rect::default();
        draw_list(f, app, main);
    }
    draw_footer(f, app, footer);
    draw_toast(f, app, main);
    draw_modal(f, app, area);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Block::new().style(Style::new().bg(C_BAR)), area);
    let left = Line::from(vec![
        s(" ◆ ", fg(C_ACCENT).bold()),
        s("Claude Hub", fg(C_BRIGHT).bold()),
        s(format!("   {} brings you back here from any terminal", key_label(&app.cfg.hub_key)), fg(C_DIM)),
    ]);
    let count = |st: Status| app.snap.agents.iter().filter(|a| a.status == st).count();
    let t = tick(app);
    let mut right = vec![];
    let waiting = count(Status::Waiting);
    if waiting > 0 {
        right.push(s(format!(" ◉ {waiting} need you "), Style::new().fg(C_BG).bg(C_WARNING).bold()));
        right.push(Span::raw("  "));
    }
    let working = count(Status::Working);
    if working > 0 {
        right.push(status_symbol(Status::Working, t));
        right.push(s(format!(" {working} working  "), fg(C_INFO)));
    }
    let idle = count(Status::Idle);
    if idle > 0 {
        right.push(s(format!("○ {idle} idle  "), fg(C_SUCCESS)));
    }
    let n = app.snap.workspaces.len();
    right.push(s(format!("{n} workspace{} ", if n == 1 { "" } else { "s" }), fg(C_MUTED)));
    let rw = width_of(&right) as u16;
    f.render_widget(Paragraph::new(left), Rect { width: area.width.saturating_sub(rw), ..area });
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), area);
}

fn panel(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(if focused { C_ACCENT } else { C_BORDER }))
        .title(title)
        .style(Style::new().bg(C_BG))
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.modal.is_none() && app.focus != Focus::Reply;
    let title = Line::from(vec![s(" Workspaces ", fg(if focused { C_BRIGHT } else { C_MUTED }).bold())]);
    let block = panel(title, focused);
    let mut inner = block.inner(area);
    f.render_widget(block, area);

    if app.focus == Focus::Filter || !app.filter.text.is_empty() {
        let [list, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
        inner = list;
        let active = app.focus == Focus::Filter;
        let line = Line::from(vec![s(" / ", fg(C_BG).bg(if active { C_ACCENT } else { C_DIM }).bold()), s(" ", Style::new()), s(app.filter.text.clone(), fg(C_BRIGHT))]);
        f.render_widget(Paragraph::new(line).style(Style::new().bg(C_PANEL)), bar);
        if active {
            set_cursor(f, &app.filter, bar.x + 4, bar.y);
        }
    }
    app.hits.list = inner;

    if !app.loaded {
        let t = tick(app);
        let line = Line::from(vec![s(SPINNER[(t / 80) as usize % 10], fg(C_ACCENT)), s(" Loading…", fg(C_MUTED))]);
        f.render_widget(Paragraph::new(line), inner);
        return;
    }
    if app.rows.is_empty() {
        let msg = if app.filter.text.is_empty() { "Nothing here yet. Press n to open a workspace." } else { "Nothing matches the filter." };
        f.render_widget(Paragraph::new(s(format!(" {msg}"), fg(C_MUTED))), inner);
        return;
    }

    let h = inner.height as usize;
    if app.hits.last_selected != Some(app.selected) {
        app.hits.last_selected = Some(app.selected);
        if app.selected < app.offset {
            app.offset = app.selected.saturating_sub(1);
        } else if app.selected >= app.offset + h {
            app.offset = app.selected + 1 - h;
        }
    }
    app.offset = app.offset.min(app.rows.len().saturating_sub(h));

    let width = inner.width as usize;
    let lines: Vec<Line> = (app.offset..(app.offset + h).min(app.rows.len())).map(|i| row_line(app, i, width)).collect();
    f.render_widget(Paragraph::new(lines), inner);

    if app.rows.len() > h {
        let thumb_h = (h * h / app.rows.len()).max(1);
        let thumb_y = app.offset * (h - thumb_h) / (app.rows.len() - h).max(1);
        for y in 0..thumb_h {
            let pos = Position::new(area.right() - 1, inner.y + (thumb_y + y) as u16);
            if let Some(cell) = f.buffer_mut().cell_mut(pos) {
                cell.set_symbol("┃").set_fg(C_DIM);
            }
        }
    }
}

fn row_line(app: &App, i: usize, width: usize) -> Line<'static> {
    let row = app.rows[i].1;
    let selected = i == app.selected;
    let t = tick(app);
    let is_child = |j: usize| matches!(app.rows.get(j).map(|r| r.1), Some(Row::Agent(_) | Row::Terminal(..)));
    let branch = if is_child(i + 1) { "├─ " } else { "╰─ " };
    let marker = if selected { s("▌", fg(C_ACCENT)) } else { Span::raw(" ") };
    let w = width.saturating_sub(1);

    let (left, right): (Vec<Span>, Vec<Span>) = match row {
        Row::Gap => (vec![], vec![]),
        Row::Header => {
            let n = app.rows.iter().filter(|(k, r)| matches!(r, Row::Agent(i) if app.snap.agents[*i].workspace.is_none()) && !k.is_empty()).count();
            (vec![s(format!("OUTSIDE CLAUDE HUB · {n}"), fg(C_MUTED).bold())], vec![])
        }
        Row::Workspace(wi) => {
            let ws = &app.snap.workspaces[wi];
            let agents = app.agents_of(ws);
            let mut l = vec![match app.worst(&agents) {
                Some(st) => status_symbol(st, t),
                None => s("▪", fg(C_DIM)),
            }];
            l.push(s(format!(" {}", ws.name), fg(C_BRIGHT).bold()));
            l.push(s(format!("  {}", shorten(&ws.path)), fg(C_DIM)));
            if ws.git.is_some() {
                l.push(Span::raw("  "));
                l.extend(git_spans(ws.git.as_ref()));
            }
            let terms = ws.windows.iter().filter(|w| !w.is_claude()).count();
            let mut r = vec![];
            if !agents.is_empty() {
                r.push(s(format!("✻{}", agents.len()), fg(C_DIM)));
            }
            if terms > 0 {
                r.push(s(format!(" ❯{terms}"), fg(C_DIM)));
            }
            (l, r)
        }
        Row::Agent(ai) => {
            let a = &app.snap.agents[ai];
            let mut l = vec![s(branch, fg(C_BORDER).add_modifier(Modifier::BOLD)), status_symbol(a.status, t), Span::raw(" ")];
            let title_style = if a.status == Status::Waiting { fg(C_BRIGHT).bold() } else { fg(C_FG) };
            l.push(s(a.title(), title_style));
            let mut detail = vec![];
            if let Some(sess) = &a.session {
                if a.status == Status::Waiting {
                    let reason = waiting_reason(sess.notification_type.as_deref()).map(String::from);
                    detail.push(s(reason.or(sess.message.clone()).unwrap_or("Waiting for you".into()), fg(C_MODIFIED)));
                } else if a.status == Status::Working {
                    if let Some(tool) = &sess.tool {
                        detail.push(s(tool.clone(), fg(C_DIM)));
                    }
                }
            }
            if a.workspace.is_none() {
                detail.push(s(format!("  {}", a.name()), fg(C_DIM).italic()));
            }
            let right = if a.session.is_some() { vec![s(age(a.timestamp()), fg(C_DIM))] } else { vec![] };
            if !detail.is_empty() {
                // The title keeps at least half the room; the detail gets the rest.
                let room = w.saturating_sub(width_of(&right) + 2);
                let lw = width_of(&l);
                if lw > room / 2 {
                    let head = l.split_off(3);
                    let budget = (room / 2).max(room.saturating_sub(width_of(&detail) + 2)).saturating_sub(width_of(&l));
                    l.extend(fit(head, budget, false));
                }
                l.push(Span::raw("  "));
                l.extend(detail);
            }
            (l, right)
        }
        Row::Terminal(wi, ti) => {
            let win = &app.snap.workspaces[wi].windows[ti];
            let mut l = vec![s(branch, fg(C_BORDER).bold()), s("❯ ", fg(C_MUTED)), s(format!("{} ", win.index), fg(C_DIM)), s(win.name.clone(), fg(C_FG))];
            if win.command != win.name {
                l.push(s(format!("  {}", win.command), fg(C_MUTED)));
            }
            (l, vec![])
        }
    };
    let rw = width_of(&right);
    let mut spans = vec![marker];
    spans.extend(fit(left, w.saturating_sub(if rw > 0 { rw + 1 } else { 0 }), true));
    if rw > 0 {
        spans.push(Span::raw(" "));
        spans.extend(right);
    }
    let mut line = Line::from(spans);
    if selected {
        line = line.style(Style::new().bg(C_SELECTION));
    } else if app.hover == Some(i) && !matches!(row, Row::Gap | Row::Header) {
        line = line.style(Style::new().bg(C_HOVER));
    }
    line
}

// ---- detail panel ----

fn heading(lines: &mut Vec<Line<'static>>, text: impl Into<String>) {
    lines.push(Line::default());
    lines.push(Line::from(s(text.into(), fg(C_MUTED).bold())));
}

/// Enough markdown for Claude's replies: headings, lists, code blocks, `code` and **bold**.
fn markdown(text: &str) -> Vec<Line<'static>> {
    let mut out = vec![];
    let mut in_code = false;
    for raw in text.lines() {
        if raw.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(Line::from(s(format!("  {raw}"), fg(C_CODE))));
            continue;
        }
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') {
            out.push(Line::from(s(trimmed.trim_start_matches('#').trim().to_string(), fg(C_BRIGHT).bold())));
            continue;
        }
        let indent = &raw[..raw.len() - trimmed.len()];
        let (bullet, body) = match trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            Some(rest) => (format!("{indent}• "), rest),
            None => (indent.to_string(), trimmed),
        };
        let mut spans = vec![s(bullet, fg(C_ACCENT))];
        for (i, part) in body.split('`').enumerate() {
            if i % 2 == 1 {
                spans.push(s(part.to_string(), fg(C_CODE)));
                continue;
            }
            for (j, piece) in part.split("**").enumerate() {
                let style = if j % 2 == 1 { fg(C_BRIGHT).bold() } else { fg(C_FG) };
                spans.push(s(piece.to_string(), style));
            }
        }
        out.push(Line::from(spans));
    }
    out
}

fn status_lines(a: &Agent) -> Vec<Line<'static>> {
    let sess = a.session.as_ref();
    let t = a.transcript.as_ref();
    let when = sess.map_or(String::new(), |s| format!(" · {}", age(s.timestamp)));
    let mut out = vec![];
    match a.status {
        Status::Waiting => {
            let sess = sess.unwrap();
            let reason = waiting_reason(sess.notification_type.as_deref()).unwrap_or("Waiting for you");
            out.push(Line::from(s(format!("◉ {reason}{when}"), fg(C_WARNING).bold())));
            if let Some(m) = &sess.message {
                out.push(Line::from(s(m.clone(), fg(C_MODIFIED))));
            }
            // The hook keeps only the latest event, so the tool behind a permission prompt comes from the transcript.
            if a.needs_permission() {
                if let Some(tool) = t.and_then(|t| t.last_tool.clone()) {
                    out.push(Line::from(vec![s("  ", fg(C_FG)), s(tool, fg(C_BRIGHT).bg(C_PANEL))]));
                }
            }
            if a.target.is_some() {
                let mut l = vec![];
                if a.needs_permission() {
                    l.extend([s(" y ", fg(C_BG).bg(C_SUCCESS).bold()), s(" approve  ", fg(C_MUTED)), s(" d ", fg(C_BRIGHT).bg(C_ERROR).bold()), s(" deny  ", fg(C_MUTED))]);
                }
                l.extend([s(" r ", fg(C_FG).bg(C_INPUT)), s(" reply", fg(C_MUTED))]);
                out.push(Line::default());
                out.push(Line::from(l));
            }
        }
        Status::Working => {
            out.push(Line::from(s(format!("● Working{when}"), fg(C_INFO).bold())));
            if let Some(tool) = sess.and_then(|s| s.tool.clone()).or_else(|| t.and_then(|t| t.last_tool.clone())) {
                out.push(Line::from(s(tool, fg(C_FG))));
            }
        }
        Status::Idle => {
            out.push(Line::from(s(format!("○ Idle{when}"), fg(C_SUCCESS).bold())));
            out.push(Line::from(s("Done with its turn. Open it to continue.", fg(C_MUTED))));
        }
        _ => {
            out.push(Line::from(s("· No status", fg(C_DIM))));
            out.push(Line::from(s("Started before claude-hub hooks were installed.", fg(C_MUTED))));
        }
    }
    out
}

fn file_lines(lines: &mut Vec<Line<'static>>, g: Option<&GitStatus>) {
    let Some(g) = g.filter(|g| !g.files.is_empty()) else { return };
    heading(lines, format!("Changed files ({})", g.files.len()));
    for (status, path) in g.files.iter().take(MAX_FILES) {
        let letter = status.chars().find(|&c| c != '.').unwrap_or('M');
        let color = match letter {
            '?' | 'A' => C_SUCCESS,
            'D' | 'U' => C_ERROR,
            'R' => C_INFO,
            _ => C_MODIFIED,
        };
        lines.push(Line::from(vec![s(format!("{letter}  "), fg(color)), s(path.clone(), fg(C_FG))]));
    }
    if g.files.len() > MAX_FILES {
        lines.push(Line::from(s(format!("… {} more", g.files.len() - MAX_FILES), fg(C_DIM))));
    }
}

fn meta_line(name: Option<&str>, path: &str, git: Option<&GitStatus>) -> Line<'static> {
    let mut v = vec![];
    if let Some(n) = name {
        v.push(s(format!("{n}  "), fg(C_FG).bold()));
    }
    v.push(s(format!("{}  ", shorten(path)), fg(C_DIM)));
    v.extend(git_spans(git));
    Line::from(v)
}

fn agent_detail(a: &Agent) -> Vec<Line<'static>> {
    let mut lines = vec![meta_line(Some(&a.name()), &a.path, a.git.as_ref())];
    if a.workspace.is_none() {
        let where_ = a.session.as_ref().and_then(|s| s.tmux_session.clone()).map_or("its own terminal".into(), |s| format!("tmux session {s}"));
        lines.push(Line::from(s(format!("Runs outside Claude Hub, in {where_}."), fg(C_MUTED).italic())));
    }
    lines.push(Line::default());
    lines.extend(status_lines(a));
    let t = a.transcript.as_ref();
    if let Some(p) = t.and_then(|t| t.last_prompt.as_ref()) {
        heading(&mut lines, "Your last request");
        lines.push(Line::from(s(p.chars().take(400).collect::<String>(), fg(C_FG))));
    }
    if let Some(text) = t.and_then(|t| t.last_text.as_ref()) {
        heading(&mut lines, "Last message");
        let all: Vec<&str> = text.lines().collect();
        lines.extend(markdown(&all[all.len().saturating_sub(MAX_MESSAGE_LINES)..].join("\n")));
    }
    file_lines(&mut lines, a.git.as_ref());
    if let Some(sess) = &a.session {
        let mut parts = vec![];
        if let Some(t) = t {
            parts.extend(t.model.clone());
            if t.turns > 0 {
                parts.push(format!("{} turn{}", t.turns, if t.turns == 1 { "" } else { "s" }));
            }
            if let Some(c) = t.cost_usd.filter(|c| *c > 0.0) {
                parts.push(format!("${c:.2}"));
            }
        }
        if let Some(m) = &sess.permission_mode {
            parts.push(format!("{m} mode"));
        }
        parts.push(sess.session_id.chars().take(8).collect());
        heading(&mut lines, "Session");
        lines.push(Line::from(s(parts.join(" · "), fg(C_DIM))));
    }
    lines
}

fn workspace_detail(app: &App, ws: &Workspace) -> Vec<Line<'static>> {
    let mut lines = vec![meta_line(None, &ws.path, ws.git.as_ref())];
    let agents = app.agents_of(ws);
    let terms = ws.windows.iter().filter(|w| !w.is_claude()).count();
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    lines.push(Line::from(s(format!("{} Claude session{} · {terms} terminal{}", agents.len(), plural(agents.len()), plural(terms)), fg(C_MUTED))));
    if ws.windows.is_empty() || agents.is_empty() && terms == 0 {
        lines.push(Line::default());
        lines.push(Line::from(vec![s("Nothing open yet. ", fg(C_MUTED)), s("c", fg(C_FG).bold()), s(" starts Claude, ", fg(C_MUTED)), s("t", fg(C_FG).bold()), s(" a terminal.", fg(C_MUTED))]));
    }
    if !agents.is_empty() {
        heading(&mut lines, "Agents");
        for &i in &agents {
            let a = &app.snap.agents[i];
            let head = status_lines(a).into_iter().next().map(|l| l.to_string()).unwrap_or_default();
            let style = if a.status == Status::Waiting { fg(C_BRIGHT).bold() } else { fg(C_FG) };
            lines.push(Line::from(vec![status_symbol(a.status, tick(app)), s(format!(" {}", a.title()), style), s(format!("  {}", head.chars().skip(2).collect::<String>()), fg(C_MUTED))]));
        }
    }
    file_lines(&mut lines, ws.git.as_ref());
    lines
}

fn draw_side(f: &mut Frame, app: &mut App, area: Rect) {
    let row = app.current();
    let t = tick(app);
    let (title, body, windows_of): (Line<'static>, Vec<Line<'static>>, Option<(usize, Option<String>)>) = match row {
        Some(Row::Agent(i)) => {
            let a = &app.snap.agents[i];
            let wi = a.workspace.as_ref().and_then(|n| app.snap.workspaces.iter().position(|w| &w.name == n));
            (Line::from(vec![s(" ", Style::new()), status_symbol(a.status, t), s(format!(" {} ", a.title()), fg(C_BRIGHT).bold())]), agent_detail(a), wi.map(|w| (w, a.window_id.clone())))
        }
        Some(Row::Workspace(wi)) => {
            let ws = &app.snap.workspaces[wi];
            (Line::from(s(format!(" {} ", ws.name), fg(C_BRIGHT).bold())), workspace_detail(app, ws), Some((wi, None)))
        }
        Some(Row::Terminal(wi, ti)) => return draw_terminal(f, app, area, wi, ti),
        _ => (
            Line::from(s(" Claude Hub ", fg(C_BRIGHT).bold())),
            vec![Line::from(s("No workspaces yet.", fg(C_BRIGHT).bold())), Line::from(s("Press n to open one, then c to start Claude in it.", fg(C_MUTED)))],
            None,
        ),
    };
    let block = panel(title, false);
    let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
    f.render_widget(block, area);

    let reply_h = if app.current_agent().is_some_and(|a| a.target.is_some()) { 3 } else { 0 };
    let windows = windows_of.as_ref().map(|(wi, _)| *wi).map(|wi| app.snap.workspaces[wi].windows.len() as u16 + 2).unwrap_or(0);
    let windows = windows.min(inner.height.saturating_sub(reply_h + 4) / 2);
    let [body_area, win_area, reply_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(windows), Constraint::Length(reply_h)]).areas(inner);
    app.hits.detail = body_area;

    let max_scroll = (body.len() as u16).saturating_sub(4);
    app.detail_scroll = app.detail_scroll.min(max_scroll);
    f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0)), body_area);

    if let Some((wi, own)) = windows_of.filter(|_| windows > 0) {
        draw_windows(f, app, win_area, wi, own.as_deref());
    }
    if reply_h > 0 {
        draw_reply(f, app, reply_area);
    }
}

/// The workspace's windows, each clickable.
fn draw_windows(f: &mut Frame, app: &mut App, area: Rect, wi: usize, own: Option<&str>) {
    let ws = app.snap.workspaces[wi].clone();
    f.render_widget(Paragraph::new(s("Windows", fg(C_MUTED).bold())), Rect { height: 1, y: area.y + 1, ..area });
    for (n, w) in ws.windows.iter().enumerate() {
        let y = area.y + 2 + n as u16;
        if y >= area.bottom() {
            break;
        }
        let kind = if w.is_claude() { s("✻ ", fg(C_ACCENT)) } else { s("❯ ", fg(C_MUTED)) };
        let mut spans = vec![s(format!("{} ", w.index), fg(C_DIM)), kind, s(w.name.clone(), fg(C_INFO).underlined())];
        if w.command != w.name && w.command != "claude" {
            spans.push(s(format!("  {}", w.command), fg(C_DIM)));
        }
        if own == Some(w.window_id.as_str()) {
            spans.push(s("  ◂ this agent", fg(C_DIM)));
        }
        let rect = Rect { y, height: 1, ..area };
        f.render_widget(Paragraph::new(Line::from(spans)), rect);
        app.hits.buttons.push((rect, Action::JumpWindow(ws.name.clone(), w.window_id.clone())));
    }
}

fn draw_reply(f: &mut Frame, app: &mut App, area: Rect) {
    let active = app.focus == Focus::Reply;
    let a = app.current_agent().unwrap();
    let placeholder = if a.needs_permission() {
        format!("Answer {}: y approves, d denies, or write what to do instead", a.name())
    } else {
        format!("Message to {} · r to type, Enter sends", a.name())
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(if active { C_ACCENT } else { C_BORDER }))
        .title(s(" Reply ", fg(if active { C_BRIGHT } else { C_MUTED })))
        .style(Style::new().bg(C_PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let content = if app.reply.text.is_empty() { s(placeholder, fg(C_DIM)) } else { s(app.reply.text.clone(), fg(C_BRIGHT)) };
    f.render_widget(Paragraph::new(content), inner);
    if active {
        set_cursor(f, &app.reply, inner.x, inner.y);
    } else {
        app.hits.buttons.push((area, Action::Reply));
    }
}

fn draw_terminal(f: &mut Frame, app: &mut App, area: Rect, wi: usize, ti: usize) {
    let ws = &app.snap.workspaces[wi];
    let w = &ws.windows[ti];
    let block = panel(Line::from(vec![s(" ❯ ", fg(C_MUTED)), s(format!("{} {} ", w.index, w.name), fg(C_BRIGHT).bold())]), false);
    let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
    f.render_widget(block, area);
    let [info, screen_area] = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(inner);
    app.hits.detail = Rect::default();
    let info_lines = vec![
        Line::from(vec![s(format!("{}  ", ws.name), fg(C_FG).bold()), s(shorten(&ws.path), fg(C_DIM))]),
        Line::from(vec![s(format!("Running {}", w.command), fg(C_FG)), s(format!("  ·  {}×{}", w.width, w.height), fg(C_DIM))]),
    ];
    f.render_widget(Paragraph::new(info_lines), info);

    let screen_block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(C_BORDER))
        .title(s(" screen · ↵ to open ", fg(C_DIM)))
        .style(Style::new().bg(C_BAR));
    let screen_inner = screen_block.inner(screen_area);
    f.render_widget(screen_block, screen_area);
    app.hits.buttons.push((screen_area, Action::JumpWindow(ws.name.clone(), w.window_id.clone())));
    match &w.screen {
        None => f.render_widget(
            Paragraph::new(s("Nothing captured yet. The preview starts with the next output in this terminal.", fg(C_MUTED))).wrap(Wrap { trim: true }),
            screen_inner,
        ),
        Some(lines) if lines.is_empty() => f.render_widget(Paragraph::new(s("(blank screen)", fg(C_DIM))), screen_inner),
        Some(lines) => {
            let skip = lines.len().saturating_sub(screen_inner.height as usize);
            f.render_widget(Paragraph::new(lines[skip..].to_vec()).style(fg(C_FG)), screen_inner);
        }
    }
}

// ---- footer, toast, modals ----

fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    f.render_widget(Block::new().style(Style::new().bg(C_BAR)), area);
    let mut x = area.x + 1;
    for (key, label, action) in app.actions() {
        let key_w = key.chars().count() as u16 + 2;
        let w = key_w + label.chars().count() as u16 + 3;
        if x + w > area.right() {
            break;
        }
        let rect = Rect::new(x, area.y, w, 1);
        let line = Line::from(vec![s(format!(" {key} "), fg(C_BRIGHT).bg(C_INPUT).bold()), s(format!(" {label}  "), fg(C_MUTED))]);
        f.render_widget(Paragraph::new(line), rect);
        app.hits.buttons.push((rect, action));
        x += w;
    }
}

fn draw_toast(f: &mut Frame, app: &App, area: Rect) {
    let Some(t) = &app.toast else { return };
    let w = (t.text.chars().count() as u16 + 4).min(area.width.saturating_sub(4)).max(10);
    let lines = (t.text.chars().count() as u16 / w.saturating_sub(4).max(1)) + 1;
    let rect = Rect::new(area.right().saturating_sub(w + 2), area.y + 1, w, lines + 2);
    let color = if t.error { C_ERROR } else { C_SUCCESS };
    f.render_widget(Clear, rect);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(fg(color)).style(Style::new().bg(C_PANEL));
    f.render_widget(Paragraph::new(s(t.text.clone(), fg(C_BRIGHT))).wrap(Wrap { trim: true }).block(block), rect);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

fn modal_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(C_ACCENT))
        .title(s(format!(" {title} "), fg(C_BRIGHT).bold()))
        .style(Style::new().bg(C_PANEL).fg(C_FG))
        .padding(ratatui::widgets::Padding::horizontal(1))
}

fn set_cursor(f: &mut Frame, input: &Input, x: u16, y: u16) {
    let w: usize = input.before_cursor().chars().map(|c| c.width().unwrap_or(0)).sum();
    f.set_cursor_position(Position::new(x + w as u16, y));
}

const HELP: &[(&str, &str)] = &[
    ("", "Workspaces and agents"),
    ("Enter", "go to the agent, terminal or workspace"),
    ("n", "open a workspace (fuzzy search or type a path)"),
    ("c / t / e", "new Claude / terminal / nvim in the workspace"),
    ("x", "close the agent, terminal or workspace"),
    ("R", "rename the workspace"),
    ("r", "reply: type a message, Enter sends it to the agent"),
    ("y / d", "approve / deny a permission prompt"),
    ("j k / arrows", "move   g G  top / bottom"),
    ("PgUp / PgDn", "scroll the detail panel"),
    ("/", "filter, Esc clears"),
    ("q", "leave (everything keeps running)"),
    ("", ""),
    ("", "From any terminal"),
    ("HUBKEY", "back to the hub, or click ◀ Claude Hub in the top bar"),
    ("", ""),
    ("", "Mouse"),
    ("click", "select; click again or double-click to open"),
    ("wheel", "scroll the list or the detail panel"),
];

fn draw_modal(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(modal) = &mut app.modal else { return };
    match modal {
        Modal::Help => {
            let hub_key = key_label(&app.cfg.hub_key);
            let lines: Vec<Line> = HELP
                .iter()
                .map(|(k, v)| match *k {
                    "" => Line::from(s(*v, fg(C_BRIGHT).bold())),
                    k => {
                        let k = if k == "HUBKEY" { hub_key.as_str() } else { k };
                        Line::from(vec![s(format!("  {k:<14}"), fg(C_ACCENT).bold()), s(*v, fg(C_FG))])
                    }
                })
                .collect();
            let rect = centered(area, 74, lines.len() as u16 + 2);
            f.render_widget(Clear, rect);
            f.render_widget(Paragraph::new(lines).block(modal_block("claude-hub · help")), rect);
        }
        Modal::Confirm { question, yes, .. } => {
            let rect = centered(area, 64, 9);
            f.render_widget(Clear, rect);
            let block = modal_block("Confirm").border_style(fg(C_WARNING));
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let text = vec![Line::from(s("⚠  Are you sure?", fg(C_WARNING).bold())), Line::default(), Line::from(s(question.clone(), fg(C_FG)))];
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), inner);
            let by = inner.bottom() - 1;
            let no_rect = Rect::new(inner.right().saturating_sub(9), by, 8, 1);
            let yes_rect = Rect::new(inner.right().saturating_sub(19), by, 9, 1);
            let style = |focused: bool, color: Color| if focused { fg(C_BRIGHT).bg(color).bold() } else { fg(C_FG).bg(C_INPUT) };
            f.render_widget(Paragraph::new(s(" Yes  y ", style(*yes, C_ERROR))), yes_rect);
            f.render_widget(Paragraph::new(s(" No  n ", style(!*yes, C_ACCENT))), no_rect);
            app.hits.buttons.push((yes_rect, Action::Confirm(true)));
            app.hits.buttons.push((no_rect, Action::Confirm(false)));
        }
        Modal::Rename { input, .. } => {
            let rect = centered(area, 60, 5);
            f.render_widget(Clear, rect);
            let block = modal_block("Rename workspace");
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let field = Rect { y: inner.y + 1, height: 1, ..inner };
            f.render_widget(Paragraph::new(s(input.text.clone(), fg(C_BRIGHT))).style(Style::new().bg(C_INPUT)), field);
            set_cursor(f, input, field.x, field.y);
        }
        Modal::Picker(p) => draw_picker(f, p, area, &mut app.hits.picker_list),
    }
}

fn draw_picker(f: &mut Frame, p: &mut Picker, area: Rect, hit: &mut Rect) {
    let rect = centered(area, area.width * 9 / 10, area.height * 85 / 100);
    f.render_widget(Clear, rect);
    let block = modal_block("Open workspace");
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let [input_area, _, list, hint] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let mut spans = vec![s("❯ ", fg(C_ACCENT).bold())];
    if p.input.text.is_empty() {
        spans.push(s("Search repos and directories, or type a path like ~/repos/…", fg(C_DIM)));
    } else {
        spans.push(s(p.input.text.clone(), fg(C_BRIGHT)));
        spans.extend(p.suggestion().map(|sug| s(sug, fg(C_DIM))));
    }
    let count = s(format!("{} ", p.options.len()), fg(C_DIM));
    f.render_widget(Paragraph::new(Line::from(spans)).style(Style::new().bg(C_INPUT)), input_area);
    f.render_widget(Paragraph::new(count).alignment(Alignment::Right), input_area);
    set_cursor(f, &p.input, input_area.x + 2, input_area.y);

    *hit = list;
    let h = list.height as usize;
    if p.selected < p.offset {
        p.offset = p.selected;
    } else if p.selected >= p.offset + h {
        p.offset = p.selected + 1 - h;
    }
    let lines: Vec<Line> = p
        .options
        .iter()
        .enumerate()
        .skip(p.offset)
        .take(h)
        .map(|(i, o)| {
            if i == p.selected {
                let mut l = picker::highlight(&o.line);
                l.spans.insert(0, s("▌", fg(C_ACCENT)));
                l.style(Style::new().bg(C_SELECTION))
            } else {
                let mut l = o.line.clone();
                l.spans.insert(0, Span::raw(" "));
                l
            }
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(Paragraph::new(s(" No matches", fg(C_MUTED))), list);
    } else {
        f.render_widget(Paragraph::new(lines), list);
    }
    f.render_widget(Paragraph::new(Picker::hint()), hint);
}
