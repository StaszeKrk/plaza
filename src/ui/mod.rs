use crate::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
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
}

fn draw_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let Some(spec) = &app.confirm else { return };
    let rect = centered_rect(60, 7, area);
    let cmd = format!("{} {}", spec.command.program, spec.command.args.join(" "));
    let text = format!(
        "Install {} from {}\nvia: {}\n\n[y] confirm   [n/esc] cancel",
        spec.targets.join(", "),
        spec.source_id.badge(),
        cmd
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" confirm install "),
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
