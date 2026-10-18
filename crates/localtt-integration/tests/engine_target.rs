//! U3 target test: the command executor, as a client uses it.
//!
//! This file is the fixed point for `docs/rfcs/engine-executor.md`: client code
//! written against the pinned API before any library code exists. It
//! intentionally does not compile until the U3 skeleton lands; that is the
//! spec-phase contract, not an oversight.

use std::collections::BTreeMap;

use localtt_adapter::store::memory::MemoryStore;
use localtt_core::{
    builders::ref_id,
    types::{
        AnyEntity,
        Audience,
        Campaign,
        CampaignTime,
        EntityId,
        EntityKind,
        FeedEntryId,
        FeedPayload,
        Formula,
        Recharge,
        Resource,
        ResourceId,
        ResourceKind,
        RestKind,
        StatblockEntity,
        StatusDuration,
        StatusId,
        StatusKind,
        UnixTime,
    },
};
use localtt_engine::{
    commands::{Command, CommandEnv, Event, StatusSpec},
    error::{ElementRef, EngineError, WarningCode},
    executor::Executor,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreFault},
    state::{LoadedCampaign, WorkingSet},
};
use localtt_integration::{fixtures, roundtrip::to_json};
use serde::Serialize;

/// The fixed wall-clock stamp the fixture cast uses.
const FIXED_TIME: i64 = 1_750_000_000;

/// The U3 environment seam: a frozen wall clock, so `updated_at` and feed
/// stamps are deterministic.
struct FixedEnv;

impl CommandEnv for FixedEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// A store whose next write can be made to fail; for the no-publish case.
struct FlakyStore {
    inner: MemoryStore,
    fail_writes: bool,
}

impl CampaignStore for FlakyStore {
    fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        self.inner.load()
    }

    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        if self.fail_writes {
            return Err(StoreFault::Unavailable {
                message: "the disk is full".to_owned(),
            });
        }
        self.inner.apply(patch)
    }
}

/// Serializes a core document; comparisons are byte-for-byte.
fn bytes<T: Serialize>(value: &T) -> String {
    to_json(value).expect("core documents serialize")
}

