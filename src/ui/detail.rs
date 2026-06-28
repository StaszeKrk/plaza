use crate::app::{App, Focus};
use crate::model::{days_ago, dep_pkg_name, PackageDetail, Provider, SourceId};
use crate::sources::installed::InstalledIndex;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// How many dependency names to list before eliding the rest with "…".
const MAX_DEPS_SHOWN: usize = 8;

/// A "depends:"/"optional:" line: each dep is its bare package name, colored
/// green when already installed so you can spot what you have at a glance.
/// `None` when there are no deps to show.
fn deps_line(
    label: &str,
    deps: &[String],
    installed: &InstalledIndex,
    pal: &crate::theme::palette::Palette,
) -> Option<Line<'static>> {
    if deps.is_empty() {
        return None;
    }
    let muted = Style::default().fg(pal.muted);
    let green = Style::default().fg(pal.installed);
    let mut spans = vec![Span::styled(format!("  {label} "), muted)];
    for (i, dep) in deps.iter().take(MAX_DEPS_SHOWN).enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ", muted));
        }
        let name = dep_pkg_name(dep);
        let style = if installed.is_installed(name) { green } else { muted };
        spans.push(Span::styled(name.to_string(), style));
    }
    if deps.len() > MAX_DEPS_SHOWN {
        spans.push(Span::styled(" …", muted));
    }
    Some(Line::from(spans))
}

/// Indented sub-lines under a provider row: size/build date (pacman), recency
/// and popularity (AUR), plus shared dependencies and license. Only fields that
/// are present render, so the rows fill in as fetched detail arrives.
fn detail_lines(
    p: &Provider,
    detail: Option<&PackageDetail>,
    now: i64,
    installed: &InstalledIndex,
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
        lines.extend(deps_line("depends:", &d.depends, installed, pal));
        lines.extend(deps_line("optional:", &d.optional_depends, installed, pal));
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
    // Package-level installed state: pacman cannot say which repo it came from,
    // only whether it is foreign (AUR) or official, so state that once here
    // rather than implying a specific source for every provider below.
    if let Some(iv) = app.installed.version(&row.name) {
        let foreign = app
            .installed_list
            .iter()
            .find(|ip| ip.name == row.name)
            .map(|ip| ip.origin == "aur");
        let origin = match foreign {
            Some(true) => " · aur",
            Some(false) => " · official",
            None => "",
        };
        header_lines.push(Line::from(Span::styled(
            format!("installed {iv}{origin}"),
            Style::default().fg(pal.installed),
        )));
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
            // One line per provider: the selection bar then highlights a single
            // row cleanly. The selected provider's detail renders below the list.
            ListItem::new(Line::from(spans))
        })
        .collect();

    // Detail of the selected provider (rendered un-highlighted under the list).
    let selected = app.detail_selected.min(providers.len().saturating_sub(1));
    let detail_body: Vec<Line> = providers
        .get(selected)
        .map(|p| {
            let detail = app.details.get(&p.detail_key(&row.name));
            detail_lines(p, detail, now, &app.installed, pal)
        })
        .unwrap_or_default();

    let detail_title =
        if app.settings.show_hotkeys { " detail · ⏎ install · esc back " } else { " detail " };
    let inner = crate::ui::themed_block(app, border, detail_title);
    let list_area = inner.inner(area);
    frame.render_widget(inner, area);

    // Cap the provider list height so the detail pane always keeps room; the
    // list scrolls if a package somehow has more providers than that.
    let list_height = (providers.len() as u16).clamp(1, 6);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(list_height),
            Constraint::Min(0),
        ])
        .split(list_area);
    frame.render_widget(header, chunks[0]);

    let list = List::new(items)
        .highlight_style(crate::ui::highlight_style(app))
        .highlight_symbol(&cursor);
    let mut state = ListState::default();
    if !providers.is_empty() {
        state.select(Some(selected));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);

    frame.render_widget(Paragraph::new(detail_body), chunks[2]);
}
