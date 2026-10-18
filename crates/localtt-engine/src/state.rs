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
    StatusDuration,
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
        let mut entities = BTreeMap::new();
        let mut expiries = BTreeMap::new();
        for entity in snapshot.entities {
            let id = entity.id();
            index_statuses(&mut expiries, id, &entity);
            entities.insert(id, entity);
        }
        let feed: VecDeque<FeedEntry> = snapshot.feed.into();
        let next_feed_id = feed.back().map_or(1, |entry| entry.id.get() + 1);
        Self {
            campaign: snapshot.campaign,
            entities,
            feed,
            next_feed_id,
            expiries,
        }
    }
}

impl WorkingSet for LoadedCampaign {
    #[inline]
    fn campaign(&self) -> &Campaign {
        &self.campaign
    }

    #[inline]
    fn entity(&self, id: EntityId) -> Result<Option<AnyEntity>, EngineError> {
        Ok(self.entities.get(&id).cloned())
    }

    #[inline]
    fn headers(&self) -> Vec<EntityHeader> {
        self.entities
            .values()
            .map(|entity| EntityHeader {
                id: entity.id(),
                kind: entity.kind(),
                name: entity.name().to_owned(),
                owner: entity.owner(),
                parent: entity.parent(),
                acl: entity.acl().to_vec(),
                rev: entity.rev(),
                updated_at: entity.updated_at(),
            })
            .collect()
    }

    #[inline]
    fn feed(&self) -> Vec<FeedEntry> {
        self.feed.iter().cloned().collect()
    }

    #[inline]
    fn next_feed_id(&self) -> u64 {
        self.next_feed_id
    }

    #[inline]
    fn expiry_candidates_until(&self, until: CampaignTime) -> Vec<(EntityId, StatusId)> {
        self.expiries
            .range(..=until)
            .flat_map(|(_, bucket)| bucket.iter().copied())
            .collect()
    }
}

impl WorkingSetMut for LoadedCampaign {
    #[inline]
    fn apply(&mut self, patch: &StatePatch<'_>) {
        if let Some(campaign) = patch.campaign {
            self.campaign = campaign.clone();
        }
        for &entity in &patch.upserts {
            let id = entity.id();
            if let Some(previous) = self.entities.get(&id) {
                deindex_statuses(&mut self.expiries, id, previous);
            }
            index_statuses(&mut self.expiries, id, entity);
            self.entities.insert(id, entity.clone());
        }
        for id in &patch.removals {
            if let Some(removed) = self.entities.remove(id) {
                deindex_statuses(&mut self.expiries, *id, &removed);
            }
        }
        // STORE-13: trimming runs before the appends, as in the store; the
        // engine caps appends so every kept id survives the cutoff.
        if let Some(keep_from) = patch.feed_keep_from {
            while self.feed.front().is_some_and(|entry| entry.id < keep_from) {
                self.feed.pop_front();
            }
        }
        for &entry in &patch.feed_appends {
            self.feed.push_back(entry.clone());
        }
        // An engine-built patch never trims the feed empty (5.6): positions are
        // never reused, so an empty feed keeps the counter instead of rewinding.
        if let Some(last) = self.feed.back() {
            self.next_feed_id = last.id.get() + 1;
        }
    }
}

/// The `Timed` statuses of one document, as `(expires_at, status)` pairs.
///
/// Only statblocks carry statuses; other kinds index nothing.
fn timed_statuses(entity: &AnyEntity) -> Vec<(CampaignTime, StatusId)> {
    let AnyEntity::Statblock(document) = entity else {
        return Vec::new();
    };
    document
        .body
        .statuses
        .iter()
        .filter_map(|status| match &status.duration {
            StatusDuration::Timed { expires_at } => Some((*expires_at, status.id)),
            StatusDuration::UntilRest { .. }
            | StatusDuration::Indefinite
            | StatusDuration::Manual { .. } => None,
        })
        .collect()
}

/// Adds one document's timed statuses to the expiry index.
fn index_statuses(
    expiries: &mut BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>,
    id: EntityId,
    entity: &AnyEntity,
) {
    for (expires_at, status) in timed_statuses(entity) {
        expiries.entry(expires_at).or_default().push((id, status));
    }
}

