use crate::app::{App, Focus};
use crate::model::{days_ago, PackageDetail, Provider, SourceId};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// How many dependency names to list before eliding the rest with "…".
const MAX_DEPS_SHOWN: usize = 8;

/// Indented sub-lines under a provider row: size/build date (pacman), recency
/// and popularity (AUR), plus shared dependencies and license. Only fields that
/// are present render, so the rows fill in as fetched detail arrives.
fn detail_lines(
    p: &Provider,
    detail: Option<&PackageDetail>,
    now: i64,
    pal: &crate::theme::palette::Palette,
) -> Vec<Line<'static>> {
    let muted = Style::default().fg(pal.muted);
    let mut lines = Vec::new();

    let mut head = Vec::new();
    if p.source_id == SourceId::Aur {
        // "updated N days ago" comes from meta.last_modified (always present);
        // popularity and maintainer come from the fetched detail.
        if let Some(lm) = p.meta.last_modified {
            let d = days_ago(lm, now);
            head.push(if d == 0 {
                "updated today".to_string()
            } else {
                format!("updated {d}d ago")
            });
        }
        if let Some(pop) = detail.and_then(|d| d.popularity) {
            head.push(format!("popularity {pop:.2}"));
        }
    } else {
        if let Some(sz) = detail.and_then(|d| d.install_size.as_deref()) {
            head.push(format!("size {sz}"));
        }
        if let Some(bd) = detail.and_then(|d| d.build_date.as_deref()) {
            head.push(format!("built {bd}"));
        }
    }
    if !head.is_empty() {
        lines.push(Line::from(Span::styled(format!("  {}", head.join(" · ")), muted)));
    }

    if let Some(d) = detail {
        if let Some(m) = &d.maintainer {
            lines.push(Line::from(Span::styled(format!("  by {m}"), muted)));
        }
        if let Some(u) = &d.repo_url {
            lines.push(Line::from(Span::styled(format!("  {u}"), Style::default().fg(pal.accent))));
        }
        if !d.depends.is_empty() {
            let shown = d.depends.iter().take(MAX_DEPS_SHOWN).cloned().collect::<Vec<_>>().join(" ");
            let more = if d.depends.len() > MAX_DEPS_SHOWN { " …" } else { "" };
            lines.push(Line::from(Span::styled(format!("  depends: {shown}{more}"), muted)));
        }
        if let Some(lic) = &d.licenses {
            lines.push(Line::from(Span::styled(format!("  license: {lic}"), muted)));
        }
    }
    lines
}

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

    let providers = app.effective_providers(row);
    // The first pacman provider is the highest-priority repo = default.
    let default_idx = providers
        .iter()
        .position(|p| p.source_id == SourceId::Pacman);

    // Project homepage: prefer the default provider's, else the first that has one.
    let url = providers
        .get(default_idx.unwrap_or(0))
        .or_else(|| providers.first())
        .and_then(|p| app.details.get(&p.detail_key(&row.name)))
        .and_then(|d| d.url.clone())
        .or_else(|| {
            providers
                .iter()
                .filter_map(|p| app.details.get(&p.detail_key(&row.name)))
                .find_map(|d| d.url.clone())
        });

    let mut header_lines = vec![
        Line::from(Span::styled(
            format!("‹ {}", row.name),
            Style::default().fg(pal.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            row.best_description.clone(),
            Style::default().fg(pal.fg),
        )),
    ];
    if let Some(u) = &url {
        header_lines.push(Line::from(Span::styled(u.clone(), Style::default().fg(pal.accent))));
    }
    header_lines.push(Line::from(""));
    let header_height = header_lines.len() as u16;
    let header = Paragraph::new(header_lines);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

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
            let mut lines = vec![Line::from(spans)];
            let detail = app.details.get(&p.detail_key(&row.name));
            lines.extend(detail_lines(p, detail, now, pal));
            ListItem::new(lines)
        })
        .collect();

    let inner = crate::ui::themed_block(app, border, " detail · ⏎ install · esc back ");
    let list_area = inner.inner(area);
    frame.render_widget(inner, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
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
