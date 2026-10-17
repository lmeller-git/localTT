//! The in-memory store: engine tests and the contract suite (RFC section 6.2).

use localtt_engine::ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreFault};

/// An in-memory [`CampaignStore`]: a plain snapshot holder with no file.
///
/// It implements the same patch preconditions and the same load gate as the
/// SQLite store, so the contract suite runs one suite against both. It has no
/// file and deliberately does not implement the `StoreBackup` capability.
#[derive(Debug, Default)]
pub struct MemoryStore {
    snapshot: Option<CampaignSnapshot>,
}

impl MemoryStore {
    /// Creates an empty store.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        todo!()
    }
}

impl CampaignStore for MemoryStore {
    #[inline]
    fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        todo!()
    }

    #[inline]
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        todo!()
    }
}
