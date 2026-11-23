//! A command's owned effect and the events it produces.

use localtt_core::types::{AnyEntity, Campaign, EntityId, EntityKind, FeedEntry, FeedEntryId};

use crate::{error::Warning, ports::store::StatePatch};

/// One command's owned effect: the durable delta plus the delivery hints.
///
/// Fields are crate-private; clients see warnings and events through
/// [`Executed`](crate::executor::Executed) only. `Debug` prints the owned
/// documents, for test failures and diagnostics.
#[derive(Debug)]
pub struct Transition {
    /// The new campaign document, when the command changed it.
    pub(crate) campaign: Option<Campaign>,
    /// Full new revisions of touched entities (ascending id).
    pub(crate) upserts: Vec<AnyEntity>,
    /// Entity ids to remove.
    pub(crate) removals: Vec<EntityId>,
    /// Feed entries to append, ascending id, capped to the ring (5.6).
    pub(crate) feed_appends: Vec<FeedEntry>,
    /// When set, drop stored feed entries with `id < keep_from` (5.6).
    pub(crate) feed_keep_from: Option<FeedEntryId>,
    /// Non-blocking rule outcomes, in the order the rules produced them.
    pub(crate) warnings: Vec<Warning>,
    /// The ordered delivery hint for U7 (5.7).
    pub(crate) events: Vec<Event>,
}

impl Transition {
    /// Borrows the owned documents into the patch the store takes.
    #[must_use]
    #[inline]
    pub fn patch(&self) -> StatePatch<'_> {
        StatePatch {
            campaign: self.campaign.as_ref(),
            upserts: self.upserts.iter().collect(),
            removals: self.removals.clone(),
            feed_appends: self.feed_appends.iter().collect(),
            feed_keep_from: self.feed_keep_from,
        }
    }

    /// An empty transition (5.2): no documents, no events, no warnings.
    pub(super) fn empty() -> Self {
        Self {
            campaign: None,
            upserts: Vec::new(),
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: None,
            warnings: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// Delivery hints for the server (U7); events carry ids, not documents.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// One entity's document was replaced; `rev` is the new revision.
    EntityChanged {
        /// The entity.
        id: EntityId,
        /// The new revision.
        rev: u64,
        /// The entity kind, for client routing.
        kind: EntityKind,
    },
    /// An entity was removed. No U3 producer; declared for the later
    /// lifecycle commands.
    EntityRemoved {
        /// The removed entity.
        id: EntityId,
    },
    /// The campaign document changed (clock, settings, players).
    CampaignChanged,
    /// Feed entries were appended; ascending ids, never empty.
    FeedAppended {
        /// The kept appends, ascending (5.6).
        ids: Vec<FeedEntryId>,
    },
}
