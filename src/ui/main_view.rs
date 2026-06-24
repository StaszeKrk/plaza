use crate::app::{ActiveView, App, Focus, MainView};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    match app.active_view {
        ActiveView::Search => match app.main_view {
            MainView::Results => draw_results(frame, app, area),
            MainView::Detail => crate::ui::detail::draw(frame, app, area),
        },
        ActiveView::Installed => draw_installed(frame, app, area),
        ActiveView::Updates => draw_updates(frame, app, area),
    }
}

fn draw_installed(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Main;
    let border = if focused { Color::Cyan } else { Color::DarkGray };

    let items: Vec<ListItem> = app
        .installed_list
        .iter()
        .map(|p| ListItem::new(format!("{:<32} {}", truncate(&p.name, 32), p.version)))
        .collect();

    let title = format!(" installed ({}) · ⏎ remove ", app.installed_list.len());
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border))
                .title(title),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !app.installed_list.is_empty() {
        state.select(Some(app.installed_selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_updates(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Main;
    let border = if focused { Color::Cyan } else { Color::DarkGray };

    let items: Vec<ListItem> = app
        .updates_list
        .iter()
        .map(|u| {
            ListItem::new(format!(
                "{:<28} {} → {}",
                truncate(&u.name, 28),
                u.old_version,
                u.new_version
            ))
        })
        .collect();

    let title = format!(" updates ({}) · ⏎ upgrade all ", app.updates_list.len());
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border))
                .title(title),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !app.updates_list.is_empty() {
        state.select(Some(app.updates_selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Main;
    let border = if focused { Color::Cyan } else { Color::DarkGray };

    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| {
            let shown = app.effective_providers(row);
            let badges: String = shown
                .iter()
                .map(|p| format!("[{}]", app.provider_badge(p)))
                .collect();
            let installed = if row.any_installed() { " ✓" } else { "" };
            let ver = shown
                .first()
                .map(|p| p.version.as_str())
                .or_else(|| row.providers.first().map(|p| p.version.as_str()))
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
