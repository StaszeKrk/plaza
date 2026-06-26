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
    let last_line = task
        .parser
        .screen()
        .contents()
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string();

    let mut body = format!("{}\n{}", task.spec.targets.join(", "), last_line);
    if !app.queue.is_empty() {
        body.push_str(&format!("\n\nqueued ({}):", app.queue.len()));
        for (i, spec) in app.queue.iter().enumerate() {
            let marker = if i == app.queue_selected { "▸" } else { " " };
            body.push_str(&format!("\n{marker} {} {}", spec.action.verb(), spec.targets.join(", ")));
        }
    }
    if app.queue_paused {
        body.push_str("\n\nqueue paused after failure");
    }

    // Context hints depend on whether the task finished and whether work is queued.
    let queued = !app.queue.is_empty();
    let hint = match (done, queued) {
        (true, true) => "⏎ continue · x clear queue · j/k·d edit",
        (true, false) => "⏎ close",
        (false, true) => "⏎ view · esc hide · j/k·d edit · x clear",
        (false, false) => "⏎ view · esc hide",
    };
    body.push_str(&format!("\n\n{hint}"));

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
    let hint = if done {
        if app.queue.is_empty() { "esc/q close" } else { "esc/q continue · x clear" }
    } else {
        "^C cancel · esc/` peek"
    };
    frame.render_widget(Clear, area);
    let block = crate::ui::themed_block(app, color, format!("{title}  ·  {hint}"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let term = PseudoTerminal::new(task.parser.screen());
    frame.render_widget(term, inner);
}

/// Present-tense label for the running action shown in the pane title.
fn running_verb(task: &ActiveTask) -> &'static str {
    match task.spec.action {
        crate::model::Action::Install => "installing",
        crate::model::Action::Remove => "removing",
        crate::model::Action::Upgrade => "upgrading",
    }
}

fn status_title(app: &App, task: &ActiveTask) -> (String, Color) {
    match task.state {
        TaskState::Running => (
            format!(" {} {} ", crate::ui::ic_running(app), running_verb(task)),
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
