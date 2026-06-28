// Many model fields/types are consumed incrementally; allow during build-up.
#![allow(dead_code)]

mod action;
mod app;
mod config;
mod event;
mod model;
mod search;
mod sources;
mod theme;
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
    let settings = crate::config::Settings::load();
    let detected = sources::detect_sources(&settings.disabled_sources);
    if detected.is_empty() {
        eprintln!("plaza: no supported package sources detected (need pacman, and yay or paru for AUR), or all sources disabled in options");
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

    let mut app = App::with_settings(source_ids, settings);
    // `checkupdates` (pacman-contrib) syncs a private db so update counts stay
    // live without root; without it we fall back to a stale `pacman -Qu`.
    app.has_checkupdates = sources::which("checkupdates");
    // Detect AUR helpers (yay/paru) and resolve the active one from settings.
    app.helpers_available = sources::detect_aur_helpers();
    app.recompute_aur_helper();
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    spawn_input_task(tx.clone());
    spawn_stats_tasks(tx.clone(), app.aur_helper_bin.clone(), app.present_sources().contains(&SourceId::Flatpak));
    spawn_theme_tick(tx.clone());
    // Warm the Flatpak AppStream cache in the background so a cold cache does not
    // make searches look empty. Best-effort; never blocks the UI.
    if app.present_sources().contains(&SourceId::Flatpak) {
        tokio::spawn(async move {
            let _ = Command::new("flatpak")
                .env("LC_ALL", "C")
                .args(["--user", "update", "--appstream"])
                .output()
                .await;
        });
    }

    terminal.draw(|f| ui::draw(f, &app))?;
    while let Some(ev) = rx.recv().await {
        if let AppEvent::ThemeReloadTick = ev {
            // Only redraw when a theme file actually changed on disk.
            if !app.poll_theme_reload() {
                continue;
            }
        } else {
            handle_event(&mut app, ev, &tx, &sources);
            if app.should_quit {
                break;
            }
            // The Manage detail pane follows the selection: ensure the highlighted
            // package's `pacman -Qi` detail is loading/cached after each event.
            if app.active_view == ActiveView::Manage {
                dispatch_manage_detail(&mut app, &tx);
            }
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

/// Live-reload metronome: nudges the event loop every 300ms so edits to the
/// active theme file are picked up even while the UI is otherwise idle.
fn spawn_theme_tick(tx: UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tick.tick().await;
            if tx.send(AppEvent::ThemeReloadTick).is_err() {
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
        AppEvent::InstalledList(list, repos) => {
            app.installed_list = list;
            app.filter_repos = repos;
            app.clamp_installed();
        }
        AppEvent::UpdatesList(list) => {
            app.updates_list = list;
            app.clamp_installed();
        }
        AppEvent::PackageDetailLoaded { key, detail } => {
            app.details.insert(key, detail);
        }
        AppEvent::ManageDetailLoaded { name, detail } => {
            app.manage_detail_inflight.remove(&name);
            app.manage_detail.insert(name, detail);
        }
        AppEvent::PtyOutput { id, bytes } => {
            let watching = app.focus == Focus::TaskPane && app.task_view == TaskView::Expanded;
            let mut prompt = app.needs_input;
            if let Some(task) = &mut app.task {
                if task.id == id {
                    task.feed(&bytes);
                    if !watching {
                        task.has_unseen_output = true;
                    }
                    let last = task
                        .parser
                        .screen()
                        .contents()
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .to_string();
                    prompt = crate::model::looks_like_prompt(&last);
                }
            }
            app.needs_input = prompt;
        }
        AppEvent::ActionFinished { id, success, code } => {
            let mut matched = false;
            if let Some(task) = &mut app.task {
                if task.id == id {
                    task.state = TaskState::Done { success, code };
                    matched = true;
                }
            }
            if matched {
                app.needs_input = false;
                // Refresh stats + installed index after each action completes.
                spawn_stats_tasks(tx.clone(), app.aur_helper_bin.clone(), app.present_sources().contains(&SourceId::Flatpak));
                if success && !app.queue.is_empty() {
                    // Auto-advance: drop the finished task and start the next item.
                    // Do not surface; keep the user wherever they currently are.
                    app.task = None;
                    start_next(app, tx, false);
                } else if !success {
                    // Pause the queue on failure; the user resumes or clears it.
                    app.queue_paused = true;
                }
                // success + empty queue: leave the finished task on screen as today.
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

/// Confirm "y": enqueue the action. If nothing is running (and the queue is not
/// paused on a prior failure), start draining immediately; otherwise it waits
/// its turn behind the running/failed task.
fn confirm_action(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    let Some(spec) = app.confirm.take() else { return };
    app.confirm_note = None;
    app.enqueue(spec);
    if !running_task(app) && !app.queue_paused {
        app.task = None; // drop a lingering finished task before starting the next
        start_next(app, tx, true);
    } else if app.task_view == TaskView::Hidden {
        // Surface the pane so the user can see the item is queued.
        app.task_view = TaskView::Peek;
    }
}

/// Pop the next pending action and spawn it in a fresh PTY task. No-op when the
/// queue is empty. `surface` brings the pane up Expanded + focused (for a task
/// the user just started); when false the user's current focus and view are left
/// untouched so an auto-advance through the queue does not yank them around.
fn start_next(app: &mut App, tx: &UnboundedSender<AppEvent>, surface: bool) {
    let Some(spec) = app.dequeue_next() else { return };
    app.needs_input = false;
    app.task_seq += 1;
    let id = app.task_seq;
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let (rows, cols) = expanded_pty_size(cols, rows);
    match start_action(spec, id, rows, cols, tx.clone()) {
        Ok(task) => {
            app.task = Some(task);
            if surface {
                app.task_view = TaskView::Expanded;
                app.focus = Focus::TaskPane;
            }
        }
        Err(e) => {
            eprintln!("plaza: failed to start action: {e}");
        }
    }
}

fn spawn_stats_tasks(tx: UnboundedSender<AppEvent>, aur_helper: Option<String>, flatpak: bool) {
    // installed counts + index
    let tx_inst = tx.clone();
    tokio::spawn(async move {
        let repo = run_count(&["-Qnq"]).await;
        let foreign = run_count(&["-Qmq"]).await;
        // One `flatpak list` call feeds both the count and the index folding.
        let fp_text = if flatpak { flatpak_list_text().await } else { String::new() };
        let fp_count = sources::installed::count_lines(&fp_text);
        let _ = tx_inst.send(AppEvent::Stats(crate::model::InstalledStats {
            repo,
            foreign,
            flatpak: fp_count,
        }));

        if let Ok(out) = Command::new("pacman").arg("-Q").output().await {
            let mut idx = sources::installed::InstalledIndex::from_query_output(
                &String::from_utf8_lossy(&out.stdout),
            );
            // Fold installed Flatpak app IDs into the index so search results show
            // installed state for Flatpak too.
            for (id, ver) in sources::flatpak::parse_installed(&fp_text) {
                idx.insert(id, ver);
            }
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
        let explicit = sources::installed::name_set(
            &text(Command::new("pacman").arg("-Qeq").output().await),
        );
        let orphan = sources::installed::name_set(
            &text(Command::new("pacman").arg("-Qdtq").output().await),
        );
        let ordered = sources::installed::ordered_repos(&sl);
        let mut list =
            sources::installed::parse_installed_list(&native, &foreign, &repos, &explicit, &orphan);
        // Append installed Flatpak apps (origin "flatpak"), then re-sort by name.
        if flatpak {
            list.extend(sources::flatpak::parse_installed_pkgs(&flatpak_list_text().await));
            list.sort_by(|a, b| a.name.cmp(&b.name));
        }
        let _ = tx_list.send(AppEvent::InstalledList(list, ordered));
    });

    // best-effort update counts + list (repos, AUR, and Flatpak)
    tokio::spawn(async move {
        let repo_text = repo_update_text().await;
        let aur_text = aur_update_text(aur_helper).await;
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
        // Flatpak updates: `remote-ls --updates` reports the upgradable app IDs
        // (no versions), so the entries carry empty version strings. This call
        // reaches the network; it runs here in the background like the others.
        if flatpak {
            if let Ok(out) = Command::new("flatpak")
                .env("LC_ALL", "C")
                .args(["remote-ls", "--user", "--app", "--updates"])
                .output()
                .await
            {
                for name in sources::flatpak::parse_updates(&String::from_utf8_lossy(&out.stdout)) {
                    list.push(sources::updates::UpdateEntry {
                        name,
                        old_version: String::new(),
                        new_version: String::new(),
                        source_id: SourceId::Flatpak,
                    });
                }
            }
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

/// Installed Flatpak apps as `app-id<TAB>version` lines, or empty on failure.
async fn flatpak_list_text() -> String {
    Command::new("flatpak")
        .env("LC_ALL", "C")
        .args(["list", "--app", "--columns=application,version"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
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

/// Raw AUR-update output from `<helper> -Qua`, or None when no helper is present.
async fn aur_update_text(helper: Option<String>) -> Option<String> {
    let helper = helper?;
    match Command::new(&helper).arg("-Qua").output().await {
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

/// Fetch extended detail for every provider of the selected row that is not
/// already cached or in flight. Each fetch runs in its own task and streams the
/// result back via `PackageDetailLoaded` (like search hits); stale results for a
/// package the user navigated away from simply sit in the cache.
fn dispatch_details(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    let Some(row) = app.selected_row() else { return };
    let name = row.name.clone();
    let mut to_fetch = Vec::new();
    for p in app.effective_providers(row) {
        let key = p.detail_key(&name);
        if app.details.contains_key(&key) || app.detail_requested.contains(&key) {
            continue;
        }
        // Fetch the provider's own target (the real package name / app ID), not
        // the row label, so a grouped variant (gimp-bin under "gimp") or a
        // Flatpak app ID resolves to the right detail.
        to_fetch.push((p.source_id, p.meta.repo.clone(), p.target.clone(), key));
    }
    for (source_id, repo, target, key) in to_fetch {
        app.detail_requested.insert(key.clone());
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Some(detail) = fetch_detail(source_id, &target, repo.as_deref()).await {
                let _ = tx.send(AppEvent::PackageDetailLoaded { key, detail });
            }
        });
    }
}

/// Fetch `pacman -Qi` detail for the highlighted Manage package if it is not
/// cached or already in flight. Mirrors `dispatch_details`; the result streams
/// back via `ManageDetailLoaded` keyed by name.
fn dispatch_manage_detail(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    let Some(pkg) = app.selected_installed() else { return };
    let name = pkg.name;
    if app.manage_detail.contains_key(&name) || app.manage_detail_inflight.contains(&name) {
        return;
    }
    app.manage_detail_inflight.insert(name.clone());
    // Flatpak apps are not in the pacman db: `flatpak info` instead of `pacman -Qi`.
    let is_flatpak = pkg.origin == "flatpak";
    let tx = tx.clone();
    tokio::spawn(async move {
        let detail = if is_flatpak {
            let out = Command::new("flatpak")
                .env("LC_ALL", "C")
                .args(["info", &name])
                .output()
                .await;
            let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            sources::flatpak::parse_info(&text)
        } else {
            let out = Command::new("pacman").arg("-Qi").arg(&name).output().await;
            let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            sources::installed::parse_pkg_detail(&text)
        };
        let _ = tx.send(AppEvent::ManageDetailLoaded { name, detail });
    });
}

/// Fetch one provider's detail: `pacman -Si repo/pkg` for repos, the AUR `info`
/// RPC for the AUR. `None` on any IO/parse failure (the field just stays empty).
async fn fetch_detail(
    source_id: SourceId,
    name: &str,
    repo: Option<&str>,
) -> Option<model::PackageDetail> {
    match source_id {
        SourceId::Pacman => {
            let target = match repo {
                Some(r) => format!("{r}/{name}"),
                None => name.to_string(),
            };
            let out = Command::new("pacman").arg("-Si").arg(&target).output().await.ok()?;
            Some(sources::pacman::parse_si_output(&String::from_utf8_lossy(&out.stdout)))
        }
        SourceId::Aur => {
            let body = reqwest::get(sources::aur::info_url(name)).await.ok()?.text().await.ok()?;
            sources::aur::parse_info_response(&body)
        }
        SourceId::Flatpak => {
            // `name` is the app ID here (the provider target). `repo` is the
            // remote. Networked (not `--cached`): the cached form omits size and
            // date, so it gives no usable detail. Same class as the AUR info RPC.
            // `--user` disambiguates a remote that exists in both installations.
            let remote = repo.unwrap_or("flathub");
            let out = Command::new("flatpak")
                .env("LC_ALL", "C")
                .args(["remote-info", "--user", remote, name])
                .output()
                .await
                .ok()?;
            Some(sources::flatpak::parse_remote_info(&String::from_utf8_lossy(&out.stdout)))
        }
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
    // Any keypress dismisses a transient status message.
    app.status_msg = None;

    // Options overlay captures input first.
    if app.options_open {
        handle_options_key(app, key);
        return;
    }

    // Confirm modal next.
    if app.confirm.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => confirm_action(app, tx),
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
        handle_task_pane_key(app, key, tx);
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
            spawn_stats_tasks(tx.clone(), app.aur_helper_bin.clone(), app.present_sources().contains(&SourceId::Flatpak));
        }
        return;
    }

    // `/` jumps straight to the search field (unless you are already typing).
    if key.code == KeyCode::Char('/') && !(app.focus == Focus::Search && app.interacting) {
        app.focus = Focus::Search;
        app.interacting = true;
        return;
    }

    // `f` toggles the repo-filter box (unless typing in the search field).
    if key.code == KeyCode::Char('f') && !(app.focus == Focus::Search && app.interacting) {
        app.toggle_filter_open();
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
        Focus::Main => interact_main(app, key, tx),
        Focus::Scope => interact_scope(app, key),
        Focus::List => interact_list(app, key),
        Focus::Filter => interact_filter(app, key),
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

/// Dismiss the finished task and resume the queue: if items remain, start the
/// next; otherwise hide the pane. Clears any failure pause.
fn dismiss_and_continue(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    app.task = None;
    app.queue_paused = false;
    if app.queue.is_empty() {
        app.dismiss_task();
    } else {
        // The user is acting from the task pane, so keep them on the next task.
        start_next(app, tx, true);
    }
}

fn handle_task_pane_key(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    let done = task_done(app);
    match app.task_view {
        TaskView::Expanded => {
            if done {
                // A finished task: Enter/Esc/q dismiss and continue the queue;
                // x clears the remaining queue too.
                match key.code {
                    KeyCode::Char('x') => {
                        app.clear_queue();
                        app.dismiss_task();
                    }
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                        dismiss_and_continue(app, tx)
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
        // Peek shows the running/finished item plus the pending queue, which is
        // navigable here (j/k select, d removes one, x clears all).
        TaskView::Peek => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.move_queue(-1),
            KeyCode::Down | KeyCode::Char('j') => app.move_queue(1),
            KeyCode::Char('d') => app.remove_queued(app.queue_selected),
            KeyCode::Char('x') => {
                if done {
                    app.clear_queue();
                    app.dismiss_task();
                } else {
                    app.clear_queue(); // drop pending; the running task keeps going
                }
            }
            KeyCode::Enter => {
                if done {
                    dismiss_and_continue(app, tx);
                } else {
                    toggle_task_pane(app); // expand the running task
                }
            }
            KeyCode::Esc => {
                if done {
                    dismiss_and_continue(app, tx);
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
                spawn_stats_tasks(tx.clone(), app.aur_helper_bin.clone(), app.present_sources().contains(&SourceId::Flatpak)); // refresh installed + updates
            }
        }
        KeyCode::Esc => app.interacting = false,
        _ => {}
    }
}

/// Interact: the Search view's content (results list ↔ provider detail).
fn interact_main(app: &mut App, key: KeyEvent, tx: &UnboundedSender<AppEvent>) {
    match app.main_view {
        MainView::Results => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
            KeyCode::Enter if app.selected_row().is_some() => {
                app.detail_selected = 0;
                app.main_view = MainView::Detail;
                dispatch_details(app, tx);
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

/// Interact: the Manage installed list. j/k move, Enter/`r` remove, `u` upgrade
/// the highlighted package (when it has a pending update).
fn interact_list(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_installed(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_installed(1),
        KeyCode::Char('u') if app.selected_installed().is_some() => request_upgrade_one(app),
        KeyCode::Enter | KeyCode::Char('r') if app.selected_installed().is_some() => {
            request_remove(app)
        }
        KeyCode::Esc => app.interacting = false,
        _ => {}
    }
}

/// Interact: the repo-filter box. j/k move, space/Enter toggle the checkbox,
/// `s` saves the active view's filter as its default, Esc steps back out (the box
/// stays if a filter is active).
fn interact_filter(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_filter(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_filter(1),
        KeyCode::Char(' ') | KeyCode::Enter => app.toggle_filter(),
        KeyCode::Char('s') => app.save_filter_default(),
        KeyCode::Esc => app.close_filter(),
        _ => {}
    }
}

/// Open the confirm modal to upgrade the highlighted package on its own. Shows a
/// message when the package has no pending update, or when an AUR upgrade needs a
/// helper that is not installed.
fn request_upgrade_one(app: &mut App) {
    let Some(spec) = app.upgrade_one_spec() else {
        app.status_msg = Some("no update available for this package".to_string());
        return;
    };
    if spec.source_id == SourceId::Aur && app.aur_helper_bin.is_none() {
        app.status_msg = Some(NO_AUR_HELPER_MSG.to_string());
        return;
    }
    let fallback = app.aur_fallback_note_for(spec.source_id);
    // Single repo upgrades are partial upgrades; surface the caveat.
    let caveat = (spec.source_id == SourceId::Pacman).then(|| {
        "single-package upgrade is a partial upgrade; sync your db first if it errors".to_string()
    });
    app.confirm_note = match (fallback, caveat) {
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
        (a, b) => a.or(b),
    };
    app.confirm = Some(spec);
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
/// An AUR-only scope with no helper installed shows a message instead.
fn request_upgrade(app: &mut App) {
    if !app.can_upgrade_selected() {
        app.status_msg = Some(NO_AUR_HELPER_MSG.to_string());
        return;
    }
    let note = if app.upgrade_scope_touches_aur() {
        app.aur_fallback_note()
    } else {
        None
    };
    app.confirm = Some(app.upgrade_spec());
    app.confirm_note = note;
}

/// Build an ActionSpec for the selected provider and open the confirm modal.
/// Confirming enqueues the install behind any running task.
fn request_install(app: &mut App) {
    let Some(row) = app.selected_row() else { return };
    let providers = app.effective_providers(row);
    if providers.is_empty() {
        return;
    }
    // Clamp like the detail view does, so a filter change that shrank the
    // provider list cannot make install target a different row than the one
    // highlighted on screen.
    let provider = &providers[app.detail_selected.min(providers.len() - 1)];
    let source_id = provider.source_id;
    // Installing from the AUR needs a helper; surface a message when none exists.
    // Pacman ignores the helper, so an empty binary is harmless for that branch.
    let aur_bin = match &app.aur_helper_bin {
        Some(bin) => bin.clone(),
        None => {
            if source_id == SourceId::Aur {
                app.status_msg = Some(NO_AUR_HELPER_MSG.to_string());
                return;
            }
            String::new()
        }
    };
    let name = row.name.clone();
    let command = provider.install_command(&name, &aur_bin);
    let recency = aur_recency_note(provider);
    // Compose the AUR fallback note (if any) with the recency warning.
    let note = match (app.aur_fallback_note_for(source_id), recency) {
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
        (a, b) => a.or(b),
    };
    app.confirm = Some(ActionSpec {
        targets: vec![name],
        source_id,
        action: Action::Install,
        command,
    });
    app.confirm_note = note;
}

const NO_AUR_HELPER_MSG: &str = "no AUR helper installed (need yay or paru)";

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
    let settings = crate::config::Settings::load();
    let sources = sources::detect_sources(&settings.disabled_sources);
    if sources.is_empty() {
        eprintln!("plaza: no supported package sources detected (need pacman, and yay or paru for AUR), or all sources disabled in options");
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
    let mut rows = merge(all_hits, &idx, settings.group_variants);
    relevance_sort(term, &mut rows);
    for row in &rows {
        let badges: Vec<&str> = row.providers.iter().map(|p| p.badge()).collect();
        let installed = if row.any_installed() { " [installed]" } else { "" };
        println!("{:<35} [{}]{}", row.name, badges.join(","), installed);
    }
    println!("\n{} packages", rows.len());
    Ok(())
}
