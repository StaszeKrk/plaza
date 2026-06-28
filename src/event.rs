use crate::model::{InstalledStats, PackageDetail, PackageHit, SourceId, UpdatesInfo};
use crate::sources::installed::{InstalledIndex, InstalledPkg, PkgDetail};
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
    /// Installed package list for the Manage view, plus the distinct repo names
    /// (priority order) from `pacman -Sl` that drive the repo filter.
    InstalledList(Vec<InstalledPkg>, Vec<String>),
    /// Upgradable package list (repos + AUR) for the Updates view.
    UpdatesList(Vec<UpdateEntry>),
    /// Extended detail for one provider, fetched lazily on opening the detail
    /// view. Keyed by `Provider::detail_key`.
    PackageDetailLoaded { key: String, detail: PackageDetail },
    /// `pacman -Qi` detail for one installed package, fetched lazily as the Manage
    /// selection moves. Keyed by package name.
    ManageDetailLoaded { name: String, detail: PkgDetail },
    PtyOutput { id: u64, bytes: Vec<u8> },
    ActionFinished { id: u64, success: bool, code: u32 },
    /// Metronome tick driving theme-file live-reload (see `poll_theme_reload`).
    ThemeReloadTick,
}
