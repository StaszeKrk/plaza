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

    // The source counters are live search-hit counts; they mean nothing in the
    // Manage view, so show them only while in Search.
    for (id, state) in app.source_status.iter().filter(|_| app.active_view != ActiveView::Manage) {
        let (txt, col) = match state {
            SourceState::Loading => ("…".to_string(), pal.muted),
            SourceState::Done(n) => (n.to_string(), pal.fg),
            SourceState::Error => ("err".to_string(), pal.danger),
        };
        let badge_col = match *id {
            SourceId::Pacman => pal.badge_repo,
            SourceId::Aur => pal.badge_aur,
            SourceId::Flatpak => pal.badge_flatpak,
        };
        spans.push(Span::styled(format!("{} ", id.badge()), Style::default().fg(badge_col)));
        spans.push(Span::styled(format!("{txt}   "), Style::default().fg(col)));
    }

    if let Some(task) = &app.task {
        let verb = task.spec.action.verb();
        let what = task.spec.targets.join(",");
        let (txt, col) = match task.state {
            TaskState::Running if app.needs_input && app.focus != Focus::TaskPane => (
                format!("{} {what} waiting for input · press ` to answer", crate::ui::ic_running(app)),
                pal.accent,
            ),
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

    // A transient status message (e.g. a missing AUR helper) takes the place of
    // the hotkey hints until the next keypress.
    if let Some(msg) = &app.status_msg {
        spans.push(Span::styled(msg.clone(), Style::default().fg(pal.danger)));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    if app.settings.show_hotkeys {
        let manage = app.active_view == ActiveView::Manage;
        let keys = if !app.interacting {
            // navigate mode: moving the hovered panel
            "navigate · ↑↓←→ move · ⏎ focus · / search · f filter · ⇥ view · o opts · q quit"
        } else {
            match app.focus {
                Focus::Search if manage => "filter · type · ⏎ list · esc back",
                Focus::Search => "search · type · ⏎ results · esc back",
                Focus::Sidebar => "↑↓ pick view · ⏎ open · esc back",
                Focus::Main => match app.main_view {
                    MainView::Results => "↑↓ move · ⏎ detail · esc back",
                    MainView::Detail => "↑↓ source · ⏎ install · esc results",
                },
                Focus::List => "↑↓ move · ⏎ actions · r remove · u all · esc back",
                Focus::Filter => "↑↓ move · space toggle · s save default · f/esc close",
                Focus::TaskPane => "task pane · `=collapse",
            }
        };
        spans.push(Span::styled(keys.to_string(), Style::default().fg(pal.muted)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
