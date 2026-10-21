//! The campaign store port: durable campaign state behind one trait
//! (`docs/rfcs/store.md` section 4.1).
//!
//! The engine owns the interface; the adapter implements it. Implementations
//! are single-writer, synchronous and may block: the composition root runs them
//! behind the serialized command executor.

use std::path::Path;

use async_trait::async_trait;
use localtt_core::types::{AnyEntity, Campaign, EntityId, FeedEntry, FeedEntryId, SchemaVersion};
use thiserror::Error;

/// Everything the store holds, materialized; startup and export use this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignSnapshot {
    /// The campaign document.
    pub campaign: Campaign,
    /// Ascending `EntityId` (UUIDv7 byte order).
    pub entities: Vec<AnyEntity>,
    /// Ascending `FeedEntryId`.
    pub feed: Vec<FeedEntry>,
}

/// One command's durable effect, applied in a single transaction.
///
/// Constructed by the engine; the store treats the documents opaquely. The
/// fields are public because the workspace constructs patches by literal
/// (workspace-internal DTO).
#[derive(Debug)]
pub struct StatePatch<'a> {
    /// Present when the campaign document changed (clock, settings, players).
    pub campaign: Option<&'a Campaign>,
    /// Full new revisions of touched entities (created or updated).
    pub upserts: Vec<&'a AnyEntity>,
    /// Entity ids to remove.
    pub removals: Vec<EntityId>,
    /// Feed entries to append, strictly ascending id.
    pub feed_appends: Vec<&'a FeedEntry>,
    /// When set, drop stored feed entries with `id < keep_from` (ring
    /// trimming); appends below the cutoff are rejected (section 5.3).
    pub feed_keep_from: Option<FeedEntryId>,
}

/// Failure of any store operation.
///
/// `message` fields are diagnostic, not user-facing; the server maps variants
/// to status codes and must not forward the messages.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum StoreFault {
    /// The store holds no campaign yet; create one first.
    #[error("the store holds no campaign")]
    NoCampaign,
    /// An empty-store operation (import) found a campaign already stored; a
    /// user-facing refusal, unlike `InvalidPatch`.
    #[error("the store already holds a campaign")]
    StoreNotEmpty,
    /// A document's major version is newer than this build supports (VER-2).
    #[error("document schema {found:?} is newer than this build supports ({supported:?})")]
    VersionRefused {
        /// The document's version as found on disk.
        found: SchemaVersion,
        /// The version this build supports.
        supported: SchemaVersion,
    },
    /// The file is not a localtt store, a document did not parse, or a bundle
    /// envelope has a foreign `format`.
    #[error("the store is corrupt: {message}")]
    Corrupt {
        /// Diagnostic detail (path, parse error, SQL).
        message: String,
    },
    /// The store file's DDL version is newer than this build knows;
    /// `user_version` is signed.
    #[error("store DDL version {found} is newer than this build knows ({supported})")]
    DdlVersionRefused {
        /// The DDL version found on disk.
        found: i32,
        /// The DDL version this build knows.
        supported: i32,
    },
    /// A document migration failed.
    #[error("document migration {from:?} -> {to:?} failed: {message}")]
    DocumentMigration {
        /// The version being migrated from.
        from: SchemaVersion,
        /// The version being migrated to.
        to: SchemaVersion,
        /// Diagnostic detail.
        message: String,
    },
    /// A DDL migration step failed.
    #[error("DDL migration {from} -> {to} failed: {message}")]
    DdlMigration {
        /// The DDL version being migrated from.
        from: i32,
        /// The DDL version being migrated to.
        to: i32,
        /// Diagnostic detail.
        message: String,
    },
    /// A bundle's `format_version` is newer than this build supports; the file
    /// is not corrupt, the build is old.
    #[error("bundle format version {found} is newer than this build supports ({supported})")]
    FormatRefused {
        /// The envelope version found in the bundle.
        found: u32,
        /// The envelope version this build supports.
        supported: u32,
    },
    /// Advisory/IO failure (missing directory, permissions, disk full, locked).
    #[error("the store is unavailable: {message}")]
    Unavailable {
        /// Diagnostic detail (path, IO error, lock timeout).
        message: String,
    },
    /// Any other backend failure.
    #[error("store backend failure: {message}")]
    Backend {
        /// Diagnostic detail.
        message: String,
    },
    /// The operation was refused before writing: a documented patch
    /// precondition failed (section 5.3). Nothing was applied.
    #[error("invalid patch: {message}")]
    InvalidPatch {
        /// The violated precondition, in diagnostic form.
        message: String,
    },
}

/// The only persistence abstraction the engine knows.
///
/// Implementations are single-writer, synchronous and may block; the
/// composition root runs them behind the serialized command executor.
pub trait CampaignStore: Send {
    /// Loads the whole campaign. Called once at startup and by export.
    fn load(&self) -> Result<CampaignSnapshot, StoreFault>;

    /// Applies a patch in one transaction: all-or-nothing. Called before the
    /// engine publishes anything.
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;
}

/// The asynchronous twin of [`CampaignStore`] (server RFC 6.2).
///
/// The borrow of `patch` lives across the returned future, so an adapter that
/// moves work to a blocking pool must build an owned mirror first. Implementers
/// that keep work in-process can apply directly.
///
/// `async_trait` keeps the trait dyn-compatible on the pinned MSRV; the boxed
/// future costs one allocation per command. The `Sync` bound is what makes the
/// `&self` load future `Send`; stores are still used behind one writer.
#[async_trait]
pub trait AsyncCampaignStore: Send + Sync {
    /// Loads the whole campaign. Called once at startup and by export.
    async fn load(&self) -> Result<CampaignSnapshot, StoreFault>;

    /// Applies a patch in one transaction: all-or-nothing.
    async fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;
}

/// Durable self-copy and durability flushing for a store implementation.
///
/// Separate from [`CampaignStore`]: the engine never calls these methods (its
/// dataflow is `load` + `apply`). The composition root and tests hold a
/// concrete store and use the capability. A file-backed implementation writes
/// a self-contained, consistent snapshot using the backend's own snapshot
/// mechanism, never a copy of live files; a store with no file does not
/// implement the trait.
pub trait StoreBackup: Send {
    /// Writes a self-contained copy of the store's current contents to `to`.
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault>;

    /// Forces durability; tests and diagnostics.
    fn sync(&mut self) -> Result<(), StoreFault>;
}
