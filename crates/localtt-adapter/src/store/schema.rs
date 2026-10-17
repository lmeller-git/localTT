//! DDL and DDL migrations: the ordered step list and `PRAGMA user_version`
//! (RFC sections 5.6 and 6.1; STORE-2, STORE-12).

use localtt_engine::ports::store::StoreFault;
use rusqlite::Connection;

/// The ordered DDL migrations; entry `n` migrates DDL version `n` to `n + 1`.
///
/// Steps are `fn(&Connection) -> Result<(), StoreFault>` and report
/// `DdlMigration` on failure. One entry today: `create_v1`.
pub(crate) const DDL_MIGRATIONS: &[fn(&Connection) -> Result<(), StoreFault>] = &[create_v1];

/// The DDL version this build knows how to open.
pub(crate) const SUPPORTED_DDL_VERSION: i32 = DDL_MIGRATIONS.len() as i32;

/// Creates the v1 schema (design section 5.2): `campaign`, `entities` with its
/// indexes, and `feed`.
fn create_v1(connection: &Connection) -> Result<(), StoreFault> {
    todo!()
}

/// Reads `PRAGMA user_version`; signed, like SQLite itself (STORE-8).
pub(crate) fn user_version(connection: &Connection) -> Result<i32, StoreFault> {
    todo!()
}

/// Writes `PRAGMA user_version`.
pub(crate) fn set_user_version(connection: &Connection, version: i32) -> Result<(), StoreFault> {
    todo!()
}

/// Applies every pending DDL migration and stamps `user_version` in one
/// transaction (S8).
///
/// The caller refuses a `user_version` newer than [`SUPPORTED_DDL_VERSION`]
/// before calling.
pub(crate) fn migrate(connection: &Connection) -> Result<(), StoreFault> {
    todo!()
}
