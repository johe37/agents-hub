//! Drawing the hub.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{Action, App, FLASH, Focus, Modal, Row};
use crate::config::shorten;
use crate::git::GitStatus;
use crate::gitlab;
use crate::input::Input;
use crate::picker::{self, Picker};
use crate::snapshot::{Agent, Workspace};
use crate::state::{Status, age};
use crate::theme::*;
use crate::tmux::key_label;

const COMMENT: &str = "\u{f0e5}";
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_MESSAGE_LINES: usize = 40;

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
        Status::Waiting => s("◉", fg(if (t / 600) % 2 == 0 { colors().warning } else { lerp(colors().warning, colors().bg, 0.5) }).bold()),
        Status::Working => s(SPINNER[(t / 80) as usize % SPINNER.len()], fg(colors().info)),
        Status::Idle => s("○", fg(colors().success)),
        Status::Unknown => s("·", fg(colors().dim)),
        Status::Ended => s("✗", fg(colors().dim)),
        Status::Stale => s("?", fg(colors().dim)),
    }
}

fn status_color(status: Status) -> Color {
    match status {
        Status::Waiting => colors().warning,
        Status::Working => colors().info,
        Status::Idle => colors().success,
        _ => colors().dim,
    }
}

/// Share of the context window in use.
fn context_fill(tokens: u64) -> f32 {
    // shortcut: transcripts don't name the window size; assume 200k until usage proves it is a 1M window.
    let window = if tokens > 200_000 { 1_000_000 } else { 200_000 };
    (tokens as f32 / window as f32).min(1.0)
}

fn context_gauge(tokens: u64) -> Vec<Span<'static>> {
    let c = colors();
    let fill = context_fill(tokens);
    let color = if fill < 0.5 { lerp(c.success, c.warning, fill * 2.0) } else { lerp(c.warning, c.error, (fill - 0.5) * 2.0) };
    let n = (fill * 8.0).round() as usize;
    vec![s("▰".repeat(n), fg(color)), s("▱".repeat(8 - n), fg(c.border)), s(format!(" {:>3}%", (fill * 100.0).round()), fg(c.dim))]
}

/// How long the agent has waited for you, warming from yellow to red over 15 minutes.
fn waiting_timer(since: u64) -> Span<'static> {
    let mins = crate::state::now().saturating_sub(since) as f32 / 60.0;
    s(format!("⏳ {}", age(since)), fg(lerp(colors().warning, colors().error, mins / 15.0)).bold())
}

/// Text in a rounded pill; caps need a Nerd Font, which the user's prompt already relies on.
fn pill(text: String, fg_c: Color, bg_c: Color, on: Color) -> Vec<Span<'static>> {
    vec![s("\u{e0b6}", fg(bg_c).bg(on)), s(text, fg(fg_c).bg(bg_c).bold()), s("\u{e0b4}", fg(bg_c).bg(on))]
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
    let mut v = vec![s(format!("⎇ {}", g.branch.as_deref().unwrap_or("-")), fg(colors().purple))];
    for (n, prefix, color) in [
        (g.dirty as u32, " ●", colors().modified),
        (g.added, " +", colors().success),
        (g.removed, " −", colors().error),
        (g.ahead, " ↑", colors().info),
        (g.behind, " ↓", colors().info),
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
    f.render_widget(Block::new().style(Style::new().bg(colors().bg).fg(colors().fg)), area);
    app.hits.buttons.clear();
    let [header, main, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(area);
    draw_header(f, app, header);
    if side_visible(area) {
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

fn side_visible(area: Rect) -> bool {
    area.width >= 100
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Block::new().style(Style::new().bg(colors().bar)), area);
    let t = tick(app);
    let title = "Claude Hub";
    let n = title.chars().count() as f32;
    let mut left = vec![s(" ✻ ", fg(gradient(&[colors().claude, colors().purple, colors().info], t as f32 / 4000.0)).bold())];
    // A slow shimmer across the title.
    left.extend(title.chars().enumerate().map(|(i, c)| s(c.to_string(), fg(gradient(&[colors().claude, colors().purple, colors().info], i as f32 / n / 2.0 - t as f32 / 4000.0)).bold())));
    left.push(s(format!("   {} brings you back here from any terminal", key_label(&app.cfg.hub_key)), fg(colors().dim)));
    let left = Line::from(left);
    let count = |st: Status| app.snap.agents.iter().filter(|a| a.status == st).count();
    let mut right = vec![];
    let waiting = count(Status::Waiting);
    if waiting > 0 {
        let pulse = ((t as f32 / 600.0 * std::f32::consts::PI).sin() + 1.0) / 2.0;
        right.extend(pill(format!("◉ {waiting} need you"), colors().bg, lerp(lerp(colors().warning, colors().bg, 0.4), colors().warning, pulse), colors().bar));
        right.push(Span::raw(" "));
    }
    let working = count(Status::Working);
    if working > 0 {
        right.extend(pill(format!("{} {working} working", SPINNER[(t / 80) as usize % SPINNER.len()]), colors().on_accent, colors().accent, colors().bar));
        right.push(Span::raw(" "));
    }
    let idle = count(Status::Idle);
    if idle > 0 {
        right.extend(pill(format!("○ {idle} idle"), colors().on_accent, colors().success, colors().bar));
        right.push(Span::raw(" "));
    }
    let n = app.snap.workspaces.len();
    right.push(s(format!("{n} workspace{} ", if n == 1 { "" } else { "s" }), fg(colors().muted)));
    let rw = width_of(&right) as u16;
    f.render_widget(Paragraph::new(left), Rect { width: area.width.saturating_sub(rw), ..area });
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), area);
}

