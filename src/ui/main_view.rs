use crate::app::{ActiveView, App, Focus, MainView};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    match app.active_view {
        ActiveView::Search => match app.main_view {
            MainView::Results => draw_results(frame, app, area),
            MainView::Detail => crate::ui::detail::draw(frame, app, area),
        },
        ActiveView::Manage => draw_manage(frame, app, area),
    }
}

/// The Manage view: a scope-chip box on top (upgrade per source or All), and an
/// installed-package list below with upgradable packages floated to the top.
fn draw_manage(frame: &mut Frame, app: &App, area: Rect) {
    let scope_active = app.is_active(Focus::Scope);
    let scope_border = block_color(app, Focus::Scope);
    let list_border = block_color(app, Focus::List);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // --- upgrade-scope chips: [ All N ] [ repo N ] [ aur N ] ---
    let mut spans: Vec<Span> = Vec::new();
    for i in 0..app.upgrade_scope_count() {
        let chip = format!(" {} {} ", app.upgrade_scope_label(i), app.upgrade_scope_pending(i));
        let style = if i == app.upgrade_scope_selected && scope_active {
            Style::default().add_modifier(Modifier::REVERSED)
        } else if i == app.upgrade_scope_selected {
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!("[{chip}]"), style));
        spans.push(Span::raw(" "));
    }
    let scope_title = if app.has_checkupdates {
        " upgrade · h/l scope · ⏎ run ".to_string()
    } else {
        " upgrade · h/l · ⏎ run · (install pacman-contrib for live counts) ".to_string()
    };
    let chips = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(scope_border))
            .title(scope_title),
    );
    frame.render_widget(chips, chunks[0]);

    // --- installed list (updates floated to top, filtered by the search bar) ---
    let rows = app.manage_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|p| {
            let mut spans = vec![
                Span::raw(format!("{:<28} ", truncate(&p.name, 28))),
                Span::styled(
                    format!("{:<16} ", truncate(&p.version, 16)),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            match app.update_for(&p.name) {
                Some(nv) => spans.push(Span::styled(
                    format!("↑{:<13} ", truncate(nv, 13)),
                    Style::default().fg(Color::Green),
                )),
                None => spans.push(Span::raw(format!("{:<15} ", ""))),
            }
            spans.push(Span::styled(
                format!("[{}]", p.origin),
                Style::default().fg(Color::Blue),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if app.manage_filter.is_empty() {
        format!(" installed ({}) · ⏎/r remove ", rows.len())
    } else {
        format!(
            " installed ({}/{}) · filter:'{}' ",
            rows.len(),
            app.installed_list.len(),
            app.manage_filter
        )
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(list_border))
                .title(title),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(app.installed_selected.min(rows.len() - 1)));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

/// Border color for a panel: cyan when active (interacting), yellow when only
/// hovered (navigate mode), dim otherwise.
pub fn block_color(app: &App, f: Focus) -> Color {
    if app.is_active(f) {
        Color::Cyan
    } else if app.is_hovered(f) {
        Color::Yellow
    } else {
        Color::DarkGray
    }
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let border = block_color(app, Focus::Main);

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
