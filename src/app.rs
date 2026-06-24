use crate::action::runner::ActiveTask;
use crate::config::Settings;
use crate::model::{
    remove_command, upgrade_command, Action, ActionSpec, InstalledStats, PackageHit, PackageRow,
    Provider, SourceId, UpdatesInfo,
};
use crate::search::aggregator::{merge, relevance_sort};
use crate::sources::installed::{InstalledIndex, InstalledPkg};
use crate::sources::updates::UpdateEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Sidebar,
    Main,
    TaskPane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainView {
    Results,
    Detail,
}

/// Which sidebar VIEW is active. Drives what the center area shows. The Search
/// view keeps the existing Results/Detail sub-state in `MainView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Search,
    Installed,
    Updates,
}

impl ActiveView {
    /// The sidebar VIEWS index for this view.
    pub fn index(self) -> usize {
        match self {
            ActiveView::Search => 0,
            ActiveView::Installed => 1,
            ActiveView::Updates => 2,
        }
    }

    /// The view at sidebar VIEWS index `i` (out-of-range falls back to Search).
    pub fn from_index(i: usize) -> ActiveView {
        match i {
            1 => ActiveView::Installed,
            2 => ActiveView::Updates,
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

/// Number of entries in the sidebar VIEWS list (Search / Installed / Updates).
pub const VIEW_COUNT: usize = 3;

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
    /// Explicitly-installed packages (`pacman -Qe`) for the Installed view.
    pub installed_list: Vec<InstalledPkg>,
    pub installed_selected: usize,
    /// Upgradable packages (repos + AUR) for the Updates view.
    pub updates_list: Vec<UpdateEntry>,
    pub updates_selected: usize,
    /// Whether `yay` is present, so Upgrade can target it (repos + AUR).
    pub has_yay: bool,
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
        // The AUR source is only enabled when yay is installed, so its presence
        // is a reliable proxy for "yay available" (used by the Upgrade action).
        let has_yay = source_ids.contains(&SourceId::Aur);
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
            updates_list: Vec::new(),
            updates_selected: 0,
            has_yay,
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
    pub const OPTIONS_COUNT: usize = 3;

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
            _ => {}
        }
        self.settings.save();
    }

    /// Is the task pane currently on screen (peek or expanded)?
    pub fn task_pane_visible(&self) -> bool {
        self.task.is_some() && self.task_view != TaskView::Hidden
    }

    /// Panes that can hold focus right now, in Tab order.
    fn visible_panes(&self) -> Vec<Focus> {
        let mut panes = vec![Focus::Search, Focus::Sidebar, Focus::Main];
        if self.task_pane_visible() {
            panes.push(Focus::TaskPane);
        }
        panes
    }

    /// Horizontal body row (left → right), used for h/l / arrow movement.
    fn body_panes(&self) -> Vec<Focus> {
        let mut panes = vec![Focus::Sidebar, Focus::Main];
        if self.task_pane_visible() {
            panes.push(Focus::TaskPane);
        }
        panes
    }

    pub fn focus_next(&mut self) {
        let panes = self.visible_panes();
        let i = panes.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = panes[(i + 1) % panes.len()];
    }

    pub fn focus_prev(&mut self) {
        let panes = self.visible_panes();
        let i = panes.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = panes[(i + panes.len() - 1) % panes.len()];
    }

    pub fn focus_left(&mut self) {
        let panes = self.body_panes();
        match panes.iter().position(|f| *f == self.focus) {
            Some(i) if i > 0 => self.focus = panes[i - 1],
            None => self.focus = Focus::Main, // coming from Search
            _ => {}
        }
    }

