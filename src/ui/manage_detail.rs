use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// The Manage detail pane: `pacman -Qi` info for the highlighted installed
/// package. Shows `loading...` until the async fetch arrives, nothing if the
/// list is empty.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let border = crate::ui::border_color(app, crate::app::Focus::List);

    let Some(pkg) = app.selected_installed() else {
        let p = Paragraph::new("").block(crate::ui::themed_block(app, border, " detail "));
        frame.render_widget(p, area);
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
            let join = |v: &[String]| if v.is_empty() { "(none)".to_string() } else { v.join("  ") };
            let reason = if d.explicit { "explicitly installed" } else { "dependency" };
            let mut out = vec![
                Line::from(Span::styled(d.name.clone(), Style::default().fg(pal.accent))),
                Line::from(Span::styled(d.description.clone(), Style::default().fg(pal.fg))),
                Line::from(Span::styled(d.url.clone(), Style::default().fg(pal.muted))),
                Line::from(""),
                field("version:", &d.version),
                field("reason:", reason),
                field("installed:", &d.install_date),
                field("built:", &d.build_date),
                field("size:", &d.size),
                field("required by:", &join(&d.required_by)),
                field("optional for:", &join(&d.optional_for)),
                field("depends on:", &join(&d.depends)),
            ];
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

    let title = format!(" {} ", pkg.name);
    let p = Paragraph::new(lines).block(crate::ui::themed_block(app, border, title));
    frame.render_widget(p, area);
}
