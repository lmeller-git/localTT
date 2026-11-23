//! The campaign store port: durable campaign state behind one trait
//! (`docs/architecture.md` section 7).
//!
//! The engine owns the interface; the adapter implements it. Implementations
//! are single-writer, synchronous and may block: the composition root runs them
//! behind the serialized command executor.

use std::collections::BTreeSet;

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

impl StatePatch<'_> {
    /// Checks the shape preconditions every store enforces: no entity upserted
    /// twice, none both upserted and removed, feed appends strictly ascending
    /// and never below the trim cutoff.
    ///
    /// The engine asserts this before it hands a patch to the store, and the
    /// adapters call it again as a guard, so the rule lives in one place. It
    /// does not check revisions or entity existence; those depend on stored
    /// state.
    ///
    /// # Errors
    ///
    /// [`StoreFault::InvalidPatch`] naming the first violated precondition.
    #[inline]
    pub fn validate(&self) -> Result<(), StoreFault> {
        let invalid = |message: String| StoreFault::InvalidPatch { message };
        let mut upserted = BTreeSet::new();
        for entity in &self.upserts {
            if !upserted.insert(entity.id()) {
                return Err(invalid(format!(
                    "entity {} appears twice in the upserts",
                    entity.id()
                )));
            }
        }
        for id in &self.removals {
            if upserted.contains(id) {
                return Err(invalid(format!("entity {id} is both upserted and removed")));
            }
        }
        for pair in self.feed_appends.windows(2) {
            if pair[0].id >= pair[1].id {
                return Err(invalid(
                    "feed appends are not strictly ascending".to_owned(),
                ));
            }
        }
        if let Some(keep_from) = self.feed_keep_from {
            for entry in &self.feed_appends {
                if entry.id < keep_from {
                    return Err(invalid(format!(
                        "feed entry {} is below the trim cutoff",
                        entry.id.get()
                    )));
                }
            }
        }
        Ok(())
    }
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

/// The asynchronous twin of [`CampaignStore`] (`docs/engine.md` section 2).
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

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::CampaignBuilder,
        types::{
            Audience,
            CampaignTime,
            Entity,
            EntityId,
            EntityKind,
            FeedEntry,
            FeedEntryId,
            FeedPayload,
            NoteBody,
            PlayerId,
            UnixTime,
        },
    };

    use super::*;

    fn note() -> AnyEntity {
        AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                EntityId::new(),
                "Session 4",
                PlayerId::new(),
                NoteBody {
                    text: String::new(),
                },
            )
            .build()
            .expect("a note kind pairs with a note body"),
        )
    }

    fn feed(id: u64) -> FeedEntry {
        FeedEntry {
            id: FeedEntryId::new(id),
            ts: UnixTime::from_seconds(0),
            clock_s: CampaignTime::from_seconds(0),
            actor: None,
            entity: None,
            audience: Audience::Public,
            payload: FeedPayload::Hint {
                text: String::new(),
            },
        }
    }

    fn patch<'a>(
        upserts: Vec<&'a AnyEntity>,
        removals: Vec<EntityId>,
        feed_appends: Vec<&'a FeedEntry>,
        feed_keep_from: Option<FeedEntryId>,
    ) -> StatePatch<'a> {
        StatePatch {
            campaign: None,
            upserts,
            removals,
            feed_appends,
            feed_keep_from,
        }
    }

    fn assert_invalid(patch: &StatePatch<'_>) {
        assert!(matches!(
            patch.validate(),
            Err(StoreFault::InvalidPatch { .. })
        ));
    }

    #[test]
    fn a_well_formed_patch_validates() {
        let (first, second) = (note(), note());
        let (early, late) = (feed(3), feed(4));

        let patch = patch(
            vec![&first, &second],
            Vec::new(),
            vec![&early, &late],
            Some(FeedEntryId::new(3)),
        );

        assert!(patch.validate().is_ok());
    }

    fn patch_default<'a>() -> StatePatch<'a> {
        patch(Vec::new(), Vec::new(), Vec::new(), None)
    }

    #[test]
    fn a_duplicate_upsert_is_invalid() {
        let entity = note();

        assert_invalid(&patch(vec![&entity, &entity], Vec::new(), Vec::new(), None));
    }

    #[test]
    fn an_entity_both_upserted_and_removed_is_invalid() {
        let entity = note();

        assert_invalid(&patch(vec![&entity], vec![entity.id()], Vec::new(), None));
    }

    #[test]
    fn unordered_or_repeated_feed_appends_are_invalid() {
        let (low, high) = (feed(1), feed(2));

        assert_invalid(&patch(Vec::new(), Vec::new(), vec![&high, &low], None));
        assert_invalid(&patch(Vec::new(), Vec::new(), vec![&low, &low], None));
    }

    #[test]
    fn a_feed_append_below_the_trim_cutoff_is_invalid() {
        let entry = feed(2);

        assert_invalid(&patch(
            Vec::new(),
            Vec::new(),
            vec![&entry],
            Some(FeedEntryId::new(3)),
        ));
    }

    #[test]
    fn a_campaign_only_patch_validates() {
        let campaign = CampaignBuilder::new("Curse of the Vault").build();

        let patch = StatePatch {
            campaign: Some(&campaign),
            ..patch_default()
        };

        assert!(patch.validate().is_ok());
    }
}
