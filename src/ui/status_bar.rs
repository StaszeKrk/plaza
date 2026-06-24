use crate::action::runner::TaskState;
use crate::app::{App, Focus, MainView, SourceState};
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
        let indicator = match task.state {
            TaskState::Running => {
                format!("◐ {} installing… `=view", task.spec.targets.join(","))
            }
            TaskState::Done { success: true, .. } => {
                format!("✓ {} done", task.spec.targets.join(","))
            }
            TaskState::Done { success: false, code } => {
                format!("✗ {} failed ({code})", task.spec.targets.join(","))
            }
        };
        parts.push(indicator);
    }

    let keys = match (app.focus, app.main_view) {
        (Focus::Search, _) => "type to search  ⏎/esc list",
        (_, MainView::Results) => "↑↓ move  ⏎ open  / search  q quit",
        (_, MainView::Detail) => "↑↓ source  ⏎ install  esc back  / search",
    };
    parts.push(keys.to_string());

    let line = format!(" {} ", parts.join("   "));
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
