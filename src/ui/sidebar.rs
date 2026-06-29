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

    let head = |s: &str| {
        Line::from(Span::styled(
            s.to_string(),
            Style::default().fg(pal.section).add_modifier(Modifier::BOLD),
        ))
    };
    let views = ["Search", "Manage"];

    let cursor = crate::ui::cursor_symbol(app);
    // Combined "updates/installed" block. The header is two-tone and each row
    // shows "Y/X": Y (pending updates) in the update color, X (installed) muted,
    // so the slash notation reads by color match to the header.
    let combo_head = Line::from(vec![
        Span::styled(
            "UPDATES",
            Style::default().fg(pal.update).add_modifier(Modifier::BOLD),
        ),
        Span::styled("/", Style::default().fg(pal.muted).add_modifier(Modifier::BOLD)),
        Span::styled(
            "INSTALLED",
            Style::default().fg(pal.muted).add_modifier(Modifier::BOLD),
        ),
    ]);
    let yx_row = |label: &str, y: Option<usize>, x: usize, selected: bool| {
        let marker = if selected && active { cursor.clone() } else { "  ".to_string() };
        let ystr = y.map(|n| n.to_string()).unwrap_or_else(|| "\u{2014}".into()); // —
        let ycol = if y.unwrap_or(0) > 0 { pal.update } else { pal.muted };
        Line::from(vec![
            Span::styled(format!("{marker}{label:<7}"), Style::default().fg(pal.fg)),
            Span::styled(ystr, Style::default().fg(ycol)),
            Span::styled(format!("/{x}"), Style::default().fg(pal.muted)),
        ])
    };

    let mut stats = vec![combo_head];
    for (i, id) in app.present_sources().iter().enumerate() {
        let (label, y, x) = match id {
            crate::model::SourceId::Pacman => ("repo", app.updates.repo, app.stats.repo),
            crate::model::SourceId::Aur => ("aur", app.updates.aur, app.stats.foreign),
            crate::model::SourceId::Flatpak => ("flatpak", app.updates.flatpak, app.stats.flatpak),
        };
        stats.push(yx_row(label, y, x, app.sidebar_selected == i));
    }
    stats.push(yx_row(
        "total",
        app.total_updates(),
        app.stats.total(),
        app.sidebar_selected == app.sidebar_total_row(),
    ));
    // Only when live update counts are unavailable, hint at the fix. The "—"
    // values already signal the missing state, so this stays to one muted line.
    if !app.has_checkupdates {
        stats.push(Line::from(Span::styled(
            " pacman-contrib".to_string(),
            Style::default().fg(pal.muted),
        )));
    }

    // VIEWS nav (anchored at the bottom so it is always visible).
    let mut nav = vec![head("VIEWS")];
    let active_idx = app.active_view.index();
    for (i, v) in views.iter().enumerate() {
        let marker = if app.sidebar_upgrade_rows() + i == app.sidebar_selected && active {
            cursor.clone()
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
    if active && app.settings.show_hotkeys {
        nav.push(Line::from(Span::styled(
            " \u{23ce} run/switch".to_string(), // ⏎
            Style::default().fg(pal.muted),
        )));
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