fn panel(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(if focused { colors().accent } else { colors().border }))
        .title(title)
        .style(Style::new().bg(colors().bg))
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.modal.is_none() && app.focus != Focus::Reply;
    let title = Line::from(vec![s(" Workspaces ", fg(if focused { colors().bright } else { colors().muted }).bold())]);
    let block = panel(title, focused);
    let mut inner = block.inner(area);
    f.render_widget(block, area);

    if app.focus == Focus::Filter || !app.filter.text.is_empty() {
        let [list, bar] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
        inner = list;
        let active = app.focus == Focus::Filter;
        let line = Line::from(vec![s(" / ", fg(colors().bg).bg(if active { colors().accent } else { colors().dim }).bold()), s(" ", Style::new()), s(app.filter.text.clone(), fg(colors().bright))]);
        f.render_widget(Paragraph::new(line).style(Style::new().bg(colors().panel)), bar);
        if active {
            set_cursor(f, &app.filter, bar.x + 4, bar.y);
        }
    }
    app.hits.list = inner;

    if !app.loaded {
        let t = tick(app);
        let line = Line::from(vec![s(SPINNER[(t / 80) as usize % 10], fg(colors().accent)), s(" Loading…", fg(colors().muted))]);
        f.render_widget(Paragraph::new(line), inner);
        return;
    }
    if app.rows.is_empty() {
        let msg = if app.filter.text.is_empty() { "Nothing here yet. Press n to open a workspace." } else { "Nothing matches the filter." };
        f.render_widget(Paragraph::new(s(format!(" {msg}"), fg(colors().muted))), inner);
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

    if let Some(y) = app.selected.checked_sub(app.offset).filter(|&y| y < h) {
        let y = inner.y + y as u16;
        for x in inner.x..inner.right() {
            let frac = (x - inner.x) as f32 / inner.width.max(1) as f32;
            if let Some(cell) = f.buffer_mut().cell_mut(Position::new(x, y)) {
                cell.set_bg(lerp(colors().selection, lerp(colors().selection, colors().bg, 0.6), frac));
            }
        }
    }

    if app.rows.len() > h {
        let thumb_h = (h * h / app.rows.len()).max(1);
        let thumb_y = app.offset * (h - thumb_h) / (app.rows.len() - h).max(1);
        for y in 0..thumb_h {
            let pos = Position::new(area.right() - 1, inner.y + (thumb_y + y) as u16);
            if let Some(cell) = f.buffer_mut().cell_mut(pos) {
                cell.set_symbol("┃").set_fg(colors().dim);
            }
        }
    }
}

fn row_line(app: &App, i: usize, width: usize) -> Line<'static> {
    let row = app.rows[i].1;
    let selected = i == app.selected;
    let t = tick(app);
    let is_child = |j: usize| matches!(app.rows.get(j).map(|r| r.1), Some(Row::Agent(_) | Row::Terminal(..) | Row::Thread(..)));
    let branch = if is_child(i + 1) { "├─ " } else { "╰─ " };
    let marker = match row {
        _ if selected => s("▌", fg(colors().bright)),
        Row::Workspace(wi) => match app.worst(&app.agents_of(&app.snap.workspaces[wi])) {
            Some(st @ (Status::Waiting | Status::Working)) => s("▎", fg(status_color(st))),
            _ => Span::raw(" "),
        },
        _ => Span::raw(" "),
    };
    let w = width.saturating_sub(1);

    let (left, right): (Vec<Span>, Vec<Span>) = match row {
        Row::Gap => (vec![], vec![]),
        Row::Header(title, n) => (vec![s(format!("{title} · {n}"), fg(colors().muted).bold())], vec![]),
        Row::Workspace(wi) => {
            let ws = &app.snap.workspaces[wi];
            let agents = app.agents_of(ws);
            let mut l = vec![match app.worst(&agents) {
                Some(st) => status_symbol(st, t),
                None => s("▪", fg(colors().dim)),
            }];
            l.push(s(format!(" {}", ws.name), fg(colors().bright).bold()));
            l.push(s(format!("  {}", shorten(&ws.path)), fg(colors().dim)));
            if ws.git.is_some() {
                l.push(Span::raw("  "));
                l.extend(git_spans(ws.git.as_ref()));
            }
            let terms = ws.windows.iter().filter(|w| !w.is_claude()).count();
            let mut r = vec![];
            if !agents.is_empty() {
                r.push(s(format!("✻{}", agents.len()), fg(colors().dim)));
            }
            if terms > 0 {
                r.push(s(format!(" ❯{terms}"), fg(colors().dim)));
            }
            if let Some(mr) = &ws.mr {
                r.push(s(format!("  !{} ", mr.iid), fg(colors().muted)));
                if mr.state == "opened" {
                    r.push(pipeline_symbol(mr.pipeline.as_deref()));
                    if mr.running() && !mr.jobs.is_empty() {
                        let done = mr.jobs.iter().filter(|j| finished(&j.status)).count();
                        r.push(s(format!(" {done}/{}", mr.jobs.len()), fg(colors().info)));
                    }
                } else {
                    r.push(s(mr.state.clone(), fg(colors().dim)));
                }
                if !mr.threads.is_empty() {
                    // Nerd Font icons can draw wider than one cell, so the count gets a space of its own.
                    r.push(s(format!("  {COMMENT} {}", mr.threads.len()), fg(colors().warning)));
                }
            }
            (l, r)
        }
        Row::Agent(ai) => {
            let a = &app.snap.agents[ai];
            let mut l = vec![s(branch, fg(colors().border).add_modifier(Modifier::BOLD)), status_symbol(a.status, t), Span::raw(" ")];
            let title_style = if a.status == Status::Waiting { fg(colors().bright).bold() } else { fg(colors().fg) };
            l.push(s(a.title(), title_style));
            let mut detail = vec![];
            if let Some(sess) = &a.session {
                if a.status == Status::Waiting {
                    let reason = waiting_reason(sess.notification_type.as_deref()).map(String::from);
                    detail.push(s(reason.or(sess.message.clone()).unwrap_or("Waiting for you".into()), fg(colors().modified)));
                } else if a.status == Status::Working {
                    if let Some(tool) = &sess.tool {
                        detail.push(s(tool.clone(), fg(colors().dim)));
                    }
                }
            }
            if a.workspace.is_none() {
                detail.push(s(format!("  {}", a.name()), fg(colors().dim).italic()));
            } else if a.background {
                detail.push(s("  background", fg(colors().dim).italic()));
            }
            let mut right = vec![];
            if a.status == Status::Waiting && a.session.is_some() {
                right.push(waiting_timer(a.timestamp()));
            } else {
                if let Some(tokens) = a.transcript.as_ref().and_then(|t| t.context_tokens).filter(|_| a.status.alive()) {
                    right.extend(context_gauge(tokens));
                }
                if a.session.is_some() {
                    right.push(s(format!(" {:>3}", age(a.timestamp())), fg(colors().dim)));
                }
            }
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
        Row::Thread(wi, ti) => {
            let th = &app.snap.workspaces[wi].mr.as_ref().unwrap().threads[ti];
            let mut l = vec![s(branch, fg(colors().border).bold()), s(format!("{COMMENT} "), fg(colors().warning))];
            if let Some(n) = th.notes.first() {
                l.push(s(n.author.clone(), fg(colors().fg).bold()));
                l.push(s(format!("  {}", th.place()), fg(colors().muted)));
                l.push(s(format!("  {}", n.body.split_whitespace().collect::<Vec<_>>().join(" ")), fg(colors().dim)));
            }
            let replies = th.notes.len().saturating_sub(1);
            (l, if replies > 0 { vec![s(format!("↩{replies}"), fg(colors().dim))] } else { vec![] })
        }
        Row::Terminal(wi, ti) => {
            let win = &app.snap.workspaces[wi].windows[ti];
            let mut l = vec![s(branch, fg(colors().border).bold()), s("❯ ", fg(colors().muted)), s(format!("{} ", win.index), fg(colors().dim)), s(win.name.clone(), fg(colors().fg))];
            if win.command != win.name {
                l.push(s(format!("  {}", win.command), fg(colors().muted)));
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
    let flash = match row {
        Row::Agent(ai) => app.changed.get(&app.snap.agents[ai].key).map(|at| (status_color(app.snap.agents[ai].status), at.elapsed())),
        _ => None,
    };
    if selected {
        line = line.style(Style::new().bg(colors().selection));
    } else if let Some((color, el)) = flash.filter(|(_, el)| *el < FLASH) {
        line = line.style(Style::new().bg(lerp(lerp(colors().bg, color, 0.45), colors().bg, el.as_secs_f32() / FLASH.as_secs_f32())));
    } else if app.hover == Some(i) && !matches!(row, Row::Gap | Row::Header(..)) {
        line = line.style(Style::new().bg(colors().hover));
    }
    line
}

// ---- detail panel ----

const MAX_PROMPT_LINES: usize = 6;

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
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
            out.push(Line::from(s(format!("  {raw}"), fg(colors().code))));
            continue;
        }
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') {
            out.push(Line::from(s(trimmed.trim_start_matches('#').trim().to_string(), fg(colors().bright).bold())));
            continue;
        }
        let indent = &raw[..raw.len() - trimmed.len()];
        let (bullet, body) = match trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            Some(rest) => (format!("{indent}• "), rest),
            None => (indent.to_string(), trimmed),
        };
        let mut spans = vec![s(bullet, fg(colors().accent))];
        for (i, part) in body.split('`').enumerate() {
            if i % 2 == 1 {
                spans.push(s(part.to_string(), fg(colors().code)));
                continue;
            }
            for (j, piece) in part.split("**").enumerate() {
                let style = if j % 2 == 1 { fg(colors().bright).bold() } else { fg(colors().fg) };
                spans.push(s(piece.to_string(), style));
            }
        }
        out.push(Line::from(spans));
    }
    out
}


