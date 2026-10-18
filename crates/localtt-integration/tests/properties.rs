//! Property and regression checks around the store boundary (RFC
//! `engine-executor.md` 5.1): after every command the store snapshot and the
//! working set are byte-identical, including the ring trim the store's patch
//! preconditions must accept and the campaign document a rest writes.

use std::collections::BTreeMap;

use localtt_adapter::store::memory::MemoryStore;
use localtt_core::types::{
    AnyEntity,
    Audience,
    Campaign,
    CampaignTime,
    DeathSaves,
    Entity,
    EntityId,
    EntityKind,
    FeedEntry,
    FeedEntryId,
    FeedPayload,
    NoteBody,
    PlayerId,
    ResourceId,
    RestKind,
    Status,
    StatusDuration,
    StatusId,
    StatusKind,
    UnixTime,
};
use localtt_engine::{
    commands::{Command, CommandEnv, StatusSpec},
    error::EngineError,
    executor::Executor,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch},
    state::{LoadedCampaign, WorkingSet},
};
use localtt_integration::{fixtures, roundtrip::to_json};
use proptest::prelude::*;
use serde::Serialize;

const FIXED_TIME: i64 = 1_750_000_000;

struct FixedEnv;

impl CommandEnv for FixedEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// Serializes a core document; comparisons are byte-for-byte.
fn bytes<T: Serialize>(value: &T) -> String {
    to_json(value).expect("core documents serialize")
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

fn seeding_patch<'a>(campaign: &'a Campaign, entities: &'a [AnyEntity]) -> StatePatch<'a> {
    StatePatch {
        campaign: Some(campaign),
        upserts: entities.iter().collect(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    }
}

/// One generated command, with slots instead of ids.
#[derive(Clone, Debug)]
enum Op {
    Damage {
        entity: u8,
        amount: u8,
    },
    Heal {
        entity: u8,
        amount: u8,
    },
    Temp {
        entity: u8,
        amount: u8,
    },
    DeathSaves {
        entity: u8,
        successes: u8,
        failures: u8,
    },
    AddStatus {
        entity: u8,
        kind: u8,
        duration: u8,
        expires_in: u16,
    },
    RemoveStatus {
        entity: u8,
        slot: u8,
    },
    EditStatus {
        entity: u8,
        slot: u8,
        duration: u8,
        expires_in: u16,
    },
    Spend {
        entity: u8,
        resource: u8,
        amount: u8,
    },
    Restore {
        entity: u8,
        resource: u8,
        amount: u8,
    },
    Advance {
        by: i32,
    },
    SetClock {
        to: i32,
    },
    Rest {
        entity: u8,
        long: bool,
        advance: Option<i32>,
    },
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<u8>(), 0u8..=40).prop_map(|(entity, amount)| Op::Damage { entity, amount }),
        (any::<u8>(), 0u8..=40).prop_map(|(entity, amount)| Op::Heal { entity, amount }),
        (any::<u8>(), 0u8..=40).prop_map(|(entity, amount)| Op::Temp { entity, amount }),
        (any::<u8>(), 0u8..=4, 0u8..=4).prop_map(|(entity, successes, failures)| Op::DeathSaves {
            entity,
            successes,
            failures
        }),
        (any::<u8>(), any::<u8>(), any::<u8>(), 0u16..=7_200).prop_map(
            |(entity, kind, duration, expires_in)| Op::AddStatus {
                entity,
                kind,
                duration,
                expires_in
            }
        ),
        (any::<u8>(), any::<u8>()).prop_map(|(entity, slot)| Op::RemoveStatus { entity, slot }),
        (any::<u8>(), any::<u8>(), any::<u8>(), 0u16..=7_200).prop_map(
            |(entity, slot, duration, expires_in)| Op::EditStatus {
                entity,
                slot,
                duration,
                expires_in
            }
        ),
        (any::<u8>(), any::<u8>(), 0u8..=4).prop_map(|(entity, resource, amount)| Op::Spend {
            entity,
            resource,
            amount
        }),
        (any::<u8>(), any::<u8>(), 0u8..=4).prop_map(|(entity, resource, amount)| Op::Restore {
            entity,
            resource,
            amount
        }),
        any::<i32>().prop_map(|by| Op::Advance { by }),
        any::<i32>().prop_map(|to| Op::SetClock { to }),
        (any::<u8>(), any::<bool>(), prop::option::of(0i32..=86_400)).prop_map(
            |(entity, long, advance)| Op::Rest {
                entity,
                long,
                advance
            }
        ),
    ]
}

