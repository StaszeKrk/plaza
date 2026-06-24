// Many model fields/types are consumed incrementally; allow during build-up.
#![allow(dead_code)]

mod action;
mod app;
mod event;
mod model;
mod search;
mod sources;
mod ui;

use crate::app::{App, Focus};
use crate::event::AppEvent;
use crate::model::SourceId;
use crate::sources::Source;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::sync::Arc;
use std::time::Duration;
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
    let detected = sources::detect_sources();
    if detected.is_empty() {
        eprintln!("plaza: no supported package sources detected (need pacman and/or yay)");
        std::process::exit(1);
    }
    let sources: Vec<Arc<dyn Source>> = detected.into_iter().map(Arc::from).collect();
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
    spawn_stats_tasks(tx.clone());

    terminal.draw(|f| ui::draw(f, &app))?;
    while let Some(ev) = rx.recv().await {
        handle_event(&mut app, ev, &tx, &sources);
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

fn handle_event(
    app: &mut App,
    ev: AppEvent,
    tx: &UnboundedSender<AppEvent>,
    sources: &[Arc<dyn Source>],
) {
    match ev {
        AppEvent::Input(Event::Key(key)) => handle_key(app, key, tx),
        AppEvent::DispatchSearch { gen } => {
            if gen == app.debounce_gen && !app.query.is_empty() {
                let query_id = app.start_query(app.query.clone());
                dispatch_search(app.query.clone(), query_id, sources, tx);
            }
        }
        AppEvent::SearchHits { query_id, source_id, hits } => {
            app.apply_search_results(query_id, source_id, hits);
        }
        AppEvent::SearchError { query_id, source_id } => {
            app.set_source_error(query_id, source_id);
        }
        AppEvent::Stats(s) => app.stats = s,
        AppEvent::Updates(u) => app.updates = u,
        AppEvent::Installed(idx) => app.installed = idx,
        _ => {}
    }
}

fn spawn_stats_tasks(tx: UnboundedSender<AppEvent>) {
    // installed counts + index
    let tx_inst = tx.clone();
    tokio::spawn(async move {
        let repo = run_count(&["-Qnq"]).await;
        let foreign = run_count(&["-Qmq"]).await;
        let _ = tx_inst.send(AppEvent::Stats(crate::model::InstalledStats { repo, foreign }));

        if let Ok(out) = Command::new("pacman").arg("-Q").output().await {
            let idx = sources::installed::InstalledIndex::from_query_output(
                &String::from_utf8_lossy(&out.stdout),
            );
            let _ = tx_inst.send(AppEvent::Installed(idx));
        }
    });

    // best-effort update counts
    tokio::spawn(async move {
        let repo = repo_update_count().await;
        let aur = aur_update_count().await;
        let _ = tx.send(AppEvent::Updates(crate::model::UpdatesInfo { repo, aur }));
    });
}

async fn run_count(args: &[&str]) -> usize {
    match Command::new("pacman").args(args).output().await {
        Ok(out) => sources::installed::count_lines(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => 0,
    }
}

async fn repo_update_count() -> Option<usize> {
    // Prefer `checkupdates` (safe, no root); fall back to `pacman -Qu`.
    if sources::which("checkupdates") {
        if let Ok(out) = Command::new("checkupdates").output().await {
            return Some(sources::updates::parse_update_count(
                &String::from_utf8_lossy(&out.stdout),
            ));
        }
    }
    match Command::new("pacman").arg("-Qu").output().await {
        Ok(out) => Some(sources::updates::parse_update_count(
            &String::from_utf8_lossy(&out.stdout),
        )),
        Err(_) => None,
    }
}

async fn aur_update_count() -> Option<usize> {
    if !sources::which("yay") {
        return None;
    }
    match Command::new("yay").arg("-Qua").output().await {
        Ok(out) => Some(sources::updates::parse_update_count(
            &String::from_utf8_lossy(&out.stdout),
        )),
        Err(_) => None,
    }
}

fn dispatch_search(
    query: String,
    query_id: u64,
    sources: &[Arc<dyn Source>],
    tx: &UnboundedSender<AppEvent>,
) {
    for src in sources {
        let src = Arc::clone(src);
        let tx = tx.clone();
        let query = query.clone();
        tokio::spawn(async move {
            let source_id = src.id();
            match src.search(&query).await {
                Ok(hits) => {
                    let _ = tx.send(AppEvent::SearchHits { query_id, source_id, hits });
                }
                Err(_) => {
                    let _ = tx.send(AppEvent::SearchError { query_id, source_id });
                }
            }
        });
    }
}

fn schedule_debounced_search(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    app.debounce_gen += 1;
    let gen = app.debounce_gen;
    let tx = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = tx.send(AppEvent::DispatchSearch { gen });
    });
}

fn handle_key(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    if app.focus == Focus::Search {
        match key.code {
            KeyCode::Char(c) => {
                app.query.push(c);
                schedule_debounced_search(app, tx);
            }
            KeyCode::Backspace => {
                app.query.pop();
                schedule_debounced_search(app, tx);
            }
            KeyCode::Esc | KeyCode::Down | KeyCode::Enter => app.focus = Focus::Main,
            _ => {}
        }
        return;
    }

    use crate::app::MainView;
    match app.main_view {
        MainView::Results => match key.code {
            KeyCode::Char('/') => app.focus = Focus::Search,
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
            KeyCode::Enter => {
                if app.selected_row().is_some() {
                    app.detail_selected = 0;
                    app.main_view = MainView::Detail;
                }
            }
            _ => {}
        },
        MainView::Detail => match key.code {
            KeyCode::Char('/') => app.focus = Focus::Search,
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Esc => app.main_view = MainView::Results,
            KeyCode::Char('j') | KeyCode::Down => {
                let n = app.selected_row().map(|r| r.providers.len()).unwrap_or(0);
                if n > 0 {
                    app.detail_selected = (app.detail_selected + 1).min(n - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.detail_selected = app.detail_selected.saturating_sub(1);
            }
            // Enter (install) is wired in Task 8.
            _ => {}
        },
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