/// What the agent is doing, and one line of detail.
fn headline(a: &Agent) -> (String, Option<String>) {
    let sess = a.session.as_ref();
    let t = a.transcript.as_ref();
    match a.status {
        Status::Waiting => {
            let reason = waiting_reason(sess.and_then(|s| s.notification_type.as_deref())).unwrap_or("Waiting for you");
            // The hook keeps only the latest event, so the tool behind a permission prompt comes from the transcript.
            let tool = if a.needs_permission() { t.and_then(|t| t.last_tool.clone()) } else { None };
            (reason.into(), tool.or_else(|| sess.and_then(|s| s.message.clone())))
        }
        Status::Working => ("Working".into(), sess.and_then(|s| s.tool.clone()).or_else(|| t.and_then(|t| t.last_tool.clone()))),
        Status::Idle => ("Done with its turn".into(), Some("Enter opens it to continue.".into())),
        Status::Ended | Status::Stale => ("Closed".into(), Some("Enter resumes it in a Claude Hub workspace.".into())),
        _ => ("No status".into(), Some("Started before claude-hub hooks were installed.".into())),
    }
}

fn meta_line(name: Option<&str>, path: &str, git: Option<&GitStatus>) -> Line<'static> {
    let mut v = vec![Span::raw(" ")];
    if let Some(n) = name {
        v.push(s(format!("{n}  "), fg(colors().muted)));
    }
    v.push(s(format!("{}  ", shorten(path)), fg(colors().dim)));
    v.extend(git_spans(git));
    v.push(Span::raw(" "));
    Line::from(v)
}