/// The fixed cast and the slot -> id mapping the generated ops use.
struct World {
    campaign: Campaign,
    entities: Vec<AnyEntity>,
    ids: Vec<EntityId>,
}

impl World {
    fn new() -> Self {
        let party = fixtures::party();
        let aria = AnyEntity::from(fixtures::golden_wizard(party.player));
        let borin = AnyEntity::from(fixtures::golden_fighter(party.player));
        let lyra = AnyEntity::from(fixtures::golden_warlock(party.player));
        let note = AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                EntityId::new(),
                "Session 4",
                PlayerId::new(),
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .build()
            .expect("a note kind pairs with a note body"),
        );
        let mut campaign = party.campaign;
        campaign.settings.feed_capacity = 2;
        let entities = vec![aria, borin, lyra, note];
        let ids = entities.iter().map(AnyEntity::id).collect();
        Self {
            campaign,
            entities,
            ids,
        }
    }

    fn entity(&self, slot: u8) -> EntityId {
        self.ids[usize::from(slot) % self.ids.len()]
    }

    /// The command for one op. Total by construction: when the pick finds no
    /// live status or resource (the note has neither, and earlier ops may have
    /// removed them all), the op falls back to a fresh id and runs as a
    /// `NotFound` probe. A case can never turn vacuous by skipping every op.
    fn command(&self, snapshot: &CampaignSnapshot, op: &Op) -> Command {
        let entity = |slot: u8| self.entity(slot);
        match *op {
            Op::Damage {
                entity: slot,
                amount,
            } => Command::ApplyDamage {
                entity: entity(slot),
                amount: i32::from(amount),
                expected_rev: None,
            },
            Op::Heal {
                entity: slot,
                amount,
            } => Command::Heal {
                entity: entity(slot),
                amount: i32::from(amount),
                expected_rev: None,
            },
            Op::Temp {
                entity: slot,
                amount,
            } => Command::SetTempHp {
                entity: entity(slot),
                amount: i32::from(amount),
                expected_rev: None,
            },
            Op::DeathSaves {
                entity: slot,
                successes,
                failures,
            } => Command::SetDeathSaves {
                entity: entity(slot),
                value: DeathSaves {
                    successes,
                    failures,
                },
                expected_rev: None,
            },
            Op::AddStatus {
                entity: slot,
                kind,
                duration,
                expires_in,
            } => Command::AddStatus {
                entity: entity(slot),
                status: StatusSpec {
                    kind: status_kind(kind),
                    name: "Injected".to_owned(),
                    reference: None,
                    text: None,
                    duration: status_duration(snapshot, duration, expires_in),
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
            Op::RemoveStatus {
                entity: entity_slot,
                slot: pick,
            } => Command::RemoveStatus {
                entity: entity(entity_slot),
                status: nth_status(snapshot, entity(entity_slot), pick).unwrap_or_default(),
                expected_rev: None,
            },
            Op::EditStatus {
                entity: entity_slot,
                slot: pick,
                duration,
                expires_in,
            } => Command::EditStatusDuration {
                entity: entity(entity_slot),
                status: nth_status(snapshot, entity(entity_slot), pick).unwrap_or_default(),
                duration: status_duration(snapshot, duration, expires_in),
                expected_rev: None,
            },
            Op::Spend {
                entity: slot,
                resource,
                amount,
            } => Command::SpendResource {
                entity: entity(slot),
                resource: nth_resource(snapshot, entity(slot), resource).unwrap_or_default(),
                amount: i32::from(amount),
                expected_rev: None,
            },
            Op::Restore {
                entity: slot,
                resource,
                amount,
            } => Command::RestoreResource {
                entity: entity(slot),
                resource: nth_resource(snapshot, entity(slot), resource).unwrap_or_default(),
                amount: i32::from(amount),
                expected_rev: None,
            },
            Op::Advance { by } => Command::AdvanceClock {
                by_seconds: i64::from(by),
            },
            Op::SetClock { to } => Command::SetClock {
                to: CampaignTime::from_seconds(i64::from(to)),
            },
            Op::Rest {
                entity: slot,
                long,
                advance,
            } => {
                let entity = entity(slot);
                let advance_seconds = advance.map(i64::from);
                if long {
                    Command::LongRest {
                        entity,
                        advance_seconds,
                        expected_rev: None,
                    }
                } else {
                    Command::ShortRest {
                        entity,
                        advance_seconds,
                        expected_rev: None,
                    }
                }
            }
        }
    }
}

fn nth_status(snapshot: &CampaignSnapshot, entity: EntityId, pick: u8) -> Option<StatusId> {
    let document = snapshot
        .entities
        .iter()
        .find(|candidate| candidate.id() == entity)?;
    let AnyEntity::Statblock(document) = document else {
        return None;
    };
    if document.body.statuses.is_empty() {
        return None;
    }
    Some(document.body.statuses[usize::from(pick) % document.body.statuses.len()].id)
}

fn nth_resource(snapshot: &CampaignSnapshot, entity: EntityId, pick: u8) -> Option<ResourceId> {
    let document = snapshot
        .entities
        .iter()
        .find(|candidate| candidate.id() == entity)?;
    let AnyEntity::Statblock(document) = document else {
        return None;
    };
    if document.body.resources.is_empty() {
        return None;
    }
    Some(document.body.resources[usize::from(pick) % document.body.resources.len()].id)
}

fn status_kind(kind: u8) -> StatusKind {
    match kind % 3 {
        0 => StatusKind::Condition,
        1 => StatusKind::Effect,
        _ => StatusKind::Concentration,
    }
}

fn status_duration(snapshot: &CampaignSnapshot, duration: u8, expires_in: u16) -> StatusDuration {
    let now = snapshot.campaign.clock.now_s.seconds();
    match duration % 4 {
        0 => StatusDuration::Timed {
            expires_at: CampaignTime::from_seconds(now + i64::from(expires_in % 7_200)),
        },
        1 => StatusDuration::UntilRest {
            rest: RestKind::Short,
        },
        2 => StatusDuration::UntilRest {
            rest: RestKind::Long,
        },
        _ => StatusDuration::Indefinite,
    }
}

fn feed(id: u64) -> FeedEntry {
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

fn timed(name: &str, expires_at: i64) -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Condition,
        name: name.to_owned(),
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

/// The proptest configuration.
///
/// Under Miri the file-backed failure persistence cannot run (`getcwd` is not
/// available with isolation) and the case count is cut to a handful: Miri
/// interprets every instruction, so the full count takes minutes and adds no
/// UB signal the handful does not. Everywhere else failing seeds persist under
/// `proptest-regressions/`.
fn property_config() -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(if cfg!(miri) { 8 } else { 48 });
    if cfg!(miri) {
        config.failure_persistence = None;
    }
    config
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn the_store_and_the_working_set_stay_in_lockstep(
        ops in prop::collection::vec(op_strategy(), 0..14)
    ) {
        let world = World::new();
        let mut store = MemoryStore::new();
        let stored = [feed(1), feed(2)];
        store
            .apply(&StatePatch {
                campaign: Some(&world.campaign),
                upserts: world.entities.iter().collect(),
                removals: Vec::new(),
                feed_appends: stored.iter().collect(),
                feed_keep_from: None,
            })
            .expect("a fresh store accepts the party");
        let mut snapshot = store.load().expect("the party was just committed");

        for op in &ops {
            let command = world.command(&snapshot, op);
            let state = LoadedCampaign::from_snapshot(snapshot.clone());
            let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));
            let outcome = executor.execute(&command);
            // A store fault here means the engine built a patch the store
            // refuses; that is an engine bug, never a test expectation.
            assert!(
                !matches!(outcome, Err(EngineError::Store(_))),
                "op {op:?} hit a store fault: {outcome:?}"
            );
            let working = state_documents(executor.state());
            drop(executor);

            let stored = store.load().expect("the store stays readable");
            assert_eq!(
                snapshot_documents(&stored),
                working,
                "op {op:?} ({outcome:?}) diverged the store from the working set"
            );
            snapshot = stored;
        }
    }
}

