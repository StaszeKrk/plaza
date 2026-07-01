use crate::action::runner::ActiveTask;
use crate::config::Settings;
use crate::model::{
    chain_commands, remove_command, remove_command_apt, remove_command_flatpak,
    source_upgrade_command,
    upgrade_one_command, Action, ActionSpec, InstalledStats, PackageDetail, PackageHit, PackageRow,
    Provider, SortDir, SortKey, SourceId, UpdatesInfo,
};
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use crate::search::aggregator::{merge, rank, relevance_sort};
use crate::sources::installed::{InstalledIndex, InstalledPkg};
use crate::sources::updates::UpdateEntry;
use crate::theme::{self, palette::Palette, skin::Skin};
use std::time::SystemTime;

/// A focusable panel. In the Search view the content panel is `Main`; in the
/// Manage view the content is the installed `List`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Sidebar,
    Main,
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
    /// Flatpak.
    Flatpak,
    /// apt (Debian).
    Apt,
    /// A Manage installation-reason choice (radio: All/Explicit/Orphans). Shown in
    /// the filter box only in the Manage view.
    Reason(crate::model::ReasonFilter),
    /// A Manage sort-key choice (radio over Name/Size/Updated). Activating the
    /// already-active key flips the sort direction. Manage view only.
    Sort(crate::model::SortKey),
    /// Action row: save the active view's current filter as its launch default
    /// (same as the `s` hotkey, but reachable with the cursor).
    SaveDefault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRow {
    pub label: String,
    pub checked: bool,
    pub id: FilterId,
}

/// A source badge for a results row: its label, its source, and the number of
/// providers sharing that label (e.g. three AUR variants -> `count == 3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadgeGroup {
    pub label: String,
    pub source_id: SourceId,
    pub count: usize,
}

/// A single setting in the Options overlay. Dispatch keys on this, not a row
/// index, so categories and headers can be inserted without renumbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionId {
    ShowHotkeys,
    Palette,
    Skin,
    Highlight,
    SearchDelay,
    CollapseRepos,
    StackVariants,
    GroupFlatpak,
    VariantBadge,
    RemoveDepth,
    AurHelper,
    FlatpakAppId,
    FloatUpdates,
    HideIdleFilter,
}

/// Per-view repo-filter state: the unchecked repo ids and the box cursor. Search
/// and Manage each keep their own, so a repo hidden in one view is unaffected in
/// the other. The box (`f`) always edits the active view's instance.
#[derive(Debug, Default, Clone)]
pub struct RepoFilter {
    pub off: BTreeSet<String>,
    pub selected: usize,
}

/// Hover-movement direction (navigate mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// What pressing Enter on the selected sidebar row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    /// An upgrade row: `upgrade_scope_selected` is now set; open the confirm.
    Upgrade,
    /// A VIEWS row: `active_view` is now switched.
    SwitchView,
}

/// One choice in the per-package Manage menu (see [`ManageMenu`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Upgrade,
    Remove,
    Cancel,
}

/// The per-package action chooser shown on Enter over an upgradable Manage row.
/// It offers Upgrade / Remove / Cancel; choosing one funnels into the normal
/// confirm modal, so the final y/n step (and its command preview) is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManageMenu {
    pub pkg: String,
    pub new_version: String,
    pub selected: usize,
}

impl ManageMenu {
    pub const ACTIONS: [MenuAction; 3] =
        [MenuAction::Upgrade, MenuAction::Remove, MenuAction::Cancel];

    /// The action under the cursor.
    pub fn action(&self) -> MenuAction {
        Self::ACTIONS[self.selected.min(Self::ACTIONS.len() - 1)]
    }
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

/// Case-insensitive name order (display, then exact name), always ascending.
/// Used as the primary order for the Name key and as the stable tiebreak for
/// every key.
fn name_cmp(a: &InstalledPkg, b: &InstalledPkg) -> std::cmp::Ordering {
    a.display
        .to_ascii_lowercase()
        .cmp(&b.display.to_ascii_lowercase())
        .then_with(|| a.name.cmp(&b.name))
}

/// Order two optional values per `dir`, with known values always before unknown
/// ones (a `None` sorts last in either direction, so unresolved size/date rows
/// collect at the end instead of jumping to the top under descending).
fn opt_cmp<T: Ord>(a: Option<T>, b: Option<T>, dir: SortDir) -> std::cmp::Ordering {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match (a, b) {
        (Some(x), Some(y)) => {
            let o = x.cmp(&y);
            if dir == SortDir::Desc {
                o.reverse()
            } else {
                o
            }
        }
        (Some(_), None) => Less,
        (None, Some(_)) => Greater,
        (None, None) => Equal,
    }
}

/// Compare two installed packages by the chosen key+direction, falling back to
/// ascending name for a stable, predictable order on ties.
fn key_cmp(a: &InstalledPkg, b: &InstalledPkg, key: SortKey, dir: SortDir) -> std::cmp::Ordering {
    let primary = match key {
        SortKey::Name => {
            let o = name_cmp(a, b);
            if dir == SortDir::Desc {
                o.reverse()
            } else {
                o
            }
        }
        SortKey::Size => opt_cmp(a.size, b.size, dir),
        SortKey::Updated => opt_cmp(a.install_date, b.install_date, dir),
    };
    primary.then_with(|| name_cmp(a, b))
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
    /// Persisted list scroll offsets (results and Manage). Kept across frames so
    /// the viewport stays put when the selection moves within it, instead of
    /// re-deriving from 0 each render (which pins the selection to the bottom).
    /// `Cell` because rendering takes `&App` but must write the adjusted offset
    /// back; the app is single-threaded so this needs no synchronization.
    pub results_offset: Cell<usize>,
    pub manage_offset: Cell<usize>,
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
    /// Manage installation-reason filter (All/Explicit/Orphans). Seeded from
    /// `settings.default_reason`, cycled with `e`.
    pub manage_reason: crate::model::ReasonFilter,
    /// Manage list sort key/direction and whether upgradable packages float to
    /// the top. All seeded from settings and persisted by "save as default".
    pub manage_sort_key: crate::model::SortKey,
    pub manage_sort_dir: crate::model::SortDir,
    pub manage_float_updates: bool,
    /// Cached `pacman -Qi` detail per installed package name, with in-flight
    /// names tracked so the same fetch is not spawned twice while scrolling.
    pub manage_detail: HashMap<String, crate::sources::installed::PkgDetail>,
    pub manage_detail_inflight: HashSet<String>,
    /// Repo filter (the `f` box), per view. Each `RepoFilter.off` is the set of
    /// *unchecked* repo identifiers (concrete repo names plus "aur"); empty means
    /// show all. Seeded at launch from the persisted defaults. `filter_repos` is
    /// the stable, ordered list of pacman repos (from `pacman -Sl`) shared by both
    /// views' checkbox rows.
    pub search_filter: RepoFilter,
    pub manage_filter_repo: RepoFilter,
    pub filter_repos: Vec<String>,
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
    /// Whether pacman is on PATH (probed once at startup). Drives hiding of
    /// Arch-only UI; independent of `disabled_sources`.
    pub pacman_present: bool,
    pub confirm: Option<ActionSpec>,
    pub confirm_note: Option<String>,
    /// Per-package action chooser, opened by Enter on an upgradable Manage row so
    /// Enter never silently guesses upgrade-vs-remove. `None` when closed.
    pub manage_menu: Option<ManageMenu>,
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
        Self::with_settings(source_ids, Settings::load())
    }