fn chat_lines(a: &Agent) -> Vec<Line<'static>> {
    let Some(t) = &a.transcript else { return vec![Line::from(s("No conversation yet.", fg(colors().muted)))] };
    let mut out = vec![];
    if let Some(p) = &t.last_prompt {
        out.push(Line::from(s("You", fg(colors().accent).bold())));
        let lines: Vec<&str> = p.lines().collect();
        for l in lines.iter().take(MAX_PROMPT_LINES) {
            out.push(Line::from(vec![s("│ ", fg(colors().accent)), s(l.chars().take(300).collect::<String>(), fg(colors().fg))]));
        }
        if lines.len() > MAX_PROMPT_LINES {
            out.push(Line::from(vec![s("│ ", fg(colors().accent)), s("…", fg(colors().dim))]));
        }
        out.push(Line::default());
    }
    if let Some(text) = &t.last_text {
        out.push(Line::from(s("✻ Claude", fg(colors().claude).bold())));
        let all: Vec<&str> = text.lines().collect();
        if all.len() > MAX_MESSAGE_LINES {
            out.push(Line::from(s("… earlier lines hidden, Enter opens the full conversation", fg(colors().dim).italic())));
        }
        out.extend(markdown(&all[all.len().saturating_sub(MAX_MESSAGE_LINES)..].join("\n")));
    }
    let mut parts: Vec<String> = t.model.iter().cloned().collect();
    if t.turns > 0 {
        parts.push(format!("{} turn{}", t.turns, plural(t.turns as usize)));
    }
    if let Some(c) = t.cost_usd.filter(|c| *c > 0.0) {
        parts.push(format!("${c:.2}"));
    }
    if let Some(sess) = &a.session {
        parts.extend(sess.permission_mode.as_ref().map(|m| format!("{m} mode")));
        parts.push(sess.session_id.chars().take(8).collect());
    }
    if !parts.is_empty() {
        out.push(Line::default());
        out.push(Line::from(s(parts.join(" · "), fg(colors().dim))));
    }
    out
}

fn change_lines(g: Option<&GitStatus>) -> Vec<Line<'static>> {
    let Some(g) = g else { return vec![Line::from(s("Not a git repository.", fg(colors().muted)))] };
    if g.files.is_empty() {
        return vec![Line::from(vec![s("✓ ", fg(colors().success).bold()), s("Working tree clean", fg(colors().fg))])];
    }
    let n = g.files.len();
    let mut out = vec![
        Line::from(vec![
            s(format!("{n} file{} changed", plural(n)), fg(colors().fg).bold()),
            s(format!("   +{}", g.added), fg(colors().success)),
            s(format!(" −{}", g.removed), fg(colors().error)),
        ]),
        Line::default(),
    ];
    for (status, path) in &g.files {
        let letter = status.chars().find(|&c| c != '.').unwrap_or('M');
        let color = match letter {
            '?' | 'A' => colors().success,
            'D' | 'U' => colors().error,
            'R' => colors().info,
            _ => colors().modified,
        };
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut l = vec![s(format!("{letter}  "), fg(color).bold()), s(name.to_string(), fg(colors().bright))];
        if !dir.is_empty() {
            l.push(s(format!("  {dir}"), fg(colors().dim)));
        }
        out.push(Line::from(l));
    }
    out
}

