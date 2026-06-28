use crate::app::{App, FilterId, Focus};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// The repo-filter box: a checkbox per repo (plus a pacman master and `aur`),
/// toggled with space. Per-repo rows indent under the master when expanded.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.is_active(Focus::Filter);
    let border = crate::ui::border_color(app, Focus::Filter);
    let pal = &app.palette;

    let rows = app.filter_checkboxes();
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let cursor = if i == app.active_filter().selected && active {
                crate::ui::cursor_symbol(app)
            } else {
                "  ".to_string()
            };
            let check = if row.checked { "[x]" } else { "[ ]" };
            let indent = if matches!(row.id, FilterId::Repo(_)) { "  " } else { "" };
            let style = if i == app.active_filter().selected && active {
                crate::ui::highlight_style(app)
            } else {
                Style::default().fg(pal.fg)
            };
            Line::from(Span::styled(
                format!("{cursor}{check} {indent}{}", row.label),
                style,
            ))
        })
        .collect();

    let p = Paragraph::new(lines).block(crate::ui::themed_block(app, border, " filter "));
    frame.render_widget(p, area);
}
