// Many model fields/types are consumed incrementally; allow during build-up.
#![allow(dead_code)]

mod action;
mod app;
mod config;
mod event;
mod model;
mod search;
mod sources;
mod ui;

use crate::action::runner::{key_to_bytes, start_action, TaskState};
use crate::app::{ActiveView, App, Dir, Focus, MainView, TaskView};
use crate::event::AppEvent;
use crate::model::{Action, ActionSpec, SourceId};
use crate::sources::Source;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::io::Write as _;
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
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
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
    // Ask the terminal (kitty keyboard protocol, e.g. ghostty) to report key
    // event types so held-key auto-repeat (Repeat) is distinguishable from a
    // real Press. Queried before the input task starts reading stdin.
    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(source_ids);
    // `checkupdates` (pacman-contrib) syncs a private db so update counts stay
    // live without root; without it we fall back to a stale `pacman -Qu`.
    app.has_checkupdates = sources::which("checkupdates");
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

    if enhanced {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
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
        // Accept presses and auto-repeat (so holding a key keeps building the
        // query); ignore only key-release events. Whether results show mid-hold
        // is governed by the debounce delay (settings.debounce_ms), which the
        // user can raise past their terminal's key-repeat delay.
        AppEvent::Input(Event::Key(key)) if key.kind != KeyEventKind::Release => {
            handle_key(app, key, tx)
        }
        AppEvent::Input(Event::Resize(w, h)) => {
            if let Some(task) = &mut app.task {
                let (rows, cols) = expanded_pty_size(w, h);
                task.resize(rows, cols);
            }
        }
        AppEvent::DispatchSearch { gen } => {
            // Only run if still on the Search tab with a non-empty query (a
            // pending debounce must not pull you back from Manage).
            if gen == app.debounce_gen
                && app.active_view == ActiveView::Search
                && !app.query.is_empty()
            {
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
        AppEvent::InstalledList(list) => {
            app.installed_list = list;
            app.clamp_installed();
        }
        AppEvent::UpdatesList(list) => {
            app.updates_list = list;
            app.clamp_installed();
        }
        AppEvent::PtyOutput { id, bytes } => {
            let watching = app.focus == Focus::TaskPane && app.task_view == TaskView::Expanded;
            if let Some(task) = &mut app.task {
                if task.id == id {
                    task.parser.process(&bytes);
                    if !watching {
                        task.has_unseen_output = true;
                    }
                }
            }
        }
        AppEvent::ActionFinished { id, success, code } => {
            let mut matched = false;
            if let Some(task) = &mut app.task {
                if task.id == id {
                    task.state = TaskState::Done { success, code };
                    matched = true;
                }
            }
            // Refresh stats + installed index after the current action completes.
            if matched {
                spawn_stats_tasks(tx.clone());
            }
        }
        _ => {}
    }
}

/// PTY size matching the expanded task-pane's inner area: the body is the
/// terminal minus the search bar (3) and status bar (1); the overlay block
/// borders take 1 row/col on each side.
fn expanded_pty_size(term_cols: u16, term_rows: u16) -> (u16, u16) {
    let rows = term_rows.saturating_sub(6).max(4);
    let cols = term_cols.saturating_sub(2).max(20);
    (rows, cols)
}

fn begin_install(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    let Some(spec) = app.confirm.take() else { return };
    app.confirm_note = None;
    // Cancel/replace any prior task: dropping it closes the PTY, so an abandoned
    // child (e.g. one still waiting at the sudo prompt) gets a hangup and exits.
    app.task = None;
    app.task_seq += 1;
    let id = app.task_seq;
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let (rows, cols) = expanded_pty_size(cols, rows);
    match start_action(spec, id, rows, cols, tx.clone()) {
        Ok(task) => {
            app.task = Some(task);
            app.task_view = TaskView::Expanded;
            app.focus = Focus::TaskPane;
        }
        Err(e) => {
            eprintln!("plaza: failed to start install: {e}");
        }
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

    // full installed list (native + foreign) with origin, for the Manage view
    let tx_list = tx.clone();
    tokio::spawn(async move {
        let text = |out: std::io::Result<std::process::Output>| {
            out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
        };
        let native = text(Command::new("pacman").arg("-Qn").output().await);
        let foreign = text(Command::new("pacman").arg("-Qm").output().await);
        let sl = text(Command::new("pacman").arg("-Sl").output().await);
        let repos = sources::installed::parse_sync_repos(&sl);
        let list = sources::installed::parse_installed_list(&native, &foreign, &repos);
        let _ = tx_list.send(AppEvent::InstalledList(list));
    });

    // best-effort update counts + list (repos and AUR)
    tokio::spawn(async move {
        let repo_text = repo_update_text().await;
        let aur_text = aur_update_text().await;
        let repo = repo_text
            .as_deref()
            .map(sources::updates::parse_update_count);
        let aur = aur_text.as_deref().map(sources::updates::parse_update_count);
        let _ = tx.send(AppEvent::Updates(crate::model::UpdatesInfo { repo, aur }));

        let mut list = Vec::new();
        if let Some(t) = &repo_text {
            list.extend(sources::updates::parse_update_list(t, SourceId::Pacman));
        }
        if let Some(t) = &aur_text {
            list.extend(sources::updates::parse_update_list(t, SourceId::Aur));
        }
        let _ = tx.send(AppEvent::UpdatesList(list));
    });
}

const RECENT_AUR_DAYS: i64 = 7;

/// Warn if a selected AUR provider's PKGBUILD changed within the recency window
/// (the AUR has seen malware pushed via edited PKGBUILDs).
fn aur_recency_note(provider: &model::Provider) -> Option<String> {
    if provider.source_id != SourceId::Aur {
        return None;
    }
    let lm = provider.meta.last_modified?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let days = model::days_ago(lm, now);
    if days <= RECENT_AUR_DAYS {
        let when = if days == 0 { "today".to_string() } else { format!("{days}d ago") };
        Some(format!(
            "⚠ AUR PKGBUILD changed {when} — review it before installing"
        ))
    } else {
        None
    }
}

async fn run_count(args: &[&str]) -> usize {
    match Command::new("pacman").args(args).output().await {
        Ok(out) => sources::installed::count_lines(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => 0,
    }
}

/// Raw repo-update output (`name old -> new` lines). Prefers `checkupdates`
/// (safe, no root); falls back to `pacman -Qu`. None if neither runs.
async fn repo_update_text() -> Option<String> {
    if sources::which("checkupdates") {
        if let Ok(out) = Command::new("checkupdates").output().await {
            return Some(String::from_utf8_lossy(&out.stdout).into_owned());
        }
    }
    match Command::new("pacman").arg("-Qu").output().await {
        Ok(out) => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        Err(_) => None,
    }
}

/// Raw AUR-update output from `yay -Qua`, or None when yay is absent.
async fn aur_update_text() -> Option<String> {
    if !sources::which("yay") {
        return None;
    }
    match Command::new("yay").arg("-Qua").output().await {
        Ok(out) => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
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
    let delay = app.settings.debounce_ms;
    let tx = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        let _ = tx.send(AppEvent::DispatchSearch { gen });
    });
}

fn handle_key(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    // Options overlay captures input first.
    if app.options_open {
        handle_options_key(app, key);
        return;
    }

    // Confirm modal next.
    if app.confirm.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => begin_install(app, tx),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.clear_confirm(),
            _ => {}
        }
        return;
    }

    // Backtick: show+expand the task pane, or collapse an expanded one to a peek.
    if key.code == KeyCode::Char('`') {
        toggle_task_pane(app);
        return;
    }

    // The focused task pane owns input — including Ctrl-C, which forwards to the
    // child as SIGINT to cancel a running install (rather than quitting Plaza).
    if app.focus == Focus::TaskPane {
        handle_task_pane_key(app, key);
        return;
    }

    // Elsewhere, Ctrl-C quits.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    // Tab / Shift-Tab toggle between the Search and Manage views.
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        app.toggle_view();
        if app.active_view == ActiveView::Manage {
            spawn_stats_tasks(tx.clone());
        }
        return;
    }

    // `/` jumps straight to the search field (unless you are already typing).
    if key.code == KeyCode::Char('/') && !(app.focus == Focus::Search && app.interacting) {
        app.focus = Focus::Search;
        app.interacting = true;
        return;
    }

    // Two modes: navigate (move the hovered panel) and interact (act inside the
    // focused panel). Enter/Space activates; Esc steps back out.
    if app.interacting {
        handle_interact_key(app, key, tx);
    } else {
        handle_navigate_key(app, key);
    }
}

