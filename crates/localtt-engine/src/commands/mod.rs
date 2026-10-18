//! The command surface (RFC-0001 5.2): the mutating operations a client
//! drives.
//!
//! The U3 slice (RFC `engine-executor.md` 4.3) is clock, statuses, hp,
//! resources and rests; later units add dice, casting, inventory, entities and
//! seats. The provisional free helpers of the pre-U3 surface are re-exported
//! unchanged (6.4), so the frozen scenario suite keeps its import paths while
//! the command effects move through the executor (5.1).

mod entities;
mod hp;
mod inventory;
mod resources;
mod rests;
mod statuses;

use localtt_core::types::{
    AnyEntity,
    Campaign,
    CampaignTime,
    DeathSaves,
    EntityId,
    EntityKind,
    FeedEntry,
    FeedEntryId,
    Modifier,
    RefId,
    ResourceId,
    StatblockEntity,
    StatusDuration,
    StatusId,
    StatusKind,
    StatusSource,
    UnixTime,
};

pub use self::{
    entities::{level_up, summon_from_template},
    hp::{apply_damage, heal, set_temp_hp},
    inventory::give_handout,
    resources::spend_resource,
    statuses::begin_concentration,
};
use crate::{
    error::{EngineError, Warning},
    ports::store::StatePatch,
    state::WorkingSet,
};

