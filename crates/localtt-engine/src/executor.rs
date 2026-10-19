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
    /// The authorization pre-check runs before any command work (RFC
    /// `authz.md` 5.7); the actor is then recorded for roll attribution and
    /// the audit trail.
    #[inline]
    pub fn execute_as(
        &mut self,
        actor: PlayerId,
        command: &Command,
    ) -> Result<Executed, EngineError> {
        self.pre_check(actor, command)?;
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
    /// before any command work runs (RFC `authz.md` 5.7). An absent target
    /// entity defers to the command's own `NotFound`.
    fn pre_check(&self, actor: PlayerId, command: &Command) -> Result<(), EngineError> {
        match classify(command) {
            Requirement::Dm => {
                if crate::authz::dm(self.state.campaign(), actor) {
                    Ok(())
                } else {
                    Err(EngineError::Unauthorized {
                        actor,
                        entity: None,
                    })
                }
            }
            Requirement::Member => {
                if crate::authz::member(self.state.campaign(), actor) {
                    Ok(())
                } else {
                    Err(EngineError::Unauthorized {
                        actor,
                        entity: None,
                    })
                }
            }
            Requirement::Read(id) => {
                let Some(entity) = self.state.entity(id)? else {
                    return Ok(());
                };
                if crate::authz::can_read(self.state.campaign(), &entity, actor) {
                    Ok(())
                } else {
                    Err(EngineError::Unauthorized {
                        actor,
                        entity: Some(id),
                    })
                }
            }
            Requirement::Write(id) => {
                let Some(entity) = self.state.entity(id)? else {
                    return Ok(());
                };
                if crate::authz::can_write(self.state.campaign(), &entity, actor) {
                    Ok(())
                } else {
                    Err(EngineError::Unauthorized {
                        actor,
                        entity: Some(id),
                    })
                }
            }
        }
    }
}