/// What the panel shows for an agent or workspace: a banner for what needs you, then tabs.
struct Side {
    title: String,
    meta: Line<'static>,
    color: Color,
    banner: Vec<Line<'static>>,
    buttons: Vec<(&'static str, &'static str, Color, Action)>,
    tabs: [(&'static str, usize); 4],
    body: Vec<Line<'static>>,
}

fn agent_side(app: &App, a: &Agent, ws: Option<&Workspace>, t: u128) -> Side {
    let color = status_color(a.status);
    let (head, detail) = headline(a);
    let when = a.session.as_ref().map_or(String::new(), |s| format!("  · {}", age(s.timestamp)));
    let mut banner = vec![Line::from(vec![status_symbol(a.status, t), s(format!(" {head}"), fg(color).bold()), s(when, fg(colors().dim))])];
    if let Some(d) = detail {
        let style = if a.needs_permission() { fg(colors().bright).bg(colors().panel) } else { fg(colors().fg) };
        banner.push(Line::from(s(d.lines().next().unwrap_or_default().to_string(), style)));
    }
    if a.workspace.is_none() && a.status.alive() {
        let where_ = a.session.as_ref().and_then(|s| s.tmux_session.clone()).map_or("its own terminal".into(), |s| format!("tmux session {s}"));
        banner.push(Line::from(s(format!("Runs outside Claude Hub, in {where_}."), fg(colors().muted).italic())));
    }
    let mut buttons = vec![];
    if a.status == Status::Waiting && a.target.is_some() {
        if a.needs_permission() {
            buttons.push(("y", "approve", colors().success, Action::Approve));
            buttons.push(("d", "deny", colors().error, Action::Deny));
        }
        buttons.push(("r", "reply", colors().input, Action::Reply));
    }
    if a.status == Status::Waiting {
        buttons.push(("m", "done", colors().input, Action::MarkDone));
    }
    if !a.status.alive() && a.session_id().is_some() {
        buttons.push(("↵", "resume", colors().accent, Action::Open));
    }
    if a.workspace.is_none() && a.status.alive() && a.pid.is_some() {
        buttons.push(("a", "adopt into the hub", colors().accent, Action::Adopt));
    }
    let git = a.git.as_ref();
    let body = match app.tab {
        0 => chat_lines(a),
        1 => change_lines(git),
        3 => pipeline_lines(ws.and_then(|w| w.mr.as_ref()), t),
        _ if ws.is_none() => vec![Line::from(s("Runs outside Claude Hub, so it has no hub windows.", fg(colors().muted)))],
        _ => vec![],
    };
    Side {
        title: a.title(),
        meta: meta_line(Some(&a.name()), &a.path, git),
        color,
        banner,
        buttons,
        tabs: [("Chat", 0), ("Changes", git.map_or(0, |g| g.files.len())), ("Windows", ws.map_or(0, |w| w.windows.len())), ("Pipeline", ws.and_then(|w| w.mr.as_ref()).map_or(0, |m| m.jobs.len()))],
        body,
    }
}

fn workspace_side(app: &App, ws: &Workspace, t: u128) -> Side {
    let agents = app.agents_of(ws);
    let count = |st: Status| agents.iter().filter(|&&i| app.snap.agents[i].status == st).count();
    let (color, symbol, head, detail) = match app.worst(&agents) {
        Some(st @ Status::Waiting) => {
            let n = count(st);
            (colors().warning, status_symbol(st, t), format!("{n} agent{} need{} you", plural(n), if n == 1 { "s" } else { "" }), None)
        }
        Some(st @ Status::Working) => {
            let n = count(st);
            (colors().info, status_symbol(st, t), format!("{n} agent{} working", plural(n)), None)
        }
        Some(_) => (colors().success, s("○", fg(colors().success)), "Nothing needs you".into(), None),
        None => (colors().dim, s("▪", fg(colors().dim)), "Nothing running".into(), Some("c starts Claude, t opens a terminal.")),
    };
    let mut banner = vec![Line::from(vec![symbol, s(format!(" {head}"), fg(color).bold())])];
    banner.extend(detail.map(|d| Line::from(s(d, fg(colors().muted)))));
    if let Some(mr) = &ws.mr {
        let status = [mr.pipeline.as_deref().map(|p| format!("pipeline {p}")), mr.merge_status.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        banner.push(Line::from(vec![s(format!("!{} ", mr.iid), fg(colors().accent).bold()), s(mr.title.clone(), fg(colors().fg))]));
        banner.push(Line::from(vec![pipeline_symbol(mr.pipeline.as_deref()), s(format!(" {status}"), fg(colors().muted)), s(format!("  ·  {} open thread{}", mr.threads.len(), plural(mr.threads.len())), fg(colors().dim))]));
    }
    if let Some(g) = &ws.git {
        let branch = g.branch.as_deref().unwrap_or_default();
        if ws.mr.is_none() && !branch.is_empty() && !matches!(branch, "main" | "master") {
            banner.push(Line::from(s("No merge request yet · p pushes and opens a draft", fg(colors().dim))));
        } else if g.ahead > 0 {
            banner.push(Line::from(s(format!("{} commit{} not pushed · p pushes", g.ahead, plural(g.ahead as usize)), fg(colors().dim))));
        }
    }
    let body = match app.tab {
        0 if agents.is_empty() => vec![Line::from(s("No agents yet. c starts Claude here.", fg(colors().muted)))],
        0 => agents
            .iter()
            .flat_map(|&i| {
                let a = &app.snap.agents[i];
                let (head, detail) = headline(a);
                let style = if a.status == Status::Waiting { fg(colors().bright).bold() } else { fg(colors().fg) };
                let detail = detail.map_or(String::new(), |d| format!(" · {}", d.lines().next().unwrap_or_default()));
                [
                    Line::from(vec![status_symbol(a.status, t), s(format!(" {}", a.title()), style)]),
                    Line::from(vec![s("  ", Style::new()), s(head, fg(status_color(a.status))), s(detail, fg(colors().dim))]),
                    Line::default(),
                ]
            })
            .collect(),
        1 => change_lines(ws.git.as_ref()),
        3 => pipeline_lines(ws.mr.as_ref(), t),
        _ => vec![],
    };
    Side {
        title: ws.name.clone(),
        meta: meta_line(None, &ws.path, ws.git.as_ref()),
        color,
        banner,
        buttons: vec![],
        tabs: [("Agents", agents.len()), ("Changes", ws.git.as_ref().map_or(0, |g| g.files.len())), ("Windows", ws.windows.len()), ("Pipeline", ws.mr.as_ref().map_or(0, |m| m.jobs.len()))],
        body,
    }
}

fn draw_side(f: &mut Frame, app: &mut App, area: Rect) {
    let t = tick(app);
    let (wi, side) = match app.current() {
        Some(Row::Terminal(wi, ti)) => return draw_terminal(f, app, area, wi, ti),
        Some(Row::Thread(..)) => return draw_thread(f, app, area),
        Some(Row::Agent(i)) => {
            let a = &app.snap.agents[i];
            let wi = a.workspace.as_ref().and_then(|n| app.snap.workspaces.iter().position(|w| &w.name == n));
            (wi, agent_side(app, a, wi.map(|w| &app.snap.workspaces[w]), t))
        }
        Some(Row::Workspace(wi)) => (Some(wi), workspace_side(app, &app.snap.workspaces[wi], t)),
        _ => {
            let block = panel(Line::from(s(" Claude Hub ", fg(colors().bright).bold())), false);
            let inner = block.inner(area).inner(Margin::new(2, 1));
            f.render_widget(block, area);
            let text = vec![Line::from(s("No workspaces yet.", fg(colors().bright).bold())), Line::from(s("Press n to open one, then c to start Claude in it.", fg(colors().muted)))];
            f.render_widget(Paragraph::new(text), inner);
            app.hits.detail = Rect::default();
            return;
        }
    };
    let block = panel(Line::from(s(format!(" {} ", side.title), fg(colors().bright).bold())), false).title_bottom(side.meta);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    f.render_widget(block, area);

    let buttons_h = if side.buttons.is_empty() { 0 } else { 2 };
    let banner_h = side.banner.len() as u16 + buttons_h + 2;
    let reply_h = if app.current_agent().is_some_and(|a| a.target.is_some()) { 3 } else { 0 };
    let [banner_area, _, tabs_area, body_area, reply_area] =
        Layout::vertical([Constraint::Length(banner_h), Constraint::Length(1), Constraint::Length(2), Constraint::Min(1), Constraint::Length(reply_h)]).areas(inner);

    // The banner: what needs you, tinted in the status color.
    let banner_block = Block::new()
        .borders(Borders::LEFT)
        .border_type(BorderType::Thick)
        .border_style(fg(side.color))
        .style(Style::new().bg(lerp(colors().bg, side.color, 0.12)))
        .padding(Padding::new(1, 1, 1, 1));
    let banner_inner = banner_block.inner(banner_area);
    f.render_widget(banner_block, banner_area);
    f.render_widget(Paragraph::new(side.banner), banner_inner);
    if buttons_h > 0 {
        let mut x = banner_inner.x;
        let y = banner_inner.bottom() - 1;
        for (key, label, color, action) in side.buttons {
            let key_fg = if color == colors().input { colors().fg } else { colors().bg };
            let line = Line::from(vec![s(format!(" {key} "), fg(key_fg).bg(color).bold()), s(format!(" {label}  "), fg(colors().muted))]);
            let w = line.width() as u16;
            let rect = Rect::new(x, y, w.min(banner_inner.right().saturating_sub(x)), 1);
            f.render_widget(Paragraph::new(line), rect);
            app.hits.buttons.push((rect, action));
            x += w;
        }
    }

    // Tabs, with the active one underlined in the accent color.
    let mut x = tabs_area.x;
    let mut under = vec![];
    for (i, (label, n)) in side.tabs.iter().enumerate() {
        let active = app.tab == i;
        let mut spans = vec![s(format!(" {label}"), if active { fg(colors().bright).bold() } else { fg(colors().muted) })];
        if *n > 0 {
            spans.push(s(format!(" {n}"), fg(if active { colors().accent } else { colors().dim })));
        }
        spans.push(Span::raw(" "));
        let w = width_of(&spans) as u16;
        let rect = Rect::new(x, tabs_area.y, w.min(tabs_area.right().saturating_sub(x)), 1);
        f.render_widget(Paragraph::new(Line::from(spans)), rect);
        app.hits.buttons.push((rect, Action::Tab(i)));
        under.push(s(if active { "━" } else { "─" }.repeat(w as usize), fg(if active { colors().accent } else { colors().border })));
        x += w + 1;
        under.push(s("─", fg(colors().border)));
    }
    under.push(s("─".repeat(tabs_area.right().saturating_sub(x) as usize), fg(colors().border)));
    f.render_widget(Paragraph::new(Line::from(under)), Rect { y: tabs_area.y + 1, height: 1, ..tabs_area });

    let body_area = body_area.inner(Margin::new(0, 1));
    app.hits.detail = body_area;
    match wi {
        Some(wi) if app.tab == 2 => {
            let own = app.current_agent().and_then(|a| a.window_id.clone());
            draw_windows(f, app, body_area, wi, own.as_deref());
        }
        _ => {
            let max_scroll = (side.body.len() as u16).saturating_sub(4);
            app.detail_scroll = app.detail_scroll.min(max_scroll);
            f.render_widget(Paragraph::new(side.body).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0)), body_area);
        }
    }
    if reply_h > 0 {
        draw_reply(f, app, reply_area);
    }
}

/// The workspace's windows, each clickable.
fn draw_windows(f: &mut Frame, app: &mut App, area: Rect, wi: usize, own: Option<&str>) {
    let ws = app.snap.workspaces[wi].clone();
    if ws.windows.is_empty() {
        f.render_widget(Paragraph::new(s("No windows. c starts Claude, t a terminal.", fg(colors().muted))), area);
    }
    for (n, w) in ws.windows.iter().enumerate() {
        let y = area.y + n as u16;
        if y >= area.bottom() {
            break;
        }
        let kind = if w.is_claude() { s("✻ ", fg(colors().claude)) } else { s("❯ ", fg(colors().muted)) };
        let mut spans = vec![s(format!("{} ", w.index), fg(colors().dim)), kind, s(w.name.clone(), fg(colors().info).underlined())];
        if w.command != w.name && w.command != "claude" {
            spans.push(s(format!("  {}", w.command), fg(colors().dim)));
        }
        if own == Some(w.window_id.as_str()) {
            spans.push(s("  ◂ this agent", fg(colors().dim)));
        }
        let rect = Rect { y, height: 1, ..area };
        f.render_widget(Paragraph::new(Line::from(spans)), rect);
        app.hits.buttons.push((rect, Action::JumpWindow(ws.name.clone(), w.window_id.clone())));
    }
}

fn draw_reply(f: &mut Frame, app: &mut App, area: Rect) {
    let active = app.focus == Focus::Reply;
    let placeholder = if app.resolving {
        "Enter resolves the thread; write something first to post it as a reply".to_string()
    } else if let Some((_, mr, _)) = app.current_thread() {
        format!("Reply on !{} in GitLab · r to type, v to resolve, Enter posts", mr.iid)
    } else if app.current_agent().unwrap().needs_permission() {
        let a = app.current_agent().unwrap();
        format!("Answer {}: y approves, d denies, or write what to do instead", a.name())
    } else {
        format!("Message to {} · r to type, Enter sends", app.current_agent().unwrap().name())
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(if active { colors().accent } else { colors().border }))
        .title(s(if app.resolving { " Resolve " } else { " Reply " }, fg(if active { colors().bright } else { colors().muted })))
        .style(Style::new().bg(colors().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Long text scrolls sideways so the cursor stays inside the box.
    let before = app.reply.before_cursor().width();
    let skip = before.saturating_sub(inner.width.saturating_sub(1) as usize);
    let content = if app.reply.text.is_empty() { s(placeholder, fg(colors().dim)) } else { s(app.reply.text.clone(), fg(colors().bright)) };
    f.render_widget(Paragraph::new(content).scroll((0, skip as u16)), inner);
    if active {
        f.set_cursor_position(Position::new(inner.x + (before - skip) as u16, inner.y));
    } else {
        app.hits.buttons.push((area, Action::Reply));
    }
}

fn draw_terminal(f: &mut Frame, app: &mut App, area: Rect, wi: usize, ti: usize) {
    let ws = &app.snap.workspaces[wi];
    let w = &ws.windows[ti];
    let block = panel(Line::from(vec![s(" ❯ ", fg(colors().muted)), s(format!("{} {} ", w.index, w.name), fg(colors().bright).bold())]), false);
    let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
    f.render_widget(block, area);
    let [info, screen_area] = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(inner);
    app.hits.detail = Rect::default();
    let info_lines = vec![
        Line::from(vec![s(format!("{}  ", ws.name), fg(colors().fg).bold()), s(shorten(&ws.path), fg(colors().dim))]),
        Line::from(vec![s(format!("Running {}", w.command), fg(colors().fg)), s(format!("  ·  {}×{}", w.width, w.height), fg(colors().dim))]),
    ];
    f.render_widget(Paragraph::new(info_lines), info);

    let screen_block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(colors().border))
        .title(s(" screen · ↵ to open ", fg(colors().dim)))
        .style(Style::new().bg(colors().bar));
    let screen_inner = screen_block.inner(screen_area);
    f.render_widget(screen_block, screen_area);
    app.hits.buttons.push((screen_area, Action::JumpWindow(ws.name.clone(), w.window_id.clone())));
    match &w.screen {
        None => f.render_widget(
            Paragraph::new(s("Nothing captured yet. The preview starts with the next output in this terminal.", fg(colors().muted))).wrap(Wrap { trim: true }),
            screen_inner,
        ),
        Some(lines) if lines.is_empty() => f.render_widget(Paragraph::new(s("(blank screen)", fg(colors().dim))), screen_inner),
        Some(lines) => {
            let skip = lines.len().saturating_sub(screen_inner.height as usize);
            f.render_widget(Paragraph::new(lines[skip..].to_vec()).style(fg(colors().fg)), screen_inner);
        }
    }
}

fn pipeline_symbol(status: Option<&str>) -> Span<'static> {
    match status {
        Some("success") => s("✓", fg(colors().success)),
        Some("failed") => s("✗", fg(colors().error)),
        Some("running" | "pending" | "created" | "preparing" | "waiting_for_resource") => s("⟳", fg(colors().info)),
        Some(_) => s("·", fg(colors().dim)),
        None => s("", Style::new()),
    }
}

fn finished(status: &str) -> bool {
    matches!(status, "success" | "failed" | "canceled" | "skipped")
}

fn job_symbol(status: &str, t: u128) -> Span<'static> {
    match status {
        "success" => s("✓", fg(colors().success)),
        "failed" => s("✗", fg(colors().error).bold()),
        "running" => s(SPINNER[(t / 80) as usize % SPINNER.len()], fg(colors().info)),
        "manual" => s("▸", fg(colors().muted)),
        "canceled" | "skipped" => s("⊘", fg(colors().dim)),
        _ => s("○", fg(colors().dim)),
    }
}

