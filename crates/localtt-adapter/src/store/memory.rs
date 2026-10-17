//! The in-memory store: engine tests and the contract suite (RFC section 6.2).

use std::collections::BTreeMap;

use localtt_core::types::{AnyEntity, CampaignId, EntityId, FeedEntry, FeedEntryId};
use localtt_engine::ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreFault};

use crate::store::{invalid_patch, parse_document, to_document, validate_shape};

/// An in-memory [`CampaignStore`]: a plain snapshot holder with no file.
///
/// It implements the same patch preconditions and the same load gate as the
/// SQLite store, so the contract suite runs one suite against both. It has no
/// file and deliberately does not implement the `StoreBackup` capability.
#[derive(Debug, Default)]
pub struct MemoryStore {
    state: StoredState,
}

/// The raw stored documents.
///
/// `apply` mutates a clone of this and swaps it in only on success (RFC section
/// 5.3); maps key the documents by id, so insertion order is not preserved and
/// `load` walks ids in order.
#[derive(Clone, Debug, Default)]
struct StoredState {
    campaign: Option<StoredCampaign>,
    entities: BTreeMap<EntityId, String>,
    feed: BTreeMap<FeedEntryId, String>,
}

/// A stored campaign document plus its id.
///
/// The id sits next to the document so `apply` can enforce the campaign
/// singleton (precondition 7) without parsing what it holds.
#[derive(Clone, Debug)]
struct StoredCampaign {
    id: CampaignId,
    doc: String,
}

impl MemoryStore {
    /// Creates an empty store.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CampaignStore for MemoryStore {
    #[inline]
    fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        let Some(stored) = &self.state.campaign else {
            return Err(StoreFault::NoCampaign);
        };
        let entities = self
            .state
            .entities
            .values()
            .map(String::as_str)
            .map(parse_document::<AnyEntity>)
            .collect::<Result<Vec<_>, _>>()?;
        let feed = self
            .state
            .feed
            .values()
            .map(String::as_str)
            .map(parse_document::<FeedEntry>)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CampaignSnapshot {
            campaign: parse_document(&stored.doc)?,
            entities,
            feed,
        })
    }

    #[inline]
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        validate(&self.state, patch)?;

        let mut next = self.state.clone();
        if let Some(campaign) = patch.campaign {
            next.campaign = Some(StoredCampaign {
                id: campaign.id,
                doc: to_document(campaign)?,
            });
        }
        for entity in patch.upserts.iter().copied() {
            next.entities.insert(entity.id(), to_document(entity)?);
        }
        for id in &patch.removals {
            next.entities.remove(id);
        }
        if let Some(keep_from) = patch.feed_keep_from {
            next.feed = next.feed.split_off(&keep_from);
        }
        for entry in patch.feed_appends.iter().copied() {
            next.feed.insert(entry.id, to_document(entry)?);
        }

        self.state = next;
        Ok(())
    }
}

