//! The single-writer command loop (RFC `engine-executor.md` 4.4, 5.1).

use localtt_core::types::{EntityId, PlayerId};

use crate::{
    commands::{Command, CommandEnv, Event, Transition},
    error::{EngineError, Warning},
    ports::store::{CampaignStore, StatePatch},
    state::{WorkingSet, WorkingSetMut},
};

/// The single-writer command loop. `Send`, deliberately not `Sync`: the
/// server serializes access (mutex or actor thread, U7).
pub struct Executor<'a> {
    state: Box<dyn WorkingSetMut>,
    store: &'a mut dyn CampaignStore,
    env: Box<dyn CommandEnv>,
}

impl<'a> Executor<'a> {
    /// Creates an executor over a working set, a store and an environment.
    #[must_use]
    #[inline]
    pub fn new(
        state: Box<dyn WorkingSetMut>,
        store: &'a mut dyn CampaignStore,
        env: Box<dyn CommandEnv>,
    ) -> Self {
        Self { state, store, env }
    }

    /// apply -> persist -> publish. Durable before visible (ENG-2): a failed
    /// store write leaves the working set untouched, and events are returned
    /// only after the commit. Engine-built patches are shape-asserted before
    /// the store write, so an engine invariant break fails here instead of
    /// surfacing as a store `InvalidPatch`.
    #[inline]
    pub fn execute(&mut self, command: &Command) -> Result<Executed, EngineError> {
        let transition = crate::commands::apply(self.state.as_ref(), command, self.env.as_mut())?;
        self.commit(transition)
    }

    /// The visible state, for services (7.8) and tests.
    #[must_use]
    #[inline]
    pub fn state(&self) -> &dyn WorkingSet {
        self.state.as_ref()
    }

    /// Executes one command attributed to `actor`.
    ///
    /// U5 records the actor for roll attribution and the audit trail; it does
    /// not authorize. U6 adds the authorization pre-check here, before any
    /// command work (RFC-0001 5.7).
    #[inline]
    pub fn execute_as(
        &mut self,
        actor: PlayerId,
        command: &Command,
    ) -> Result<Executed, EngineError> {
        let transition = crate::commands::apply_as(
            self.state.as_ref(),
            command,
            self.env.as_mut(),
            Some(actor),
        )?;
        self.commit(transition)
    }

    /// Persists and publishes one computed transition (5.1): revision
    /// assertions on every upsert, patch-shape assertions, then store first,
    /// working set second (durable before visible).
    fn commit(&mut self, transition: Transition) -> Result<Executed, EngineError> {
        for upsert in &transition.upserts {
            let id = upsert.id();
            let previous = self
                .state
                .entity(id)?
                .expect("an upsert replaces an existing document");
            match previous.rev().checked_add(1) {
                Some(expected) => assert!(
                    upsert.rev() == expected,
                    "engine bug: the upsert for {id} must stamp revision {expected}, got {}",
                    upsert.rev()
                ),
                None => panic!("entity {id} is at the revision ceiling: no next revision exists"),
            }
        }
        let patch = transition.patch();
        assert_patch_shape(&patch);
        self.store.apply(&patch)?;
        self.state.apply(&patch);
        Ok(Executed {
            warnings: transition.warnings,
            events: transition.events,
        })
    }

    /// The U6 pre-check: classifies `command` and rejects a disallowed actor
    /// before any command work runs (RFC `authz.md` 5.7).
    fn pre_check(&self, actor: PlayerId, command: &Command) -> Result<(), EngineError> {
        todo!()
    }
}

/// What a command needs from the actor before any command work runs
/// (RFC `authz.md` 4.3).
enum Requirement {
    /// The actor must be this campaign's DM (`AdvanceClock`, `SetClock`).
    Dm,
    /// The actor must be a roster member; no entity is involved.
    Member,
    /// The actor must be able to read the entity.
    Read(EntityId),
    /// The actor must be able to write the entity.
    Write(EntityId),
}

/// Classifies `command` into the pre-check [`Requirement`] it must satisfy
/// (RFC `authz.md` 5.7).
fn classify(command: &Command) -> Requirement {
    todo!()
}

/// Fails fast when an engine-built patch violates the store's shape
/// preconditions (5.1, `StatePatch`): upserts strictly ascending by id (no
/// duplicate revisions) and feed appends strictly ascending and never below
/// the trim cutoff. The store re-checks these and would answer
/// `InvalidPatch`; asserting here keeps an engine bug local, with the command
/// context, instead of indistinguishable from a store fault.
fn assert_patch_shape(patch: &StatePatch<'_>) {
    assert!(
        patch
            .upserts
            .windows(2)
            .all(|pair| pair[0].id() < pair[1].id()),
        "engine bug: upserts are not strictly ascending by id"
    );
    assert!(
        patch
            .feed_appends
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id),
        "engine bug: feed appends are not strictly ascending"
    );
    if let Some(keep_from) = patch.feed_keep_from {
        assert!(
            patch.feed_appends.iter().all(|entry| entry.id >= keep_from),
            "engine bug: a feed append sits below the trim cutoff {keep_from:?}"
        );
    }
}

