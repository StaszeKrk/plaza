use crate::action::runner::{ActiveTask, TaskState};
use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;

pub fn draw_peek(frame: &mut Frame, app: &App, area: Rect) {
    let Some(task) = &app.task else { return };
    let (title, color) = status_title(task);
    let done = matches!(task.state, TaskState::Done { .. });
    let hint = if done { "⏎ view · esc close" } else { "⏎ view · esc hide" };
    let last_line = task
        .parser
        .screen()
        .contents()
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string();
    let body = format!("{}\n{}\n\n{}", task.spec.targets.join(", "), last_line, hint);
    let p = Paragraph::new(body).wrap(Wrap { trim: true }).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(title),
    );
    frame.render_widget(p, area);
}

pub fn draw_overlay(frame: &mut Frame, app: &App, area: Rect) {
    let Some(task) = &app.task else { return };
    let (title, color) = status_title(task);
    let done = matches!(task.state, TaskState::Done { .. });
    let hint = if done { "esc/q close" } else { "^C cancel · esc/` peek" };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(format!("{title}  ·  {hint}"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let term = PseudoTerminal::new(task.parser.screen());
    frame.render_widget(term, inner);
}

fn status_title(task: &ActiveTask) -> (String, Color) {
    match task.state {
        TaskState::Running => (" ◐ installing ".to_string(), Color::Yellow),
        TaskState::Done { success: true, .. } => (" ✓ done ".to_string(), Color::Green),
        TaskState::Done { success: false, code } => {
            (format!(" ✗ failed ({code}) "), Color::Red)
        }
    }
}
