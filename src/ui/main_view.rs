use crate::app::{ActiveView, App, Focus, MainView};
use crate::model::{HighlightMode, SourceId};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
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

/// The Manage view: installed-package list with upgradable packages floated to
/// the top, filtered by the search bar.
fn draw_manage(frame: &mut Frame, app: &App, area: Rect) {
    let list_border = crate::ui::border_color(app, Focus::List);
    let pal = &app.palette;

    // --- installed list (updates floated to top, filtered by the search bar) ---
    let rows = app.manage_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|pk| {
            let mut spans = name_cell(
                app.manage_label(pk),
                &app.manage_filter,
                Style::default().fg(pal.fg),
                app.settings.highlight,
                pal.accent,
            );
            spans.push(Span::styled(
                format!("{:<16} ", truncate(&pk.version, 16)),
                Style::default().fg(pal.muted),
            ));
            match app.update_for(&pk.name) {
                Some(nv) => spans.push(Span::styled(
                    format!("{}{:<13} ", crate::ui::ic_update(app), truncate(nv, 13)),
                    Style::default().fg(pal.update),
                )),
                None => spans.push(Span::raw(format!("{:<15} ", ""))),
            }
            // pacman cannot tell which repo a native package came from, only that
            // it is foreign (AUR) or not, so badge just official vs aur (or flatpak).
            let (label, src) = match pk.origin.as_str() {
                "aur" => ("aur", SourceId::Aur),
                "flatpak" => ("flatpak", SourceId::Flatpak),
                _ => ("official", SourceId::Pacman),
            };
            spans.push(crate::ui::badge_span(app, label, src));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let reason = match app.manage_reason {
        crate::model::ReasonFilter::All => String::new(),
        r => format!("· {} ", r.label()),
    };
    let hints = if app.settings.show_hotkeys { "· \u{23ce} upgrade/remove · r remove · u all " } else { "" };
    let title = if app.manage_filter.is_empty() {
        format!(" installed ({}) {reason}{hints}", rows.len())
    } else {
        format!(
            " installed ({}/{}) {reason}· filter:'{}' ",
            rows.len(),
            app.installed_list.len(),
            app.manage_filter
        )
    };
    let cursor = crate::ui::cursor_symbol(app);
    // One outer box titled with the list header; the list and the detail pane sit
    // inside it as two parts separated by a vertical divider (drawn by the pane).
    let outer = crate::ui::themed_block(app, list_border, title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let list = List::new(items)
        .highlight_style(crate::ui::highlight_style(app))
        .highlight_symbol(&cursor);
    let mut state = ListState::default();
    *state.offset_mut() = app.manage_offset.get();
    if !rows.is_empty() {
        state.select(Some(app.installed_selected.min(rows.len() - 1)));
    }
    // On a narrow terminal there is no room for the detail part, so the list fills
    // the box.
    if inner.width >= 80 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Min(0)])
            .split(inner);
        frame.render_stateful_widget(list, cols[0], &mut state);
        crate::ui::manage_detail::draw(frame, app, cols[1]);
    } else {
        frame.render_stateful_widget(list, inner, &mut state);
    }
    // Persist the offset ratatui adjusted so the next frame keeps the viewport.
    app.manage_offset.set(state.offset());
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let border = crate::ui::border_color(app, Focus::Main);
    let pal = &app.palette;
    let pkg_icon = crate::ui::ic_package(app);
    let cursor = crate::ui::cursor_symbol(app);

    let rows = app.search_rows();
    let items: Vec<ListItem> = rows
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
            spans.extend(name_cell(
                &row.name,
                &app.query,
                Style::default().fg(name_color),
                app.settings.highlight,
                pal.accent,
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
        .block(crate::ui::themed_block(app, border, format!(" results ({}) ", rows.len())))
        .highlight_style(crate::ui::highlight_style(app))
        .highlight_symbol(&cursor);

    let mut state = ListState::default();
    *state.offset_mut() = app.results_offset.get();
    if !rows.is_empty() {
        state.select(Some(app.results_selected.min(rows.len() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
    app.results_offset.set(state.offset());
}

/// The package-name cell for a list: the name padded to 28 chars, with the part
/// matching `query` styled per `mode` so the eye finds it in substring matches.
fn name_cell(
    name: &str,
    query: &str,
    base: Style,
    mode: HighlightMode,
    accent: Color,
) -> Vec<Span<'static>> {
    let shown = truncate(name, 28);
    let pad = " ".repeat(28usize.saturating_sub(shown.chars().count()) + 1);
    let range = if mode == HighlightMode::Off {
        None
    } else {
        crate::search::aggregator::match_range(&shown, query)
    };
    match range {
        Some((s, e)) => {
            let hi = match mode {
                HighlightMode::Color => base.fg(accent),
                HighlightMode::Underline => base.add_modifier(Modifier::UNDERLINED),
                HighlightMode::Both => base.fg(accent).add_modifier(Modifier::UNDERLINED),
                HighlightMode::Off => base,
            };
            vec![
                Span::styled(shown[..s].to_string(), base),
                Span::styled(shown[s..e].to_string(), hi),
                Span::styled(format!("{}{}", &shown[e..], pad), base),
            ]
        }
        None => vec![Span::styled(format!("{shown}{pad}"), base)],
    }
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
