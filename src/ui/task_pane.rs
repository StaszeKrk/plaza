use crate::app::App;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

pub fn draw_peek(frame: &mut Frame, _app: &App, area: Rect) {
    frame.render_widget(Block::default().borders(Borders::ALL).title(" task "), area);
}

pub fn draw_overlay(frame: &mut Frame, _app: &App, _area: Rect) {
    let _ = frame; // full implementation in Task 8
}
