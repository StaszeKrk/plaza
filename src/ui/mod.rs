use crate::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // search bar
            Constraint::Min(0),    // body
            Constraint::Length(1), // status bar
        ])
        .split(area);

    draw_search_bar(frame, app, vchunks[0]);
    draw_body(frame, app, vchunks[1]);
    draw_status_bar(frame, app, vchunks[2]);
}

fn draw_search_bar(frame: &mut Frame, app: &App, area: Rect) {
    let text = format!("/ {}", app.query);
    let p = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Plaza "));
    frame.render_widget(p, area);
}

fn draw_body(frame: &mut Frame, _app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(0)])
        .split(area);

    let sidebar = Block::default().borders(Borders::ALL).title(" sidebar ");
    let main = Block::default().borders(Borders::ALL).title(" results ");
    frame.render_widget(sidebar, chunks[0]);
    frame.render_widget(main, chunks[1]);
}

fn draw_status_bar(frame: &mut Frame, _app: &App, area: Rect) {
    let p = Paragraph::new(" ↑↓ move  ⏎ open  / search  q quit ")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(p, area);
}
