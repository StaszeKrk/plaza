use crate::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let Some(row) = app.selected_row() else {
        frame.render_widget(Block::default().borders(Borders::ALL).title(" detail "), area);
        return;
    };
    let focused = app.focus == Focus::Main;
    let border = if focused { Color::Cyan } else { Color::DarkGray };

    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("‹ {}", row.name),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(row.best_description.clone()),
        Line::from(""),
    ]);

    let items: Vec<ListItem> = row
        .providers
        .iter()
        .map(|p| {
            let inst = if p.installed {
                format!("✓ {}", p.installed_version.as_deref().unwrap_or(""))
            } else {
                String::new()
            };
            let notes = if let Some(votes) = p.meta.votes {
                let m = if p.meta.maintained { "maintained" } else { "orphaned" };
                let ood = if p.meta.out_of_date { " · out-of-date" } else { "" };
                format!("{votes} votes · {m}{ood}")
            } else if let Some(repo) = &p.meta.repo {
                format!("official · {repo}")
            } else {
                String::new()
            };
            ListItem::new(format!(
                "{:<6} {:<14} {:<12} {}",
                p.source_id.badge(),
                p.version,
                inst,
                notes
            ))
        })
        .collect();

    let inner = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(" detail · ⏎ install · esc back ");
    let list_area = inner.inner(area);
    frame.render_widget(inner, area);

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(0),
        ])
        .split(list_area);
    frame.render_widget(header, chunks[0]);

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸ ");
    let mut state = ListState::default();
    if !row.providers.is_empty() {
        state.select(Some(app.detail_selected.min(row.providers.len() - 1)));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);
}
