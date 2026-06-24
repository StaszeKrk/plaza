use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, _app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(" ↑↓ move  ⏎ open  / search  q quit ")
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