/// The status a stage shows: its most pressing job.
fn stage_status<'a>(jobs: &[&'a gitlab::Job]) -> &'a str {
    let rank = |s: &str| match s {
        "failed" => 0,
        "running" => 1,
        "pending" | "created" | "preparing" | "waiting_for_resource" => 2,
        "manual" => 3,
        "success" => 4,
        _ => 5,
    };
    jobs.iter().map(|j| j.status.as_str()).min_by_key(|s| rank(s)).unwrap_or("created")
}

fn duration(secs: f64) -> String {
    let secs = secs.round() as u64;
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
    }
}

/// The head pipeline: a strip of stages, then every job under its stage.
fn pipeline_lines(mr: Option<&gitlab::Mr>, t: u128) -> Vec<Line<'static>> {
    let Some(mr) = mr else { return vec![Line::from(s("No merge request for this branch.", fg(colors().muted)))] };
    if mr.jobs.is_empty() {
        return vec![Line::from(s(format!("!{} has no pipeline yet.", mr.iid), fg(colors().muted)))];
    }
    let stages = mr.stages();
    let mut strip = vec![];
    for (i, (name, jobs)) in stages.iter().enumerate() {
        if i > 0 {
            strip.push(s(" ── ", fg(colors().border)));
        }
        let st = stage_status(jobs);
        strip.push(job_symbol(st, t));
        strip.push(s(format!(" {name}"), if st == "failed" { fg(colors().error).bold() } else { fg(colors().fg) }));
    }
    let mut out = vec![Line::from(strip), Line::default()];
    for (name, jobs) in &stages {
        let longest = jobs.iter().filter_map(|j| j.duration).fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.max(d))));
        out.push(Line::from(vec![
            job_symbol(stage_status(jobs), t),
            s(format!(" {name:<30}"), fg(colors().bright).bold()),
            s(longest.map(duration).unwrap_or_default(), fg(colors().dim)),
        ]));
        for j in jobs {
            let when = j.duration.map(duration).unwrap_or_else(|| if j.status == "running" { "running".into() } else { j.status.clone() });
            out.push(Line::from(vec![s("   ", Style::new()), job_symbol(&j.status, t), s(format!(" {:<27}", j.name), fg(colors().fg)), s(when, fg(colors().dim))]));
        }
        out.push(Line::default());
    }
    out
}

