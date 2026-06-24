use crate::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Search;
    let border = if focused { Color::Cyan } else { Color::DarkGray };
    let cursor = if focused { "▏" } else { "" };
    let p = Paragraph::new(format!("/ {}{}", app.query, cursor)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(" Plaza "),
    );
    frame.render_widget(p, area);
}