/// Navigate mode: arrows/hjkl move the hovered panel; Enter/Space activates it.
fn handle_navigate_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => try_quit(app),
        KeyCode::Char('o') => {
            app.options_open = true;
            app.options_selected = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => app.hover_move(Dir::Up),
        KeyCode::Down | KeyCode::Char('j') => app.hover_move(Dir::Down),
        KeyCode::Left | KeyCode::Char('h') => app.hover_move(Dir::Left),
        KeyCode::Right | KeyCode::Char('l') => app.hover_move(Dir::Right),
        KeyCode::Enter | KeyCode::Char(' ') => app.interacting = true,
        _ => {}
    }
}

/// Interact mode: dispatch to the focused panel's handler.
fn handle_interact_key(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    match app.focus {
        Focus::Search => interact_search(app, key, tx),
        Focus::Sidebar => interact_sidebar(app, key, tx),
        Focus::Main => interact_main(app, key),
        Focus::Scope => interact_scope(app, key),
        Focus::List => interact_list(app, key),
        Focus::TaskPane => {} // the task pane owns input via handle_task_pane_key
    }
}

fn running_task(app: &App) -> bool {
    matches!(app.task.as_ref().map(|t| &t.state), Some(TaskState::Running))
}

fn task_done(app: &App) -> bool {
    matches!(app.task.as_ref().map(|t| &t.state), Some(TaskState::Done { .. }))
}

