use crate::action::runner::{ActiveTask, TaskState};
use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;

pub fn draw_peek(frame: &mut Frame, app: &App, area: Rect) {
    let Some(task) = &app.task else { return };
    let (title, color) = status_title(app, task);
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
    let p = Paragraph::new(body)
        .style(Style::default().fg(app.palette.fg))
        .wrap(Wrap { trim: true })
        .block(crate::ui::themed_block(app, color, title));
    frame.render_widget(p, area);
}

pub fn draw_overlay(frame: &mut Frame, app: &App, area: Rect) {
    let Some(task) = &app.task else { return };
    let (title, color) = status_title(app, task);
    let done = matches!(task.state, TaskState::Done { .. });
    let hint = if done { "esc/q close" } else { "^C cancel · esc/` peek" };
    frame.render_widget(Clear, area);
    let block = crate::ui::themed_block(app, color, format!("{title}  ·  {hint}"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let term = PseudoTerminal::new(task.parser.screen());
    frame.render_widget(term, inner);
}

fn status_title(app: &App, task: &ActiveTask) -> (String, Color) {
    match task.state {
        TaskState::Running => (
            format!(" {} installing ", crate::ui::ic_running(app)),
            app.palette.warning,
        ),
        TaskState::Done { success: true, .. } => (
            format!(" {} done ", crate::ui::ic_success(app)),
            app.palette.success,
        ),
        TaskState::Done { success: false, code } => (
            format!(" {} failed ({code}) ", crate::ui::ic_fail(app)),
            app.palette.danger,
        ),
    }
}