/// Removes all of one document's timed statuses from the expiry index.
///
/// The document being replaced is deindexed whole; its successor is indexed
/// again with whatever statuses it carries.
fn deindex_statuses(
    expiries: &mut BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>,
    id: EntityId,
    entity: &AnyEntity,
) {
    for (expires_at, _) in timed_statuses(entity) {
        if let Some(bucket) = expiries.get_mut(&expires_at) {
            bucket.retain(|(candidate, _)| *candidate != id);
            if bucket.is_empty() {
                expiries.remove(&expires_at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            Audience,
            Entity,
            FeedEntryId,
            FeedPayload,
            Status,
            StatusDuration,
            StatusKind,
        },
    };

    use super::*;

    const FIXED_TIME: i64 = 1_750_000_000;

    fn campaign(clock_s: i64) -> Campaign {
        let mut campaign = CampaignBuilder::new("Curse of the Vault").build();
        campaign.clock.now_s = CampaignTime::from_seconds(clock_s);
        campaign
    }

    fn timed_status(expires_at: i64) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Condition,
            name: "Hex".to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration: StatusDuration::Timed {
                expires_at: CampaignTime::from_seconds(expires_at),
            },
            source: None,
            modifiers: Vec::new(),
        }
    }

    fn statblock(id: EntityId, rev: u64, statuses: Vec<Status>) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(32, 32);
        let mut body = builder.build().expect("abilities and hp are set");
        body.statuses = statuses;
        AnyEntity::from(
            Entity::builder(EntityKind::Pc, id, "Aria", PlayerId::new(), body)
                .rev(rev)
                .created_at(UnixTime::from_seconds(FIXED_TIME))
                .updated_at(UnixTime::from_seconds(FIXED_TIME))
                .build()
                .expect("a character kind pairs with a statblock body"),
        )
    }

    fn feed_entry(id: u64) -> FeedEntry {
        FeedEntry {
            id: FeedEntryId::new(id),
            ts: UnixTime::from_seconds(FIXED_TIME),
            clock_s: CampaignTime::from_seconds(0),
            actor: None,
            entity: None,
            audience: Audience::Public,
            payload: FeedPayload::Hint {
                text: format!("hint {id}"),
            },
        }
    }

    fn snapshot(entities: Vec<AnyEntity>, feed: Vec<FeedEntry>) -> CampaignSnapshot {
        CampaignSnapshot {
            campaign: campaign(0),
            entities,
            feed,
        }
    }

    fn feed_ids(state: &LoadedCampaign) -> Vec<u64> {
        state.feed().iter().map(|entry| entry.id.get()).collect()
    }

    #[test]
    fn from_snapshot_builds_the_views_and_the_expiry_index() {
        let id = EntityId::new();
        let hex = timed_status(3_600);
        let entity = statblock(id, 0, vec![hex.clone()]);
        let state = LoadedCampaign::from_snapshot(snapshot(
            vec![entity.clone()],
            vec![feed_entry(1), feed_entry(2)],
        ));

        assert_eq!(state.campaign().clock.now_s, CampaignTime::from_seconds(0));
        assert_eq!(
            state.entity(id).expect("the load-all view never fails"),
            Some(entity)
        );
        assert_eq!(
            state
                .entity(EntityId::new())
                .expect("the load-all view never fails"),
            None
        );
        let headers = state.headers();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].id, id);
        assert_eq!(headers[0].kind, EntityKind::Pc);
        assert_eq!(headers[0].rev, 0);
        assert_eq!(feed_ids(&state), vec![1, 2]);
        assert_eq!(state.next_feed_id(), 3);
        assert_eq!(
            state.expiry_candidates_until(CampaignTime::from_seconds(3_599)),
            Vec::new()
        );
        assert_eq!(
            state.expiry_candidates_until(CampaignTime::from_seconds(3_600)),
            vec![(id, hex.id)],
            "the expiry instant itself is a candidate (soft expiry)"
        );
    }

    #[test]
    fn apply_upserts_removals_and_trims_before_appending() {
        let id = EntityId::new();
        let mut state = LoadedCampaign::from_snapshot(snapshot(
            vec![statblock(id, 0, vec![timed_status(3_600)])],
            vec![feed_entry(1), feed_entry(2), feed_entry(3)],
        ));

        let moved = timed_status(7_200);
        let revised = statblock(id, 1, vec![moved.clone()]);
        let campaign = campaign(600);
        let appended = feed_entry(4);
        state.apply(&StatePatch {
            campaign: Some(&campaign),
            upserts: vec![&revised],
            removals: Vec::new(),
            feed_appends: vec![&appended],
            feed_keep_from: Some(FeedEntryId::new(2)),
        });

        assert_eq!(
            state.campaign().clock.now_s,
            CampaignTime::from_seconds(600)
        );
        assert_eq!(
            state
                .entity(id)
                .expect("the load-all view never fails")
                .expect("the upsert kept the entity")
                .rev(),
            1
        );
        assert!(
            state
                .expiry_candidates_until(CampaignTime::from_seconds(3_600))
                .is_empty(),
            "the replaced document stops being a candidate"
        );
        assert_eq!(
            state.expiry_candidates_until(CampaignTime::from_seconds(7_200)),
            vec![(id, moved.id)]
        );
        assert_eq!(feed_ids(&state), vec![2, 3, 4]);
        assert_eq!(state.next_feed_id(), 5);

        state.apply(&StatePatch {
            campaign: None,
            upserts: Vec::new(),
            removals: vec![id],
            feed_appends: Vec::new(),
            feed_keep_from: None,
        });

        assert_eq!(
            state.entity(id).expect("the load-all view never fails"),
            None
        );
        assert!(state.headers().is_empty());
        assert!(
            state
                .expiry_candidates_until(CampaignTime::from_seconds(7_200))
                .is_empty(),
            "a removed document leaves no index residue"
        );
        assert_eq!(
            state.next_feed_id(),
            5,
            "removing documents does not rewind the feed counter"
        );
    }

    #[test]
    fn an_empty_feed_after_trimming_keeps_the_counter() {
        let mut state =
            LoadedCampaign::from_snapshot(snapshot(Vec::new(), vec![feed_entry(1), feed_entry(2)]));

        state.apply(&StatePatch {
            campaign: None,
            upserts: Vec::new(),
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: Some(FeedEntryId::new(3)),
        });

        assert!(state.feed().is_empty());
        assert_eq!(
            state.next_feed_id(),
            3,
            "feed positions are never reused across a trim (5.6)"
        );
    }
}