/// Quit, unless an install is still running — in which case surface it instead.
fn try_quit(app: &mut App) {
    if running_task(app) {
        app.task_view = TaskView::Expanded;
        app.focus = Focus::TaskPane;
        if let Some(t) = &mut app.task {
            t.has_unseen_output = false;
        }
    } else {
        app.should_quit = true;
    }
}

/// Backtick handler: expand+focus the task pane, or collapse it to a peek.
fn toggle_task_pane(app: &mut App) {
    if app.task.is_none() {
        return;
    }
    match app.task_view {
        TaskView::Expanded => {
            app.task_view = TaskView::Peek;
            app.focus = app.content_landing();
            app.interacting = false;
        }
        TaskView::Peek | TaskView::Hidden => {
            app.task_view = TaskView::Expanded;
            app.focus = Focus::TaskPane;
            if let Some(t) = &mut app.task {
                t.has_unseen_output = false;
            }
        }
    }
}

fn handle_task_pane_key(app: &mut App, key: KeyEvent) {
    let done = task_done(app);
    match app.task_view {
        TaskView::Expanded => {
            if done {
                // Nothing to forward; any dismiss key closes it.
                match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('x') => {
                        app.dismiss_task()
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Esc => app.task_view = TaskView::Peek, // step down, keep focus
                    _ => {
                        if let Some(task) = &mut app.task {
                            if let Some(bytes) = key_to_bytes(key) {
                                let _ = task.writer.write_all(&bytes);
                                let _ = task.writer.flush();
                            }
                        }
                    }
                }
            }
        }
        TaskView::Peek => match key.code {
            KeyCode::Enter => toggle_task_pane(app), // expand
            KeyCode::Esc | KeyCode::Char('x') => {
                if done {
                    app.dismiss_task();
                } else {
                    app.task_view = TaskView::Hidden; // hide a running task
                    app.focus = app.content_landing();
                    app.interacting = false;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.focus = app.content_landing();
                app.interacting = false;
            }
            KeyCode::Char('/') => {
                app.focus = Focus::Search;
                app.interacting = true;
            }
            KeyCode::Char('q') => try_quit(app),
            _ => {}
        },
        TaskView::Hidden => {
            app.focus = app.content_landing();
            app.interacting = false;
        }
    }
}

fn handle_options_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('o') => app.options_open = false,
        KeyCode::Up | KeyCode::Char('k') => app.move_options(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_options(1),
        KeyCode::Char(' ') | KeyCode::Enter => app.toggle_option(),
        _ => {}
    }
}

