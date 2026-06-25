use crate::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.is_active(Focus::Search);
    let border = if active {
        Color::Cyan
    } else if app.is_hovered(Focus::Search) {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let cursor = if active { "▏" } else { "" };
    let p = Paragraph::new(format!("/ {}{}", app.search_text(), cursor)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(" Plaza "),
    );
    frame.render_widget(p, area);
}
