use crate::app::{App, Focus};
use crate::model::SourceId;
use crate::theme::skin::{BadgeMode, HighlightMode};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

pub mod detail;
pub mod main_view;
pub mod search_bar;
pub mod sidebar;
pub mod status_bar;
pub mod task_pane;
pub mod welcome;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    // Paint the themed background first; panels and text render on top and
    // leave unwritten cells tinted. `None` keeps the terminal's own background.
    if let Some(bg) = app.palette.bg {
        frame.render_widget(Block::default().style(Style::default().bg(bg)), area);
    }
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    search_bar::draw(frame, app, vchunks[0]);
    draw_body(frame, app, vchunks[1]);
    status_bar::draw(frame, app, vchunks[2]);
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::TaskView;
    // Task pane: a right column when peeking, a full overlay when expanded,
    // nothing when hidden (even if a task is still running in the background).
    let show_peek = app.task.is_some() && app.task_view == TaskView::Peek;
    let body = if show_peek {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Min(0), Constraint::Length(34)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Min(0)])
            .split(area)
    };

    sidebar::draw(frame, app, body[0]);
    main_view::draw(frame, app, body[1]);
    if show_peek {
        task_pane::draw_peek(frame, app, body[2]);
    }
    if app.task.is_some() && app.task_view == TaskView::Expanded {
        task_pane::draw_overlay(frame, app, area);
    }

    if app.confirm.is_some() {
        draw_confirm(frame, app, area);
    }
    if app.options_open {
        draw_options(frame, app, area);
    }
}

// --- shared themed helpers ---------------------------------------------------

/// Border color for a focusable panel: active vs hovered vs idle, per palette.
pub fn border_color(app: &App, f: Focus) -> Color {
    let p = &app.palette;
    if app.is_active(f) {
        p.border_active
    } else if app.is_hovered(f) {
        p.border_hover
    } else {
        p.border_idle
    }
}

/// A panel block carrying the skin's border (type + glyphs), the given border
/// color, a title in the palette title color, and the themed background.
pub fn themed_block(app: &App, border_col: Color, title: impl Into<String>) -> Block<'static> {
    let mut b = Block::default()
        .borders(app.skin.border.borders())
        .border_set(app.skin.border.set())
        .border_style(Style::default().fg(border_col))
        .title(Span::styled(
            title.into(),
            Style::default().fg(app.palette.title),
        ));
    if let Some(bg) = app.palette.bg {
        b = b.style(Style::default().bg(bg));
    }
    b
}

/// The selection highlight style for lists, per the skin's highlight mode.
pub fn highlight_style(app: &App) -> Style {
    let p = &app.palette;
    match app.skin.highlight {
        HighlightMode::Bar => Style::default()
            .fg(p.highlight_fg)
            .bg(p.highlight_bg)
            .add_modifier(Modifier::BOLD),
        HighlightMode::Reversed => Style::default().add_modifier(Modifier::REVERSED),
        HighlightMode::Bold => Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
    }
}

/// The list cursor symbol (skin icon when icons are on, else a plain triangle),
/// with a trailing space.
pub fn cursor_symbol(app: &App) -> String {
    let c = if app.skin.icons.enabled {
        app.skin.icons.cursor.as_str()
    } else {
        "\u{25b8}" // ▸
    };
    format!("{c} ")
}

/// A colored source badge rendered per the skin's badge mode.
pub fn badge_span(app: &App, label: &str, source: SourceId) -> Span<'static> {
    let color = match source {
        SourceId::Aur => app.palette.badge_aur,
        SourceId::Pacman if app.settings.collapse_repos => app.palette.badge_official,
        SourceId::Pacman => app.palette.badge_repo,
    };
    match app.skin.badge {
        BadgeMode::Brackets => Span::styled(format!("[{label}]"), Style::default().fg(color)),
        BadgeMode::Bare => Span::styled(label.to_string(), Style::default().fg(color)),
        BadgeMode::Chip => Span::styled(
            format!(" {label} "),
            Style::default().fg(app.palette.highlight_fg).bg(color),
        ),
    }
}

/// Icon accessors: the skin glyph when icons are enabled, else a portable
/// unicode fallback. They borrow from `app` so callers can format with them.
pub fn ic_package(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.package
    } else {
        ""
    }
}
pub fn ic_check(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.installed
    } else {
        "\u{2713}" // ✓
    }
}
pub fn ic_update(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.update
    } else {
        "\u{2191}" // ↑
    }
}
pub fn ic_running(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.running
    } else {
        "\u{25d0}" // ◐
    }
}
pub fn ic_success(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.success
    } else {
        "\u{2713}" // ✓
    }
}
pub fn ic_fail(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.fail
    } else {
        "\u{2717}" // ✗
    }
}
pub fn ic_search(app: &App) -> &str {
    if app.skin.icons.enabled {
        &app.skin.icons.search
    } else {
        "/"
    }
}

