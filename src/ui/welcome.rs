//! The branded welcome / hero screen shown in the main pane before any search.
//! It also advertises the active theme and where to customize it, which is how
//! theming stays discoverable without cluttering the Options menu.

use crate::app::App;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const WORDMARK: [&str; 6] = [
    "██████╗ ██╗      █████╗ ███████╗ █████╗ ",
    "██╔══██╗██║     ██╔══██╗╚══███╔╝██╔══██╗",
    "██████╔╝██║     ███████║  ███╔╝ ███████║",
    "██╔═══╝ ██║     ██╔══██║ ███╔╝  ██╔══██║",
    "██║     ███████╗██║  ██║███████╗██║  ██║",
    "╚═╝     ╚══════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝",
];

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let accent = Style::default().fg(pal.accent).add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(pal.muted);

    let hint = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(k.to_string(), accent),
            Span::styled(format!("  {d}"), muted),
        ])
    };

    let mut lines: Vec<Line> = vec![Line::from("")];
    for w in WORDMARK {
        lines.push(Line::from(Span::styled(w.to_string(), accent)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "search pacman + the AUR, together",
        muted,
    )));
    lines.push(Line::from(""));
    lines.push(hint("/", "search"));
    lines.push(hint("⇥", "switch view"));
    lines.push(hint("o", "options"));
    lines.push(hint("q", "quit"));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(app.theme_footer(), muted)));

    let para = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(crate::ui::themed_block(
            app,
            crate::ui::border_color(app, crate::app::Focus::Main),
            " plaza ",
        ));
    frame.render_widget(para, area);
}