/// The party's entities as one creation patch (the client seeds the store
/// directly; `CreateEntity` is a later command slice). No feed appends: these
/// tests start from an empty feed.
fn seeding_patch<'a>(campaign: &'a Campaign, entities: &'a [AnyEntity]) -> StatePatch<'a> {
    StatePatch {
        campaign: Some(campaign),
        upserts: entities.iter().collect(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    }
}

/// The durable shape of a working set: campaign bytes, entities keyed by id
/// (comparison order-independent) and feed entries in ascending id order.
type Documents = (
    String,
    BTreeMap<EntityId, String>,
    Vec<(FeedEntryId, String)>,
);

fn state_documents(state: &dyn WorkingSet) -> Documents {
    let entities = state
        .headers()
        .into_iter()
        .map(|header| {
            let entity = state
                .entity(header.id)
                .expect("the working set resolves its own headers")
                .expect("a header implies a body");
            (header.id, bytes(&entity))
        })
        .collect();
    let feed = state
        .feed()
        .into_iter()
        .map(|entry| (entry.id, bytes(&entry)))
        .collect();
    (bytes(state.campaign()), entities, feed)
}

fn snapshot_documents(snapshot: &CampaignSnapshot) -> Documents {
    (
        bytes(&snapshot.campaign),
        snapshot
            .entities
            .iter()
            .map(|entity| (entity.id(), bytes(entity)))
            .collect(),
        snapshot
            .feed
            .iter()
            .map(|entry| (entry.id, bytes(entry)))
            .collect(),
    )
}

/// One statblock as the working set holds it.
fn statblock_of(state: &dyn WorkingSet, id: EntityId) -> StatblockEntity {
    match state
        .entity(id)
        .expect("the entity is loaded")
        .expect("the entity exists")
    {
        AnyEntity::Statblock(statblock) => statblock,
        other => panic!("{id} is a statblock, got {other:?}"),
    }
}

/// One statblock resource as the working set holds it.
fn resource_of(state: &dyn WorkingSet, entity: EntityId, resource: ResourceId) -> Resource {
    statblock_of(state, entity)
        .body
        .resources
        .into_iter()
        .find(|candidate| candidate.id == resource)
        .expect("the entity owns the resource")
}

#[test]
fn advancing_the_clock_expires_a_status_and_appends_a_feed_entry() {
    // Arrange: a warlock whose Hex times out at campaign second 3600.
    let party = fixtures::party();
    let lyra = fixtures::golden_warlock(party.player);
    let hex = lyra.body.statuses[0].clone();
    assert_eq!(hex.name, "Hex");
    assert_eq!(
        hex.duration,
        StatusDuration::Timed {
            expires_at: CampaignTime::from_seconds(3_600),
        }
    );
    let entities = [AnyEntity::from(lyra.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state =
        LoadedCampaign::from_snapshot(store.load().expect("the warlock was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    // Act: advance exactly onto the expiry instant.
    let executed = executor
        .execute(&Command::AdvanceClock { by_seconds: 3_600 })
        .expect("advancing the clock");

    // Assert: the campaign moved, the expired status stayed on the sheet (soft
    // expiry), and one `StatusExpired` entry reached the feed.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::CampaignChanged,
            Event::FeedAppended {
                ids: vec![FeedEntryId::new(1)],
            },
        ]
    );
    let working = executor.state();
    assert_eq!(
        working.campaign().clock.now_s,
        CampaignTime::from_seconds(3_600)
    );
    let sheet = statblock_of(working, lyra.id);
    assert_eq!(sheet.rev, 0, "a soft expiry does not rewrite the sheet");
    assert!(
        sheet.body.statuses.iter().any(|status| status.id == hex.id),
        "soft expiry keeps the status in the document"
    );
    let feed = working.feed();
    assert_eq!(feed.len(), 1);
    let entry = &feed[0];
    assert_eq!(entry.id, FeedEntryId::new(1));
    assert_eq!(entry.ts, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(entry.clock_s, CampaignTime::from_seconds(3_600));
    assert_eq!(entry.actor, Some(party.player));
    assert_eq!(entry.entity, Some(lyra.id));
    assert_eq!(entry.audience, Audience::RollerAndDm);
    assert_eq!(
        entry.payload,
        FeedPayload::StatusExpired {
            status: hex.id,
            name: "Hex".to_owned(),
        }
    );

    // And the reload mirrors the working set byte for byte.
    let mirrored = state_documents(working);
    drop(executor);
    let loaded = store.load().expect("the clock move persisted");
    assert_eq!(snapshot_documents(&loaded), mirrored);
}

#[test]
fn the_store_mirrors_the_working_set_after_a_command_sequence() {
    // Arrange: a wizard and a warlock.
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let lyra = fixtures::golden_warlock(party.player);
    let slot = aria.body.resources[0].id;
    let pact = lyra.body.resources[0].id;
    let entities = [AnyEntity::from(aria.clone()), AnyEntity::from(lyra.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state = LoadedCampaign::from_snapshot(store.load().expect("the party was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    // Act: damage, a status and a spend, in order.
    executor
        .execute(&Command::ApplyDamage {
            entity: aria.id,
            amount: 9,
            expected_rev: None,
        })
        .expect("damage is bookkeeping");
    executor
        .execute(&Command::AddStatus {
            entity: aria.id,
            status: StatusSpec {
                kind: StatusKind::Condition,
                name: "Prone".to_owned(),
                reference: Some(ref_id("srd2014:condition/prone").expect("a literal ref")),
                text: None,
                duration: StatusDuration::Indefinite,
                source: None,
                modifiers: Vec::new(),
            },
            expected_rev: None,
        })
        .expect("statuses are bookkeeping");
    executor
        .execute(&Command::SpendResource {
            entity: lyra.id,
            resource: pact,
            amount: 1,
            expected_rev: None,
        })
        .expect("spending is bookkeeping");

    // Assert: every stamped revision is visible, and nothing diverges.
    let working = executor.state();
    let aria_now = statblock_of(working, aria.id);
    assert_eq!(aria_now.body.hp.current, 23);
    assert_eq!(aria_now.rev, 2);
    assert_eq!(
        aria_now.updated_at,
        UnixTime::from_seconds(FIXED_TIME),
        "an upsert is stamped with the environment clock"
    );
    assert_eq!(aria_now.body.statuses.len(), 1);
    assert_eq!(
        aria_now.body.statuses[0].started_at,
        CampaignTime::from_seconds(0),
        "a status starts on the campaign clock"
    );
    assert_eq!(resource_of(working, aria.id, slot).current, 2);
    assert_eq!(resource_of(working, lyra.id, pact).current, 1);
    let mirrored = state_documents(working);
    drop(executor);

    let loaded = store.load().expect("every command persisted");
    assert_eq!(snapshot_documents(&loaded), mirrored);
}

#[test]
fn spending_a_resource_at_zero_warns_and_succeeds() {
    let party = fixtures::party();
    let lyra = fixtures::golden_warlock(party.player);
    let pact = lyra.body.resources[0].id;
    let entities = [AnyEntity::from(lyra.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state =
        LoadedCampaign::from_snapshot(store.load().expect("the warlock was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    executor
        .execute(&Command::SpendResource {
            entity: lyra.id,
            resource: pact,
            amount: 2,
            expected_rev: None,
        })
        .expect("spending the pact slots");

    let executed = executor
        .execute(&Command::SpendResource {
            entity: lyra.id,
            resource: pact,
            amount: 1,
            expected_rev: None,
        })
        .expect("overspending is warned, never blocked");

    assert_eq!(executed.warnings.len(), 1);
    let warning = &executed.warnings[0];
    assert_eq!(warning.code, WarningCode::ResourceOverspent);
    assert_eq!(warning.entity, Some(lyra.id));
    assert_eq!(warning.element, Some(ElementRef::Resource(pact)));
    // The exact sentence is not contractual in U3 (5.8); the code and the
    // scoping are.
    assert!(!warning.message.is_empty());
    assert_eq!(
        executed.events,
        vec![Event::EntityChanged {
            id: lyra.id,
            rev: 2,
            kind: EntityKind::Pc,
        }]
    );
    assert_eq!(resource_of(executor.state(), lyra.id, pact).current, -1);
}

#[test]
fn a_stale_revision_conflicts_and_writes_nothing() {
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state = LoadedCampaign::from_snapshot(store.load().expect("the wizard was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    executor
        .execute(&Command::ApplyDamage {
            entity: aria.id,
            amount: 9,
            expected_rev: Some(0),
        })
        .expect("the fresh revision matches");
    let before = state_documents(executor.state());

    let result = executor.execute(&Command::ApplyDamage {
        entity: aria.id,
        amount: 3,
        expected_rev: Some(0),
    });
    match result {
        Err(EngineError::Conflict {
            entity,
            expected,
            actual,
        }) => {
            assert_eq!(entity, aria.id);
            assert_eq!(expected, 0);
            assert_eq!(actual, 1);
        }
        other => panic!("a stale revision must conflict, got {other:?}"),
    }

    assert_eq!(state_documents(executor.state()), before);
    drop(executor);
    let loaded = store.load().expect("the refused command wrote nothing");
    assert_eq!(snapshot_documents(&loaded), before);
}

#[test]
fn unknown_elements_and_entities_are_not_found() {
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let entities = [AnyEntity::from(aria.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state = LoadedCampaign::from_snapshot(store.load().expect("the wizard was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));
    let before = state_documents(executor.state());

    let missing = StatusId::new();
    let result = executor.execute(&Command::RemoveStatus {
        entity: aria.id,
        status: missing,
        expected_rev: None,
    });
    match result {
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Status(status)),
        }) => {
            assert_eq!(entity, aria.id);
            assert_eq!(status, missing);
        }
        other => panic!("an unknown status must be NotFound, got {other:?}"),
    }

    let ghost = EntityId::new();
    let result = executor.execute(&Command::ApplyDamage {
        entity: ghost,
        amount: 1,
        expected_rev: None,
    });
    assert!(matches!(
        result,
        Err(EngineError::NotFound {
            entity,
            element: None,
        }) if entity == ghost
    ));

    let result = executor.execute(&Command::ApplyDamage {
        entity: aria.id,
        amount: -1,
        expected_rev: None,
    });
    assert!(matches!(result, Err(EngineError::Malformed { .. })));

    assert_eq!(state_documents(executor.state()), before);
    drop(executor);
    let loaded = store.load().expect("the refused commands wrote nothing");
    assert_eq!(snapshot_documents(&loaded), before);
}

#[test]
fn crossing_dawn_refills_dawn_resources_and_rewinding_warns() {
    // Arrange: two casters with spent resources that recharge at dawn.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let mut lyra = fixtures::golden_warlock(party.player);
    let slot = aria.body.resources[0].id;
    let crystal: ResourceId = ResourceId::new();
    aria.body.resources.push(Resource {
        id: crystal,
        name: "Dawn Crystal".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(1),
        current: 0,
        recharge: Recharge::Dawn,
    });
    let gem: ResourceId = ResourceId::new();
    lyra.body.resources.push(Resource {
        id: gem,
        name: "Dawn Gem".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(2),
        current: 0,
        recharge: Recharge::Dawn,
    });
    // Hex would expire on this move; the expiry sweep is pinned elsewhere.
    lyra.body.statuses.clear();
    let entities = [AnyEntity::from(aria.clone()), AnyEntity::from(lyra.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state =
        LoadedCampaign::from_snapshot(store.load().expect("the casters were just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    // Spend the long-rest slot so the dawn refill is provably scoped.
    executor
        .execute(&Command::SpendResource {
            entity: aria.id,
            resource: slot,
            amount: 1,
            expected_rev: None,
        })
        .expect("spending the 3rd-level slot");

    // Act: cross dawn (day 0, dawn_hour 6) by advancing 6 hours.
    let executed = executor
        .execute(&Command::AdvanceClock { by_seconds: 21_600 })
        .expect("advancing the clock across dawn");

    // Assert: both refilled entities are reported, in ascending id order.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::CampaignChanged,
            Event::EntityChanged {
                id: aria.id,
                rev: 2,
                kind: EntityKind::Pc,
            },
            Event::EntityChanged {
                id: lyra.id,
                rev: 1,
                kind: EntityKind::Pc,
            },
        ]
    );
    let working = executor.state();
    assert_eq!(resource_of(working, aria.id, crystal).current, 1);
    assert_eq!(resource_of(working, lyra.id, gem).current, 2);
    assert_eq!(
        resource_of(working, aria.id, slot).current,
        1,
        "dawn is not a rest: long-rest slots stay spent"
    );

    // Act: rewind across the same dawn.
    let executed = executor
        .execute(&Command::SetClock {
            to: CampaignTime::from_seconds(0),
        })
        .expect("correcting the clock backwards");

    assert_eq!(executed.warnings.len(), 1);
    assert_eq!(executed.warnings[0].code, WarningCode::DawnRewind);
    assert_eq!(executed.events, vec![Event::CampaignChanged]);
    assert_eq!(
        resource_of(executor.state(), aria.id, crystal).current,
        1,
        "rewinding never reverses a refill"
    );
    assert_eq!(
        resource_of(executor.state(), lyra.id, gem).current,
        2,
        "rewinding never reverses a refill"
    );
    assert_eq!(
        executor.state().campaign().clock.now_s,
        CampaignTime::from_seconds(0)
    );
}

#[test]
fn a_short_rest_refills_short_resources_and_clears_rest_statuses() {
    // Arrange: the fighter, a spent Second Wind, and a short-rest status.
    let party = fixtures::party();
    let borin = fixtures::golden_fighter(party.player);
    let second_wind = borin.body.resources[0].id;
    let entities = [AnyEntity::from(borin.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state =
        LoadedCampaign::from_snapshot(store.load().expect("the fighter was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    executor
        .execute(&Command::SpendResource {
            entity: borin.id,
            resource: second_wind,
            amount: 1,
            expected_rev: None,
        })
        .expect("spending Second Wind");
    executor
        .execute(&Command::AddStatus {
            entity: borin.id,
            status: StatusSpec {
                kind: StatusKind::Effect,
                name: "Winded".to_owned(),
                reference: None,
                text: None,
                duration: StatusDuration::UntilRest {
                    rest: RestKind::Short,
                },
                source: None,
                modifiers: Vec::new(),
            },
            expected_rev: None,
        })
        .expect("adding a rest-typed status");

    // Act: a short rest that also advances the clock by ten minutes.
    let executed = executor
        .execute(&Command::ShortRest {
            entity: borin.id,
            advance_seconds: Some(600),
            expected_rev: None,
        })
        .expect("a short rest");

    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::CampaignChanged,
            Event::EntityChanged {
                id: borin.id,
                rev: 3,
                kind: EntityKind::Pc,
            },
        ]
    );
    let working = executor.state();
    assert_eq!(resource_of(working, borin.id, second_wind).current, 1);
    assert!(statblock_of(working, borin.id).body.statuses.is_empty());
    assert_eq!(
        working.campaign().clock.now_s,
        CampaignTime::from_seconds(600)
    );
}

#[test]
fn one_transition_emits_campaign_entity_and_feed_events() {
    // Arrange: the fighter with a spent Second Wind and a timed status that
    // expires exactly when the rest advance lands.
    let party = fixtures::party();
    let borin = fixtures::golden_fighter(party.player);
    let second_wind = borin.body.resources[0].id;
    let entities = [AnyEntity::from(borin.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let state =
        LoadedCampaign::from_snapshot(store.load().expect("the fighter was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    executor
        .execute(&Command::SpendResource {
            entity: borin.id,
            resource: second_wind,
            amount: 1,
            expected_rev: None,
        })
        .expect("spending Second Wind");
    executor
        .execute(&Command::AddStatus {
            entity: borin.id,
            status: StatusSpec {
                kind: StatusKind::Effect,
                name: "Bleeding".to_owned(),
                reference: None,
                text: None,
                duration: StatusDuration::Timed {
                    expires_at: CampaignTime::from_seconds(3_600),
                },
                source: None,
                modifiers: Vec::new(),
            },
            expected_rev: None,
        })
        .expect("adding a timed status");

    // Act: a short rest whose advance lands exactly on the expiry instant.
    let executed = executor
        .execute(&Command::ShortRest {
            entity: borin.id,
            advance_seconds: Some(3_600),
            expected_rev: None,
        })
        .expect("a short rest across the expiry");

    // Assert: one transition reports campaign, entity and feed changes, in
    // that order.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![
            Event::CampaignChanged,
            Event::EntityChanged {
                id: borin.id,
                rev: 3,
                kind: EntityKind::Pc,
            },
            Event::FeedAppended {
                ids: vec![FeedEntryId::new(1)],
            },
        ]
    );
    let working = executor.state();
    assert_eq!(
        working.campaign().clock.now_s,
        CampaignTime::from_seconds(3_600)
    );
    assert_eq!(resource_of(working, borin.id, second_wind).current, 1);
    let sheet = statblock_of(working, borin.id);
    let status = sheet
        .body
        .statuses
        .first()
        .expect("the expired status stays on the sheet");
    let feed = working.feed();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].actor, Some(party.player));
    assert_eq!(feed[0].entity, Some(borin.id));
    assert_eq!(feed[0].audience, Audience::RollerAndDm);
    assert_eq!(feed[0].clock_s, CampaignTime::from_seconds(3_600));
    assert_eq!(
        feed[0].payload,
        FeedPayload::StatusExpired {
            status: status.id,
            name: "Bleeding".to_owned(),
        }
    );
}

#[test]
fn a_store_fault_publishes_nothing() {
    // Arrange: a wizard in a store whose next write fails.
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let entities = [AnyEntity::from(aria.clone())];
    let mut inner = MemoryStore::new();
    inner
        .apply(&seeding_patch(&party.campaign, &entities))
        .expect("a fresh store accepts the party");
    let mut store = FlakyStore {
        inner,
        fail_writes: true,
    };
    let state = LoadedCampaign::from_snapshot(store.load().expect("the wizard was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));
    let before = state_documents(executor.state());

    // Act: a command whose store write fails.
    let result = executor.execute(&Command::ApplyDamage {
        entity: aria.id,
        amount: 9,
        expected_rev: None,
    });

    // Assert: the port fault is transparent, and nothing was published.
    match result {
        Err(EngineError::Store(StoreFault::Unavailable { .. })) => {}
        other => panic!("a store fault must surface transparently, got {other:?}"),
    }
    assert_eq!(state_documents(executor.state()), before);
    drop(executor);
    let loaded = store.load().expect("the refused command wrote nothing");
    assert_eq!(snapshot_documents(&loaded), before);
}

#[test]
fn the_executor_is_send() {
    fn assert_send<T: Send>() {}

    assert_send::<Executor<'static>>();
}
