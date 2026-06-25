use crate::action::runner::TaskState;
use crate::app::{ActiveView, App, Focus, MainView, SourceState};
use crate::model::SourceId;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];

    for (id, state) in &app.source_status {
        let (txt, col) = match state {
            SourceState::Loading => ("…".to_string(), pal.muted),
            SourceState::Done(n) => (n.to_string(), pal.fg),
            SourceState::Error => ("err".to_string(), pal.danger),
        };
        let badge_col = match *id {
            SourceId::Pacman => pal.badge_repo,
            SourceId::Aur => pal.badge_aur,
        };
        spans.push(Span::styled(format!("{} ", id.badge()), Style::default().fg(badge_col)));
        spans.push(Span::styled(format!("{txt}   "), Style::default().fg(col)));
    }

    if let Some(task) = &app.task {
        let verb = task.spec.action.verb();
        let what = task.spec.targets.join(",");
        let (txt, col) = match task.state {
            TaskState::Running => (
                format!("{} {what} {verb}ing… `=view", crate::ui::ic_running(app)),
                pal.warning,
            ),
            TaskState::Done { success: true, .. } => {
                (format!("{} {what} done", crate::ui::ic_success(app)), pal.success)
            }
            TaskState::Done { success: false, code } => (
                format!("{} {what} failed ({code})", crate::ui::ic_fail(app)),
                pal.danger,
            ),
        };
        spans.push(Span::styled(format!("{txt}   "), Style::default().fg(col)));
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
        spans.push(Span::styled(keys.to_string(), Style::default().fg(pal.muted)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
