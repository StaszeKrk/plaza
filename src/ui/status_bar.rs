use crate::action::runner::TaskState;
use crate::app::{ActiveView, App, Focus, MainView, SourceState};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let mut parts: Vec<String> = Vec::new();

    for (id, state) in &app.source_status {
        let s = match state {
            SourceState::Loading => "…".to_string(),
            SourceState::Done(n) => n.to_string(),
            SourceState::Error => "err".to_string(),
        };
        parts.push(format!("{} {}", id.badge(), s));
    }

    if let Some(task) = &app.task {
        let verb = task.spec.action.verb();
        let what = task.spec.targets.join(",");
        let indicator = match task.state {
            TaskState::Running => format!("◐ {what} {verb}ing… `=view"),
            TaskState::Done { success: true, .. } => format!("✓ {what} done"),
            TaskState::Done { success: false, code } => format!("✗ {what} failed ({code})"),
        };
        parts.push(indicator);
    }

    if app.settings.show_hotkeys {
        let keys = match app.focus {
            Focus::Search => "type to search  ⏎/esc list",
            _ => match app.active_view {
                ActiveView::Search => match app.main_view {
                    MainView::Results => "↑↓ move  ⏎ open  / search  o options  q quit",
                    MainView::Detail => "↑↓ source  ⏎ install  esc back  o options",
                },
                ActiveView::Installed => "↑↓ move  ⏎ remove  / search  o options  q quit",
                ActiveView::Updates => "↑↓ move  ⏎ upgrade all  / search  o options  q quit",
            },
        };
        parts.push(keys.to_string());
    }

    let line = format!(" {} ", parts.join("   "));
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
