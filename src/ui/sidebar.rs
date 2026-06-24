use crate::app::{App, Focus};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Sidebar;
    let border = if focused { Color::Cyan } else { Color::DarkGray };

    let upd = |o: Option<usize>| o.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
    let views = ["Search", "Installed", "Updates"];

    let mut lines = vec![
        Line::from(span_bold("INSTALLED")),
        Line::from(format!(" repo   {:>6}", app.stats.repo)),
        Line::from(format!(" aur    {:>6}", app.stats.foreign)),
        Line::from(format!(" total  {:>6}", app.stats.total())),
        Line::from(""),
        Line::from(span_bold("UPDATES")),
        Line::from(format!(" repo   {:>6}", upd(app.updates.repo))),
        Line::from(format!(" aur    {:>6}", upd(app.updates.aur))),
        Line::from(""),
        Line::from(span_bold("VIEWS")),
    ];
    let active = app.active_view.index();
    for (i, v) in views.iter().enumerate() {
        let marker = if i == app.sidebar_selected && focused { "▸ " } else { "  " };
        // A dot marks the view the center area is currently showing.
        let active_mark = if i == active { "•" } else { " " };
        let style = if i == active {
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{active_mark}{v}"),
            style,
        )));
    }

    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(" plaza "),
    );
    frame.render_widget(p, area);
}

fn span_bold(s: &str) -> Span<'static> {
    Span::styled(
        s.to_string(),
        Style::default().add_modifier(Modifier::BOLD).fg(Color::Gray),
    )
}
