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
        let manage = app.active_view == ActiveView::Manage;
        let keys = if !app.interacting {
            // navigate mode: moving the hovered panel
            "navigate · ↑↓←→ move · ⏎ focus · / search · ⇥ view · o opts · q quit"
        } else {
            match app.focus {
                Focus::Search if manage => "filter · type · ⏎ list · esc back",
                Focus::Search => "search · type · ⏎ results · esc back",
                Focus::Sidebar => "↑↓ pick view · ⏎ open · esc back",
                Focus::Main => match app.main_view {
                    MainView::Results => "↑↓ move · ⏎ detail · esc back",
                    MainView::Detail => "↑↓ source · ⏎ install · esc results",
                },
                Focus::Scope => "h/l scope · ⏎ upgrade · esc back",
                Focus::List => "↑↓ move · ⏎/r remove · esc back",
                Focus::TaskPane => "task pane · `=collapse",
            }
        };
        parts.push(keys.to_string());
    }

    let line = format!(" {} ", parts.join("   "));
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
