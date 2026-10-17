//! Store implementations behind the engine's `CampaignStore` port
//! (`docs/rfcs/store.md` section 4.3).
//!
//! Layout per RFC section 6.5: `memory` (engine tests and the contract suite),
//! `sqlite` (the shipped store), `export` (bundle decode and import), and the
//! crate-internal `schema`, `doc_migrations` and `backup` modules.

pub mod export;
pub mod memory;
pub mod sqlite;

mod backup;
pub(crate) mod doc_migrations;
pub(crate) mod schema;

pub use export::{decode_bundle, import_into};
pub use memory::MemoryStore;
pub use sqlite::{SqliteOptions, SqliteStore};
