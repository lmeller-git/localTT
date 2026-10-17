//! The `StoreBackup` capability for the SQLite store: online backup through
//! the backend's own snapshot mechanism, and durability flushing (RFC section
//! 4.3; STORE-7, STORE-12).

use std::path::Path;

use localtt_engine::ports::store::{StoreBackup, StoreFault};

use crate::store::sqlite::SqliteStore;

impl StoreBackup for SqliteStore {
    #[inline]
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault> {
        todo!()
    }

    #[inline]
    fn sync(&mut self) -> Result<(), StoreFault> {
        todo!()
    }
}
