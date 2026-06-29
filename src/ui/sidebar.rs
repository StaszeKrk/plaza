use crate::app::{App, Focus};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.is_active(Focus::Sidebar);
    let border = crate::ui::border_color(app, Focus::Sidebar);
    let pal = &app.palette;
    let flatpak = app.present_sources().contains(&crate::model::SourceId::Flatpak);

    let head = |s: &str| {
        Line::from(Span::styled(
            s.to_string(),
            Style::default().fg(pal.section).add_modifier(Modifier::BOLD),
        ))
    };
    // Aligned "label   value" row (label padded to 7, value right-aligned to 5).
    let stat = |label: &str, n: usize| {
        Line::from(Span::styled(
            format!(" {label:<7}{n:>5}"),
            Style::default().fg(pal.fg),
        ))
    };
    let upd = |o: Option<usize>| o.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
    let upd_line = |label: &str, o: Option<usize>| {
        let col = if o.unwrap_or(0) > 0 { pal.update } else { pal.fg };
        Line::from(Span::styled(
            format!(" {label:<7}{:>5}", upd(o)),
            Style::default().fg(col),
        ))
    };
    let views = ["Search", "Manage"];

    // Stats block (top, clipped on a short sidebar before the nav is).
    let mut stats = vec![head("INSTALLED"), stat("repo", app.stats.repo), stat("aur", app.stats.foreign)];
    if flatpak {
        stats.push(stat("flatpak", app.stats.flatpak));
    }
    stats.extend([
        stat("total", app.stats.total()),
        head("UPDATES"),
        upd_line("repo", app.updates.repo),
        upd_line("aur", app.updates.aur),
    ]);
    if flatpak {
        stats.push(upd_line("flatpak", app.updates.flatpak));
    }

    // VIEWS nav (anchored at the bottom so it is always visible).
    let mut nav = vec![head("VIEWS")];
    let active_idx = app.active_view.index();
    for (i, v) in views.iter().enumerate() {
        let marker = if i == app.sidebar_selected && active {
            crate::ui::cursor_symbol(app)
        } else {
            "  ".to_string()
        };
        let active_mark = if i == active_idx { "•" } else { " " };
        let style = if i == active_idx {
            Style::default().add_modifier(Modifier::BOLD).fg(pal.accent)
        } else {
            Style::default().fg(pal.fg)
        };
        nav.push(Line::from(Span::styled(format!("{marker}{active_mark}{v}"), style)));
    }

    let block = crate::ui::themed_block(app, border, " plaza ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let nav_h = nav.len() as u16;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(nav_h)])
        .split(inner);
    frame.render_widget(Paragraph::new(stats), chunks[0]);
    frame.render_widget(Paragraph::new(nav), chunks[1]);
}