    /// Build the app with explicit settings (so tests can inject defaults).
    pub fn with_settings(source_ids: Vec<SourceId>, settings: Settings) -> Self {
        let source_status = source_ids
            .into_iter()
            .map(|id| (id, SourceState::Done(0)))
            .collect();
        let search_filter = RepoFilter {
            off: settings.default_search_filter_off.iter().cloned().collect(),
            selected: 0,
        };
        let manage_filter_repo = RepoFilter {
            off: settings.default_manage_filter_off.iter().cloned().collect(),
            selected: 0,
        };
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
            results_offset: Cell::new(0),
            manage_offset: Cell::new(0),
            details: HashMap::new(),
            detail_requested: HashSet::new(),
            sidebar_selected: 0,
            installed_list: Vec::new(),
            installed_selected: 0,
            manage_filter: String::new(),
            manage_reason: settings.default_reason,
            manage_sort_key: settings.default_manage_sort_key,
            manage_sort_dir: settings.default_manage_sort_dir,
            manage_float_updates: settings.default_manage_float_updates,
            manage_detail: HashMap::new(),
            manage_detail_inflight: HashSet::new(),
            search_filter,
            manage_filter_repo,
            filter_repos: Vec::new(),
            updates_list: Vec::new(),
            upgrade_scope_selected: 0,
            interacting: true,
            has_checkupdates: false,
            pacman_present: false,
            confirm: None,
            confirm_note: None,
            manage_menu: None,
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
            // Collapse only the pacman repos to the default (highest-priority) one;
            // keep every non-pacman provider (AUR, Flatpak, …) as-is so they are
            // still shown, fetched for detail, and installable.
            let mut out: Vec<&Provider> = Vec::new();
            if let Some(p) = row.providers.iter().find(|p| p.source_id == SourceId::Pacman) {
                out.push(p);
            }
            out.extend(row.providers.iter().filter(|p| p.source_id != SourceId::Pacman));
            out
        } else {
            row.providers.iter().collect()
        }
    }

    /// Ordered source badges for a row: each distinct badge label once, with the
    /// number of providers sharing it (so three AUR variants give `aur` count 3).
    pub fn badge_groups(&self, row: &PackageRow) -> Vec<BadgeGroup> {
        let mut groups: Vec<BadgeGroup> = Vec::new();
        for p in self.effective_providers(row) {
            let label = self.provider_badge(p).to_string();
            match groups.iter_mut().find(|g| g.label == label) {
                Some(g) => g.count += 1,
                None => groups.push(BadgeGroup { label, source_id: p.source_id, count: 1 }),
            }
        }
        groups
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

    /// Open the per-package action menu for the highlighted Manage row, but only
    /// when it has a pending update. Returns false (and opens nothing) otherwise,
    /// so the caller can fall back to a direct remove for an up-to-date package.
    pub fn open_manage_menu(&mut self) -> bool {
        let Some(pkg) = self.selected_installed() else { return false };
        let Some(version) = self.update_for(&pkg.name) else { return false };
        self.manage_menu = Some(ManageMenu {
            pkg: pkg.name.clone(),
            new_version: version.to_string(),
            selected: 0,
        });
        true
    }

    /// Move the Manage menu cursor, clamped to the action list.
    pub fn move_manage_menu(&mut self, delta: i32) {
        if let Some(menu) = &mut self.manage_menu {
            let max = ManageMenu::ACTIONS.len() as i32 - 1;
            menu.selected = (menu.selected as i32 + delta).clamp(0, max) as usize;
        }
    }

    pub fn close_manage_menu(&mut self) {
        self.manage_menu = None;
    }

    // --- Options overlay ---

    /// The options menu grouped into categories. Drives both rendering and
    /// navigation; headers are not selectable.
    pub fn option_layout() -> &'static [(&'static str, &'static [OptionId])] {
        use OptionId::*;
        &[
            ("Appearance", &[Palette, Skin, Highlight]),
            ("Search", &[SearchDelay, CollapseRepos, StackVariants, GroupFlatpak, VariantBadge]),
            ("Manage", &[RemoveDepth, AurHelper, FlatpakAppId, FloatUpdates]),
            ("Filters", &[HideIdleFilter]),
            ("General", &[ShowHotkeys]),
        ]
    }

    /// The selectable option ids in layout order (the cursor indexes this).
    pub fn flat_options() -> Vec<OptionId> {
        Self::option_layout()
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect()
    }

    /// The option under the cursor.
    pub fn selected_option(&self) -> OptionId {
        let flat = Self::flat_options();
        flat[self.options_selected.min(flat.len() - 1)]
    }

    pub fn move_options(&mut self, delta: i32) {
        let max = Self::flat_options().len() as i32 - 1;
        let next = (self.options_selected as i32 + delta).clamp(0, max);
        self.options_selected = next as usize;
    }

    pub fn toggle_option(&mut self) {
        match self.selected_option() {
            OptionId::ShowHotkeys => self.settings.show_hotkeys = !self.settings.show_hotkeys,
            OptionId::CollapseRepos => self.settings.collapse_repos = !self.settings.collapse_repos,
            OptionId::StackVariants => self.settings.stack_variants = !self.settings.stack_variants,
            OptionId::GroupFlatpak => self.settings.group_flatpak = !self.settings.group_flatpak,
            OptionId::VariantBadge => {
                self.settings.variant_badge = self.settings.variant_badge.next()
            }
            OptionId::FlatpakAppId => self.settings.flatpak_app_id = !self.settings.flatpak_app_id,
            OptionId::FloatUpdates => {
                self.manage_float_updates = !self.manage_float_updates;
                self.settings.default_manage_float_updates = self.manage_float_updates;
            }
            OptionId::Palette => self.cycle_palette(),
            OptionId::Skin => self.cycle_skin(),
            OptionId::Highlight => self.settings.highlight = self.settings.highlight.next(),
            OptionId::SearchDelay => {
                self.settings.debounce_ms = next_debounce(self.settings.debounce_ms)
            }
            OptionId::RemoveDepth => self.settings.remove_depth = self.settings.remove_depth.next(),
            OptionId::AurHelper => self.cycle_aur_helper(),
            OptionId::HideIdleFilter => {
                self.settings.hide_idle_filter = !self.settings.hide_idle_filter
            }
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
            ActiveView::Manage => Focus::List,
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
    /// the left, content on the right.
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

            (Focus::List, Dir::Up) => Focus::Search,
            (Focus::List, Dir::Left) => Focus::Sidebar,
            (Focus::List, Dir::Right) if self.task_pane_visible() => Focus::TaskPane,

            (Focus::TaskPane, Dir::Left) => self.content_landing(),

            (f, _) => f,
        };
        // Entering the sidebar defaults the cursor to the current view's row.
        if next == Focus::Sidebar && self.focus != Focus::Sidebar {
            self.sidebar_selected = self.sidebar_view_row(self.active_view);
        }
        self.focus = next;
    }

    pub fn move_sidebar(&mut self, delta: i32) {
        let max = self.sidebar_item_count() as i32 - 1;
        let next = (self.sidebar_selected as i32 + delta).clamp(0, max);
        self.sidebar_selected = next as usize;
    }

    /// Selectable upgrade rows in the sidebar: one per present source, then a
    /// "total" row. These precede the VIEWS rows in the combined list.
    pub fn sidebar_upgrade_rows(&self) -> usize {
        self.source_status.len() + 1
    }

    /// Index of the "total" upgrade row (last of the upgrade rows).
    pub fn sidebar_total_row(&self) -> usize {
        self.source_status.len()
    }

    /// Total number of selectable sidebar rows (upgrade rows + VIEWS rows).
    pub fn sidebar_item_count(&self) -> usize {
        self.sidebar_upgrade_rows() + VIEW_COUNT
    }

    /// The `sidebar_selected` index of `view`'s VIEWS row.
    pub fn sidebar_view_row(&self, view: ActiveView) -> usize {
        self.sidebar_upgrade_rows() + view.index()
    }

    /// If the selected sidebar row is an upgrade row, the upgrade scope it maps to
    /// (`0` = All/total, `n` = the nth present source). `None` for VIEWS rows.
    pub fn sidebar_selected_scope(&self) -> Option<usize> {
        let n = self.source_status.len();
        match self.sidebar_selected {
            i if i < n => Some(i + 1), // source row -> that source's scope
            i if i == n => Some(0),    // total row -> All
            _ => None,                 // VIEWS row
        }
    }

    /// Total pending updates across sources, or `None` when no source's count is
    /// known yet (e.g. pacman-contrib missing and flatpak not yet checked).
    pub fn total_updates(&self) -> Option<usize> {
        let any_known = self.updates.repo.is_some()
            || self.updates.aur.is_some()
            || self.updates.flatpak.is_some()
            || self.updates.apt.is_some();
        any_known.then_some(self.updates_list.len())
    }

    /// Act on the selected sidebar row. Upgrade rows set the upgrade scope and
    /// return `Upgrade`; VIEWS rows switch the active view and return `SwitchView`.
    pub fn activate_sidebar(&mut self) -> SidebarAction {
        match self.sidebar_selected_scope() {
            Some(scope) => {
                self.upgrade_scope_selected = scope;
                SidebarAction::Upgrade
            }
            None => {
                let view_idx = self.sidebar_selected - self.sidebar_upgrade_rows();
                self.active_view = ActiveView::from_index(view_idx);
                if self.active_view == ActiveView::Manage {
                    self.clamp_installed();
                }
                SidebarAction::SwitchView
            }
        }
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

    /// Reset to the empty/welcome state: discard any in-flight results (by
    /// bumping the query id), drop the result rows, and zero the footer source
    /// counters. Used when the query is cleared back to blank so a stale list
    /// from an intermediate query does not linger.
    pub fn clear_search(&mut self) {
        self.query.clear();
        self.query_id += 1;
        self.hits_buffer.clear();
        self.rows.clear();
        self.results_selected = 0;
        for (_, state) in &mut self.source_status {
            *state = SourceState::Done(0);
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
        self.sidebar_selected = self.sidebar_view_row(ActiveView::Search);
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
        self.rows = merge(
            self.hits_buffer.clone(),
            &self.installed,
            self.effective_stack_variants(),
            self.settings.group_flatpak,
        );
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

    /// Toggle between the Search and Manage views (the `Tab` key). Lands ready to
    /// type in Search, or browsing the list in Manage.
    pub fn toggle_view(&mut self) {
        self.active_view = match self.active_view {
            ActiveView::Search => ActiveView::Manage,
            ActiveView::Manage => ActiveView::Search,
        };
        self.sidebar_selected = self.sidebar_view_row(self.active_view);
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
            .filter(|p| {
                q.is_empty()
                    || p.name.to_ascii_lowercase().contains(&q)
                    || p.display.to_ascii_lowercase().contains(&q)
            })
            .filter(|p| !self.manage_filter_repo.off.contains(&p.origin))
            .filter(|p| match self.manage_reason {
                crate::model::ReasonFilter::All => true,
                crate::model::ReasonFilter::Explicit => p.explicit,
                crate::model::ReasonFilter::Orphans => p.orphan,
            })
            .collect();
        let key = self.manage_sort_key;
        let dir = self.manage_sort_dir;
        let float = self.manage_float_updates;
        rows.sort_by(|a, b| {
            let (au, bu) = (updates.contains(a.name.as_str()), updates.contains(b.name.as_str()));
            if q.is_empty() {
                // No filter: optionally float upgradable packages up, then sort by
                // the chosen key+direction.
                if float {
                    bu.cmp(&au).then_with(|| key_cmp(a, b, key, dir))
                } else {
                    key_cmp(a, b, key, dir)
                }
            } else {
                // Filtering: match quality (exact > prefix > substring) wins over the
                // upgradable float, so typing a name surfaces it even if something
                // upgradable also matches. The chosen sort breaks ties within a rank
                // (`key_cmp` ends with a total name order, so it is the final
                // discriminator).
                rank(&q, &a.name)
                    .cmp(&rank(&q, &b.name))
                    .then_with(|| bu.cmp(&au))
                    .then_with(|| key_cmp(a, b, key, dir))
            }
        });
        rows
    }

    /// Choose the Manage sort key. Selecting a new key applies that key's default
    /// direction; selecting the key already active flips the direction.
    pub fn select_sort(&mut self, key: SortKey) {
        if self.manage_sort_key == key {
            self.manage_sort_dir = self.manage_sort_dir.flip();
        } else {
            self.manage_sort_key = key;
            self.manage_sort_dir = key.default_dir();
        }
    }

    /// The label shown for an installed package in the Manage list: the human
    /// name normally, or the reverse-DNS app ID for Flatpak when the
    /// `flatpak_app_id` option is on.
    pub fn manage_label<'a>(&self, pkg: &'a InstalledPkg) -> &'a str {
        if self.settings.flatpak_app_id && pkg.origin == "flatpak" {
            &pkg.name
        } else {
            &pkg.display
        }
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

    /// The repo filter for the active view (Manage vs Search). The box edits and
    /// renders this one.
    pub fn active_filter(&self) -> &RepoFilter {
        match self.active_view {
            ActiveView::Manage => &self.manage_filter_repo,
            _ => &self.search_filter,
        }
    }

    pub fn active_filter_mut(&mut self) -> &mut RepoFilter {
        match self.active_view {
            ActiveView::Manage => &mut self.manage_filter_repo,
            _ => &mut self.search_filter,
        }
    }

    /// Whether `repo` (a concrete repo name or "aur") is shown in the active
    /// view's filter. Box rendering (checkboxes, master) reads this; the result
    /// and Manage lists key on their own view's set directly.
    pub fn repo_shown(&self, repo: &str) -> bool {
        !self.active_filter().off.contains(repo)
    }

    /// With the hide-when-idle option off, the box is always on screen. With it
    /// on, the box shows only while it is the focused panel (opened or hovered) or
    /// while a filter is active (so it doubles as the "filter active" indicator).
    pub fn filter_box_visible(&self) -> bool {
        !self.settings.hide_idle_filter
            || self.focus == Focus::Filter
            || !self.active_filter().off.is_empty()
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
        if self.present_sources().contains(&SourceId::Flatpak) {
            rows.push(FilterRow {
                label: "flatpak".into(),
                checked: self.repo_shown("flatpak"),
                id: FilterId::Flatpak,
            });
        }
        if self.present_sources().contains(&SourceId::Apt) {
            rows.push(FilterRow {
                label: "apt".into(),
                checked: self.repo_shown("apt"),
                id: FilterId::Apt,
            });
        }
        // Reason rows (radio) live in the Manage view only.
        if self.active_view == ActiveView::Manage {
            use crate::model::ReasonFilter::*;
            for r in [All, Explicit, Orphans] {
                rows.push(FilterRow {
                    label: r.label().to_string(),
                    checked: self.manage_reason == r,
                    id: FilterId::Reason(r),
                });
            }
            // Sort rows (radio over the keys) plus the float-updates checkbox.
            use crate::model::SortKey::*;
            for k in [Name, Size, Updated] {
                rows.push(FilterRow {
                    label: k.label().to_string(),
                    checked: self.manage_sort_key == k,
                    id: FilterId::Sort(k),
                });
            }
        }
        // Action row: save the current filter as the launch default.
        rows.push(FilterRow {
            label: "save as default".to_string(),
            checked: false,
            id: FilterId::SaveDefault,
        });
        rows
    }

    /// Flip one identifier in/out of the active view's unchecked set.
    fn toggle_repo_off(&mut self, id: &str) {
        let off = &mut self.active_filter_mut().off;
        if !off.remove(id) {
            off.insert(id.to_string());
        }
    }

    /// Toggle the highlighted checkbox. The master flips every pacman repo
    /// together; a repo or `aur` row flips just itself. Re-clamps both list
    /// selections, since the visible rows may shrink.
    pub fn toggle_filter(&mut self) {
        let Some(row) = self.filter_checkboxes().get(self.active_filter().selected).cloned() else {
            return;
        };
        match row.id {
            FilterId::Master => {
                let repos = self.filter_repos.clone();
                let checked = self.pacman_master_checked();
                let off = &mut self.active_filter_mut().off;
                if checked {
                    off.extend(repos);
                } else {
                    for r in &repos {
                        off.remove(r);
                    }
                }
            }
            FilterId::Repo(r) => self.toggle_repo_off(&r),
            FilterId::Aur => self.toggle_repo_off("aur"),
            FilterId::Flatpak => self.toggle_repo_off("flatpak"),
            FilterId::Apt => self.toggle_repo_off("apt"),
            FilterId::Reason(r) => self.manage_reason = r, // radio: select
            FilterId::Sort(k) => self.select_sort(k),      // radio, or flip dir
            FilterId::SaveDefault => {
                self.save_filter_default();
                return; // not a filter change; nothing to re-clamp
            }
        }
        self.clamp_after_filter();
    }

    /// Move the filter-box cursor, clamped to the rendered rows.
    pub fn move_filter(&mut self, delta: i32) {
        let len = self.filter_checkboxes().len();
        let cur = self.active_filter().selected;
        self.active_filter_mut().selected = clamp_index(cur, delta, len);
    }

    /// Persist the active view's current repo filter as its launch default
    /// (the `s` key in the filter box). Saves only the active view.
    pub fn save_filter_default(&mut self) {
        let mut off: Vec<String> = self.active_filter().off.iter().cloned().collect();
        off.sort();
        let which = match self.active_view {
            ActiveView::Manage => {
                self.settings.default_manage_filter_off = off;
                self.settings.default_reason = self.manage_reason;
                self.settings.default_manage_sort_key = self.manage_sort_key;
                self.settings.default_manage_sort_dir = self.manage_sort_dir;
                "Manage"
            }
            _ => {
                self.settings.default_search_filter_off = off;
                "Search"
            }
        };
        self.settings.save();
        self.status_msg = Some(format!("saved {which} filter default"));
    }

    /// Open the filter box and focus it (the `f` key).
    pub fn open_filter(&mut self) {
        self.focus = Focus::Filter;
        self.interacting = true;
        let max = self.filter_checkboxes().len().saturating_sub(1);
        let cur = self.active_filter().selected;
        self.active_filter_mut().selected = cur.min(max);
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
            .filter(|row| {
                row.providers
                    .iter()
                    .any(|p| !self.search_filter.off.contains(p.badge()))
            })
            .collect()
    }

    /// Build a Remove spec for the selected installed package (using the
    /// depth configured in Options), or None if nothing is selected.
    pub fn remove_spec(&self) -> Option<ActionSpec> {
        let pkg = self.selected_installed()?;
        // Flatpak removal goes through `flatpak uninstall`, not pacman, and has no
        // `-R` depth family; route on the origin set by the installed-list builder.
        if pkg.origin == "flatpak" {
            return Some(ActionSpec {
                targets: vec![pkg.name.clone()],
                source_id: SourceId::Flatpak,
                action: Action::Remove,
                command: remove_command_flatpak(&pkg.name),
            });
        }
        // apt removal goes through `apt-get`, mapping the depth to apt verbs.
        if pkg.origin == "apt" {
            return Some(ActionSpec {
                targets: vec![pkg.name.clone()],
                source_id: SourceId::Apt,
                action: Action::Remove,
                command: remove_command_apt(&pkg.name, self.settings.remove_depth),
            });
        }
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

    /// Variant stacking (`-bin`/`-git`) only makes sense on Arch/AUR; force it off
    /// on non-Arch so Debian names like `python3-git` are never mis-merged.
    pub fn effective_stack_variants(&self) -> bool {
        self.settings.stack_variants && self.pacman_present
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

    /// Serializes the few tests that mutate the process-global `XDG_CONFIG_HOME`,
    /// so they cannot interleave with each other under parallel test threads.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
        InstalledPkg { name: name.into(), version: "1".into(), origin: "repo".into(), ..Default::default() }
    }

    fn ipkg_meta(name: &str, size: Option<u64>, date: Option<i64>) -> InstalledPkg {
        InstalledPkg {
            name: name.into(),
            display: name.into(),
            size,
            install_date: date,
            ..Default::default()
        }
    }

    #[test]
    fn float_updates_is_an_option_not_a_filter_row() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.active_view = ActiveView::Manage;
        assert!(!app.filter_checkboxes().iter().any(|r| r.label.contains("float")));
        let before = app.manage_float_updates;
        app.options_selected =
            App::flat_options().iter().position(|o| *o == OptionId::FloatUpdates).unwrap();
        app.toggle_option();
        assert_eq!(app.manage_float_updates, !before);
        assert_eq!(app.settings.default_manage_float_updates, !before);
    }

    #[test]
    fn filter_box_has_sort_rows_in_manage() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.active_view = ActiveView::Manage;
        let rows = app.filter_checkboxes();
        assert!(rows.iter().any(|r| r.id == FilterId::Sort(SortKey::Name)));
        assert!(rows.iter().any(|r| r.id == FilterId::Sort(SortKey::Size)));
        assert!(rows.iter().any(|r| r.id == FilterId::Sort(SortKey::Updated)));
        let name_row = rows.iter().find(|r| r.id == FilterId::Sort(SortKey::Name)).unwrap();
        assert!(name_row.checked); // active key is checked
        app.active_view = ActiveView::Search;
        assert!(!app.filter_checkboxes().iter().any(|r| matches!(r.id, FilterId::Sort(_))));
    }

    #[test]
    fn toggling_sort_row_selects_and_persists() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.active_view = ActiveView::Manage;
        let idx = app
            .filter_checkboxes()
            .iter()
            .position(|r| r.id == FilterId::Sort(SortKey::Size))
            .unwrap();
        app.active_filter_mut().selected = idx;
        app.toggle_filter();
        assert_eq!(app.manage_sort_key, SortKey::Size);
        assert_eq!(app.manage_sort_dir, SortDir::Desc);
        app.save_filter_default();
        assert_eq!(app.settings.default_manage_sort_key, SortKey::Size);
        assert_eq!(app.settings.default_manage_sort_dir, SortDir::Desc);
        assert!(app.settings.default_manage_float_updates);
    }

    #[test]
    fn select_sort_sets_default_dir_then_flips() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        assert_eq!(app.manage_sort_key, SortKey::Name);
        app.select_sort(SortKey::Size);
        assert_eq!(app.manage_sort_key, SortKey::Size);
        assert_eq!(app.manage_sort_dir, SortDir::Desc); // size default
        app.select_sort(SortKey::Size); // same key flips
        assert_eq!(app.manage_sort_dir, SortDir::Asc);
        app.select_sort(SortKey::Name); // new key -> its default
        assert_eq!(app.manage_sort_dir, SortDir::Asc);
    }

    #[test]
    fn manage_rows_sorts_by_size_and_puts_none_last() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.installed_list = vec![
            ipkg_meta("a", Some(10), None),
            ipkg_meta("b", Some(300), None),
            ipkg_meta("c", None, None),
            ipkg_meta("d", Some(50), None),
        ];
        app.manage_float_updates = false;
        app.manage_sort_key = SortKey::Size;
        app.manage_sort_dir = SortDir::Desc;
        let order: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, vec!["b", "d", "a", "c"]);
        app.manage_sort_dir = SortDir::Asc;
        let order: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, vec!["a", "d", "b", "c"]); // None still last
    }

    #[test]
    fn manage_rows_float_updates_overrides_sort() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.installed_list =
            vec![ipkg_meta("a", Some(10), None), ipkg_meta("b", Some(300), None)];
        app.updates_list = vec![UpdateEntry {
            name: "a".into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: SourceId::Pacman,
        }];
        app.manage_sort_key = SortKey::Size;
        app.manage_sort_dir = SortDir::Desc;
        app.manage_float_updates = true;
        let order: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, vec!["a", "b"]); // upgradable 'a' floats above larger 'b'
    }

    #[test]
    fn manage_rows_name_sort_is_case_insensitive_both_dirs() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.installed_list = vec![ipkg_meta("Bravo", None, None), ipkg_meta("alpha", None, None)];
        app.manage_float_updates = false;
        app.manage_sort_key = SortKey::Name;
        app.manage_sort_dir = SortDir::Asc;
        assert_eq!(
            app.manage_rows().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "Bravo"]
        );
        app.manage_sort_dir = SortDir::Desc;
        assert_eq!(
            app.manage_rows().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["Bravo", "alpha"]
        );
    }

    #[test]
    fn start_query_bumps_id_and_clears() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        let id1 = app.start_query("fire".into());
        let id2 = app.start_query("firefox".into());
        assert_ne!(id1, id2);
        assert!(app.rows.is_empty());
        assert_eq!(app.query, "firefox");
    }

    #[test]
    fn clear_search_drops_rows_and_discards_inflight() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        let id = app.start_query("firefox".into());
        app.apply_search_results(id, SourceId::Pacman, vec![hit("firefox", SourceId::Pacman)]);
        assert_eq!(app.rows.len(), 1);

        app.clear_search();
        assert!(app.query.is_empty());
        assert!(app.rows.is_empty());
        assert_eq!(app.results_selected, 0);
        // Counters back to zero, not the previous hit count.
        assert!(matches!(app.source_status[0].1, SourceState::Done(0)));
        // The bumped id means a late result from the old query is now stale.
        app.apply_search_results(id, SourceId::Aur, vec![hit("firefox-bin", SourceId::Aur)]);
        assert!(app.rows.is_empty());
    }

    #[test]
    fn applies_matching_results_and_ignores_stale() {
        // Keep variants separate so this stays a test about staleness, not grouping.
        let settings = Settings { stack_variants: false, ..Settings::default() };
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], settings);
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
    fn effective_stack_variants_requires_pacman() {
        let settings = Settings { stack_variants: true, ..Settings::default() };
        let mut app = App::with_settings(vec![SourceId::Apt], settings);
        app.pacman_present = true;
        assert!(app.effective_stack_variants());
        app.pacman_present = false;
        assert!(!app.effective_stack_variants()); // forced off on non-Arch
    }

    #[test]
    fn active_view_index_roundtrip() {
        for v in [ActiveView::Search, ActiveView::Manage] {
            assert_eq!(ActiveView::from_index(v.index()), v);
        }
        assert_eq!(ActiveView::from_index(99), ActiveView::Search);
    }

    #[test]
    fn flat_options_has_every_id_once() {
        let flat = App::flat_options();
        let mut seen = flat.clone();
        seen.sort_by_key(|o| *o as usize);
        seen.dedup_by_key(|o| *o as usize);
        assert_eq!(flat.len(), seen.len(), "duplicate OptionId in layout");
        assert!(!flat.is_empty());
    }

    #[test]
    fn move_options_clamps_to_flat_len() {
        let mut app = App::with_settings(vec![], Settings::default());
        app.move_options(-5);
        assert_eq!(app.options_selected, 0);
        app.move_options(1000);
        assert_eq!(app.options_selected, App::flat_options().len() - 1);
    }

    #[test]
    fn toggle_show_hotkeys_via_id() {
        let _env = ENV_LOCK.lock().unwrap();
        // toggle_option persists; keep it off the real ~/.config.
        let tmp = std::env::temp_dir().join(format!("plaza-opt-test-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
        let mut app = App::with_settings(vec![], Settings::default());
        let idx = App::flat_options()
            .iter()
            .position(|o| *o == OptionId::ShowHotkeys)
            .unwrap();
        app.options_selected = idx;
        let before = app.settings.show_hotkeys;
        app.toggle_option();
        assert_ne!(app.settings.show_hotkeys, before);
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn cycling_palette_changes_selection() {
        let mut app = App::with_settings(vec![], Settings::default());
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
        let mut app = App::with_settings(vec![], Settings::default());
        let before = app.settings.skin.clone();
        app.cycle_skin();
        assert_ne!(app.settings.skin, before);
    }

    #[test]
    fn toggle_view_flips_search_and_manage() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        // Search view: Search → Main → Sidebar
        app.focus = Focus::Search;
        app.hover_move(Dir::Down);
        assert_eq!(app.focus, Focus::Main);
        app.hover_move(Dir::Left);
        assert_eq!(app.focus, Focus::Sidebar);
        app.hover_move(Dir::Up);
        assert_eq!(app.focus, Focus::Search);
        // Manage view: Search → List
        app.active_view = ActiveView::Manage;
        app.focus = Focus::Search;
        app.hover_move(Dir::Down);
        assert_eq!(app.focus, Focus::List);
        app.hover_move(Dir::Up);
        assert_eq!(app.focus, Focus::Search);
    }

    #[test]
    fn manage_rows_filter_and_float_updates() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
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
    fn manage_menu_opens_only_for_upgradable() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.installed_list = vec![ipkg("firefox"), ipkg("firewalld")];
        app.updates_list = vec![UpdateEntry {
            name: "firewalld".into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: SourceId::Pacman,
        }];
        // upgradable row floats to the top.
        app.installed_selected = 0;
        assert!(app.open_manage_menu());
        let menu = app.manage_menu.as_ref().unwrap();
        assert_eq!(menu.pkg, "firewalld");
        assert_eq!(menu.new_version, "2");
        assert_eq!(menu.action(), MenuAction::Upgrade);
        app.close_manage_menu();
        assert!(app.manage_menu.is_none());

        // a row with no pending update does not open the menu.
        app.installed_selected = 1;
        assert!(!app.open_manage_menu());
        assert!(app.manage_menu.is_none());
    }

    #[test]
    fn manage_menu_move_clamps_and_maps_actions() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.installed_list = vec![ipkg("firewalld")];
        app.updates_list = vec![UpdateEntry {
            name: "firewalld".into(),
            old_version: "1".into(),
            new_version: "2".into(),
            source_id: SourceId::Pacman,
        }];
        app.open_manage_menu();
        assert_eq!(app.manage_menu.as_ref().unwrap().action(), MenuAction::Upgrade);
        app.move_manage_menu(1);
        assert_eq!(app.manage_menu.as_ref().unwrap().action(), MenuAction::Remove);
        app.move_manage_menu(5); // clamps at the last action
        assert_eq!(app.manage_menu.as_ref().unwrap().action(), MenuAction::Cancel);
        app.move_manage_menu(-9); // clamps at the first
        assert_eq!(app.manage_menu.as_ref().unwrap().action(), MenuAction::Upgrade);
    }

    #[test]
    fn manage_filter_match_beats_upgradable() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.installed_list = vec![ipkg("a"), ipkg("b")];
        app.move_installed(-5);
        assert_eq!(app.installed_selected, 0);
        app.move_installed(10);
        assert_eq!(app.installed_selected, 1);
        assert_eq!(app.selected_installed().unwrap().name, "b");
    }

    #[test]
    fn remove_spec_uses_configured_depth() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
    fn remove_spec_routes_flatpak_to_uninstall() {
        let mut app = App::with_settings(vec![SourceId::Flatpak], Settings::default());
        app.installed_list = vec![InstalledPkg {
            name: "org.mozilla.firefox".into(),
            version: "1".into(),
            origin: "flatpak".into(),
            ..Default::default()
        }];
        let spec = app.remove_spec().expect("spec");
        assert_eq!(spec.source_id, SourceId::Flatpak);
        assert_eq!(spec.command.program, "flatpak");
        assert_eq!(spec.command.args, vec!["uninstall", "--user", "org.mozilla.firefox"]);
    }

    #[test]
    fn apt_filter_row_present_and_toggles() {
        let mut app = App::with_settings(vec![SourceId::Apt], Settings::default());
        let rows = app.filter_checkboxes();
        assert!(rows.iter().any(|r| matches!(r.id, FilterId::Apt)));
        assert!(app.repo_shown("apt")); // shown by default
        app.toggle_repo_off("apt");
        assert!(!app.repo_shown("apt")); // toggled off
    }

    #[test]
    fn remove_spec_routes_apt_to_apt_get() {
        let mut app = App::with_settings(vec![SourceId::Apt], Settings::default());
        app.installed_list = vec![InstalledPkg {
            name: "vim".into(),
            version: "9.0".into(),
            origin: "apt".into(),
            ..Default::default()
        }];
        let spec = app.remove_spec().expect("spec");
        assert_eq!(spec.source_id, SourceId::Apt);
        assert_eq!(spec.command.program, "sudo");
        assert_eq!(spec.command.args[0], "apt-get");
        assert_eq!(spec.command.args.last().unwrap(), "vim");
    }

    #[test]
    fn remove_spec_none_when_list_empty() {
        let app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        assert!(app.remove_spec().is_none());
    }

    /// App with both sources present and yay resolved as the AUR helper.
    fn app_with_yay() -> App {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
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
        app.upgrade_scope_selected = 2; // aur
        let spec = app.upgrade_spec();
        assert_eq!(spec.source_id, SourceId::Aur);
        assert_eq!(spec.command.program, "yay");
        assert_eq!(spec.command.args, vec!["-Sua"]);
    }

    #[test]
    fn upgrade_spec_drops_aur_when_no_helper() {
        // No helper installed: "All" still upgrades repos, AUR leg is dropped.
        let app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        assert!(app.aur_helper_bin.is_none());
        let all = app.upgrade_spec();
        // Single remaining command (pacman) is returned unwrapped by chain_commands.
        assert_eq!(all.command.program, "sudo");
        assert_eq!(all.command.args, vec!["pacman", "-Syu"]);
    }

    #[test]
    fn can_upgrade_selected_gates_aur_only_scope() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        // App::new loads the real user config; pin the setting so the test does
        // not depend on the build machine's ~/.config/plaza/settings.json.
        app.settings.aur_helper = crate::model::AurHelper::Auto;
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
        app.enqueue(spec("a", Action::Install));
        app.enqueue(spec("b", Action::Remove));
        assert_eq!(app.queue.len(), 2);
        assert_eq!(app.dequeue_next().unwrap().targets, vec!["a"]);
        assert_eq!(app.dequeue_next().unwrap().targets, vec!["b"]);
        assert!(app.dequeue_next().is_none());
    }

    #[test]
    fn remove_queued_drops_item_and_clamps_selection() {
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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
            target: "pkg".into(),
            meta: SourceMeta {
                repo: (source == SourceId::Pacman).then(|| repo.to_string()),
                ..Default::default()
            },
        }
    }

    fn row_with(name: &str, providers: Vec<Provider>) -> PackageRow {
        PackageRow { name: name.into(), providers, best_description: String::new() }
    }

    #[test]
    fn badge_groups_collapses_same_label_runs() {
        let app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        let row = row_with(
            "cork-rs",
            vec![
                prov(SourceId::Pacman, "extra"),
                prov(SourceId::Aur, ""),
                prov(SourceId::Aur, ""),
                prov(SourceId::Aur, ""),
            ],
        );
        let groups = app.badge_groups(&row);
        let summary: Vec<(&str, usize)> =
            groups.iter().map(|g| (g.label.as_str(), g.count)).collect();
        assert_eq!(summary, vec![("extra", 1), ("aur", 3)]);
    }

    fn app_with_repos() -> App {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        app.filter_repos = vec!["extra".into(), "world".into(), "multilib".into()];
        app
    }

    #[test]
    fn repo_shown_reflects_off_set() {
        let mut app = app_with_repos();
        assert!(app.repo_shown("extra"));
        app.search_filter.off.insert("extra".into());
        assert!(!app.repo_shown("extra"));
        assert!(app.repo_shown("world"));
    }

    #[test]
    fn manage_rows_filter_by_reason() {
        use crate::model::ReasonFilter;
        let mut app = app_with_repos();
        app.installed_list = vec![
            InstalledPkg { name: "firefox".into(), explicit: true, ..Default::default() },
            InstalledPkg { name: "libfoo".into(), ..Default::default() },
            InstalledPkg { name: "ldb".into(), orphan: true, ..Default::default() },
        ];
        app.manage_reason = ReasonFilter::All;
        assert_eq!(app.manage_rows().len(), 3);
        app.manage_reason = ReasonFilter::Explicit;
        let n: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(n, vec!["firefox"]);
        app.manage_reason = ReasonFilter::Orphans;
        let n: Vec<&str> = app.manage_rows().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(n, vec!["ldb"]);
    }

    #[test]
    fn selecting_reason_row_sets_filter_and_clamps() {
        use crate::model::ReasonFilter;
        let mut app = app_with_repos();
        app.active_view = ActiveView::Manage;
        app.installed_list = vec![
            InstalledPkg { name: "firefox".into(), explicit: true, ..Default::default() },
            InstalledPkg { name: "ldb".into(), orphan: true, ..Default::default() },
        ];
        app.installed_selected = 1;
        // find and select the "explicit" reason row in the filter box
        let rows = app.filter_checkboxes();
        let idx = rows
            .iter()
            .position(|r| r.id == FilterId::Reason(ReasonFilter::Explicit))
            .unwrap();
        app.manage_filter_repo.selected = idx;
        app.toggle_filter();
        assert_eq!(app.manage_reason, ReasonFilter::Explicit);
        assert_eq!(app.installed_selected, 0); // clamped after rows shrank
    }

    #[test]
    fn save_filter_default_persists_active_view_only() {
        // Redirect config writes to a temp dir so the test never touches the real
        // ~/.config. save() reads XDG_CONFIG_HOME (see config::config_base).
        let _env = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("plaza-test-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
        let mut app = App::with_settings(
            vec![SourceId::Pacman, SourceId::Aur],
            Settings::default(),
        );
        app.filter_repos = vec!["extra".into(), "world".into(), "multilib".into()];
        app.active_view = ActiveView::Manage;
        app.manage_filter_repo.off.insert("multilib".into());
        app.save_filter_default();
        assert_eq!(app.settings.default_manage_filter_off, vec!["multilib".to_string()]);
        assert!(app.settings.default_search_filter_off.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    #[test]
    fn new_app_seeds_filters_from_defaults() {
        let settings = Settings {
            default_search_filter_off: vec!["world".into()],
            default_manage_filter_off: vec!["aur".into()],
            ..Default::default()
        };
        let app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], settings);
        assert!(app.search_filter.off.contains("world"));
        assert!(app.manage_filter_repo.off.contains("aur"));
    }

    #[test]
    fn active_filter_follows_view() {
        let mut app = app_with_repos();
        app.active_view = ActiveView::Search;
        app.active_filter_mut().off.insert("extra".into());
        assert!(app.active_filter().off.contains("extra"));
        app.active_view = ActiveView::Manage;
        assert!(app.active_filter().off.is_empty());
    }

    #[test]
    fn per_view_filters_are_independent() {
        let mut app = app_with_repos();
        app.rows = vec![row_with("b", vec![prov(SourceId::Pacman, "multilib")])];
        app.installed_list = vec![InstalledPkg {
            name: "b".into(),
            version: "1".into(),
            origin: "multilib".into(),
            ..Default::default()
        }];
        // Hide multilib in Search only.
        app.search_filter.off.insert("multilib".into());
        assert!(app.search_rows().is_empty()); // search hides it
        assert_eq!(app.manage_rows().len(), 1); // manage unaffected
    }

    #[test]
    fn toggle_master_fills_and_clears_pacman_repos_only() {
        let mut app = app_with_repos();
        app.search_filter.off.insert("aur".into()); // aur off, must stay untouched
        // master is row 0; checked initially (no repo off)
        assert!(app.pacman_master_checked());
        app.search_filter.selected = 0;
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
        app.search_filter.selected = 1; // extra
        app.toggle_filter();
        assert!(!app.repo_shown("extra"));
        assert!(!app.pacman_master_checked()); // one repo off
        app.search_filter.selected = 4; // aur
        app.toggle_filter();
        assert!(!app.repo_shown("aur"));
    }

    #[test]
    fn filter_checkboxes_expanded_and_collapsed() {
        let mut app = app_with_repos();
        let labels: Vec<String> =
            app.filter_checkboxes().into_iter().map(|r| r.label).collect();
        assert_eq!(
            labels,
            vec!["all repos", "extra", "world", "multilib", "aur", "save as default"]
        );
        app.settings.collapse_repos = true;
        let labels: Vec<String> =
            app.filter_checkboxes().into_iter().map(|r| r.label).collect();
        assert_eq!(labels, vec!["repo", "aur", "save as default"]);
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
        app.search_filter.off.insert("multilib".into());
        let names: Vec<&str> = app.search_rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c"]); // b dropped; c kept via its aur provider
        app.search_filter.off.insert("aur".into());
        let names: Vec<&str> = app.search_rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a"]); // now c's providers are all off
    }

    #[test]
    fn manage_rows_drops_filtered_origins() {
        let mut app = app_with_repos();
        app.installed_list = vec![
            InstalledPkg { name: "a".into(), version: "1".into(), origin: "extra".into(), ..Default::default() },
            InstalledPkg { name: "b".into(), version: "1".into(), origin: "aur".into(), ..Default::default() },
        ];
        app.manage_filter_repo.off.insert("aur".into());
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
        app.search_filter.selected = 3; // multilib
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
        app.search_filter.off.insert("extra".into());
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
        let mut app = App::with_settings(vec![SourceId::Pacman], Settings::default());
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

    #[test]
    fn sidebar_index_model_maps_rows_to_scopes() {
        let mut app = App::with_settings(
            vec![SourceId::Pacman, SourceId::Aur, SourceId::Flatpak],
            Settings::default(),
        );
        // 3 sources + total = 4 upgrade rows; + 2 VIEWS = 6 items total.
        assert_eq!(app.sidebar_upgrade_rows(), 4);
        assert_eq!(app.sidebar_total_row(), 3);
        assert_eq!(app.sidebar_item_count(), 6);
        assert_eq!(app.sidebar_view_row(ActiveView::Search), 4);
        assert_eq!(app.sidebar_view_row(ActiveView::Manage), 5);

        // App does not derive Clone, so mutate the one app and check each index.
        let expected = [Some(1), Some(2), Some(3), Some(0), None, None];
        for (i, want) in expected.iter().enumerate() {
            app.sidebar_selected = i;
            assert_eq!(app.sidebar_selected_scope(), *want, "row {i}");
        }
    }

    #[test]
    fn move_sidebar_clamps_across_full_list() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        // 2 sources + total + 2 views = 5 items, max index 4.
        app.sidebar_selected = 0;
        app.move_sidebar(-1);
        assert_eq!(app.sidebar_selected, 0);
        app.move_sidebar(100);
        assert_eq!(app.sidebar_selected, 4);
    }

    #[test]
    fn activate_sidebar_upgrade_row_sets_scope() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        app.sidebar_selected = 1; // aur upgrade row
        assert_eq!(app.activate_sidebar(), SidebarAction::Upgrade);
        assert_eq!(app.upgrade_scope_selected, 2);

        app.sidebar_selected = app.sidebar_total_row(); // total
        assert_eq!(app.activate_sidebar(), SidebarAction::Upgrade);
        assert_eq!(app.upgrade_scope_selected, 0);
    }

    #[test]
    fn activate_sidebar_view_row_switches_view() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        app.sidebar_selected = app.sidebar_view_row(ActiveView::Manage);
        assert_eq!(app.activate_sidebar(), SidebarAction::SwitchView);
        assert_eq!(app.active_view, ActiveView::Manage);
    }

    #[test]
    fn total_updates_is_none_until_a_source_count_is_known() {
        let mut app = App::with_settings(vec![SourceId::Pacman, SourceId::Aur], Settings::default());
        assert_eq!(app.total_updates(), None);
        app.updates.repo = Some(2);
        app.updates_list = vec![
            UpdateEntry {
                name: "a".into(),
                old_version: "1".into(),
                new_version: "2".into(),
                source_id: SourceId::Pacman,
            },
            UpdateEntry {
                name: "b".into(),
                old_version: "1".into(),
                new_version: "2".into(),
                source_id: SourceId::Pacman,
            },
            UpdateEntry {
                name: "c".into(),
                old_version: "1".into(),
                new_version: "2".into(),
                source_id: SourceId::Pacman,
            },
        ];
        // total_updates returns updates_list.len(), not the per-source field.
        assert_eq!(app.total_updates(), Some(3));
    }
}
