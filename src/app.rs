use crate::action::runner::ActiveTask;
use crate::config::Settings;
use crate::model::{
    chain_commands, remove_command, source_upgrade_command, Action, ActionSpec, InstalledStats,
    PackageHit, PackageRow, Provider, SourceId, UpdatesInfo,
};
use crate::search::aggregator::{merge, relevance_sort};
use crate::sources::installed::{InstalledIndex, InstalledPkg};
use crate::sources::updates::UpdateEntry;

/// A focusable panel. In the Search view the content panel is `Main`; in the
/// Manage view it splits into `Scope` (upgrade chips) and `List` (installed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Sidebar,
    Main,
    Scope,
    List,
    TaskPane,
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
    pub sidebar_selected: usize,
    /// All installed packages (`pacman -Qn` + `-Qm`) for the Manage view.
    pub installed_list: Vec<InstalledPkg>,
    pub installed_selected: usize,
    /// Manage-view filter text. Kept separate from `query` so each tab keeps its
    /// own search box contents.
    pub manage_filter: String,
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
    pub task_view: TaskView,
    pub task_seq: u64,
    pub settings: Settings,
    pub options_open: bool,
    pub options_selected: usize,
    pub should_quit: bool,
}

impl App {
    pub fn new(source_ids: Vec<SourceId>) -> Self {
        let source_status = source_ids
            .into_iter()
            .map(|id| (id, SourceState::Done(0)))
            .collect();
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
            sidebar_selected: 0,
            installed_list: Vec::new(),
            installed_selected: 0,
            manage_filter: String::new(),
            updates_list: Vec::new(),
            upgrade_scope_selected: 0,
            interacting: true,
            has_checkupdates: false,
            confirm: None,
            confirm_note: None,
            task: None,
            task_view: TaskView::Hidden,
            task_seq: 0,
            settings: Settings::load(),
            options_open: false,
            options_selected: 0,
            should_quit: false,
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
    pub const OPTIONS_COUNT: usize = 4;

    pub fn move_options(&mut self, delta: i32) {
        let max = Self::OPTIONS_COUNT as i32 - 1;
        let next = (self.options_selected as i32 + delta).clamp(0, max);
        self.options_selected = next as usize;
    }

    pub fn toggle_option(&mut self) {
        match self.options_selected {
            0 => self.settings.show_hotkeys = !self.settings.show_hotkeys,
            1 => self.settings.collapse_repos = !self.settings.collapse_repos,
            2 => self.settings.debounce_ms = next_debounce(self.settings.debounce_ms),
            3 => self.settings.remove_depth = self.settings.remove_depth.next(),
            _ => {}
        }
        self.settings.save();
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
        if self.results_selected >= self.rows.len() {
            self.results_selected = self.rows.len().saturating_sub(1);
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
        self.rows.get(self.results_selected)
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
            .collect();
        rows.sort_by(|a, b| {
            let (au, bu) = (updates.contains(a.name.as_str()), updates.contains(b.name.as_str()));
            bu.cmp(&au).then_with(|| a.name.cmp(&b.name))
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
        if self.upgrade_scope_selected == 0 || self.upgrade_scope_selected > sources.len() {
            let cmds: Vec<_> = sources.iter().map(|id| source_upgrade_command(*id)).collect();
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
                command: source_upgrade_command(id),
            }
        }
    }

    /// Move the results selection by delta, clamped to [0, len-1].
    pub fn move_selection(&mut self, delta: i32) {
        if self.rows.is_empty() {
            self.results_selected = 0;
            return;
        }
        let max = self.rows.len() as i32 - 1;
        let next = (self.results_selected as i32 + delta).clamp(0, max);
        self.results_selected = next as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceMeta;

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

    #[test]
    fn upgrade_spec_all_chains_present_sources() {
        let app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        // chip 0 = All
        let all = app.upgrade_spec();
        assert_eq!(all.action, Action::Upgrade);
        assert_eq!(all.command.program, "sh");
        assert_eq!(all.command.args[1], "sudo pacman -Syu && yay -Sua");
    }

    #[test]
    fn upgrade_spec_single_source_scope() {
        let mut app = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        // chips: [All, repo, aur]
        assert_eq!(app.upgrade_scope_count(), 3);
        app.upgrade_scope_selected = 2; // aur
        let spec = app.upgrade_spec();
        assert_eq!(spec.source_id, SourceId::Aur);
        assert_eq!(spec.command.program, "yay");
        assert_eq!(spec.command.args, vec!["-Sua"]);
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