/// Interact: the search field. Typing searches (Search view) or filters the
/// installed list (Manage view). Enter submits and hovers the results; Esc
/// cancels back to navigate.
fn interact_search(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    let filtering = app.active_view == ActiveView::Manage;
    match key.code {
        KeyCode::Char(c) => {
            if filtering {
                app.manage_filter.push(c);
                app.installed_selected = 0;
            } else {
                app.query.push(c);
                schedule_debounced_search(app, tx);
            }
        }
        KeyCode::Backspace => {
            if filtering {
                app.manage_filter.pop();
                app.installed_selected = 0;
            } else {
                app.query.pop();
                schedule_debounced_search(app, tx);
            }
        }
        KeyCode::Enter => {
            // submit: focus the results/list so they can be navigated right away
            app.focus = app.content_landing();
            app.interacting = true;
        }
        KeyCode::Esc => app.interacting = false,
        _ => {}
    }
}

/// Interact: the sidebar VIEWS list. Up/down highlight, Enter selects the view.
fn interact_sidebar(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_sidebar(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_sidebar(1),
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.select_sidebar_view();
            app.interacting = false;
            app.focus = app.content_landing();
            if app.active_view == ActiveView::Manage {
                spawn_stats_tasks(tx.clone()); // refresh installed + updates
            }
        }
        KeyCode::Esc => app.interacting = false,
        _ => {}
    }
}

/// Interact: the Search view's content (results list ↔ provider detail).
fn interact_main(app: &mut App, key: KeyEvent) {
    match app.main_view {
        MainView::Results => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
            KeyCode::Enter if app.selected_row().is_some() => {
                app.detail_selected = 0;
                app.main_view = MainView::Detail;
            }
            KeyCode::Esc => app.interacting = false,
            _ => {}
        },
        MainView::Detail => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.detail_selected = app.detail_selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let n = match app.selected_row() {
                    Some(r) => app.effective_providers(r).len(),
                    None => 0,
                };
                if n > 0 {
                    app.detail_selected = (app.detail_selected + 1).min(n - 1);
                }
            }
            KeyCode::Enter => request_install(app),
            KeyCode::Esc => app.main_view = MainView::Results, // back to results
            _ => {}
        },
    }
}

/// Interact: the Manage upgrade-scope chips. h/l pick a scope, Enter runs it.
fn interact_scope(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => app.move_upgrade_scope(-1),
        KeyCode::Right | KeyCode::Char('l') => app.move_upgrade_scope(1),
        KeyCode::Enter => request_upgrade(app),
        KeyCode::Esc => app.interacting = false,
        _ => {}
    }
}

/// Interact: the Manage installed list. j/k move, Enter/`r` remove.
fn interact_list(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_installed(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_installed(1),
        KeyCode::Enter | KeyCode::Char('r') if app.selected_installed().is_some() => {
            request_remove(app)
        }
        KeyCode::Esc => app.interacting = false,
        _ => {}
    }
}

/// Open the confirm modal to remove the selected installed package (at the
/// depth configured in Options).
fn request_remove(app: &mut App) {
    if let Some(spec) = app.remove_spec() {
        app.confirm = Some(spec);
        app.confirm_note = None;
    }
}

/// Open the confirm modal for the selected upgrade scope (All or one source).
fn request_upgrade(app: &mut App) {
    app.confirm = Some(app.upgrade_spec());
    app.confirm_note = None;
}

/// Build an ActionSpec for the selected provider and open the confirm modal.
/// If a task is still running, the confirm will note it gets cancelled.
fn request_install(app: &mut App) {
    let Some(row) = app.selected_row() else { return };
    let providers = app.effective_providers(row);
    let Some(provider) = providers.get(app.detail_selected) else { return };
    let name = row.name.clone();
    let source_id = provider.source_id;
    let command = provider.install_command(&name);
    let note = aur_recency_note(provider);
    app.confirm = Some(ActionSpec {
        targets: vec![name],
        source_id,
        action: Action::Install,
        command,
    });
    app.confirm_note = note;
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
        let badges: Vec<&str> = row.providers.iter().map(|p| p.badge()).collect();
        let installed = if row.any_installed() { " [installed]" } else { "" };
        println!("{:<35} [{}]{}", row.name, badges.join(","), installed);
    }
    println!("\n{} packages", rows.len());
    Ok(())
}
