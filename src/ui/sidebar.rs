use crate::app::App;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, _app: &App, area: Rect) {
    frame.render_widget(Block::default().borders(Borders::ALL).title(" plaza "), area);
}