/// The U3 command slice: clock, statuses, hp, resources, rests. Later units
/// add variants (dice, casting, inventory, entities, seats).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Moves the clock by `by_seconds`; a DM command.
    AdvanceClock {
        /// The signed move in campaign seconds.
        by_seconds: i64,
    },
    /// Sets the clock to an absolute instant; a DM command.
    SetClock {
        /// The new campaign instant; pre-origin times are legal.
        to: CampaignTime,
    },
    /// Adds a status; the command layer mints its id and stamps
    /// `started_at` from the campaign clock.
    AddStatus {
        /// The target entity.
        entity: EntityId,
        /// The client-supplied part of the new status.
        status: StatusSpec,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Removes a status from an entity.
    RemoveStatus {
        /// The target entity.
        entity: EntityId,
        /// The status to remove.
        status: StatusId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Replaces a status's duration.
    EditStatusDuration {
        /// The target entity.
        entity: EntityId,
        /// The status to edit.
        status: StatusId,
        /// The new duration.
        duration: StatusDuration,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Applies damage: temp HP absorbs first, then current.
    ApplyDamage {
        /// The target entity.
        entity: EntityId,
        /// The damage amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Heals: current HP rises, never above max; temp HP is untouched.
    Heal {
        /// The target entity.
        entity: EntityId,
        /// The healing amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Sets temp HP, non-stacking (keeps the higher value).
    SetTempHp {
        /// The target entity.
        entity: EntityId,
        /// The temp HP amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Replaces an entity's death-save tally.
    SetDeathSaves {
        /// The target entity.
        entity: EntityId,
        /// The new tally; fields above 3 are `Malformed`.
        value: DeathSaves,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Spends a tracked resource; overspending is allowed and warned.
    SpendResource {
        /// The target entity.
        entity: EntityId,
        /// The resource to spend.
        resource: ResourceId,
        /// The amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Restores a tracked resource; clamped when the maximum is a constant.
    RestoreResource {
        /// The target entity.
        entity: EntityId,
        /// The resource to restore.
        resource: ResourceId,
        /// The amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// A short rest: refills `Recharge::Short` resources and clears
    /// `UntilRest { rest: Short }` statuses.
    ShortRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are `Malformed`.
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// A long rest: short plus `Recharge::Long` refills and
    /// `UntilRest { rest: Long }` clears.
    LongRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are `Malformed`.
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
}

/// The client-supplied part of a new status; the command layer mints the id
/// and stamps `started_at` from the campaign clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSpec {
    /// The status kind; adding a `Concentration` status while one is already
    /// active warns `MultipleConcentration` (5.4) and appends the new status.
    pub kind: StatusKind,
    /// Display name.
    pub name: String,
    /// Optional reference into the content catalogs.
    pub reference: Option<RefId>,
    /// Optional free text (markdown, opaque).
    pub text: Option<String>,
    /// How the status ends.
    pub duration: StatusDuration,
    /// Optional provenance of the status.
    pub source: Option<StatusSource>,
    /// Stat modifiers the status applies.
    pub modifiers: Vec<Modifier>,
}

/// Everything a command may consult that is not the working set.
///
/// U3-minimal: `reference()` and `roll()` land with U4/U5.
pub trait CommandEnv: Send {
    /// Wall-clock time for `updated_at` and feed `ts` stamps.
    fn now(&self) -> UnixTime;
}

/// Pure with respect to `state`: computes what should happen and what it
/// costs. State is not modified; the executor persists and then applies.
#[inline]
pub fn apply(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    match command {
        Command::ApplyDamage {
            entity,
            amount,
            expected_rev,
        } => hp::apply_damage_command(state, *entity, *amount, *expected_rev, env),
        _ => todo!(),
    }
}

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

/// The common prelude (5.4): the target document must exist, and an explicit
/// `expected_rev` must match the authoritative revision.
fn load_target(
    state: &dyn WorkingSet,
    entity: EntityId,
    expected_rev: Option<u64>,
) -> Result<AnyEntity, EngineError> {
    let document = state.entity(entity)?.ok_or(EngineError::NotFound {
        entity,
        element: None,
    })?;
    if let Some(expected) = expected_rev {
        let actual = document.rev();
        if actual != expected {
            return Err(EngineError::Conflict {
                entity,
                expected,
                actual,
            });
        }
    }
    Ok(document)
}

/// Loads a target of a command that addresses statblock-only fields (hit
/// points, statuses, resources). Other entity kinds cannot carry those
/// fields, so the command is `Malformed` (5.4).
fn load_statblock(
    state: &dyn WorkingSet,
    entity: EntityId,
    expected_rev: Option<u64>,
) -> Result<StatblockEntity, EngineError> {
    match load_target(state, entity, expected_rev)? {
        AnyEntity::Statblock(document) => Ok(document),
        AnyEntity::Note(_) | AnyEntity::Handout(_) => Err(EngineError::Malformed {
            reason: format!("entity {entity} is not a statblock"),
        }),
    }
}

/// Stamps a touched copy: `rev = old.rev + 1`, `updated_at = now` (5.5).
fn stamp(entity: &mut AnyEntity, now: UnixTime) {
    let rev = entity.rev() + 1;
    match entity {
        AnyEntity::Statblock(document) => {
            document.rev = rev;
            document.updated_at = now;
        }
        AnyEntity::Note(document) => {
            document.rev = rev;
            document.updated_at = now;
        }
        AnyEntity::Handout(document) => {
            document.rev = rev;
            document.updated_at = now;
        }
    }
}

/// Wraps one stamped document into the transition a single-upsert command
/// produces: one `EntityChanged`, no campaign or feed change.
fn one_upsert(entity: AnyEntity, warnings: Vec<Warning>) -> Transition {
    let events = vec![Event::EntityChanged {
        id: entity.id(),
        rev: entity.rev(),
        kind: entity.kind(),
    }];
    Transition {
        campaign: None,
        upserts: vec![entity],
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
        warnings,
        events,
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{Abilities, Entity, NoteBody, PlayerId},
    };

    use super::*;
    use crate::{error::WarningCode, ports::store::CampaignSnapshot, state::LoadedCampaign};

    const FIXED_TIME: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }
    }

    fn wizard(id: EntityId, max: i32, current: i32, temp: i32) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(max, current);
        let mut body = builder.build().expect("abilities and hp are set");
        body.hp.temp = temp;
        AnyEntity::from(
            Entity::builder(EntityKind::Pc, id, "Aria", PlayerId::new(), body)
                .build()
                .expect("a character kind pairs with a statblock body"),
        )
    }

    fn state_with(entities: Vec<AnyEntity>) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: CampaignBuilder::new("Curse of the Vault").build(),
            entities,
            feed: Vec::new(),
        })
    }

    #[test]
    fn damage_consumes_temp_hp_first_and_stamps_the_copy() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 5)]);

        let transition = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 12,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("damage is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(
            transition.events,
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }]
        );
        assert_eq!(transition.upserts.len(), 1);
        let updated = &transition.upserts[0];
        assert_eq!(updated.id(), id);
        assert_eq!(updated.rev(), 1);
        assert_eq!(updated.updated_at(), UnixTime::from_seconds(FIXED_TIME));
        match updated {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.hp.temp, 0);
                assert_eq!(document.body.hp.current, 25);
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
        assert_eq!(
            state
                .entity(id)
                .expect("the load-all view never fails")
                .expect("the entity exists")
                .rev(),
            0,
            "apply itself leaves the working set untouched"
        );
        let patch = transition.patch();
        assert!(patch.campaign.is_none());
        assert_eq!(patch.upserts.len(), 1);
        assert!(patch.removals.is_empty());
        assert!(patch.feed_appends.is_empty());
        assert!(patch.feed_keep_from.is_none());
    }

    #[test]
    fn damage_below_zero_warns_and_still_commits() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 5, 0)]);

        let transition = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 9,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("going below zero is warned, never blocked");

        assert_eq!(transition.warnings.len(), 1);
        let warning = &transition.warnings[0];
        assert_eq!(warning.code, WarningCode::NegativeHp);
        assert_eq!(warning.entity, Some(id));
        assert!(warning.element.is_none());
        assert!(!warning.message.is_empty());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.hp.current, -4),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_negative_amount_is_malformed() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

        let result = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: -1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn an_unknown_target_is_not_found() {
        let ghost = EntityId::new();
        let state = state_with(vec![wizard(EntityId::new(), 32, 32, 0)]);

        let result = apply(
            &state,
            &Command::ApplyDamage {
                entity: ghost,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: None,
            }) if entity == ghost
        ));
    }

    #[test]
    fn a_non_statblock_target_is_malformed() {
        let id = EntityId::new();
        let note = AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                id,
                "Session 4",
                PlayerId::new(),
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .build()
            .expect("a note kind pairs with a note body"),
        );
        let state = state_with(vec![note]);

        let result = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn a_stale_revision_conflicts_before_any_effect() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

        let result = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 1,
                expected_rev: Some(4),
            },
            &mut FixedEnv,
        );

        match result {
            Err(EngineError::Conflict {
                entity,
                expected,
                actual,
            }) => {
                assert_eq!(entity, id);
                assert_eq!(expected, 4);
                assert_eq!(actual, 0);
            }
            other => panic!("a stale revision must conflict, got {other:?}"),
        }
    }
}
