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
    let sel = app.active_filter().selected;
    let mut lines: Vec<Line> = Vec::new();
    let mut reason_header = false;
    for (i, row) in rows.iter().enumerate() {
        // A "reason" sub-heading once, before the first reason (radio) row.
        if matches!(row.id, FilterId::Reason(_)) && !reason_header {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(" reason", Style::default().fg(pal.muted))));
            reason_header = true;
        }
        let selected = i == sel && active;
        let cursor = if selected { crate::ui::cursor_symbol(app) } else { "  ".to_string() };
        // Reasons are mutually exclusive, so show them as radios, not checkboxes.
        let mark = match row.id {
            FilterId::Reason(_) => if row.checked { "(•)" } else { "( )" },
            _ => if row.checked { "[x]" } else { "[ ]" },
        };
        let indent = if matches!(row.id, FilterId::Repo(_)) { "  " } else { "" };
        let style = if selected {
            crate::ui::highlight_style(app)
        } else {
            Style::default().fg(pal.fg)
        };
        lines.push(Line::from(Span::styled(
            format!("{cursor}{mark} {indent}{}", row.label),
            style,
        )));
    }

    let p = Paragraph::new(lines).block(crate::ui::themed_block(app, border, " filter "));
    frame.render_widget(p, area);
}
