// Many model fields/types are consumed incrementally; allow during build-up.
#![allow(dead_code)]

mod action;
mod app;
mod event;
mod model;
mod search;
mod sources;
mod ui;

use crate::app::App;
use crate::event::AppEvent;
use crate::model::SourceId;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use tokio::process::Command;
use tokio::sync::mpsc::{self, UnboundedSender};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--search") {
        let term = args.get(pos + 1).cloned().unwrap_or_default();
        if term.is_empty() {
            eprintln!("usage: plaza --search <term>");
            std::process::exit(2);
        }
        return run_search_cli(&term).await;
    }
    run_tui().await
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));
}

async fn run_tui() -> anyhow::Result<()> {
    let sources = sources::detect_sources();
    if sources.is_empty() {
        eprintln!("plaza: no supported package sources detected (need pacman and/or yay)");
        std::process::exit(1);
    }
    let source_ids: Vec<SourceId> = sources.iter().map(|s| s.id()).collect();

    install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(source_ids);
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    spawn_input_task(tx.clone());

    terminal.draw(|f| ui::draw(f, &app))?;

    while let Some(ev) = rx.recv().await {
        handle_event(&mut app, ev, &tx);
        if app.should_quit {
            break;
        }
        terminal.draw(|f| ui::draw(f, &app))?;
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn spawn_input_task(tx: UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        while let Some(Ok(ev)) = reader.next().await {
            if tx.send(AppEvent::Input(ev)).is_err() {
                break;
            }
        }
    });
}

fn handle_event(app: &mut App, ev: AppEvent, _tx: &UnboundedSender<AppEvent>) {
    if let AppEvent::Input(Event::Key(key)) = ev {
        handle_key(app, key);
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }
    if let KeyCode::Char('q') = key.code {
        app.should_quit = true;
    }
}

// --- core --search CLI ---

async fn installed_index() -> sources::installed::InstalledIndex {
    match Command::new("pacman").arg("-Q").output().await {
        Ok(out) => sources::installed::InstalledIndex::from_query_output(
            &String::from_utf8_lossy(&out.stdout),
        ),
        Err(_) => sources::installed::InstalledIndex::default(),
    }
}

async fn run_search_cli(term: &str) -> anyhow::Result<()> {
    use crate::search::aggregator::{merge, relevance_sort};
    let sources = sources::detect_sources();
    if sources.is_empty() {
        eprintln!("plaza: no supported package sources detected (need pacman and/or yay)");
        std::process::exit(1);
    }
    let mut handles = Vec::new();
    for src in sources {
        let term = term.to_string();
        handles.push(tokio::spawn(async move {
            (src.display_name().to_string(), src.search(&term).await)
        }));
    }
    let mut all_hits = Vec::new();
    for h in handles {
        let (name, result) = h.await?;
        match result {
            Ok(hits) => all_hits.extend(hits),
            Err(e) => eprintln!("plaza: source '{name}' failed: {e}"),
        }
    }
    let idx = installed_index().await;
    let mut rows = merge(all_hits, &idx);
    relevance_sort(term, &mut rows);
    for row in &rows {
        let badges: Vec<&str> = row.providers.iter().map(|p| p.source_id.badge()).collect();
        let installed = if row.any_installed() { " [installed]" } else { "" };
        println!("{:<35} [{}]{}", row.name, badges.join(","), installed);
    }
    println!("\n{} packages", rows.len());
    Ok(())
}
