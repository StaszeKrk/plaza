use crate::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.is_active(Focus::Sidebar);
    let border = crate::ui::border_color(app, Focus::Sidebar);
    let pal = &app.palette;

    let head = |s: &str| {
        Line::from(Span::styled(
            s.to_string(),
            Style::default().fg(pal.section).add_modifier(Modifier::BOLD),
        ))
    };
    let fgl = |s: String| Line::from(Span::styled(s, Style::default().fg(pal.fg)));
    let upd = |o: Option<usize>| o.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
    let upd_line = |label: &str, o: Option<usize>| {
        let col = if o.unwrap_or(0) > 0 { pal.update } else { pal.fg };
        Line::from(Span::styled(
            format!(" {label:<6} {:>6}", upd(o)),
            Style::default().fg(col),
        ))
    };
    let views = ["Search", "Manage"];

    let mut lines = vec![
        head("INSTALLED"),
        fgl(format!(" repo   {:>6}", app.stats.repo)),
        fgl(format!(" aur    {:>6}", app.stats.foreign)),
        fgl(format!(" total  {:>6}", app.stats.total())),
        Line::from(""),
        head("UPDATES"),
        upd_line("repo", app.updates.repo),
        upd_line("aur", app.updates.aur),
        Line::from(""),
        head("VIEWS"),
    ];
    let active_idx = app.active_view.index();
    for (i, v) in views.iter().enumerate() {
        let marker = if i == app.sidebar_selected && active {
            crate::ui::cursor_symbol(app)
        } else {
            "  ".to_string()
        };
        // A dot marks the view the center area is currently showing.
        let active_mark = if i == active_idx { "•" } else { " " };
        let style = if i == active_idx {
            Style::default().add_modifier(Modifier::BOLD).fg(pal.accent)
        } else {
            Style::default().fg(pal.fg)
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{active_mark}{v}"),
            style,
        )));
    }

    let p = Paragraph::new(lines).block(crate::ui::themed_block(app, border, " plaza "));
    frame.render_widget(p, area);
}
