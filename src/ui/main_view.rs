use crate::app::{ActiveView, App, Focus, MainView};
use crate::model::SourceId;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    match app.active_view {
        ActiveView::Search => match app.main_view {
            MainView::Results => {
                // Empty search + no results: the branded welcome / hero screen.
                if app.query.is_empty() && app.rows.is_empty() {
                    crate::ui::welcome::draw(frame, app, area);
                } else {
                    draw_results(frame, app, area);
                }
            }
            MainView::Detail => crate::ui::detail::draw(frame, app, area),
        },
        ActiveView::Manage => draw_manage(frame, app, area),
    }
}

/// The Manage view: a scope-chip box on top (upgrade per source or All), and an
/// installed-package list below with upgradable packages floated to the top.
fn draw_manage(frame: &mut Frame, app: &App, area: Rect) {
    let scope_active = app.is_active(Focus::Scope);
    let scope_border = crate::ui::border_color(app, Focus::Scope);
    let list_border = crate::ui::border_color(app, Focus::List);
    let pal = &app.palette;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // --- upgrade-scope chips: [ All N ] [ repo N ] [ aur N ] ---
    let mut spans: Vec<Span> = Vec::new();
    for i in 0..app.upgrade_scope_count() {
        let chip = format!(" {} {} ", app.upgrade_scope_label(i), app.upgrade_scope_pending(i));
        let style = if i == app.upgrade_scope_selected && scope_active {
            crate::ui::highlight_style(app)
        } else if i == app.upgrade_scope_selected {
            Style::default().add_modifier(Modifier::BOLD).fg(pal.accent)
        } else {
            Style::default().fg(pal.muted)
        };
        spans.push(Span::styled(format!("[{chip}]"), style));
        spans.push(Span::raw(" "));
    }
    let scope_title = if app.has_checkupdates {
        " upgrade · h/l scope · ⏎ run ".to_string()
    } else {
        " upgrade · h/l · ⏎ run · (install pacman-contrib for live counts) ".to_string()
    };
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(crate::ui::themed_block(app, scope_border, scope_title)),
        chunks[0],
    );

    // --- installed list (updates floated to top, filtered by the search bar) ---
    let rows = app.manage_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|pk| {
            let mut spans = vec![
                Span::styled(format!("{:<28} ", truncate(&pk.name, 28)), Style::default().fg(pal.fg)),
                Span::styled(
                    format!("{:<16} ", truncate(&pk.version, 16)),
                    Style::default().fg(pal.muted),
                ),
            ];
            match app.update_for(&pk.name) {
                Some(nv) => spans.push(Span::styled(
                    format!("{}{:<13} ", crate::ui::ic_update(app), truncate(nv, 13)),
                    Style::default().fg(pal.update),
                )),
                None => spans.push(Span::raw(format!("{:<15} ", ""))),
            }
            let src = if pk.origin == "aur" { SourceId::Aur } else { SourceId::Pacman };
            spans.push(crate::ui::badge_span(app, &pk.origin, src));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if app.manage_filter.is_empty() {
        format!(" installed ({}) · ⏎/r remove · u upgrade ", rows.len())
    } else {
        format!(
            " installed ({}/{}) · filter:'{}' ",
            rows.len(),
            app.installed_list.len(),
            app.manage_filter
        )
    };
    let cursor = crate::ui::cursor_symbol(app);
    let list = List::new(items)
        .block(crate::ui::themed_block(app, list_border, title))
        .highlight_style(crate::ui::highlight_style(app))
        .highlight_symbol(&cursor);

    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(app.installed_selected.min(rows.len() - 1)));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let border = crate::ui::border_color(app, Focus::Main);
    let pal = &app.palette;
    let pkg_icon = crate::ui::ic_package(app);
    let cursor = crate::ui::cursor_symbol(app);

    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| {
            let shown = app.effective_providers(row);
            let ver = shown
                .first()
                .map(|p| p.version.as_str())
                .or_else(|| row.providers.first().map(|p| p.version.as_str()))
                .unwrap_or("");
            let mut spans: Vec<Span> = Vec::new();
            if !pkg_icon.is_empty() {
                spans.push(Span::styled(format!("{pkg_icon} "), Style::default().fg(pal.muted)));
            }
            // Installed packages show their name green wherever they appear.
            let name_color = if row.any_installed() { pal.installed } else { pal.fg };
            spans.push(Span::styled(
                format!("{:<28} ", truncate(&row.name, 28)),
                Style::default().fg(name_color),
            ));
            for prov in &shown {
                spans.push(crate::ui::badge_span(app, app.provider_badge(prov), prov.source_id));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(ver.to_string(), Style::default().fg(pal.muted)));
            if row.any_installed() {
                spans.push(Span::styled(
                    format!("  {}", crate::ui::ic_check(app)),
                    Style::default().fg(pal.installed),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(crate::ui::themed_block(app, border, format!(" results ({}) ", app.rows.len())))
        .highlight_style(crate::ui::highlight_style(app))
        .highlight_symbol(&cursor);

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
