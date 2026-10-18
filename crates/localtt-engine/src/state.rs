//! The read view and the load-all working set (RFC `engine-executor.md` 4.2,
//! 6.2).

use std::collections::{BTreeMap, VecDeque};

use localtt_core::types::{
    AclEntry,
    AnyEntity,
    Campaign,
    CampaignTime,
    EntityId,
    EntityKind,
    FeedEntry,
    PlayerId,
    StatusId,
    UnixTime,
};

use crate::{
    error::EngineError,
    ports::store::{CampaignSnapshot, StatePatch},
};

/// Common header of an entity, without its body (design 5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityHeader {
    /// The entity id.
    pub id: EntityId,
    /// The entity kind.
    pub kind: EntityKind,
    /// Display name.
    pub name: String,
    /// The owning player.
    pub owner: PlayerId,
    /// The owning entity, for summons and mounts.
    pub parent: Option<EntityId>,
    /// Read/write grants beyond the owner.
    pub acl: Vec<AclEntry>,
    /// Optimistic-concurrency revision.
    pub rev: u64,
    /// Wall-clock stamp of the last write.
    pub updated_at: UnixTime,
}

/// Read view commands and services use.
///
/// Owned return values keep a future lazy implementation possible (ENG-3); the
/// load-all one never fails.
pub trait WorkingSet: Send {
    /// The campaign document.
    fn campaign(&self) -> &Campaign;
    /// One entity document, if any.
    fn entity(&self, id: EntityId) -> Result<Option<AnyEntity>, EngineError>;
    /// Every entity's header, ascending id.
    fn headers(&self) -> Vec<EntityHeader>;
    /// The feed, ascending id.
    fn feed(&self) -> Vec<FeedEntry>;
    /// The id the next feed append receives.
    fn next_feed_id(&self) -> u64;
    /// Every indexed `(entity, status)` whose `expires_at <= until`, in an
    /// unspecified order; the caller filters and sorts.
    fn expiry_candidates_until(&self, until: CampaignTime) -> Vec<(EntityId, StatusId)>;
}

/// Mutated only by the executor, after the store commit.
pub trait WorkingSetMut: WorkingSet {
    /// Applies one durable transition: upserts, removals, feed appends and
    /// trimming; keeps the expiry index and `next_feed_id` in sync.
    fn apply(&mut self, patch: &StatePatch<'_>);
}

/// The load-all working set (ENG-3); fields are private.
#[derive(Debug)]
pub struct LoadedCampaign {
    campaign: Campaign,
    entities: BTreeMap<EntityId, AnyEntity>,
    feed: VecDeque<FeedEntry>,
    next_feed_id: u64,
    expiries: BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>,
}

impl LoadedCampaign {
    /// Wraps a loaded snapshot: builds the entity map, recomputes the expiry
    /// index from the documents and `next_feed_id` from the feed tail.
    #[must_use]
    #[inline]
    pub fn from_snapshot(snapshot: CampaignSnapshot) -> Self {
        todo!()
    }
}

impl WorkingSet for LoadedCampaign {
    #[inline]
    fn campaign(&self) -> &Campaign {
        todo!()
    }

    #[inline]
    fn entity(&self, id: EntityId) -> Result<Option<AnyEntity>, EngineError> {
        todo!()
    }

    #[inline]
    fn headers(&self) -> Vec<EntityHeader> {
        todo!()
    }

    #[inline]
    fn feed(&self) -> Vec<FeedEntry> {
        todo!()
    }

    #[inline]
    fn next_feed_id(&self) -> u64 {
        todo!()
    }

    #[inline]
    fn expiry_candidates_until(&self, until: CampaignTime) -> Vec<(EntityId, StatusId)> {
        todo!()
    }
}

impl WorkingSetMut for LoadedCampaign {
    #[inline]
    fn apply(&mut self, patch: &StatePatch<'_>) {
        todo!()
    }
}
