use crate::app::{App, Focus};
use crate::model::SourceId;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let Some(row) = app.selected_row() else {
        frame.render_widget(
            crate::ui::themed_block(app, app.palette.border_idle, " detail "),
            area,
        );
        return;
    };
    let pal = &app.palette;
    let border = crate::ui::border_color(app, Focus::Main);

    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("‹ {}", row.name),
            Style::default().fg(pal.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            row.best_description.clone(),
            Style::default().fg(pal.fg),
        )),
        Line::from(""),
    ]);

    let providers = app.effective_providers(row);
    // The first pacman provider is the highest-priority repo = default.
    let default_idx = providers
        .iter()
        .position(|p| p.source_id == SourceId::Pacman);

    let cursor = crate::ui::cursor_symbol(app);
    let items: Vec<ListItem> = providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut spans = vec![
                crate::ui::badge_span(app, app.provider_badge(p), p.source_id),
                Span::raw("  "),
                Span::styled(format!("{:<14} ", p.version), Style::default().fg(pal.fg)),
            ];
            if p.installed {
                spans.push(Span::styled(
                    format!(
                        "{} {}  ",
                        crate::ui::ic_check(app),
                        p.installed_version.as_deref().unwrap_or("")
                    ),
                    Style::default().fg(pal.installed),
                ));
            }
            let (note, col) = if let Some(votes) = p.meta.votes {
                let m = if p.meta.maintained { "maintained" } else { "orphaned" };
                if p.meta.out_of_date {
                    (format!("{votes} votes · {m} · out-of-date"), pal.danger)
                } else {
                    (format!("{votes} votes · {m}"), pal.muted)
                }
            } else if Some(i) == default_idx {
                ("official · default".to_string(), pal.muted)
            } else if p.meta.repo.is_some() {
                ("official".to_string(), pal.muted)
            } else {
                (String::new(), pal.muted)
            };
            if !note.is_empty() {
                spans.push(Span::styled(note, Style::default().fg(col)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let inner = crate::ui::themed_block(app, border, " detail · ⏎ install · esc back ");
    let list_area = inner.inner(area);
    frame.render_widget(inner, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(list_area);
    frame.render_widget(header, chunks[0]);

    let list = List::new(items)
        .highlight_style(crate::ui::highlight_style(app))
        .highlight_symbol(&cursor);
    let mut state = ListState::default();
    if !providers.is_empty() {
        state.select(Some(app.detail_selected.min(providers.len() - 1)));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);
}
