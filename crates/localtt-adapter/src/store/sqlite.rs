//! The SQLite store (RFC sections 4.3 and 6.1).

use std::path::Path;

use localtt_engine::ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreFault};
use rusqlite::Connection;

/// The shipped file-backed store: one `rusqlite` connection, documents in
/// `doc` columns next to the extracted projection columns (VER-3, STORE-1).
#[derive(Debug)]
pub struct SqliteStore {
    connection: Connection,
}

/// Opening options; `Default` is what [`SqliteStore::open`] uses.
///
/// `#[non_exhaustive]` keeps later options additive.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct SqliteOptions {
    /// Online startup backup before DDL migrations (design section 5.7);
    /// skipped for a fresh file. `--no-backup` maps to `false` (U7).
    pub startup_backup: bool,
}

impl SqliteStore {
    /// Opens the campaign file, creating it when missing or zero-length.
    ///
    /// Sets the connection pragmas outside any transaction, verifies file
    /// identity (`application_id` magic; a foreign database is `Corrupt`) and
    /// the DDL version (refusing a newer one), takes an atomic startup backup,
    /// then applies pending DDL migrations in one transaction.
    #[inline]
    pub fn open(path: &Path) -> Result<Self, StoreFault> {
        todo!()
    }

    /// Like [`SqliteStore::open`], with explicit opening options.
    #[inline]
    pub fn open_with(path: &Path, options: SqliteOptions) -> Result<Self, StoreFault> {
        todo!()
    }
}

impl CampaignStore for SqliteStore {
    #[inline]
    fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        todo!()
    }

    #[inline]
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        todo!()
    }
}
