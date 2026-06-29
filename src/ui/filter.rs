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
    let mut sel_line = 0usize; // rendered line index of the cursor row (for scroll)
    for (i, row) in rows.iter().enumerate() {
        // A "reason" sub-heading once, before the first reason (radio) row.
        if matches!(row.id, FilterId::Reason(_)) && !reason_header {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(" reason", Style::default().fg(pal.muted))));
            reason_header = true;
        }
        // The save action gets a blank separator above it.
        if matches!(row.id, FilterId::SaveDefault) {
            lines.push(Line::from(""));
        }
        if i == sel {
            sel_line = lines.len();
        }
        let selected = i == sel && active;
        let cursor = if selected { crate::ui::cursor_symbol(app) } else { "  ".to_string() };
        let line = if matches!(row.id, FilterId::SaveDefault) {
            // Action row, not a checkbox.
            let style = if selected {
                crate::ui::highlight_style(app)
            } else {
                Style::default().fg(pal.accent)
            };
            Line::from(Span::styled(format!("{cursor}» {}", row.label), style))
        } else {
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
            Line::from(Span::styled(format!("{cursor}{mark} {indent}{}", row.label), style))
        };
        lines.push(line);
    }

    // Scroll so the cursor row stays visible when the box is taller than its area.
    let inner_h = area.height.saturating_sub(2) as usize; // top + bottom border
    let offset = if inner_h > 0 && sel_line >= inner_h {
        (sel_line + 1 - inner_h) as u16
    } else {
        0
    };
    let p = Paragraph::new(lines)
        .scroll((offset, 0))
        .block(crate::ui::themed_block(app, border, " filter "));
    frame.render_widget(p, area);
}