/// A review thread: every note in it, with a reply box below.
fn draw_thread(f: &mut Frame, app: &mut App, area: Rect) {
    let Some((ws, mr, th)) = app.current_thread() else { return };
    let block = panel(Line::from(vec![s(format!(" {COMMENT} "), fg(colors().warning)), s(format!("{} ", th.place()), fg(colors().bright).bold())]), false)
        .title_bottom(Line::from(s(format!(" !{} {} ", mr.iid, mr.title), fg(colors().dim))));
    let inner = block.inner(area).inner(Margin::new(1, 0));
    f.render_widget(block, area);
    let [head, body_area, reply_area] = Layout::vertical([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)]).areas(inner);
    let hint = Line::from(vec![s(format!("{}  ", ws.name), fg(colors().fg).bold()), s("f puts it in Claude's prompt · v resolves", fg(colors().dim))]);
    f.render_widget(Paragraph::new(vec![Line::default(), hint]), head);
    let mut lines = vec![];
    for n in &th.notes {
        lines.push(Line::from(s(n.author.clone(), fg(colors().accent).bold())));
        lines.extend(n.body.lines().map(|l| Line::from(s(l.to_string(), fg(colors().fg)))));
        lines.push(Line::default());
    }
    app.detail_scroll = app.detail_scroll.min((lines.len() as u16).saturating_sub(4));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0)), body_area);
    app.hits.detail = body_area;
    draw_reply(f, app, reply_area);
}

// ---- footer, toast, modals ----

fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    f.render_widget(Block::new().style(Style::new().bg(colors().bar)), area);
    let mut x = area.x + 1;
    for (key, label, action) in app.actions(side_visible(f.area())) {
        let key_w = key.chars().count() as u16 + 2;
        let w = key_w + label.chars().count() as u16 + 3;
        if x + w > area.right() {
            break;
        }
        let rect = Rect::new(x, area.y, w, 1);
        let line = Line::from(vec![s(format!(" {key} "), fg(colors().bright).bg(colors().input).bold()), s(format!(" {label}  "), fg(colors().muted))]);
        f.render_widget(Paragraph::new(line), rect);
        app.hits.buttons.push((rect, action));
        x += w;
    }
}

fn draw_toast(f: &mut Frame, app: &App, area: Rect) {
    let Some(t) = &app.toast else { return };
    let w = (t.text.chars().count() as u16 + 4).min(area.width.saturating_sub(4)).max(10);
    let lines = (t.text.chars().count() as u16 / w.saturating_sub(4).max(1)) + 1;
    let ms = t.at.elapsed().as_millis() as f32;
    let ease = 1.0 - (1.0 - (ms / 220.0).min(1.0)).powi(3);
    let slide = ((w + 2) as f32 * (1.0 - ease)) as u16;
    let x = area.right().saturating_sub(w + 2) + slide;
    let rect = Rect::new(x, area.y + 1, w.min(area.right().saturating_sub(x)), lines + 2);
    if rect.width < 3 {
        return;
    }
    let (color, icon) = if t.error { (colors().error, "✗") } else { (colors().success, "✓") };
    let fade = ((ms - 3200.0) / 800.0).clamp(0.0, 1.0);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(lerp(color, colors().border, fade)))
        .title(s(format!(" {icon} "), fg(color).bold()))
        .style(Style::new().bg(colors().panel));
    f.render_widget(Paragraph::new(s(t.text.clone(), fg(lerp(colors().bright, colors().muted, fade)))).wrap(Wrap { trim: true }).block(block), rect);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

fn modal_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(colors().accent))
        .title(s(format!(" {title} "), fg(colors().bright).bold()))
        .style(Style::new().bg(colors().panel).fg(colors().fg))
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
    ("a", "adopt an agent running outside the hub into a workspace"),
    ("m", "mark done: clear waiting on the agent or workspace"),
    ("w", "jump to the next agent waiting for you"),
    ("↵ on closed", "resume a recently closed agent in a workspace"),
    ("r", "reply: type a message, Enter sends it to the agent"),
    ("y / d", "approve / deny a permission prompt"),
    ("j k / arrows", "move   g G  top / bottom"),
    ("h l / ← → / Tab", "previous / next panel tab: chat, changes, windows, pipeline (or 1–4)"),
    ("PgUp / PgDn", "scroll the detail panel"),
    ("/", "filter, Esc clears"),
    ("q", "leave (everything keeps running)"),
    ("", ""),
    ("", "Merge request threads (listed under a workspace)"),
    ("Enter", "open the thread in GitLab"),
    ("r", "reply in the thread"),
    ("f", "put the thread in Claude's prompt and go there; Enter sends it"),
    ("v", "resolve the thread, with an optional reply (\"Fixed in <commit>\")"),
    ("p", "push the workspace's branch; opens a draft merge request if it has none"),
    ("Ctrl+R", "refresh the workspace's merge request, pipeline and threads now"),
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
                    "" => Line::from(s(*v, fg(colors().bright).bold())),
                    k => {
                        let k = if k == "HUBKEY" { hub_key.as_str() } else { k };
                        Line::from(vec![s(format!("  {k:<14}"), fg(colors().accent).bold()), s(*v, fg(colors().fg))])
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
            let block = modal_block("Confirm").border_style(fg(colors().warning));
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let text = vec![Line::from(s("⚠  Are you sure?", fg(colors().warning).bold())), Line::default(), Line::from(s(question.clone(), fg(colors().fg)))];
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), inner);
            let by = inner.bottom() - 1;
            let no_rect = Rect::new(inner.right().saturating_sub(9), by, 8, 1);
            let yes_rect = Rect::new(inner.right().saturating_sub(19), by, 9, 1);
            let style = |focused: bool, color: Color| if focused { fg(colors().on_accent).bg(color).bold() } else { fg(colors().fg).bg(colors().input) };
            f.render_widget(Paragraph::new(s(" Yes  y ", style(*yes, colors().error))), yes_rect);
            f.render_widget(Paragraph::new(s(" No  n ", style(!*yes, colors().accent))), no_rect);
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
            f.render_widget(Paragraph::new(s(input.text.clone(), fg(colors().bright))).style(Style::new().bg(colors().input)), field);
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

    let mut spans = vec![s("❯ ", fg(colors().accent).bold())];
    if p.input.text.is_empty() {
        spans.push(s("Search repos and directories, or type a path like ~/repos/…", fg(colors().dim)));
    } else {
        spans.push(s(p.input.text.clone(), fg(colors().bright)));
        spans.extend(p.suggestion().map(|sug| s(sug, fg(colors().dim))));
    }
    let count = s(format!("{} ", p.options.len()), fg(colors().dim));
    f.render_widget(Paragraph::new(Line::from(spans)).style(Style::new().bg(colors().input)), input_area);
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
                l.spans.insert(0, s("▌", fg(colors().accent)));
                l.style(Style::new().bg(colors().selection))
            } else {
                let mut l = o.line.clone();
                l.spans.insert(0, Span::raw(" "));
                l
            }
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(Paragraph::new(s(" No matches", fg(colors().muted))), list);
    } else {
        f.render_widget(Paragraph::new(lines), list);
    }
    f.render_widget(Paragraph::new(Picker::hint()), hint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_fill_picks_window() {
        assert_eq!(context_fill(100_000), 0.5);
        assert_eq!(context_fill(500_000), 0.5);
        assert_eq!(context_fill(5_000_000), 1.0);
    }
}
