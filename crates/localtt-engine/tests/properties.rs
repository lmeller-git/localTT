//! Property checks for the load-all working set (RFC `engine-executor.md` 6.2):
//! after any command sequence, the expiry index is exactly the documents'
//! `Timed` statuses, the feed stays within the ring and the feed counter
//! follows the tail without ever going backwards.
//!
//! The sequence is driven through `commands::apply` and `WorkingSetMut::apply`
//! directly - no store - so the invariant failures point at the state layer.

use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{
        Abilities,
        AnyEntity,
        Campaign,
        CampaignTime,
        DeathSaves,
        Entity,
        EntityId,
        EntityKind,
        Formula,
        NoteBody,
        PlayerId,
        Recharge,
        Resource,
        ResourceId,
        ResourceKind,
        RestKind,
        Status,
        StatusDuration,
        StatusId,
        StatusKind,
        UnixTime,
    },
};
use localtt_engine::{
    commands::{Command, CommandEnv, StatusSpec, apply},
    ports::store::CampaignSnapshot,
    state::{LoadedCampaign, WorkingSet, WorkingSetMut},
};
use proptest::prelude::*;

const FIXED_TIME: i64 = 1_750_000_000;

struct FixedEnv;

impl CommandEnv for FixedEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// One generated command, with slots instead of ids: the ids are the cast's,
/// the slots pick among them (and are reduced modulo the live collection).
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

/// The fixed cast: two statblocks and a note, with the statblocks' resource
/// ids so slots resolve.
struct Cast {
    campaign: Campaign,
    entities: Vec<AnyEntity>,
}

impl Cast {
    fn new() -> Self {
        let mut campaign = CampaignBuilder::new("Curse of the Vault").build();
        campaign.settings.feed_capacity = 2;
        campaign.settings.dawn_hour = 6;
        let wizard = statblock(
            &[
                resource("Second Wind", 2, 2, Recharge::Short),
                resource("Dawn Crystal", 1, 1, Recharge::Dawn),
                resource("Spell Slots", 3, 3, Recharge::Long),
            ],
            &[timed_status(3_600), until_rest_status(RestKind::Short)],
        );
        let fighter = statblock(
            &[
                resource("Stamina", 2, 1, Recharge::Short),
                resource("Dawn Gem", 2, 0, Recharge::Dawn),
                resource("Spell Slots", 2, 2, Recharge::Long),
            ],
            &[until_rest_status(RestKind::Long), indefinite_status()],
        );
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
        Self {
            campaign,
            entities: vec![wizard, fighter, note],
        }
    }

    fn entity(&self, slot: u8) -> EntityId {
        self.entities[usize::from(slot) % self.entities.len()].id()
    }

    /// The `slot`-th resource of the addressed statblock, when it has one.
    fn resource(&self, state: &dyn WorkingSet, slot: u8, pick: u8) -> Option<ResourceId> {
        let document = match state.entity(self.entity(slot)).ok()?? {
            AnyEntity::Statblock(document) => document,
            AnyEntity::Note(_) | AnyEntity::Handout(_) => return None,
        };
        if document.body.resources.is_empty() {
            return None;
        }
        Some(document.body.resources[usize::from(pick) % document.body.resources.len()].id)
    }

    /// The `slot`-th status of the addressed statblock, when it has one.
    fn status(&self, state: &dyn WorkingSet, slot: u8, pick: u8) -> Option<StatusId> {
        let document = match state.entity(self.entity(slot)).ok()?? {
            AnyEntity::Statblock(document) => document,
            AnyEntity::Note(_) | AnyEntity::Handout(_) => return None,
        };
        if document.body.statuses.is_empty() {
            return None;
        }
        Some(document.body.statuses[usize::from(pick) % document.body.statuses.len()].id)
    }
}

