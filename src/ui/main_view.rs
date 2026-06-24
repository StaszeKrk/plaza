use crate::app::{App, Focus, MainView};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    match app.main_view {
        MainView::Results => draw_results(frame, app, area),
        MainView::Detail => crate::ui::detail::draw(frame, app, area),
    }
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Main;
    let border = if focused { Color::Cyan } else { Color::DarkGray };

    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| {
            let badges: String = row.providers.iter().map(|p| format!("[{}]", p.badge())).collect();
            let installed = if row.any_installed() { " ✓" } else { "" };
            let ver = row
                .providers
                .first()
                .map(|p| p.version.as_str())
                .unwrap_or("");
            ListItem::new(format!(
                "{:<28} {} {}{}",
                truncate(&row.name, 28),
                badges,
                ver,
                installed
            ))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border))
                .title(format!(" results ({}) ", app.rows.len())),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !app.rows.is_empty() {
        state.select(Some(app.results_selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
