use crate::action::runner::ActiveTask;
use crate::config::Settings;
use crate::model::{
    chain_commands, remove_command, source_upgrade_command, upgrade_one_command, Action,
    ActionSpec, InstalledStats, PackageDetail, PackageHit, PackageRow, Provider, SourceId,
    UpdatesInfo,
};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use crate::search::aggregator::{merge, rank, relevance_sort};
use crate::sources::installed::{InstalledIndex, InstalledPkg};
use crate::sources::updates::UpdateEntry;
use crate::theme::{self, palette::Palette, skin::Skin};
use std::time::SystemTime;

/// A focusable panel. In the Search view the content panel is `Main`; in the
/// Manage view it splits into `Scope` (upgrade chips) and `List` (installed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Sidebar,
    Main,
    Scope,
    List,
    Filter,
    TaskPane,
}

/// One row in the repo-filter box: a checkbox plus what toggling it acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterId {
    /// The pacman master: toggles every pacman repo at once (aur untouched).
    Master,
    /// A single concrete repo (e.g. "extra").
    Repo(String),
    /// The AUR.
    Aur,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRow {
    pub label: String,
    pub checked: bool,
    pub id: FilterId,
}

/// Hover-movement direction (navigate mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainView {
    Results,
    Detail,
}

/// Which sidebar VIEW is active. Drives what the center area shows. The Search
/// view keeps the existing Results/Detail sub-state in `MainView`; Manage merges
/// installed browsing and upgrades into one view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Search,
    Manage,
}

impl ActiveView {
    /// The sidebar VIEWS index for this view.
    pub fn index(self) -> usize {
        match self {
            ActiveView::Search => 0,
            ActiveView::Manage => 1,
        }
    }

    /// The view at sidebar VIEWS index `i` (out-of-range falls back to Search).
    pub fn from_index(i: usize) -> ActiveView {
        match i {
            1 => ActiveView::Manage,
            _ => ActiveView::Search,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    Loading,
    Done(usize),
    Error,
}

/// Cycle the search-debounce presets (ms). Raise it past your key-repeat delay
/// so holding a key stops flashing intermediate results.
fn next_debounce(cur: u64) -> u64 {
    match cur {
        d if d < 400 => 400,
        d if d < 600 => 600,
        d if d < 800 => 800,
        _ => 250,
    }
}

/// Move `cur` by `delta` and clamp to a valid index in a list of length `len`
/// (0 when the list is empty).
fn clamp_index(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = len as i32 - 1;
    (cur as i32 + delta).clamp(0, max) as usize
}

/// Visibility of the background-task (install) pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskView {
    Hidden,
    Peek,
    Expanded,
}

/// Number of entries in the sidebar VIEWS list (Search / Manage).
pub const VIEW_COUNT: usize = 2;

pub struct App {
    pub query: String,
    pub query_id: u64,
    pub debounce_gen: u64,
    pub rows: Vec<PackageRow>,
    pub hits_buffer: Vec<PackageHit>,
    pub installed: InstalledIndex,
    pub stats: InstalledStats,
    pub updates: UpdatesInfo,
    pub source_status: Vec<(SourceId, SourceState)>,
    pub focus: Focus,
    pub active_view: ActiveView,
    pub main_view: MainView,
    pub results_selected: usize,
    pub detail_selected: usize,
    /// Extended per-provider detail, fetched lazily on opening the detail view
    /// and keyed by `Provider::detail_key`. `detail_requested` tracks in-flight
    /// keys so a fetch is dispatched at most once per provider.
    pub details: HashMap<String, PackageDetail>,
    pub detail_requested: HashSet<String>,
    pub sidebar_selected: usize,
    /// All installed packages (`pacman -Qn` + `-Qm`) for the Manage view.
    pub installed_list: Vec<InstalledPkg>,
    pub installed_selected: usize,
    /// Manage-view filter text. Kept separate from `query` so each tab keeps its
    /// own search box contents.
    pub manage_filter: String,
    /// Repo filter (the `f` box). `repo_filter_off` is the set of *unchecked*
    /// repo identifiers (concrete repo names plus "aur"); empty means show all.
    /// Session-only, not persisted. `filter_repos` is the stable, ordered list of
    /// pacman repos (from `pacman -Sl`) that populate the checkbox rows.
    pub repo_filter_off: BTreeSet<String>,
    pub filter_repos: Vec<String>,
    pub filter_selected: usize,
    /// Upgradable packages (repos + AUR); drives the scope chips and `↑` markers.
    pub updates_list: Vec<UpdateEntry>,
    /// Selected upgrade-scope chip: 0 = All, 1..=n = the nth present source.
    pub upgrade_scope_selected: usize,
    /// Navigate mode (hover panels) vs interact mode (act inside the focused
    /// panel). Enter/Space activates the hovered panel; Esc steps back out.
    pub interacting: bool,
    /// Whether `checkupdates` is available (else update counts are stale until a
    /// real `pacman -Sy`).
    pub has_checkupdates: bool,
    pub confirm: Option<ActionSpec>,
    pub confirm_note: Option<String>,
    pub task: Option<ActiveTask>,
    /// Pending actions, drained one at a time after the running `task` finishes.
    pub queue: VecDeque<ActionSpec>,
    /// Set when a task finished with a failure: the queue stops draining until
    /// the user dismisses (resume) or clears it.
    pub queue_paused: bool,
    /// Selected pending item in the task pane's queue list (for per-item removal).
    pub queue_selected: usize,
    pub task_view: TaskView,
    pub task_seq: u64,
    pub settings: Settings,
    /// Active resolved theme: colors (`palette`) and shape (`skin`), the
    /// registries the Options rows cycle, and the mtimes the reload poll watches.
    pub palettes: Vec<(String, Palette)>,
    pub skins: Vec<(String, Skin)>,
    pub palette: Palette,
    pub skin: Skin,
    palette_mtime: Option<SystemTime>,
    skin_mtime: Option<SystemTime>,
    pub options_open: bool,
    pub options_selected: usize,
    /// Which AUR helper binaries are installed, as `(yay, paru)`. Set at startup.
    pub helpers_available: (bool, bool),
    /// Resolved AUR helper binary per `settings.aur_helper` + availability, or
    /// `None` when no helper is installed. Recomputed when the setting changes.
    pub aur_helper_bin: Option<String>,
    /// True when the configured helper was missing and we fell back to the other.
    pub aur_helper_fell_back: bool,
    /// Transient one-line status message (e.g. "no AUR helper installed"), shown
    /// in the status bar until the next keypress.
    pub status_msg: Option<String>,
    /// True while the running task's latest output line looks like a prompt
    /// waiting for input. Drives the status-bar alert when the user is off the
    /// task pane. Recomputed on each chunk of PTY output.
    pub needs_input: bool,
    pub should_quit: bool,
}

fn file_mtime_opt(path: Option<std::path::PathBuf>) -> Option<SystemTime> {
    let p = path?;
    std::fs::metadata(p).ok()?.modified().ok()
}

impl App {
    pub fn new(source_ids: Vec<SourceId>) -> Self {
        let source_status = source_ids
            .into_iter()
            .map(|id| (id, SourceState::Done(0)))
            .collect();
        let settings = Settings::load();
        let base = crate::config::config_base();
        let pal_dir = base.as_ref().map(|b| b.join("plaza").join("palettes"));
        let skn_dir = base.as_ref().map(|b| b.join("plaza").join("skins"));
        let (palettes, _) = theme::palette_registry(pal_dir.as_deref());
        let (skins, _) = theme::skin_registry(skn_dir.as_deref());
        let palette = theme::resolve_palette(&palettes, &settings.palette);
        let skin = theme::resolve_skin(&skins, &settings.skin);
        let palette_mtime = file_mtime_opt(theme::user_palette_path(&settings.palette));
        let skin_mtime = file_mtime_opt(theme::user_skin_path(&settings.skin));
        App {
            query: String::new(),
            query_id: 0,
            debounce_gen: 0,
            rows: Vec::new(),
            hits_buffer: Vec::new(),
            installed: InstalledIndex::default(),
            stats: InstalledStats::default(),
            updates: UpdatesInfo::default(),
            source_status,
            focus: Focus::Search,
            active_view: ActiveView::Search,
            main_view: MainView::Results,
            results_selected: 0,
            detail_selected: 0,
            details: HashMap::new(),
            detail_requested: HashSet::new(),
            sidebar_selected: 0,
            installed_list: Vec::new(),
            installed_selected: 0,
            manage_filter: String::new(),
            repo_filter_off: BTreeSet::new(),
            filter_repos: Vec::new(),
            filter_selected: 0,
            updates_list: Vec::new(),
            upgrade_scope_selected: 0,
            interacting: true,
            has_checkupdates: false,
            confirm: None,
            confirm_note: None,
            task: None,
            queue: VecDeque::new(),
            queue_paused: false,
            queue_selected: 0,
            task_view: TaskView::Hidden,
            task_seq: 0,
            settings,
            palettes,
            skins,
            palette,
            skin,
            palette_mtime,
            skin_mtime,
            options_open: false,
            options_selected: 0,
            helpers_available: (false, false),
            aur_helper_bin: None,
            aur_helper_fell_back: false,
            status_msg: None,
            needs_input: false,
            should_quit: false,
        }
    }