/// Enforces the store-state `apply` preconditions (RFC section 5.3 items 3, 5,
/// 7 and 8): removals name stored entities, feed appends never collide with
/// stored ids (even ones this patch would trim), the campaign id matches, and
/// entities require a stored campaign.
///
/// Runs before anything is written: a violation is an engine bug, and the
/// store refuses the whole patch.
fn validate(state: &StoredState, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
    validate_shape(patch)?;

    let carries_documents =
        !patch.upserts.is_empty() || !patch.removals.is_empty() || !patch.feed_appends.is_empty();
    if state.campaign.is_none() && patch.campaign.is_none() && carries_documents {
        return Err(invalid_patch(
            "entities and feed entries require a stored campaign",
        ));
    }

    if let (Some(stored), Some(campaign)) = (&state.campaign, patch.campaign)
        && stored.id != campaign.id
    {
        return Err(invalid_patch(format!(
            "campaign {} does not match the stored campaign {}",
            campaign.id, stored.id
        )));
    }

    for id in &patch.removals {
        if !state.entities.contains_key(id) {
            return Err(invalid_patch(format!("entity {id} is not stored")));
        }
    }
    for entry in &patch.feed_appends {
        if state.feed.contains_key(&entry.id) {
            return Err(invalid_patch(format!(
                "feed entry {} is already stored",
                entry.id.get()
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::CampaignBuilder,
        types::{
            Audience,
            Campaign,
            CampaignTime,
            Entity,
            EntityId,
            EntityKind,
            FeedEntry,
            FeedEntryId,
            FeedPayload,
            NoteBody,
            PlayerId,
            SchemaVersion,
            UnixTime,
        },
    };
    use serde::Serialize;

    use super::*;

    const FIXED_TIME: i64 = 1_750_000_000;

    fn campaign() -> Campaign {
        CampaignBuilder::new("Curse of the Vault").build()
    }

    fn note(id: EntityId, owner: PlayerId) -> AnyEntity {
        AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                id,
                "Session 4",
                owner,
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .created_at(UnixTime::from_seconds(FIXED_TIME))
            .updated_at(UnixTime::from_seconds(FIXED_TIME))
            .build()
            .expect("a note kind pairs with a note body"),
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

    fn bytes<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("documents serialize")
    }

    fn creation_patch<'a>(
        campaign: &'a Campaign,
        entities: &'a [AnyEntity],
        feed: &'a [FeedEntry],
    ) -> StatePatch<'a> {
        StatePatch {
            campaign: Some(campaign),
            upserts: entities.iter().collect(),
            removals: Vec::new(),
            feed_appends: feed.iter().collect(),
            feed_keep_from: None,
        }
    }

    #[test]
    fn load_reports_no_campaign_for_an_empty_store() {
        let store = MemoryStore::new();

        assert!(matches!(store.load(), Err(StoreFault::NoCampaign)));
    }

    #[test]
    fn apply_then_load_round_trips_documents_in_order() {
        let campaign = campaign();
        let owner = PlayerId::new();
        let entities = [
            note(EntityId::new(), owner),
            note(EntityId::new(), owner),
            note(EntityId::new(), owner),
        ];
        let feed = [feed_entry(1), feed_entry(2)];
        let mut store = MemoryStore::new();

        store
            .apply(&creation_patch(&campaign, &entities, &feed))
            .expect("a fresh store accepts the creation patch");
        let loaded = store.load().expect("the patch was just committed");

        assert_eq!(bytes(&loaded.campaign), bytes(&campaign));
        let applied = entities
            .iter()
            .map(|entity| (entity.id(), bytes(entity)))
            .collect::<BTreeMap<_, _>>();
        let stored = loaded
            .entities
            .iter()
            .map(|entity| (entity.id(), bytes(entity)))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(stored, applied);
        let ids = loaded
            .entities
            .iter()
            .map(AnyEntity::id)
            .collect::<Vec<_>>();
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "entities load in ascending id order"
        );
        assert_eq!(
            loaded.feed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![FeedEntryId::new(1), FeedEntryId::new(2)],
            "feed loads in ascending id order"
        );
    }

    #[test]
    fn invalid_patch_is_refused_without_writing() {
        let campaign = campaign();
        let owner = PlayerId::new();
        let stored_entity = note(EntityId::new(), owner);
        let mut store = MemoryStore::new();
        store
            .apply(&creation_patch(&campaign, &[stored_entity], &[]))
            .expect("a fresh store accepts the creation patch");
        let before = store.load().expect("the patch was just committed");

        let other_campaign = CampaignBuilder::new("Another campaign").build();
        let goblin = note(EntityId::new(), owner);
        let patch = StatePatch {
            campaign: Some(&other_campaign),
            upserts: vec![&goblin],
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: None,
        };
        assert!(matches!(
            store.apply(&patch),
            Err(StoreFault::InvalidPatch { .. })
        ));

        let after = store
            .load()
            .expect("the refused patch left the store readable");
        assert_eq!(bytes(&after.campaign), bytes(&before.campaign));
        assert_eq!(after.entities.len(), before.entities.len());
        assert!(
            !after
                .entities
                .iter()
                .any(|entity| entity.id() == goblin.id())
        );
    }

    #[test]
    fn a_newer_document_major_is_refused_on_load() {
        let mut future = campaign();
        future.schema_version = SchemaVersion { major: 2, minor: 0 };
        let mut store = MemoryStore::new();

        store
            .apply(&StatePatch {
                campaign: Some(&future),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("apply writes documents as given; the gate is on load");

        match store.load() {
            Err(StoreFault::VersionRefused { found, supported }) => {
                assert_eq!(found, future.schema_version);
                assert_eq!(supported, SchemaVersion::CURRENT);
            }
            other => panic!("a future major must be refused, got {other:?}"),
        }
    }

    #[test]
    fn trimming_runs_before_appends_and_stored_ids_still_collide() {
        let campaign = campaign();
        let feed = [feed_entry(1), feed_entry(2), feed_entry(3)];
        let mut store = MemoryStore::new();
        store
            .apply(&creation_patch(&campaign, &[], &feed))
            .expect("a fresh store accepts the creation patch");

        let appended = feed_entry(4);
        store
            .apply(&StatePatch {
                campaign: None,
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: vec![&appended],
                feed_keep_from: Some(FeedEntryId::new(2)),
            })
            .expect("trimming below the cutoff and appending above it");
        let loaded = store.load().expect("the patch was just committed");
        assert_eq!(
            loaded.feed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![
                FeedEntryId::new(2),
                FeedEntryId::new(3),
                FeedEntryId::new(4)
            ]
        );

        // Entry 2 is below keep_from = 3, so the shape check refuses it as
        // "below the trim cutoff" (precondition 6). The stored-id rule (5)
        // never speaks: S12's case is refused earlier, with the same
        // `InvalidPatch` outcome.
        let trimmed_away = feed_entry(2);
        assert!(matches!(
            store.apply(&StatePatch {
                campaign: None,
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: vec![&trimmed_away],
                feed_keep_from: Some(FeedEntryId::new(3)),
            }),
            Err(StoreFault::InvalidPatch { .. })
        ));
        let loaded = store
            .load()
            .expect("the refused patch left the store readable");
        assert_eq!(
            loaded.feed.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![
                FeedEntryId::new(2),
                FeedEntryId::new(3),
                FeedEntryId::new(4)
            ]
        );
    }
}