    pub fn focus_right(&mut self) {
        let panes = self.body_panes();
        match panes.iter().position(|f| *f == self.focus) {
            Some(i) if i + 1 < panes.len() => self.focus = panes[i + 1],
            None => self.focus = Focus::Main, // coming from Search
            _ => {}
        }
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
            self.focus = Focus::Main;
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

    /// Activate the view highlighted in the sidebar and reset its selection.
    pub fn select_sidebar_view(&mut self) {
        self.active_view = ActiveView::from_index(self.sidebar_selected);
        match self.active_view {
            ActiveView::Installed => {
                self.installed_selected = self
                    .installed_selected
                    .min(self.installed_list.len().saturating_sub(1));
            }
            ActiveView::Updates => {
                self.updates_selected = self
                    .updates_selected
                    .min(self.updates_list.len().saturating_sub(1));
            }
            ActiveView::Search => {}
        }
    }

    // --- Installed view ---

    pub fn selected_installed(&self) -> Option<&InstalledPkg> {
        self.installed_list.get(self.installed_selected)
    }

    /// Move the Installed-view selection by delta, clamped to the list.
    pub fn move_installed(&mut self, delta: i32) {
        self.installed_selected = clamp_index(self.installed_selected, delta, self.installed_list.len());
    }

    /// Build a Remove spec for the selected installed package, or None if the
    /// list is empty. `recursive` chooses `-Rns` over plain `-R`.
    pub fn remove_spec(&self, recursive: bool) -> Option<ActionSpec> {
        let pkg = self.selected_installed()?;
        Some(ActionSpec {
            targets: vec![pkg.name.clone()],
            source_id: SourceId::Pacman,
            action: Action::Remove { recursive },
            command: remove_command(&pkg.name, recursive),
        })
    }

    // --- Updates view ---

    pub fn selected_update(&self) -> Option<&UpdateEntry> {
        self.updates_list.get(self.updates_selected)
    }

    /// Move the Updates-view selection by delta, clamped to the list.
    pub fn move_updates(&mut self, delta: i32) {
        self.updates_selected = clamp_index(self.updates_selected, delta, self.updates_list.len());
    }

    /// Build the "upgrade everything" spec. Uses yay when present (repos + AUR).
    pub fn upgrade_spec(&self) -> ActionSpec {
        ActionSpec {
            targets: vec!["all".to_string()],
            source_id: if self.has_yay { SourceId::Aur } else { SourceId::Pacman },
            action: Action::Upgrade,
            command: upgrade_command(self.has_yay),
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
        for v in [ActiveView::Search, ActiveView::Installed, ActiveView::Updates] {
            assert_eq!(ActiveView::from_index(v.index()), v);
        }
        assert_eq!(ActiveView::from_index(99), ActiveView::Search);
    }

    #[test]
    fn select_sidebar_view_switches_active_view() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.sidebar_selected = 1;
        app.select_sidebar_view();
        assert_eq!(app.active_view, ActiveView::Installed);
        app.sidebar_selected = 2;
        app.select_sidebar_view();
        assert_eq!(app.active_view, ActiveView::Updates);
        app.sidebar_selected = 0;
        app.select_sidebar_view();
        assert_eq!(app.active_view, ActiveView::Search);
    }

    #[test]
    fn installed_selection_clamps_and_reads() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.installed_list = vec![
            InstalledPkg { name: "a".into(), version: "1".into() },
            InstalledPkg { name: "b".into(), version: "2".into() },
        ];
        app.move_installed(-5);
        assert_eq!(app.installed_selected, 0);
        app.move_installed(10);
        assert_eq!(app.installed_selected, 1);
        assert_eq!(app.selected_installed().unwrap().name, "b");
    }

    #[test]
    fn remove_spec_uses_selected_installed_package() {
        let mut app = App::new(vec![SourceId::Pacman]);
        app.installed_list = vec![InstalledPkg { name: "firefox".into(), version: "1".into() }];
        let spec = app.remove_spec(false).expect("spec");
        assert_eq!(spec.targets, vec!["firefox"]);
        assert_eq!(spec.action, Action::Remove { recursive: false });
        assert_eq!(spec.command.args, vec!["pacman", "-R", "firefox"]);

        let recursive = app.remove_spec(true).expect("spec");
        assert_eq!(recursive.action, Action::Remove { recursive: true });
        assert_eq!(recursive.command.args, vec!["pacman", "-Rns", "firefox"]);
    }

    #[test]
    fn remove_spec_none_when_list_empty() {
        let app = App::new(vec![SourceId::Pacman]);
        assert!(app.remove_spec(false).is_none());
    }

    #[test]
    fn upgrade_spec_targets_yay_when_present() {
        let with_yay = App::new(vec![SourceId::Pacman, SourceId::Aur]);
        let spec = with_yay.upgrade_spec();
        assert_eq!(spec.action, Action::Upgrade);
        assert_eq!(spec.command.program, "yay");

        let no_yay = App::new(vec![SourceId::Pacman]);
        assert_eq!(no_yay.upgrade_spec().command.program, "sudo");
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