impl Op {
    fn command(&self, state: &dyn WorkingSet, cast: &Cast) -> Option<Command> {
        let entity = |slot: u8| cast.entity(slot);
        match *self {
            Op::Damage {
                entity: slot,
                amount,
            } => Some(Command::ApplyDamage {
                entity: entity(slot),
                amount: i32::from(amount),
                expected_rev: None,
            }),
            Op::Heal {
                entity: slot,
                amount,
            } => Some(Command::Heal {
                entity: entity(slot),
                amount: i32::from(amount),
                expected_rev: None,
            }),
            Op::Temp {
                entity: slot,
                amount,
            } => Some(Command::SetTempHp {
                entity: entity(slot),
                amount: i32::from(amount),
                expected_rev: None,
            }),
            Op::DeathSaves {
                entity: slot,
                successes,
                failures,
            } => Some(Command::SetDeathSaves {
                entity: entity(slot),
                value: DeathSaves {
                    successes,
                    failures,
                },
                expected_rev: None,
            }),
            Op::AddStatus {
                entity: slot,
                kind,
                duration,
                expires_in,
            } => Some(Command::AddStatus {
                entity: entity(slot),
                status: StatusSpec {
                    kind: status_kind(kind),
                    name: "Injected".to_owned(),
                    reference: None,
                    text: None,
                    duration: status_duration(state, duration, expires_in),
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            }),
            Op::RemoveStatus {
                entity: slot,
                slot: pick,
            } => Some(Command::RemoveStatus {
                entity: entity(slot),
                status: cast.status(state, slot, pick)?,
                expected_rev: None,
            }),
            Op::EditStatus {
                entity: slot,
                slot: pick,
                duration,
                expires_in,
            } => Some(Command::EditStatusDuration {
                entity: entity(slot),
                status: cast.status(state, slot, pick)?,
                duration: status_duration(state, duration, expires_in),
                expected_rev: None,
            }),
            Op::Spend {
                entity: slot,
                resource: pick,
                amount,
            } => Some(Command::SpendResource {
                entity: entity(slot),
                resource: cast.resource(state, slot, pick)?,
                amount: i32::from(amount),
                expected_rev: None,
            }),
            Op::Restore {
                entity: slot,
                resource: pick,
                amount,
            } => Some(Command::RestoreResource {
                entity: entity(slot),
                resource: cast.resource(state, slot, pick)?,
                amount: i32::from(amount),
                expected_rev: None,
            }),
            Op::Advance { by } => Some(Command::AdvanceClock {
                by_seconds: i64::from(by),
            }),
            Op::SetClock { to } => Some(Command::SetClock {
                to: CampaignTime::from_seconds(i64::from(to)),
            }),
            Op::Rest {
                entity: slot,
                long,
                advance,
            } => {
                let entity = entity(slot);
                let advance_seconds = advance.map(i64::from);
                if long {
                    Some(Command::LongRest {
                        entity,
                        advance_seconds,
                        expected_rev: None,
                    })
                } else {
                    Some(Command::ShortRest {
                        entity,
                        advance_seconds,
                        expected_rev: None,
                    })
                }
            }
        }
    }
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

fn status_kind(kind: u8) -> StatusKind {
    match kind % 3 {
        0 => StatusKind::Condition,
        1 => StatusKind::Effect,
        _ => StatusKind::Concentration,
    }
}

fn status_duration(state: &dyn WorkingSet, duration: u8, expires_in: u16) -> StatusDuration {
    let now = state.campaign().clock.now_s.seconds();
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

fn statblock(resources: &[Resource], statuses: &[Status]) -> AnyEntity {
    let mut builder = StatblockBuilder::character();
    builder.abilities(Abilities::all(10)).hp(32, 32);
    let mut body = builder.build().expect("abilities and hp are set");
    body.resources = resources.to_vec();
    body.statuses = statuses.to_vec();
    AnyEntity::from(
        Entity::builder(
            EntityKind::Pc,
            EntityId::new(),
            "Aria",
            PlayerId::new(),
            body,
        )
        .created_at(UnixTime::from_seconds(FIXED_TIME))
        .updated_at(UnixTime::from_seconds(FIXED_TIME))
        .build()
        .expect("a character kind pairs with a statblock body"),
    )
}

fn resource(name: &str, max: i32, current: i32, recharge: Recharge) -> Resource {
    Resource {
        id: ResourceId::new(),
        name: name.to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(max),
        current,
        recharge,
    }
}

fn timed_status(expires_at: i64) -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Condition,
        name: "Bleeding".to_owned(),
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

fn until_rest_status(rest: RestKind) -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Effect,
        name: "Winded".to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::UntilRest { rest },
        source: None,
        modifiers: Vec::new(),
    }
}

fn indefinite_status() -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Condition,
        name: "Prone".to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: Vec::new(),
    }
}