    /// Recompute the resolved AUR helper binary and fallback flag from the
    /// current `settings.aur_helper` and detected `helpers_available`. Call at
    /// startup (after detection) and whenever the setting changes.
    pub fn recompute_aur_helper(&mut self) {
        let (yay, paru) = self.helpers_available;
        match crate::model::resolve_aur_helper(self.settings.aur_helper, yay, paru) {
            Some((bin, fell_back)) => {
                self.aur_helper_bin = Some(bin.to_string());
                self.aur_helper_fell_back = fell_back;
            }
            None => {
                self.aur_helper_bin = None;
                self.aur_helper_fell_back = false;
            }
        }
    }

    /// The providers to display/select for `row`. With `collapse_repos`, all
    /// pacman repos collapse to just the default (highest-priority) one, plus
    /// the AUR; otherwise every repo is listed in priority order.
    pub fn effective_providers<'a>(&self, row: &'a PackageRow) -> Vec<&'a Provider> {
        if self.settings.collapse_repos {
            let mut out: Vec<&Provider> = Vec::new();
            if let Some(p) = row.providers.iter().find(|p| p.source_id == SourceId::Pacman) {
                out.push(p);
            }
            if let Some(p) = row.providers.iter().find(|p| p.source_id == SourceId::Aur) {
                out.push(p);
            }
            out
        } else {
            row.providers.iter().collect()
        }
    }

    /// Badge label for a provider, honoring `collapse_repos` (pacman → "official").
    pub fn provider_badge<'a>(&self, p: &'a Provider) -> &'a str {
        if self.settings.collapse_repos && p.source_id == SourceId::Pacman {
            "official"
        } else {
            p.badge()
        }
    }

    pub fn clear_confirm(&mut self) {
        self.confirm = None;
        self.confirm_note = None;
    }

    // --- Options overlay ---

    /// Number of option rows.
    pub const OPTIONS_COUNT: usize = 8;

    pub fn move_options(&mut self, delta: i32) {
        let max = Self::OPTIONS_COUNT as i32 - 1;
        let next = (self.options_selected as i32 + delta).clamp(0, max);
        self.options_selected = next as usize;
    }

    pub fn toggle_option(&mut self) {
        match self.options_selected {
            0 => self.settings.show_hotkeys = !self.settings.show_hotkeys,
            1 => self.settings.collapse_repos = !self.settings.collapse_repos,
            2 => self.cycle_palette(),
            3 => self.cycle_skin(),
            4 => self.settings.debounce_ms = next_debounce(self.settings.debounce_ms),
            5 => self.settings.remove_depth = self.settings.remove_depth.next(),
            6 => self.cycle_aur_helper(),
            7 => self.settings.hide_idle_filter = !self.settings.hide_idle_filter,
            _ => {}
        }
        self.settings.save();
    }

    /// Advance the AUR helper setting through `Auto` plus the installed helpers,
    /// then re-resolve the active binary.
    pub fn cycle_aur_helper(&mut self) {
        let (yay, paru) = self.helpers_available;
        self.settings.aur_helper =
            crate::model::next_aur_helper(self.settings.aur_helper, yay, paru);
        self.recompute_aur_helper();
    }

    /// Advance to the next palette in the registry, persisting and re-resolving.
    pub fn cycle_palette(&mut self) {
        let ns = theme::names(&self.palettes);
        self.settings.palette = theme::next_name(&ns, &self.settings.palette);
        self.palette = theme::resolve_palette(&self.palettes, &self.settings.palette);
        self.palette_mtime = None; // re-stat the new selection on the next poll
    }

    /// Advance to the next skin in the registry, persisting and re-resolving.
    pub fn cycle_skin(&mut self) {
        let ns = theme::names(&self.skins);
        self.settings.skin = theme::next_name(&ns, &self.settings.skin);
        self.skin = theme::resolve_skin(&self.skins, &self.settings.skin);
        self.skin_mtime = None;
    }

    /// Reload the active palette/skin if its user file changed on disk. Returns
    /// whether anything changed (so the caller can skip redundant redraws).
    pub fn poll_theme_reload(&mut self) -> bool {
        let mut changed = false;
        if let Some(path) = theme::user_palette_path(&self.settings.palette) {
            let m = std::fs::metadata(&path).ok().and_then(|md| md.modified().ok());
            if m.is_some() && m != self.palette_mtime {
                if let Some(p) = theme::load_palette_file(&path) {
                    self.palette = p;
                    changed = true;
                }
                self.palette_mtime = m;
            }
        }
        if let Some(path) = theme::user_skin_path(&self.settings.skin) {
            let m = std::fs::metadata(&path).ok().and_then(|md| md.modified().ok());
            if m.is_some() && m != self.skin_mtime {
                if let Some(s) = theme::load_skin_file(&path) {
                    self.skin = s;
                    changed = true;
                }
                self.skin_mtime = m;
            }
        }
        changed
    }

    /// The welcome-screen footer advertising the active theme + customize path.
    pub fn theme_footer(&self) -> String {
        format!(
            "palette: {} · skin: {} · edit ~/.config/plaza/",
            self.settings.palette, self.settings.skin
        )
    }

    /// Is the task pane currently on screen (peek or expanded)?
    pub fn task_pane_visible(&self) -> bool {
        self.task.is_some() && self.task_view != TaskView::Hidden
    }

    /// Is `f` the focused panel and currently being interacted with (active)?
    pub fn is_active(&self, f: Focus) -> bool {
        self.focus == f && self.interacting
    }

    /// Is `f` the focused panel but only hovered (navigate mode)?
    pub fn is_hovered(&self, f: Focus) -> bool {
        self.focus == f && !self.interacting
    }

    /// The top content panel for the active view (where Search/Sidebar lead).
    pub fn content_top(&self) -> Focus {
        match self.active_view {
            ActiveView::Search => Focus::Main,
            ActiveView::Manage => Focus::Scope,
        }
    }

    /// The panel to land on when entering a view (browsing target).
    pub fn content_landing(&self) -> Focus {
        match self.active_view {
            ActiveView::Search => Focus::Main,
            ActiveView::Manage => Focus::List,
        }
    }

    /// Move the hovered panel (navigate mode). Layout: Search on top, Sidebar on
    /// the left, content on the right (Manage splits into Scope over List).
    pub fn hover_move(&mut self, dir: Dir) {
        let top = self.content_top();
        let next = match (self.focus, dir) {
            (Focus::Search, Dir::Down) => top,
            (Focus::Search, Dir::Left) => Focus::Sidebar,

            (Focus::Sidebar, Dir::Up) => Focus::Search,
            (Focus::Sidebar, Dir::Right) => top,
            (Focus::Sidebar, Dir::Down) if self.filter_box_visible() => Focus::Filter,

            (Focus::Filter, Dir::Up) => Focus::Sidebar,
            (Focus::Filter, Dir::Right) => top,

            (Focus::Main, Dir::Up) => Focus::Search,
            (Focus::Main, Dir::Left) => Focus::Sidebar,
            (Focus::Main, Dir::Right) if self.task_pane_visible() => Focus::TaskPane,

            (Focus::Scope, Dir::Up) => Focus::Search,
            (Focus::Scope, Dir::Left) => Focus::Sidebar,
            (Focus::Scope, Dir::Down) => Focus::List,
            (Focus::Scope, Dir::Right) if self.task_pane_visible() => Focus::TaskPane,

            (Focus::List, Dir::Up) => Focus::Scope,
            (Focus::List, Dir::Left) => Focus::Sidebar,
            (Focus::List, Dir::Right) if self.task_pane_visible() => Focus::TaskPane,

            (Focus::TaskPane, Dir::Left) => self.content_landing(),

            (f, _) => f,
        };
        self.focus = next;
    }

    pub fn move_sidebar(&mut self, delta: i32) {
        let max = VIEW_COUNT as i32 - 1;
        let next = (self.sidebar_selected as i32 + delta).clamp(0, max);
        self.sidebar_selected = next as usize;
    }

    /// Drop the finished/aborted task and hide the pane.
    pub fn dismiss_task(&mut self) {
        self.task = None;
        self.task_view = TaskView::Hidden;
        if self.focus == Focus::TaskPane {
            self.focus = self.content_landing();
            self.interacting = false;
        }
    }

    /// Begin a new search generation. Returns the new query_id.
    pub fn start_query(&mut self, query: String) -> u64 {
        self.query = query;
        self.query_id += 1;
        self.hits_buffer.clear();
        self.rows.clear();
        self.results_selected = 0;
        // A search always lands in the Search view's results.
        self.active_view = ActiveView::Search;
        self.sidebar_selected = ActiveView::Search.index();
        self.main_view = MainView::Results;
        for (_, state) in &mut self.source_status {
            *state = SourceState::Loading;
        }
        self.query_id
    }

    pub fn apply_search_results(
        &mut self,
        query_id: u64,
        source_id: SourceId,
        hits: Vec<PackageHit>,
    ) {
        if query_id != self.query_id {
            return; // stale
        }
        let count = hits.len();
        self.hits_buffer.extend(hits);
        self.rows = merge(self.hits_buffer.clone(), &self.installed);
        relevance_sort(&self.query, &mut self.rows);
        let visible = self.search_rows().len();
        if self.results_selected >= visible {
            self.results_selected = visible.saturating_sub(1);
        }
        self.set_source_state(source_id, SourceState::Done(count));
    }

    pub fn set_source_error(&mut self, query_id: u64, source_id: SourceId) {
        if query_id != self.query_id {
            return;
        }
        self.set_source_state(source_id, SourceState::Error);
    }

    fn set_source_state(&mut self, source_id: SourceId, state: SourceState) {
        if let Some(entry) = self.source_status.iter_mut().find(|(id, _)| *id == source_id) {
            entry.1 = state;
        }
    }

    pub fn selected_row(&self) -> Option<&PackageRow> {
        self.search_rows().get(self.results_selected).copied()
    }

    // --- sidebar VIEWS ---

    /// Activate the view highlighted in the sidebar.
    pub fn select_sidebar_view(&mut self) {
        self.active_view = ActiveView::from_index(self.sidebar_selected);
        if self.active_view == ActiveView::Manage {
            self.clamp_installed();
        }
    }

    /// Toggle between the Search and Manage views (the `Tab` key). Lands ready to
    /// type in Search, or browsing the list in Manage.
    pub fn toggle_view(&mut self) {
        self.active_view = match self.active_view {
            ActiveView::Search => ActiveView::Manage,
            ActiveView::Manage => ActiveView::Search,
        };
        self.sidebar_selected = self.active_view.index();
        match self.active_view {
            ActiveView::Search => {
                self.focus = Focus::Search;
                self.interacting = true;
            }
            ActiveView::Manage => {
                self.focus = Focus::List;
                self.interacting = false;
                self.clamp_installed();
            }
        }
    }

    // --- Manage view: installed list ---

    /// New version available for `name`, if any (drives the `↑` marker).
    pub fn update_for(&self, name: &str) -> Option<&str> {
        self.updates_list
            .iter()
            .find(|u| u.name == name)
            .map(|u| u.new_version.as_str())
    }

    /// The text the search bar is currently editing, which depends on the active
    /// view (online search vs Manage filter). Each view keeps its own.
    pub fn search_text(&self) -> &str {
        match self.active_view {
            ActiveView::Search => &self.query,
            ActiveView::Manage => &self.manage_filter,
        }
    }

    /// The installed packages to show in the Manage list: filtered by the
    /// Manage filter (case-insensitive substring), with upgradable packages
    /// floated to the top.
    pub fn manage_rows(&self) -> Vec<&InstalledPkg> {
        let q = self.manage_filter.to_ascii_lowercase();
        let updates: std::collections::HashSet<&str> =
            self.updates_list.iter().map(|u| u.name.as_str()).collect();
        let mut rows: Vec<&InstalledPkg> = self
            .installed_list
            .iter()
            .filter(|p| q.is_empty() || p.name.to_ascii_lowercase().contains(&q))
            .filter(|p| self.repo_shown(&p.origin))
            .collect();
        rows.sort_by(|a, b| {
            let (au, bu) = (updates.contains(a.name.as_str()), updates.contains(b.name.as_str()));
            if q.is_empty() {
                // No filter: float upgradable packages up, then alphabetical.
                bu.cmp(&au).then_with(|| a.name.cmp(&b.name))
            } else {
                // Filtering: match quality (exact > prefix > substring) wins over the
                // upgradable float, so typing a name surfaces it even if something
                // upgradable also matches. Upgradable breaks ties within a rank.
                rank(&q, &a.name)
                    .cmp(&rank(&q, &b.name))
                    .then_with(|| bu.cmp(&au))
                    .then_with(|| a.name.len().cmp(&b.name.len()))
                    .then_with(|| a.name.cmp(&b.name))
            }
        });
        rows
    }

    /// The selected installed package (indexes the filtered/sorted rows).
    pub fn selected_installed(&self) -> Option<InstalledPkg> {
        self.manage_rows().get(self.installed_selected).map(|p| (*p).clone())
    }

    /// Move the Manage-list selection by delta, clamped to the visible rows.
    pub fn move_installed(&mut self, delta: i32) {
        self.installed_selected = clamp_index(self.installed_selected, delta, self.manage_rows().len());
    }

    /// Clamp the list selection into the current (possibly filtered) row range.
    pub fn clamp_installed(&mut self) {
        let n = self.manage_rows().len();
        self.installed_selected = self.installed_selected.min(n.saturating_sub(1));
    }

    // --- repo filter (the `f` box) ---

    /// Whether packages from `repo` (a concrete repo name or "aur") are shown.
    pub fn repo_shown(&self, repo: &str) -> bool {
        !self.repo_filter_off.contains(repo)
    }

    /// With the hide-when-idle option off, the box is always on screen. With it
    /// on, the box shows only while it is the focused panel (opened or hovered) or
    /// while a filter is active (so it doubles as the "filter active" indicator).
    pub fn filter_box_visible(&self) -> bool {
        !self.settings.hide_idle_filter
            || self.focus == Focus::Filter
            || !self.repo_filter_off.is_empty()
    }

    /// True when no pacman repo is filtered out (drives the master checkbox).
    pub fn pacman_master_checked(&self) -> bool {
        self.filter_repos.iter().all(|r| self.repo_shown(r))
    }

    /// The checkbox rows to render, honoring `collapse_repos`. Collapsed: a single
    /// `repo` master plus `aur`. Expanded: an `all repos` master, each repo, then
    /// `aur`. The `aur` row appears only when the AUR source is present.
    pub fn filter_checkboxes(&self) -> Vec<FilterRow> {
        let mut rows = Vec::new();
        if self.settings.collapse_repos {
            rows.push(FilterRow {
                label: "repo".into(),
                checked: self.pacman_master_checked(),
                id: FilterId::Master,
            });
        } else {
            rows.push(FilterRow {
                label: "all repos".into(),
                checked: self.pacman_master_checked(),
                id: FilterId::Master,
            });
            for r in &self.filter_repos {
                rows.push(FilterRow {
                    label: r.clone(),
                    checked: self.repo_shown(r),
                    id: FilterId::Repo(r.clone()),
                });
            }
        }
        if self.present_sources().contains(&SourceId::Aur) {
            rows.push(FilterRow {
                label: "aur".into(),
                checked: self.repo_shown("aur"),
                id: FilterId::Aur,
            });
        }
        rows
    }

    /// Flip one identifier in/out of the unchecked set.
    fn toggle_repo_off(&mut self, id: &str) {
        if !self.repo_filter_off.remove(id) {
            self.repo_filter_off.insert(id.to_string());
        }
    }

    /// Toggle the highlighted checkbox. The master flips every pacman repo
    /// together; a repo or `aur` row flips just itself. Re-clamps both list
    /// selections, since the visible rows may shrink.
    pub fn toggle_filter(&mut self) {
        let Some(row) = self.filter_checkboxes().get(self.filter_selected).cloned() else {
            return;
        };
        match row.id {
            FilterId::Master => {
                let repos = self.filter_repos.clone();
                if self.pacman_master_checked() {
                    self.repo_filter_off.extend(repos);
                } else {
                    for r in &repos {
                        self.repo_filter_off.remove(r);
                    }
                }
            }
            FilterId::Repo(r) => self.toggle_repo_off(&r),
            FilterId::Aur => self.toggle_repo_off("aur"),
        }
        self.clamp_after_filter();
    }

    /// Move the filter-box cursor, clamped to the rendered rows.
    pub fn move_filter(&mut self, delta: i32) {
        self.filter_selected = clamp_index(self.filter_selected, delta, self.filter_checkboxes().len());
    }

    /// Open the filter box and focus it (the `f` key).
    pub fn open_filter(&mut self) {
        self.focus = Focus::Filter;
        self.interacting = true;
        self.filter_selected = self.filter_selected.min(self.filter_checkboxes().len().saturating_sub(1));
    }

    /// Close the filter box: move focus back to the content area. It stays
    /// rendered only if a filter is active (and not hidden when idle).
    pub fn close_filter(&mut self) {
        if self.focus == Focus::Filter {
            self.focus = self.content_landing();
            self.interacting = false;
        }
    }

    /// `f`: toggle the box open/closed, based on whether it currently has focus.
    pub fn toggle_filter_open(&mut self) {
        if self.focus == Focus::Filter {
            self.close_filter();
        } else {
            self.open_filter();
        }
    }

    /// Keep both list selections inside their (possibly shrunken) filtered ranges.
    fn clamp_after_filter(&mut self) {
        let n = self.search_rows().len();
        self.results_selected = self.results_selected.min(n.saturating_sub(1));
        self.clamp_installed();
    }

    /// The search results after applying the repo filter. A row is kept when at
    /// least one of its providers' repos is shown.
    pub fn search_rows(&self) -> Vec<&PackageRow> {
        self.rows
            .iter()
            .filter(|row| row.providers.iter().any(|p| self.repo_shown(p.badge())))
            .collect()
    }

    /// Build a Remove spec for the selected installed package (using the
    /// depth configured in Options), or None if nothing is selected.
    pub fn remove_spec(&self) -> Option<ActionSpec> {
        let pkg = self.selected_installed()?;
        Some(ActionSpec {
            targets: vec![pkg.name.clone()],
            source_id: SourceId::Pacman,
            action: Action::Remove,
            command: remove_command(&pkg.name, self.settings.remove_depth),
        })
    }

    // --- action queue ---

    /// Append a pending action to the queue.
    pub fn enqueue(&mut self, spec: ActionSpec) {
        self.queue.push_back(spec);
    }

    /// Pop the next pending action (front of the queue).
    pub fn dequeue_next(&mut self) -> Option<ActionSpec> {
        self.queue.pop_front()
    }

    /// Drop pending item `i` (no-op if out of range), keeping the selection in range.
    pub fn remove_queued(&mut self, i: usize) {
        if i < self.queue.len() {
            self.queue.remove(i);
            self.queue_selected = self.queue_selected.min(self.queue.len().saturating_sub(1));
        }
    }

    /// Drop all pending actions and clear the pause state.
    pub fn clear_queue(&mut self) {
        self.queue.clear();
        self.queue_paused = false;
        self.queue_selected = 0;
    }

    /// Move the queue selection by delta, clamped to the pending range.
    pub fn move_queue(&mut self, delta: i32) {
        self.queue_selected = clamp_index(self.queue_selected, delta, self.queue.len());
    }

    /// The source a pending update for `name` comes from, if any.
    pub fn update_source_for(&self, name: &str) -> Option<SourceId> {
        self.updates_list.iter().find(|u| u.name == name).map(|u| u.source_id)
    }

    /// Build an Upgrade spec for the selected installed package, but only when it
    /// has a pending update. The repo path upgrades unqualified; the AUR path
    /// uses the resolved helper (an empty binary when none is installed, which
    /// the caller gates before running).
    pub fn upgrade_one_spec(&self) -> Option<ActionSpec> {
        let pkg = self.selected_installed()?;
        let source = self.update_source_for(&pkg.name)?;
        let aur_bin = self.aur_helper_bin.as_deref().unwrap_or("");
        Some(ActionSpec {
            targets: vec![pkg.name.clone()],
            source_id: source,
            action: Action::Upgrade,
            command: upgrade_one_command(&pkg.name, source, aur_bin),
        })
    }

    // --- upgrade scope selector ---

    /// Present sources in detection order (the basis for the scope chips).
    pub fn present_sources(&self) -> Vec<SourceId> {
        self.source_status.iter().map(|(id, _)| *id).collect()
    }

    /// Number of scope chips: All + one per present source.
    pub fn upgrade_scope_count(&self) -> usize {
        1 + self.source_status.len()
    }

    /// Move the selected scope chip, clamped to the chip range.
    pub fn move_upgrade_scope(&mut self, delta: i32) {
        self.upgrade_scope_selected =
            clamp_index(self.upgrade_scope_selected, delta, self.upgrade_scope_count());
    }

    /// Label for scope chip `i` (0 = "All", else the source badge).
    pub fn upgrade_scope_label(&self, i: usize) -> &'static str {
        match i {
            0 => "All",
            n => self
                .present_sources()
                .get(n - 1)
                .map(|id| id.badge())
                .unwrap_or("?"),
        }
    }

    /// Pending-update count for scope chip `i` (All = every pending update).
    pub fn upgrade_scope_pending(&self, i: usize) -> usize {
        match i {
            0 => self.updates_list.len(),
            n => match self.present_sources().get(n - 1) {
                Some(id) => self.updates_list.iter().filter(|u| u.source_id == *id).count(),
                None => 0,
            },
        }
    }

    /// Build the upgrade spec for the selected scope chip. Chip 0 ("All") chains
    /// every present source's upgrade into one task; a source chip upgrades just
    /// that source.
    pub fn upgrade_spec(&self) -> ActionSpec {
        let sources = self.present_sources();
        // Pacman ignores the helper arg; AUR needs the resolved binary. When no
        // helper is installed the AUR scope is unreachable (gated by the caller),
        // so an empty string here is never actually run.
        let aur_bin = self.aur_helper_bin.as_deref().unwrap_or("");
        let have_aur = self.aur_helper_bin.is_some();
        if self.upgrade_scope_selected == 0 || self.upgrade_scope_selected > sources.len() {
            // "All": chain each present source, but drop the AUR leg when no
            // helper is installed so the repo upgrade still runs.
            let cmds: Vec<_> = sources
                .iter()
                .filter(|id| **id != SourceId::Aur || have_aur)
                .map(|id| source_upgrade_command(*id, aur_bin))
                .collect();
            ActionSpec {
                targets: vec!["all".to_string()],
                source_id: sources.first().copied().unwrap_or(SourceId::Pacman),
                action: Action::Upgrade,
                command: chain_commands(&cmds),
            }
        } else {
            let id = sources[self.upgrade_scope_selected - 1];
            ActionSpec {
                targets: vec![id.badge().to_string()],
                source_id: id,
                action: Action::Upgrade,
                command: source_upgrade_command(id, aur_bin),
            }
        }
    }

    /// Whether the selected upgrade scope can actually run. An AUR-only scope
    /// needs a helper; "All" and repo scopes always can.
    pub fn can_upgrade_selected(&self) -> bool {
        let sources = self.present_sources();
        if self.upgrade_scope_selected == 0 || self.upgrade_scope_selected > sources.len() {
            return true;
        }
        let id = sources[self.upgrade_scope_selected - 1];
        id != SourceId::Aur || self.aur_helper_bin.is_some()
    }

    /// Whether the selected upgrade scope includes the AUR (so an AUR-helper
    /// note is relevant). "All" includes it when AUR is a present source.
    pub fn upgrade_scope_touches_aur(&self) -> bool {
        let sources = self.present_sources();
        if self.upgrade_scope_selected == 0 || self.upgrade_scope_selected > sources.len() {
            sources.contains(&SourceId::Aur)
        } else {
            sources[self.upgrade_scope_selected - 1] == SourceId::Aur
        }
    }

    /// Note for the confirm modal when the configured AUR helper was missing and
    /// Plaza fell back to the other one. `None` when no fallback happened.
    pub fn aur_fallback_note(&self) -> Option<String> {
        if !self.aur_helper_fell_back {
            return None;
        }
        let bin = self.aur_helper_bin.as_deref()?;
        Some(format!(
            "configured AUR helper ({}) not found; using {}",
            self.settings.aur_helper.label(),
            bin
        ))
    }

    /// The fallback note, but only when the action targets the AUR.
    pub fn aur_fallback_note_for(&self, source_id: SourceId) -> Option<String> {
        if source_id == SourceId::Aur {
            self.aur_fallback_note()
        } else {
            None
        }
    }

    /// Move the results selection by delta, clamped to the filtered rows.
    pub fn move_selection(&mut self, delta: i32) {
        self.results_selected = clamp_index(self.results_selected, delta, self.search_rows().len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CommandLine, SourceMeta};

    fn hit(name: &str, source: SourceId) -> PackageHit {
        PackageHit {
            name: name.into(),
            version: "1".into(),
            source_id: source,
            description: String::new(),
            meta: SourceMeta::default(),
        }
    }

    fn ipkg(name: &str) -> InstalledPkg {
        InstalledPkg { name: name.into(), version: "1".into(), origin: "repo".into() }
    }

    #[test]
    fn start_query_bumps_id_and_clears() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        let id1 = app.start_query("fire".into());
        let id2 = app.start_query("firefox".into());
        assert_ne!(id1, id2);
        assert!(app.rows.is_empty());
        assert_eq!(app.query, "firefox");
    }

    #[test]
    fn applies_matching_results_and_ignores_stale() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        let id = app.start_query("firefox".into());

        app.apply_search_results(id, SourceId::Pacman, vec![hit("firefox", SourceId::Pacman)]);
        assert_eq!(app.rows.len(), 1);

        app.apply_search_results(id - 1, SourceId::Aur, vec![hit("firefox-bin", SourceId::Aur)]);
        assert_eq!(app.rows.len(), 1);

        app.apply_search_results(id, SourceId::Aur, vec![hit("firefox-bin", SourceId::Aur)]);
        assert_eq!(app.rows.len(), 2);
        assert_eq!(app.rows[0].name, "firefox");
    }

    #[test]
    fn active_view_index_roundtrip() {
        for v in [ActiveView::Search, ActiveView::Manage] {
            assert_eq!(ActiveView::from_index(v.index()), v);
        }
        assert_eq!(ActiveView::from_index(99), ActiveView::Search);
    }

    #[test]
    fn options_count_matches_rows() {
        assert_eq!(App::OPTIONS_COUNT, 8);
    }

    #[test]
    fn cycling_palette_changes_selection() {
        let mut app = App::new(vec![]);
        let before = app.settings.palette.clone();
        app.cycle_palette();
        assert_ne!(app.settings.palette, before);
        assert_eq!(
            app.palette,
            theme::resolve_palette(&app.palettes, &app.settings.palette)
        );
    }

    #[test]
    fn cycling_skin_changes_selection() {
        let mut app = App::new(vec![]);
        let before = app.settings.skin.clone();
        app.cycle_skin();
        assert_ne!(app.settings.skin, before);
    }

    #[test]
    fn toggle_view_flips_search_and_manage() {
        let mut app = App::new(vec![SourceId::Pacman]);
        assert_eq!(app.active_view, ActiveView::Search);
        app.toggle_view();
        assert_eq!(app.active_view, ActiveView::Manage);
        assert_eq!(app.focus, Focus::List);
        assert!(!app.interacting); // lands in navigate mode, browsing
        app.toggle_view();
        assert_eq!(app.active_view, ActiveView::Search);
        assert_eq!(app.focus, Focus::Search);
        assert!(app.interacting); // ready to type
    }

    #[test]
    fn hover_move_navigates_panels() {
        let mut app = App::new(vec![SourceId::Pacman]);
        // Search view: Search → Main → Sidebar
        app.focus = Focus::Search;
        app.hover_move(Dir::Down);
        assert_eq!(app.focus, Focus::Main);
        app.hover_move(Dir::Left);
        assert_eq!(app.focus, Focus::Sidebar);
        app.hover_move(Dir::Up);
        assert_eq!(app.focus, Focus::Search);
        // Manage view: Search → Scope → List
        app.active_view = ActiveView::Manage;
        app.focus = Focus::Search;
        app.hover_move(Dir::Down);
        assert_eq!(app.focus, Focus::Scope);
        app.hover_move(Dir::Down);
        assert_eq!(app.focus, Focus::List);
        app.hover_move(Dir::Up);
        assert_eq!(app.focus, Focus::Scope);
    }

    #[test]
    fn manage_rows_filter_and_float_updates() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        app.installed_list = vec![ipkg("alpha"), ipkg("firefox"), ipkg("firewalld"), ipkg("zoxide")];
        app.updates_list = vec![UpdateEntry {
            name: "firewalld".into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: SourceId::Pacman,
        }];
        // no filter: upgradable floats to top, rest alphabetical
        let rows: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(rows, vec!["firewalld", "alpha", "firefox", "zoxide"]);
        assert_eq!(app.update_for("firewalld"), Some("2"));
        assert_eq!(app.update_for("firefox"), None);
        // filter narrows to the two "fire*" names (firewalld still first)
        app.manage_filter = "fire".into();
        let rows: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(rows, vec!["firewalld", "firefox"]);
    }

    #[test]
    fn manage_filter_match_beats_upgradable() {
        let mut app = App::new(vec![SourceId::Pacman]);
        // "libfire" is upgradable but only a substring match; "firefox" is a prefix
        // match with no update. The prefix match must win over the upgradable float.
        app.installed_list = vec![ipkg("firefox"), ipkg("libfire")];
        app.updates_list = vec![UpdateEntry {
            name: "libfire".into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: SourceId::Pacman,
        }];
        app.manage_filter = "fire".into();
        let rows: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(rows, vec!["firefox", "libfire"]);
    }

    #[test]
    fn installed_selection_clamps_and_reads() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.installed_list = vec![ipkg("a"), ipkg("b")];
        app.move_installed(-5);
        assert_eq!(app.installed_selected, 0);
        app.move_installed(10);
        assert_eq!(app.installed_selected, 1);
        assert_eq!(app.selected_installed().unwrap().name, "b");
    }

    #[test]
    fn remove_spec_uses_configured_depth() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.installed_list = vec![ipkg("firefox")];
        let spec = app.remove_spec().expect("spec");
        assert_eq!(spec.targets, vec!["firefox"]);
        assert_eq!(spec.action, Action::Remove);
        // default depth is -Rs
        assert_eq!(spec.command.args, vec!["pacman", "-Rs", "firefox"]);

        app.settings.remove_depth = crate::model::RemoveDepth::Purge;
        let purge = app.remove_spec().expect("spec");
        assert_eq!(purge.command.args, vec!["pacman", "-Rns", "firefox"]);
    }

    #[test]
    fn remove_spec_none_when_list_empty() {
        let app = App::new(vec![SourceId::Pacman]);
        assert!(app.remove_spec().is_none());
    }

    /// App with both sources present and yay resolved as the AUR helper.
    fn app_with_yay() -> App {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        app.helpers_available = (true, false);
        app.recompute_aur_helper();
        app
    }

    #[test]
    fn upgrade_spec_all_chains_present_sources() {
        let app = app_with_yay();
        // chip 0 = All
        let all = app.upgrade_spec();
        assert_eq!(all.action, Action::Upgrade);
        assert_eq!(all.command.program, "sh");
        assert_eq!(all.command.args[1], "sudo pacman -Syu && yay -Sua");
    }

    #[test]
    fn upgrade_spec_single_source_scope() {
        let mut app = app_with_yay();
        // chips: [All, repo, aur]
        assert_eq!(app.upgrade_scope_count(), 3);
        app.upgrade_scope_selected = 2; // aur
        let spec = app.upgrade_spec();
        assert_eq!(spec.source_id, SourceId::Aur);
        assert_eq!(spec.command.program, "yay");
        assert_eq!(spec.command.args, vec!["-Sua"]);
    }

    #[test]
    fn upgrade_spec_drops_aur_when_no_helper() {
        // No helper installed: "All" still upgrades repos, AUR leg is dropped.
        let app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        assert!(app.aur_helper_bin.is_none());
        let all = app.upgrade_spec();
        // Single remaining command (pacman) is returned unwrapped by chain_commands.
        assert_eq!(all.command.program, "sudo");
        assert_eq!(all.command.args, vec!["pacman", "-Syu"]);
    }

    #[test]
    fn can_upgrade_selected_gates_aur_only_scope() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        // No helper: All (0) and repo (1) ok, AUR (2) blocked.
        app.upgrade_scope_selected = 0;
        assert!(app.can_upgrade_selected());
        app.upgrade_scope_selected = 1;
        assert!(app.can_upgrade_selected());
        app.upgrade_scope_selected = 2;
        assert!(!app.can_upgrade_selected());
        // With a helper, the AUR scope is allowed.
        app.helpers_available = (true, false);
        app.recompute_aur_helper();
        assert!(app.can_upgrade_selected());
    }

    #[test]
    fn cycle_aur_helper_recomputes_binary() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        app.helpers_available = (true, true); // both installed
        app.recompute_aur_helper();
        // Auto prefers paru.
        assert_eq!(app.aur_helper_bin.as_deref(), Some("paru"));
        // Auto -> yay.
        app.cycle_aur_helper();
        assert_eq!(app.settings.aur_helper, crate::model::AurHelper::Yay);
        assert_eq!(app.aur_helper_bin.as_deref(), Some("yay"));
        // yay -> paru.
        app.cycle_aur_helper();
        assert_eq!(app.settings.aur_helper, crate::model::AurHelper::Paru);
        assert_eq!(app.aur_helper_bin.as_deref(), Some("paru"));
    }

    #[test]
    fn upgrade_scope_pending_counts_per_source() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        let upd = |name: &str, src| UpdateEntry {
            name: name.into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: src,
        };
        app.updates_list = vec![
            upd("a", SourceId::Pacman),
            upd("b", SourceId::Pacman),
            upd("c", SourceId::Aur),
        ];
        assert_eq!(app.upgrade_scope_pending(0), 3); // All
        assert_eq!(app.upgrade_scope_pending(1), 2); // repo
        assert_eq!(app.upgrade_scope_pending(2), 1); // aur
        assert_eq!(app.upgrade_scope_label(1), "repo");
        assert_eq!(app.upgrade_scope_label(2), "aur");
    }

    fn spec(name: &str, action: Action) -> ActionSpec {
        ActionSpec {
            targets: vec![name.into()],
            source_id: SourceId::Pacman,
            action,
            command: CommandLine { program: "true".into(), args: vec![] },
        }
    }

    #[test]
    fn enqueue_and_dequeue_preserve_order() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.enqueue(spec("a", Action::Install));
        app.enqueue(spec("b", Action::Remove));
        assert_eq!(app.queue.len(), 2);
        assert_eq!(app.dequeue_next().unwrap().targets, vec!["a"]);
        assert_eq!(app.dequeue_next().unwrap().targets, vec!["b"]);
        assert!(app.dequeue_next().is_none());
    }

    #[test]
    fn remove_queued_drops_item_and_clamps_selection() {
        let mut app = App::new(vec![SourceId::Pacman]);
        for n in ["a", "b", "c"] {
            app.enqueue(spec(n, Action::Install));
        }
        app.queue_selected = 2;
        app.remove_queued(2);
        let names: Vec<&str> = app.queue.iter().map(|s| s.targets[0].as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(app.queue_selected, 1); // clamped into the shorter list
        app.remove_queued(5); // out of range is a no-op
        assert_eq!(app.queue.len(), 2);
    }

    #[test]
    fn clear_queue_empties_and_resets_pause() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.enqueue(spec("a", Action::Install));
        app.queue_paused = true;
        app.queue_selected = 0;
        app.clear_queue();
        assert!(app.queue.is_empty());
        assert!(!app.queue_paused);
        assert_eq!(app.queue_selected, 0);
    }

    #[test]
    fn move_queue_clamps_to_pending_range() {
        let mut app = App::new(vec![SourceId::Pacman]);
        for n in ["a", "b"] {
            app.enqueue(spec(n, Action::Install));
        }
        app.move_queue(-5);
        assert_eq!(app.queue_selected, 0);
        app.move_queue(10);
        assert_eq!(app.queue_selected, 1);
    }

    #[test]
    fn update_source_for_reads_updates_list() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        app.updates_list = vec![
            UpdateEntry { name: "firefox".into(), old_version: "1".into(), new_version: "2".into(), source_id: SourceId::Pacman },
            UpdateEntry { name: "yay".into(), old_version: "1".into(), new_version: "2".into(), source_id: SourceId::Aur },
        ];
        assert_eq!(app.update_source_for("firefox"), Some(SourceId::Pacman));
        assert_eq!(app.update_source_for("yay"), Some(SourceId::Aur));
        assert_eq!(app.update_source_for("zoxide"), None);
    }

    #[test]
    fn upgrade_one_spec_for_selected_updatable_package() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.installed_list = vec![ipkg("firefox"), ipkg("zoxide")];
        app.updates_list = vec![UpdateEntry {
            name: "firefox".into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: SourceId::Pacman,
        }];
        // firefox floats to the top (has an update), so it is selected at index 0.
        let s = app.upgrade_one_spec().expect("spec");
        assert_eq!(s.action, Action::Upgrade);
        assert_eq!(s.targets, vec!["firefox"]);
        assert_eq!(s.command.args, vec!["pacman", "-S", "firefox"]);
        // A package with no pending update yields nothing.
        app.installed_selected = 1; // zoxide
        assert!(app.upgrade_one_spec().is_none());
    }

    fn prov(source: SourceId, repo: &str) -> Provider {
        Provider {
            source_id: source,
            version: "1".into(),
            installed: false,
            installed_version: None,
            meta: SourceMeta {
                repo: (source == SourceId::Pacman).then(|| repo.to_string()),
                ..Default::default()
            },
        }
    }

    fn row_with(name: &str, providers: Vec<Provider>) -> PackageRow {
        PackageRow { name: name.into(), providers, best_description: String::new() }
    }

    fn app_with_repos() -> App {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        app.filter_repos = vec!["extra".into(), "world".into(), "multilib".into()];
        app
    }

    #[test]
    fn repo_shown_reflects_off_set() {
        let mut app = app_with_repos();
        assert!(app.repo_shown("extra"));
        app.repo_filter_off.insert("extra".into());
        assert!(!app.repo_shown("extra"));
        assert!(app.repo_shown("world"));
    }

    #[test]
    fn toggle_master_fills_and_clears_pacman_repos_only() {
        let mut app = app_with_repos();
        app.repo_filter_off.insert("aur".into()); // aur off, must stay untouched
        // master is row 0; checked initially (no repo off)
        assert!(app.pacman_master_checked());
        app.filter_selected = 0;
        app.toggle_filter(); // uncheck master -> all pacman repos off
        assert!(!app.repo_shown("extra"));
        assert!(!app.repo_shown("world"));
        assert!(!app.repo_shown("multilib"));
        assert!(!app.repo_shown("aur")); // unchanged
        assert!(!app.pacman_master_checked());
        app.toggle_filter(); // re-check master -> all pacman repos back on
        assert!(app.repo_shown("extra"));
        assert!(app.pacman_master_checked());
        assert!(!app.repo_shown("aur")); // still off
    }

    #[test]
    fn toggle_single_repo_and_aur() {
        let mut app = app_with_repos();
        // rows: [all repos, extra, world, multilib, aur]
        app.filter_selected = 1; // extra
        app.toggle_filter();
        assert!(!app.repo_shown("extra"));
        assert!(!app.pacman_master_checked()); // one repo off
        app.filter_selected = 4; // aur
        app.toggle_filter();
        assert!(!app.repo_shown("aur"));
    }

    #[test]
    fn filter_checkboxes_expanded_and_collapsed() {
        let mut app = app_with_repos();
        let labels: Vec<String> =
            app.filter_checkboxes().into_iter().map(|r| r.label).collect();
        assert_eq!(labels, vec!["all repos", "extra", "world", "multilib", "aur"]);
        app.settings.collapse_repos = true;
        let labels: Vec<String> =
            app.filter_checkboxes().into_iter().map(|r| r.label).collect();
        assert_eq!(labels, vec!["repo", "aur"]);
    }

    #[test]
    fn search_rows_filters_by_shown_repos() {
        let mut app = app_with_repos();
        app.rows = vec![
            row_with("a", vec![prov(SourceId::Pacman, "extra")]),
            row_with("b", vec![prov(SourceId::Pacman, "multilib")]),
            // multi-provider: shown while any provider's repo is shown
            row_with("c", vec![prov(SourceId::Pacman, "multilib"), prov(SourceId::Aur, "")]),
        ];
        app.repo_filter_off.insert("multilib".into());
        let names: Vec<&str> = app.search_rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c"]); // b dropped; c kept via its aur provider
        app.repo_filter_off.insert("aur".into());
        let names: Vec<&str> = app.search_rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a"]); // now c's providers are all off
    }

    #[test]
    fn manage_rows_drops_filtered_origins() {
        let mut app = app_with_repos();
        app.installed_list = vec![
            InstalledPkg { name: "a".into(), version: "1".into(), origin: "extra".into() },
            InstalledPkg { name: "b".into(), version: "1".into(), origin: "aur".into() },
        ];
        app.repo_filter_off.insert("aur".into());
        let names: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["a"]);
    }

    #[test]
    fn toggling_filter_clamps_results_selection() {
        let mut app = app_with_repos();
        app.rows = vec![
            row_with("a", vec![prov(SourceId::Pacman, "extra")]),
            row_with("b", vec![prov(SourceId::Pacman, "multilib")]),
        ];
        app.results_selected = 1; // on "b"
        app.filter_selected = 3; // multilib
        app.toggle_filter(); // hides "b"
        assert_eq!(app.search_rows().len(), 1);
        assert_eq!(app.results_selected, 0); // clamped
    }

    #[test]
    fn filter_box_visible_follows_option() {
        let mut app = app_with_repos();
        // Option off: always on screen, regardless of focus or active filter.
        app.settings.hide_idle_filter = false;
        app.focus = Focus::Search;
        assert!(app.filter_box_visible());
        // Option on: shows only while focused or while a filter is active.
        app.settings.hide_idle_filter = true;
        assert!(!app.filter_box_visible());
        app.focus = Focus::Filter;
        assert!(app.filter_box_visible()); // focused (open or hovered)
        app.focus = Focus::Search;
        app.repo_filter_off.insert("extra".into());
        assert!(app.filter_box_visible()); // active filter keeps it up
    }

    #[test]
    fn leaving_filter_box_does_not_strand_it_visible() {
        // Regression: opening the box then jumping away (e.g. `/` or Tab) used to
        // leave a stranded "open" flag that pinned the box on screen and defeated
        // the hide-when-idle option. Visibility now follows focus.
        let mut app = app_with_repos();
        app.settings.hide_idle_filter = true;
        app.open_filter(); // `f`
        assert_eq!(app.focus, Focus::Filter);
        assert!(app.filter_box_visible());
        app.focus = Focus::Search; // `/` jumps away without close_filter
        assert!(!app.filter_box_visible());
    }

    #[test]
    fn selection_clamps_within_bounds() {
        let mut app = App::new(vec![SourceId::Pacman]);
        let id = app.start_query("f".into());
        app.apply_search_results(
            id,
            SourceId::Pacman,
            vec![hit("a", SourceId::Pacman), hit("b", SourceId::Pacman)],
        );
        app.move_selection(-5);
        assert_eq!(app.results_selected, 0);
        app.move_selection(10);
        assert_eq!(app.results_selected, 1);
        assert_eq!(app.selected_row().unwrap().name, app.rows[1].name);
    }
}