/// What a command needs from the actor before any command work runs
/// (RFC `authz.md` 4.3).
#[derive(Debug, PartialEq, Eq)]
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
/// (RFC `authz.md` 5.7). Matched exhaustively: a new command variant must
/// choose a requirement here.
fn classify(command: &Command) -> Requirement {
    match command {
        Command::AdvanceClock { .. } | Command::SetClock { .. } => Requirement::Dm,
        Command::Roll {
            entity: Some(id), ..
        } => Requirement::Read(*id),
        Command::Roll { entity: None, .. } => Requirement::Member,
        Command::AddStatus { entity, .. }
        | Command::RemoveStatus { entity, .. }
        | Command::EditStatusDuration { entity, .. }
        | Command::ApplyDamage { entity, .. }
        | Command::Heal { entity, .. }
        | Command::SetTempHp { entity, .. }
        | Command::SetDeathSaves { entity, .. }
        | Command::SpendResource { entity, .. }
        | Command::RestoreResource { entity, .. }
        | Command::ShortRest { entity, .. }
        | Command::LongRest { entity, .. }
        | Command::CastSpell { entity, .. }
        | Command::BeginConcentration { entity, .. }
        | Command::EndConcentration { entity, .. }
        | Command::SpendHitDie { entity, .. } => Requirement::Write(*entity),
    }
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
            AclEntry,
            AclLevel,
            AnyEntity,
            Campaign,
            CampaignTime,
            DeathSaves,
            DiceExpr,
            EntityId,
            EntityKind,
            FeedEntry,
            PlayerId,
            ResourceId,
            SpellEntryId,
            StatblockEntity,
            StatusDuration,
            StatusId,
            StatusKind,
            UnixTime,
        },
    };

    use super::*;
    use crate::{
        commands::{RollSpec, StatusSpec},
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

    /// A pc owned by `owner` and carrying `acl`.
    fn owned_wizard(id: EntityId, owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(32, 32);
        let mut entity = builder
            .build_entity(EntityKind::Pc, id, "Aria", owner)
            .expect("abilities and hp are set");
        entity.acl = acl;
        AnyEntity::from(entity)
    }

    #[test]
    fn execute_as_refuses_an_unauthorized_writer() {
        let id = EntityId::new();
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("Dungeon Master");
        let owner = builder.player("Owner");
        let stranger = builder.player("Stranger");
        let writer = builder.player("Writer");
        let campaign = builder.build();
        let mut store = RecordingStore::seeded(
            campaign,
            vec![owned_wizard(
                id,
                owner,
                vec![AclEntry {
                    player: writer,
                    level: AclLevel::Write,
                    field_mask: None,
                }],
            )],
        );
        let snapshot = store.load().expect("the double was just seeded");
        let mut executor = Executor::new(
            Box::new(LoadedCampaign::from_snapshot(snapshot)),
            &mut store,
            Box::new(FixedEnv),
        );
        let damage = Command::ApplyDamage {
            entity: id,
            amount: 5,
            expected_rev: None,
        };

        let result = executor.execute_as(stranger, &damage);
        assert!(matches!(
            result,
            Err(EngineError::Unauthorized { actor, entity: Some(target) })
                if actor == stranger && target == id
        ));
        assert_eq!(
            statblock_of(executor.state(), id).body.hp.current,
            32,
            "the denial leaves the working set untouched"
        );

        executor
            .execute_as(writer, &damage)
            .expect("the write grant permits damage");
        assert_eq!(statblock_of(executor.state(), id).body.hp.current, 27);
        executor
            .execute_as(owner, &damage)
            .expect("the owner permits damage");
        assert_eq!(statblock_of(executor.state(), id).body.hp.current, 22);
        executor
            .execute_as(dm, &damage)
            .expect("the DM permits damage");
        assert_eq!(statblock_of(executor.state(), id).body.hp.current, 17);
    }

    #[test]
    fn the_pre_check_defers_absent_targets_and_guards_levels() {
        let id = EntityId::new();
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("Dungeon Master");
        let owner = builder.player("Owner");
        let stranger = builder.player("Stranger");
        let outsider = PlayerId::new();
        let campaign = builder.build();
        let mut store = RecordingStore::seeded(campaign, vec![owned_wizard(id, owner, Vec::new())]);
        let snapshot = store.load().expect("the double was just seeded");
        let mut executor = Executor::new(
            Box::new(LoadedCampaign::from_snapshot(snapshot)),
            &mut store,
            Box::new(FixedEnv),
        );

        // An absent target defers to the command's own NotFound, for any actor.
        let ghost = EntityId::new();
        let result = executor.execute_as(
            stranger,
            &Command::ApplyDamage {
                entity: ghost,
                amount: 1,
                expected_rev: None,
            },
        );
        assert!(matches!(
            result,
            Err(EngineError::NotFound { entity, element: None }) if entity == ghost
        ));

        // The clock is DM-only.
        let result = executor.execute_as(stranger, &Command::AdvanceClock { by_seconds: 60 });
        assert!(matches!(
            result,
            Err(EngineError::Unauthorized { actor, entity: None }) if actor == stranger
        ));
        executor
            .execute_as(dm, &Command::AdvanceClock { by_seconds: 60 })
            .expect("the DM moves the clock");
        assert_eq!(
            executor.state().campaign().clock.now_s,
            CampaignTime::from_seconds(60)
        );

        // A targeted roll needs a read; a free roll, membership alone.
        let targeted = Command::Roll {
            entity: Some(id),
            spec: RollSpec::Expression {
                expr: DiceExpr::Const { value: 1 },
                target: None,
            },
            audience: None,
        };
        let result = executor.execute_as(stranger, &targeted);
        assert!(matches!(
            result,
            Err(EngineError::Unauthorized { actor, entity: Some(target) })
                if actor == stranger && target == id
        ));
        let free = Command::Roll {
            entity: None,
            spec: RollSpec::Expression {
                expr: DiceExpr::Const { value: 1 },
                target: None,
            },
            audience: None,
        };
        let result = executor.execute_as(outsider, &free);
        assert!(matches!(
            result,
            Err(EngineError::Unauthorized { actor, entity: None }) if actor == outsider
        ));
        let result = executor.execute_as(stranger, &free);
        assert!(
            matches!(result, Err(EngineError::Environment { .. })),
            "membership passed; only the dice port is missing"
        );
    }

    /// One status spec for the sweep; nothing about it is under test here.
    fn status_spec() -> StatusSpec {
        StatusSpec {
            kind: StatusKind::Effect,
            name: "Stunned".to_owned(),
            reference: None,
            text: None,
            duration: StatusDuration::Indefinite,
            source: None,
            modifiers: Vec::new(),
        }
    }

    #[test]
    fn classify_names_a_requirement_for_every_variant() {
        let id = EntityId::new();
        let roll = RollSpec::Expression {
            expr: DiceExpr::Const { value: 1 },
            target: None,
        };
        let cases = [
            (Command::AdvanceClock { by_seconds: 60 }, Requirement::Dm),
            (
                Command::SetClock {
                    to: CampaignTime::from_seconds(0),
                },
                Requirement::Dm,
            ),
            (
                Command::AddStatus {
                    entity: id,
                    status: status_spec(),
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::RemoveStatus {
                    entity: id,
                    status: StatusId::new(),
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::EditStatusDuration {
                    entity: id,
                    status: StatusId::new(),
                    duration: StatusDuration::Indefinite,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::ApplyDamage {
                    entity: id,
                    amount: 1,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::Heal {
                    entity: id,
                    amount: 1,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::SetTempHp {
                    entity: id,
                    amount: 1,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::SetDeathSaves {
                    entity: id,
                    value: DeathSaves::default(),
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::SpendResource {
                    entity: id,
                    resource: ResourceId::new(),
                    amount: 1,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::RestoreResource {
                    entity: id,
                    resource: ResourceId::new(),
                    amount: 1,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::ShortRest {
                    entity: id,
                    advance_seconds: None,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::LongRest {
                    entity: id,
                    advance_seconds: None,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::Roll {
                    entity: None,
                    spec: roll.clone(),
                    audience: None,
                },
                Requirement::Member,
            ),
            (
                Command::Roll {
                    entity: Some(id),
                    spec: roll,
                    audience: None,
                },
                Requirement::Read(id),
            ),
            (
                Command::CastSpell {
                    entity: id,
                    spell: SpellEntryId::new(),
                    slot: ResourceId::new(),
                    upcast_slots: 0,
                    concentrate: false,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::BeginConcentration {
                    entity: id,
                    status: status_spec(),
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::EndConcentration {
                    entity: id,
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
            (
                Command::SpendHitDie {
                    entity: id,
                    resource: ResourceId::new(),
                    expected_rev: None,
                },
                Requirement::Write(id),
            ),
        ];
        assert_eq!(
            cases.len(),
            19,
            "one case per variant, plus the two Roll spellings"
        );

        for (command, expected) in cases {
            assert_eq!(classify(&command), expected, "{command:?}");
        }
    }
}
