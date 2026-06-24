use crate::model::{InstalledStats, PackageHit, SourceId, UpdatesInfo};
use crate::sources::installed::InstalledIndex;

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
    PtyOutput { id: u64, bytes: Vec<u8> },
    ActionFinished { id: u64, success: bool, code: u32 },
}