// --- overlays ----------------------------------------------------------------

fn draw_confirm(frame: &mut Frame, app: &App, area: Rect) {
    use crate::model::Action;
    let Some(spec) = &app.confirm else { return };
    let busy = matches!(
        app.task.as_ref().map(|t| &t.state),
        Some(crate::action::runner::TaskState::Running)
    );
    let cmd = format!("{} {}", spec.command.program, spec.command.args.join(" "));
    let headline = match spec.action {
        Action::Install => {
            format!("Install {} from {}", spec.targets.join(", "), spec.source_id.badge())
        }
        Action::Remove => format!("Remove {}", spec.targets.join(", ")),
        Action::Upgrade => format!("Upgrade {} packages", spec.targets.join(", ")),
    };
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            headline,
            Style::default().fg(app.palette.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("via: {cmd}"),
            Style::default().fg(app.palette.muted),
        )),
    ];
    if let Some(note) = &app.confirm_note {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            note.clone(),
            Style::default().fg(app.palette.warning),
        )));
    }
    if busy {
        if let Some(running) = &app.task {
            let ahead = app.queue.len() + 1; // running task + anything already queued
            lines.push(Line::from(Span::styled(
                format!(
                    "queues behind the running {} of {} (#{} in line)",
                    running.spec.action.verb(),
                    running.spec.targets.join(", "),
                    ahead + 1
                ),
                Style::default().fg(app.palette.muted),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[y] confirm   [n/esc] cancel",
        Style::default().fg(app.palette.muted),
    )));

    let title = format!(" confirm {} ", spec.action.verb());
    let width = lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(0) as u16
        + 4;
    let height = lines.len() as u16 + 2; // + borders
    let rect = centered_rect(width.max(66), height, area);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).block(themed_block(app, app.palette.warning, title)),
        rect,
    );
}

fn draw_options(frame: &mut Frame, app: &App, area: Rect) {
    let sel = app.options_selected;
    let check = |b: bool| if b { "[x]" } else { "[ ]" };
    let row = |selected: bool, text: String| -> Line<'static> {
        let marker = if selected {
            cursor_symbol(app)
        } else {
            "  ".to_string()
        };
        let style = if selected {
            highlight_style(app)
        } else {
            Style::default().fg(app.palette.fg)
        };
        Line::from(Span::styled(format!("{marker}{text}"), style))
    };

    let lines: Vec<Line> = vec![
        row(
            sel == 0,
            format!("{} Show hotkeys in status bar", check(app.settings.show_hotkeys)),
        ),
        row(
            sel == 1,
            format!("{} Group repos as [official]", check(app.settings.collapse_repos)),
        ),
        row(sel == 2, format!("    Palette: {}", app.settings.palette)),
        row(sel == 3, format!("    Skin: {}", app.settings.skin)),
        row(
            sel == 4,
            format!("    Search delay: {}ms", app.settings.debounce_ms),
        ),
        row(
            sel == 5,
            format!("    Remove depth: {}", app.settings.remove_depth.label()),
        ),
        row(sel == 6, format!("    AUR helper: {}", aur_helper_label(app))),
        Line::from(""),
        Line::from(Span::styled(
            "  \u{2191}\u{2193} move \u{b7} space toggle/cycle \u{b7} esc close",
            Style::default().fg(app.palette.muted),
        )),
    ];

    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = centered_rect(46, height, area);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).block(themed_block(app, app.palette.accent, " Options ")),
        rect,
    );
}

/// Label for the AUR-helper options row: the configured choice, plus the
/// resolved binary when it differs (a fallback) or a hint when none is installed.
fn aur_helper_label(app: &App) -> String {
    let setting = app.settings.aur_helper.label();
    match (app.helpers_available, &app.aur_helper_bin) {
        ((false, false), _) => format!("{setting} (none installed)"),
        (_, Some(bin)) if app.aur_helper_fell_back => format!("{setting} -> {bin}"),
        (_, Some(bin)) if setting == "auto" => format!("{setting} ({bin})"),
        _ => setting.to_string(),
    }
}

/// A centered rect `width` cols by `height` rows (clamped to `area`).
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect { x, y, width: w, height: h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ActiveView;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    // Render the whole UI into an in-memory buffer and return its text. Proves
    // the draw path runs without panicking under the new theming.
    fn render(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn welcome_screen_renders_with_theme_footer() {
        let app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        let text = render(&app);
        assert!(text.contains("plaza"));
        assert!(text.contains("palette:"));
        assert!(text.contains("skin:"));
    }

    #[test]
    fn options_overlay_renders_theme_rows() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.options_open = true;
        let text = render(&app);
        assert!(text.contains("Palette:"));
        assert!(text.contains("Skin:"));
    }

    #[test]
    fn manage_view_renders_without_panic() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.active_view = ActiveView::Manage;
        let _ = render(&app);
    }
}