#[test]
fn a_ring_overflow_through_the_store_keeps_both_copies_equal() {
    // Arrange: capacity 2, four timed statuses and two stored feed entries.
    let party = fixtures::party();
    let mut borin = fixtures::golden_fighter(party.player);
    borin.body.statuses = vec![
        timed("A", 100),
        timed("B", 200),
        timed("C", 300),
        timed("D", 1_000),
    ];
    let mut campaign = party.campaign.clone();
    campaign.settings.feed_capacity = 2;
    let entities = [AnyEntity::from(borin.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&StatePatch {
            campaign: Some(&campaign),
            upserts: entities.iter().collect(),
            removals: Vec::new(),
            feed_appends: vec![&feed(1), &feed(2)],
            feed_keep_from: None,
        })
        .expect("a fresh store accepts the campaign and its feed");
    let state =
        LoadedCampaign::from_snapshot(store.load().expect("the fighter was just committed"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(FixedEnv));

    // Act: one move sweeps three expiries, minting 3, 4 and 5 against a ring
    // of two, so the trim and the appends land in the same patch.
    executor
        .execute(&Command::AdvanceClock { by_seconds: 300 })
        .expect("a clock move is bookkeeping");

    let feed_ids = |state: &dyn WorkingSet| {
        state
            .feed()
            .into_iter()
            .map(|entry| entry.id.get())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        feed_ids(executor.state()),
        vec![4, 5],
        "the newest two minted entries survive; the stored 1 and 2 are trimmed"
    );
    assert_eq!(executor.state().next_feed_id(), 6);

    // Act: one more expiry mints 6 and trims 4.
    executor
        .execute(&Command::AdvanceClock { by_seconds: 700 })
        .expect("a clock move is bookkeeping");

    assert_eq!(feed_ids(executor.state()), vec![5, 6]);
    assert_eq!(executor.state().next_feed_id(), 7);

    // Assert: the store accepted every patch and mirrors the working set.
    let working = state_documents(executor.state());
    drop(executor);
    let loaded = store.load().expect("both moves persisted");
    assert_eq!(snapshot_documents(&loaded), working);
}

#[test]
fn a_rest_that_advances_and_expires_mirrors_the_store() {
    // Arrange: the fighter with a spent Second Wind, no statuses, capacity 2.
    let party = fixtures::party();
    let mut borin = fixtures::golden_fighter(party.player);
    let second_wind = borin.body.resources[0].id;
    borin.body.statuses.clear();
    let mut campaign = party.campaign.clone();
    campaign.settings.feed_capacity = 2;
    let entities = [AnyEntity::from(borin.clone())];
    let mut store = MemoryStore::new();
    store
        .apply(&seeding_patch(&campaign, &entities))
        .expect("a fresh store accepts the fighter");
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
                kind: StatusKind::Condition,
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

    // Act: a rest whose advance lands exactly on the expiry instant, so one
    // patch carries the campaign document, the entity and a feed append.
    executor
        .execute(&Command::ShortRest {
            entity: borin.id,
            advance_seconds: Some(3_600),
            expected_rev: None,
        })
        .expect("a short rest across the expiry");

    // Assert: the campaign write is visible and the reload is byte-identical.
    let working = executor.state();
    assert_eq!(
        working.campaign().clock.now_s,
        CampaignTime::from_seconds(3_600)
    );
    assert_eq!(working.feed().len(), 1, "the expiry reached the feed");
    let mirrored = state_documents(working);
    drop(executor);
    let loaded = store.load().expect("the rest persisted");
    assert_eq!(snapshot_documents(&loaded), mirrored);
    assert_eq!(
        loaded.campaign.clock.now_s,
        CampaignTime::from_seconds(3_600),
        "the rest's campaign write reads back from the store"
    );
}
