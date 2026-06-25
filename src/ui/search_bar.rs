use crate::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.is_active(Focus::Search);
    let border = crate::ui::border_color(app, Focus::Search);
    let pal = &app.palette;
    let prompt = crate::ui::ic_search(app);
    let cursor = if active { "\u{258f}" } else { "" }; // ▏
    let line = Line::from(vec![
        Span::styled(format!("{prompt} "), Style::default().fg(pal.accent)),
        Span::styled(app.search_text().to_string(), Style::default().fg(pal.fg)),
        Span::styled(cursor.to_string(), Style::default().fg(pal.accent)),
    ]);
    let p = Paragraph::new(line).block(crate::ui::themed_block(app, border, " Plaza "));
    frame.render_widget(p, area);
}
