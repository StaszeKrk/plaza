use crate::action::runner::ActiveTask;
use crate::config::Settings;
use crate::model::{
    ActionSpec, InstalledStats, PackageHit, PackageRow, Provider, SourceId, UpdatesInfo,
};
use crate::search::aggregator::{merge, relevance_sort};
use crate::sources::installed::InstalledIndex;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    Loading,
    Done(usize),
    Error,
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
    pub main_view: MainView,
    pub results_selected: usize,
    pub detail_selected: usize,
    pub sidebar_selected: usize,
    pub confirm: Option<ActionSpec>,
    pub confirm_note: Option<String>,
    pub task: Option<ActiveTask>,
    pub task_view: TaskView,
    pub task_seq: u64,
    pub settings: Settings,
    pub repos: Vec<String>,
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
            main_view: MainView::Results,
            results_selected: 0,
            detail_selected: 0,
            sidebar_selected: 0,
            confirm: None,
            confirm_note: None,
            task: None,
            task_view: TaskView::Hidden,
            task_seq: 0,
            settings: Settings::load(),
            repos: Vec::new(),
            options_open: false,
            options_selected: 0,
            should_quit: false,
        }
    }

    /// Providers of `row` that aren't filtered out by hidden-repo settings.
    /// (AUR is never hidden by a repo filter.)
    pub fn visible_providers<'a>(&self, row: &'a PackageRow) -> Vec<&'a Provider> {
        row.providers
            .iter()
            .filter(|p| match &p.meta.repo {
                Some(repo) => !self.settings.is_repo_hidden(repo),
                None => true,
            })
            .collect()
    }

    pub fn clear_confirm(&mut self) {
        self.confirm = None;
        self.confirm_note = None;
    }

    // --- Options overlay ---

    /// Number of toggles: "show hotkeys" + one per known repo.
    pub fn options_count(&self) -> usize {
        1 + self.repos.len()
    }

    pub fn move_options(&mut self, delta: i32) {
        let max = self.options_count() as i32 - 1;
        let next = (self.options_selected as i32 + delta).clamp(0, max.max(0));
        self.options_selected = next as usize;
    }

    pub fn toggle_option(&mut self) {
        if self.options_selected == 0 {
            self.settings.show_hotkeys = !self.settings.show_hotkeys;
        } else if let Some(repo) = self.repos.get(self.options_selected - 1).cloned() {
            self.settings.toggle_repo(&repo);
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
