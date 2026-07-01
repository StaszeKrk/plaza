use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// The Manage detail pane: `pacman -Qi` info for the highlighted installed
/// package. Rendered as the right half of the Manage box, separated from the list
/// by a single vertical divider (no box of its own). Shows `loading...` until the
/// async fetch arrives, nothing if the list is empty.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let border = crate::ui::border_color(app, crate::app::Focus::List);

    // A left-edge divider that matches the skin (none for borderless skins).
    let divider = Block::default()
        .borders(app.skin.border.borders() & Borders::LEFT)
        .border_set(app.skin.border.set())
        .border_style(Style::default().fg(border));
    let inner = divider.inner(area);
    frame.render_widget(divider, area);
    let area = inner;

    let Some(pkg) = app.selected_installed() else {
        return;
    };

    let lines: Vec<Line> = match app.manage_detail.get(&pkg.name) {
        None => vec![Line::from(Span::styled("loading...", Style::default().fg(pal.muted)))],
        Some(d) => {
            let field = |k: &str, v: &str| -> Line<'static> {
                Line::from(vec![
                    Span::styled(format!("{k:<13}"), Style::default().fg(pal.muted)),
                    Span::styled(v.to_string(), Style::default().fg(pal.fg)),
                ])
            };
            let reason = if d.explicit { "explicitly installed" } else { "dependency" };
            // Header, then only the fields that actually have a value. Empty ones
            // (e.g. apt has no build date or "optional for") drop their whole line
            // instead of showing a blank label or "(none)".
            let mut out = vec![Line::from(Span::styled(
                d.name.clone(),
                Style::default().fg(pal.accent),
            ))];
            if !d.description.is_empty() {
                out.push(Line::from(Span::styled(d.description.clone(), Style::default().fg(pal.fg))));
            }
            if !d.url.is_empty() {
                out.push(Line::from(Span::styled(d.url.clone(), Style::default().fg(pal.muted))));
            }
            out.push(Line::from(""));
            out.push(field("reason:", reason));
            for (k, v) in [
                ("version:", &d.version),
                ("installed:", &d.install_date),
                ("built:", &d.build_date),
                ("size:", &d.size),
            ] {
                if !v.is_empty() {
                    out.push(field(k, v));
                }
            }
            for (k, v) in [
                ("required by:", &d.required_by),
                ("optional for:", &d.optional_for),
                ("depends on:", &d.depends),
            ] {
                if !v.is_empty() {
                    out.push(field(k, &v.join("  ")));
                }
            }
            if pkg.orphan {
                out.push(Line::from(""));
                out.push(Line::from(Span::styled(
                    "orphan: nothing requires this",
                    Style::default().fg(pal.update),
                )));
            }
            out
        }
    };

    // Pad one column so the text does not hug the divider.
    let body = Rect { x: area.x + 1, width: area.width.saturating_sub(1), ..area };
    frame.render_widget(Paragraph::new(lines), body);
}
