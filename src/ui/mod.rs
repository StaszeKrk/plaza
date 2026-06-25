use crate::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub mod detail;
pub mod main_view;
pub mod search_bar;
pub mod sidebar;
pub mod status_bar;
pub mod task_pane;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
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
    let mut lines: Vec<String> = vec![headline, format!("via: {}", cmd)];
    if let Some(note) = &app.confirm_note {
        lines.push(String::new());
        lines.push(note.clone());
    }
    if busy {
        if let Some(running) = &app.task {
            lines.push(format!(
                "⚠ cancels the running {} of {}",
                running.spec.action.verb(),
                running.spec.targets.join(", ")
            ));
        }
    }
    lines.push(String::new());
    lines.push("[y] confirm   [n/esc] cancel".to_string());

    let title = format!(" confirm {} ", spec.action.verb());
    let height = lines.len() as u16 + 2; // + borders
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16 + 4;
    let rect = centered_rect(width.max(66), height, area);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines.join("\n")).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(title),
        ),
        rect,
    );
}

fn draw_options(frame: &mut Frame, app: &App, area: Rect) {
    let sel = app.options_selected;
    let check = |b: bool| if b { "[x]" } else { "[ ]" };
    let row = |selected: bool, text: String| -> Line<'static> {
        let marker = if selected { "▸ " } else { "  " };
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
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
        row(
            sel == 2,
            format!("    Search delay: {}ms", app.settings.debounce_ms),
        ),
        row(
            sel == 3,
            format!("    Remove depth: {}", app.settings.remove_depth.label()),
        ),
        Line::from(""),
        Line::from(Span::styled(
            "  ↑↓ move · space toggle/cycle · esc close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = centered_rect(46, height, area);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Options "),
        ),
        rect,
    );
}

/// A centered rect `width` cols by `height` rows (clamped to `area`).
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect { x, y, width: w, height: h }
}