/// Every timed status the documents carry, optionally within the soft-expiry
/// window: `expires_at <= until`.
fn timed_candidates(
    state: &LoadedCampaign,
    until: Option<CampaignTime>,
) -> Vec<(EntityId, StatusId)> {
    let mut candidates = Vec::new();
    for header in state.headers() {
        let Some(AnyEntity::Statblock(document)) = state
            .entity(header.id)
            .expect("the load-all view never fails")
        else {
            continue;
        };
        for status in &document.body.statuses {
            let StatusDuration::Timed { expires_at } = status.duration else {
                continue;
            };
            if until.is_none_or(|until| expires_at <= until) {
                candidates.push((header.id, status.id));
            }
        }
    }
    candidates.sort();
    candidates
}

/// 6.2: the index equals a recomputation from the documents, for the whole
/// index and for a window query at the current clock instant.
fn assert_index_matches_documents(state: &LoadedCampaign) {
    let mut indexed = state.expiry_candidates_until(CampaignTime::from_seconds(i64::MAX));
    indexed.sort();
    assert_eq!(
        indexed,
        timed_candidates(state, None),
        "the index is exactly the documents' timed statuses"
    );

    let now = state.campaign().clock.now_s;
    let mut window = state.expiry_candidates_until(now);
    window.sort();
    assert_eq!(
        window,
        timed_candidates(state, Some(now)),
        "a window query is `expires_at <= until`"
    );
}

/// 5.6: ids only move forward, the ring holds, and the counter follows the tail.
fn assert_ring_invariants(state: &LoadedCampaign) {
    let feed = state.feed();
    assert!(
        feed.windows(2).all(|pair| pair[0].id < pair[1].id),
        "feed ids are strictly ascending"
    );
    let ring = usize::try_from(state.campaign().settings.feed_capacity.max(1))
        .expect("a u32 feed capacity fits usize");
    assert!(feed.len() <= ring, "the feed stays within the ring");
    let expected = feed
        .last()
        .map_or(1, |entry| entry.id.get().saturating_add(1));
    assert_eq!(
        state.next_feed_id(),
        expected,
        "the counter follows the surviving tail"
    );
}

/// The proptest configuration.
///
/// Under Miri the file-backed failure persistence cannot run (`getcwd` is not
/// available with isolation) and the case count is cut to a handful: Miri
/// interprets every instruction, so the full count takes minutes and adds no
/// UB signal the handful does not. Everywhere else failing seeds persist under
/// `proptest-regressions/`.
fn property_config() -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(if cfg!(miri) { 8 } else { 64 });
    if cfg!(miri) {
        config.failure_persistence = None;
    }
    config
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn the_working_set_survives_arbitrary_command_sequences(
        ops in prop::collection::vec(op_strategy(), 0..16)
    ) {
        let cast = Cast::new();
        let mut state = LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: cast.campaign.clone(),
            entities: cast.entities.clone(),
            feed: Vec::new(),
        });

        for op in ops {
            let Some(command) = op.command(&state, &cast) else {
                continue;
            };
            let Ok(transition) = apply(&state, &command, &mut FixedEnv) else {
                continue;
            };
            let patch = transition.patch();
            state.apply(&patch);

            assert_index_matches_documents(&state);
            assert_ring_invariants(&state);
        }

        assert_index_matches_documents(&state);
        assert_ring_invariants(&state);
    }
}
