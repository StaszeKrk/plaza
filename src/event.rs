use crate::model::{InstalledStats, PackageHit, SourceId, UpdatesInfo};
use crate::sources::installed::{InstalledIndex, InstalledPkg};
use crate::sources::updates::UpdateEntry;

/// Everything the render loop reacts to.
pub enum AppEvent {
    Input(crossterm::event::Event),
    DispatchSearch { gen: u64 },
    SearchHits {
        query_id: u64,
        source_id: SourceId,
        hits: Vec<PackageHit>,
    },
    SearchError {
        query_id: u64,
        source_id: SourceId,
    },
    Stats(InstalledStats),
    Updates(UpdatesInfo),
    Installed(InstalledIndex),
    /// Explicitly-installed package list (`pacman -Qe`) for the Installed view.
    InstalledList(Vec<InstalledPkg>),
    /// Upgradable package list (repos + AUR) for the Updates view.
    UpdatesList(Vec<UpdateEntry>),
    PtyOutput { id: u64, bytes: Vec<u8> },
    ActionFinished { id: u64, success: bool, code: u32 },
    /// Metronome tick driving theme-file live-reload (see `poll_theme_reload`).
    ThemeReloadTick,
}