/// A committed command's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executed {
    /// Non-blocking rule outcomes, in the order the rules produced them.
    pub warnings: Vec<Warning>,
    /// The ordered delivery hint for U7 (5.7).
    pub events: Vec<Event>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            AnyEntity,
            Campaign,
            EntityId,
            EntityKind,
            FeedEntry,
            PlayerId,
            StatblockEntity,
            UnixTime,
        },
    };

    use super::*;
    use crate::{
        ports::store::{CampaignSnapshot, StatePatch, StoreFault},
        state::LoadedCampaign,
    };

    const FIXED_TIME: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }
    }

    /// A store double: keeps the applied documents as its snapshot, and can be
    /// switched to failing writes.
    struct RecordingStore {
        campaign: Option<Campaign>,
        entities: BTreeMap<EntityId, AnyEntity>,
        feed: Vec<FeedEntry>,
        fail_writes: bool,
    }

    impl RecordingStore {
        fn seeded(campaign: Campaign, entities: Vec<AnyEntity>) -> Self {
            Self {
                campaign: Some(campaign),
                entities: entities
                    .into_iter()
                    .map(|entity| (entity.id(), entity))
                    .collect(),
                feed: Vec::new(),
                fail_writes: false,
            }
        }
    }

    impl CampaignStore for RecordingStore {
        fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
            let campaign = self.campaign.clone().ok_or(StoreFault::NoCampaign)?;
            Ok(CampaignSnapshot {
                campaign,
                entities: self.entities.values().cloned().collect(),
                feed: self.feed.clone(),
            })
        }

        fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
            if self.fail_writes {
                return Err(StoreFault::Unavailable {
                    message: "the disk is full".to_owned(),
                });
            }
            if let Some(campaign) = patch.campaign {
                self.campaign = Some(campaign.clone());
            }
            for entity in &patch.upserts {
                self.entities.insert(entity.id(), (*entity).clone());
            }
            for id in &patch.removals {
                self.entities.remove(id);
            }
            if let Some(keep_from) = patch.feed_keep_from {
                self.feed.retain(|entry| entry.id >= keep_from);
            }
            for entry in &patch.feed_appends {
                self.feed.push((*entry).clone());
            }
            Ok(())
        }
    }

    fn wizard(id: EntityId, max: i32, current: i32) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(max, current);
        AnyEntity::from(
            builder
                .build_entity(EntityKind::Pc, id, "Aria", PlayerId::new())
                .expect("abilities and hp are set"),
        )
    }

    fn statblock_of(state: &dyn WorkingSet, id: EntityId) -> StatblockEntity {
        match state
            .entity(id)
            .expect("the load-all view never fails")
            .expect("the entity exists")
        {
            AnyEntity::Statblock(document) => document,
            other => panic!("{id} is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn execute_persists_then_publishes() {
        let id = EntityId::new();
        let campaign = CampaignBuilder::new("Curse of the Vault").build();
        let mut store = RecordingStore::seeded(campaign, vec![wizard(id, 32, 32)]);
        let snapshot = store.load().expect("the double was just seeded");
        let mut executor = Executor::new(
            Box::new(LoadedCampaign::from_snapshot(snapshot)),
            &mut store,
            Box::new(FixedEnv),
        );

        let executed = executor
            .execute(&Command::ApplyDamage {
                entity: id,
                amount: 12,
                expected_rev: None,
            })
            .expect("damage is bookkeeping");

        assert_eq!(
            executed.events,
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }]
        );
        let working = statblock_of(executor.state(), id);
        assert_eq!(working.body.hp.current, 20);
        assert_eq!(working.rev, 1);
        assert_eq!(working.updated_at, UnixTime::from_seconds(FIXED_TIME));
        drop(executor);

        let loaded = store.load().expect("the write went through");
        assert_eq!(loaded.entities.len(), 1);
        assert_eq!(loaded.entities[0].rev(), 1);
        match &loaded.entities[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.hp.current, 20),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_store_fault_leaves_both_copies_untouched() {
        let id = EntityId::new();
        let campaign = CampaignBuilder::new("Curse of the Vault").build();
        let mut store = RecordingStore::seeded(campaign, vec![wizard(id, 32, 32)]);
        store.fail_writes = true;
        let snapshot = store.load().expect("the double was just seeded");
        let mut executor = Executor::new(
            Box::new(LoadedCampaign::from_snapshot(snapshot)),
            &mut store,
            Box::new(FixedEnv),
        );

        let result = executor.execute(&Command::ApplyDamage {
            entity: id,
            amount: 12,
            expected_rev: None,
        });

        assert!(matches!(
            result,
            Err(EngineError::Store(StoreFault::Unavailable { .. }))
        ));
        let working = statblock_of(executor.state(), id);
        assert_eq!(working.body.hp.current, 32, "the publish never ran");
        assert_eq!(working.rev, 0);
        drop(executor);

        let loaded = store
            .load()
            .expect("the failed write left the store readable");
        assert_eq!(loaded.entities.len(), 1);
        assert_eq!(loaded.entities[0].rev(), 0);
    }

    #[test]
    fn a_refused_command_changes_neither_copy() {
        let id = EntityId::new();
        let campaign = CampaignBuilder::new("Curse of the Vault").build();
        let mut store = RecordingStore::seeded(campaign, vec![wizard(id, 32, 32)]);
        let snapshot = store.load().expect("the double was just seeded");
        let mut executor = Executor::new(
            Box::new(LoadedCampaign::from_snapshot(snapshot)),
            &mut store,
            Box::new(FixedEnv),
        );

        let result = executor.execute(&Command::ApplyDamage {
            entity: id,
            amount: 12,
            expected_rev: Some(7),
        });

        match result {
            Err(EngineError::Conflict {
                entity,
                expected,
                actual,
            }) => {
                assert_eq!(entity, id);
                assert_eq!(expected, 7);
                assert_eq!(actual, 0);
            }
            other => panic!("a stale revision must conflict, got {other:?}"),
        }
        assert_eq!(statblock_of(executor.state(), id).body.hp.current, 32);
        drop(executor);

        let loaded = store.load().expect("the refused command wrote nothing");
        assert_eq!(loaded.entities.len(), 1);
        assert_eq!(loaded.entities[0].rev(), 0);
    }
}
