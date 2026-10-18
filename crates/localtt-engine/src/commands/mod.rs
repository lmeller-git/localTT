//! The command surface (RFC-0001 5.2): the mutating operations a client
//! drives.
//!
//! The U3 slice (RFC `engine-executor.md` 4.3) is clock, statuses, hp,
//! resources and rests; later units add dice, casting, inventory, entities and
//! seats. The provisional free helpers of the pre-U3 surface are re-exported
//! unchanged (6.4), so the frozen scenario suite keeps its import paths while
//! the command effects move through the executor (5.1).

mod clock;
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
        Command::AdvanceClock { by_seconds } => {
            clock::advance_clock_command(state, *by_seconds, env)
        }
        Command::SetClock { to } => clock::set_clock_command(state, *to, env),
        Command::AddStatus {
            entity,
            status,
            expected_rev,
        } => statuses::add_status_command(state, *entity, status, *expected_rev, env),
        Command::RemoveStatus {
            entity,
            status,
            expected_rev,
        } => statuses::remove_status_command(state, *entity, *status, *expected_rev, env),
        Command::EditStatusDuration {
            entity,
            status,
            duration,
            expected_rev,
        } => statuses::edit_status_duration_command(
            state,
            *entity,
            *status,
            duration,
            *expected_rev,
            env,
        ),
        Command::ApplyDamage {
            entity,
            amount,
            expected_rev,
        } => hp::apply_damage_command(state, *entity, *amount, *expected_rev, env),
        Command::Heal {
            entity,
            amount,
            expected_rev,
        } => hp::heal_command(state, *entity, *amount, *expected_rev, env),
        Command::SetTempHp {
            entity,
            amount,
            expected_rev,
        } => hp::set_temp_hp_command(state, *entity, *amount, *expected_rev, env),
        Command::SetDeathSaves {
            entity,
            value,
            expected_rev,
        } => hp::set_death_saves_command(state, *entity, *value, *expected_rev, env),
        Command::SpendResource {
            entity,
            resource,
            amount,
            expected_rev,
        } => resources::spend_resource_command(
            state,
            *entity,
            *resource,
            *amount,
            *expected_rev,
            env,
        ),
        Command::RestoreResource {
            entity,
            resource,
            amount,
            expected_rev,
        } => resources::restore_resource_command(
            state,
            *entity,
            *resource,
            *amount,
            *expected_rev,
            env,
        ),
        Command::ShortRest {
            entity,
            advance_seconds,
            expected_rev,
        } => rests::short_rest_command(state, *entity, *advance_seconds, *expected_rev, env),
        Command::LongRest {
            entity,
            advance_seconds,
            expected_rev,
        } => rests::long_rest_command(state, *entity, *advance_seconds, *expected_rev, env),
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

    /// An empty transition (5.2): no documents, no events, no warnings.
    fn empty() -> Self {
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

/// Caps one transition's minted feed entries to the ring (5.6): the newest
/// `c = capacity.max(1)` survive, and the returned cutoff drops stored entries
/// below them.
fn cap_feed(capacity: u32, entries: Vec<FeedEntry>) -> (Vec<FeedEntry>, Option<FeedEntryId>) {
    let ring = capacity.max(1);
    let ring_u64 = u64::from(ring);
    let ring_usize = usize::try_from(ring).expect("a u32 feed capacity fits usize");
    let Some(last) = entries.last() else {
        return (entries, None);
    };
    let keep_from = if last.id.get() + 1 > ring_u64 {
        Some(FeedEntryId::new(last.id.get() + 1 - ring_u64))
    } else {
        None
    };
    if entries.len() <= ring_usize {
        return (entries, keep_from);
    }
    let dropped = entries.len() - ring_usize;
    (entries.into_iter().skip(dropped).collect(), keep_from)
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            Audience,
            Entity,
            FeedPayload,
            Formula,
            FormulaExpr,
            NoteBody,
            PlayerId,
            Recharge,
            Resource,
            ResourceId,
            ResourceKind,
            RestKind,
            Status,
        },
    };

    use super::*;
    use crate::{
        error::{ElementRef, WarningCode},
        ports::store::CampaignSnapshot,
        state::LoadedCampaign,
    };

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

    fn wizard_with_statuses(id: EntityId, statuses: Vec<Status>) -> AnyEntity {
        let mut document = wizard(id, 32, 32, 0);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.statuses = statuses;
        document
    }

    fn resource(
        id: ResourceId,
        name: &str,
        max: Formula,
        current: i32,
        recharge: Recharge,
    ) -> Resource {
        Resource {
            id,
            name: name.to_owned(),
            kind: ResourceKind::Feature,
            max,
            current,
            recharge,
        }
    }

    fn wizard_with_resources(id: EntityId, resources: Vec<Resource>) -> AnyEntity {
        let mut document = wizard(id, 32, 32, 0);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.resources = resources;
        document
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

    fn until_rest_status(name: &str, rest: RestKind) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Effect,
            name: name.to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration: StatusDuration::UntilRest { rest },
            source: None,
            modifiers: Vec::new(),
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

    fn concentration_status(expires_at: i64) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Concentration,
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

    fn state_with(entities: Vec<AnyEntity>) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: CampaignBuilder::new("Curse of the Vault").build(),
            entities,
            feed: Vec::new(),
        })
    }

    fn campaign_with(feed_capacity: u32, dawn_hour: u8, clock_s: i64) -> Campaign {
        let mut campaign = CampaignBuilder::new("Curse of the Vault").build();
        campaign.clock.now_s = CampaignTime::from_seconds(clock_s);
        campaign.settings.feed_capacity = feed_capacity;
        campaign.settings.dawn_hour = dawn_hour;
        campaign
    }

    fn state_with_campaign(campaign: Campaign, entities: Vec<AnyEntity>) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign,
            entities,
            feed: Vec::new(),
        })
    }

    fn with_owner(mut document: AnyEntity, owner: PlayerId) -> AnyEntity {
        match &mut document {
            AnyEntity::Statblock(statblock) => statblock.owner = owner,
            other => panic!("the fixture is a statblock, got {other:?}"),
        }
        document
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

    #[test]
    fn add_status_mints_started_at_and_appends() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

        let transition = apply(
            &state,
            &Command::AddStatus {
                entity: id,
                status: StatusSpec {
                    kind: StatusKind::Condition,
                    name: "Prone".to_owned(),
                    reference: None,
                    text: None,
                    duration: StatusDuration::Indefinite,
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("statuses are bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(
            transition.events,
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }]
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.statuses.len(), 1);
                let status = &document.body.statuses[0];
                assert_eq!(status.name, "Prone");
                assert_eq!(status.started_at, CampaignTime::from_seconds(0));
                assert_eq!(status.duration, StatusDuration::Indefinite);
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_second_concentration_appends_and_warns() {
        let id = EntityId::new();
        let state = state_with(vec![wizard_with_statuses(
            id,
            vec![concentration_status(3_600)],
        )]);

        let transition = apply(
            &state,
            &Command::AddStatus {
                entity: id,
                status: StatusSpec {
                    kind: StatusKind::Concentration,
                    name: "Hunter's Mark".to_owned(),
                    reference: None,
                    text: None,
                    duration: StatusDuration::Indefinite,
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("concentration overlaps are warned, never blocked");

        assert_eq!(transition.warnings.len(), 1);
        let warning = &transition.warnings[0];
        assert_eq!(warning.code, WarningCode::MultipleConcentration);
        assert_eq!(warning.entity, Some(id));
        assert!(warning.element.is_none());
        assert!(!warning.message.is_empty());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.statuses.len(), 2, "both concentrations stay");
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn an_expired_concentration_does_not_block_a_new_one() {
        let id = EntityId::new();
        let state = state_with(vec![wizard_with_statuses(
            id,
            vec![concentration_status(0)],
        )]);

        let transition = apply(
            &state,
            &Command::AddStatus {
                entity: id,
                status: StatusSpec {
                    kind: StatusKind::Concentration,
                    name: "Hunter's Mark".to_owned(),
                    reference: None,
                    text: None,
                    duration: StatusDuration::Indefinite,
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("statuses are bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
    }

    #[test]
    fn remove_status_takes_it_out() {
        let id = EntityId::new();
        let status = concentration_status(3_600);
        let state = state_with(vec![wizard_with_statuses(id, vec![status.clone()])]);

        let transition = apply(
            &state,
            &Command::RemoveStatus {
                entity: id,
                status: status.id,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("removing an existing status is bookkeeping");

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert!(document.body.statuses.is_empty()),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn removing_an_unknown_status_is_not_found() {
        let id = EntityId::new();
        let ghost = StatusId::new();
        let state = state_with(vec![wizard_with_statuses(id, Vec::new())]);

        let result = apply(
            &state,
            &Command::RemoveStatus {
                entity: id,
                status: ghost,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: Some(ElementRef::Status(status)),
            }) if entity == id && status == ghost
        ));
    }

    #[test]
    fn edit_status_duration_replaces_it() {
        let id = EntityId::new();
        let status = concentration_status(3_600);
        let state = state_with(vec![wizard_with_statuses(id, vec![status.clone()])]);

        let transition = apply(
            &state,
            &Command::EditStatusDuration {
                entity: id,
                status: status.id,
                duration: StatusDuration::Indefinite,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("editing an existing status is bookkeeping");

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                let status = &document.body.statuses[0];
                assert_eq!(status.duration, StatusDuration::Indefinite);
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn editing_an_unknown_status_is_not_found() {
        let id = EntityId::new();
        let ghost = StatusId::new();
        let state = state_with(vec![wizard_with_statuses(id, Vec::new())]);

        let result = apply(
            &state,
            &Command::EditStatusDuration {
                entity: id,
                status: ghost,
                duration: StatusDuration::Indefinite,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: Some(ElementRef::Status(status)),
            }) if entity == id && status == ghost
        ));
    }

    #[test]
    fn heal_clamps_at_max_and_leaves_temp_alone() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 30, 5)]);

        let transition = apply(
            &state,
            &Command::Heal {
                entity: id,
                amount: 5,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("healing more than the missing hit points is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.hp.current, 32);
                assert_eq!(document.body.hp.temp, 5, "healing is not temp HP");
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn healing_a_negative_amount_is_malformed() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 30, 0)]);

        let result = apply(
            &state,
            &Command::Heal {
                entity: id,
                amount: -1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn set_temp_hp_keeps_the_higher_value() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 5)]);

        let lower = apply(
            &state,
            &Command::SetTempHp {
                entity: id,
                amount: 3,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("temp HP non-stacking is bookkeeping");
        match &lower.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.hp.temp, 5),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }

        let higher = apply(
            &state,
            &Command::SetTempHp {
                entity: id,
                amount: 9,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("temp HP non-stacking is bookkeeping");
        match &higher.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.hp.temp, 9),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn setting_negative_temp_hp_is_malformed() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

        let result = apply(
            &state,
            &Command::SetTempHp {
                entity: id,
                amount: -1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn set_death_saves_replaces_the_tally() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 0, 0)]);
        let value = DeathSaves {
            successes: 2,
            failures: 1,
        };

        let transition = apply(
            &state,
            &Command::SetDeathSaves {
                entity: id,
                value,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("death saves are bookkeeping");

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.death_saves, value),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn death_saves_above_three_are_malformed() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 0, 0)]);

        let result = apply(
            &state,
            &Command::SetDeathSaves {
                entity: id,
                value: DeathSaves {
                    successes: 0,
                    failures: 4,
                },
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn spending_a_resource_takes_it_down() {
        let id = EntityId::new();
        let slot = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                slot,
                "Spell Slots (1st)",
                Formula::Const(3),
                3,
                Recharge::Long,
            )],
        )]);

        let transition = apply(
            &state,
            &Command::SpendResource {
                entity: id,
                resource: slot,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("spending a tracked resource is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, 2),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn overspending_warns_and_stays_on_the_sheet() {
        let id = EntityId::new();
        let slot = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                slot,
                "Pact Magic",
                Formula::Const(2),
                0,
                Recharge::Short,
            )],
        )]);

        let transition = apply(
            &state,
            &Command::SpendResource {
                entity: id,
                resource: slot,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("overspending is warned, never blocked");

        assert_eq!(transition.warnings.len(), 1);
        let warning = &transition.warnings[0];
        assert_eq!(warning.code, WarningCode::ResourceOverspent);
        assert_eq!(warning.entity, Some(id));
        assert_eq!(warning.element, Some(ElementRef::Resource(slot)));
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, -1),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn spending_a_negative_amount_is_malformed() {
        let id = EntityId::new();
        let slot = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                slot,
                "Pact Magic",
                Formula::Const(2),
                2,
                Recharge::Short,
            )],
        )]);

        let result = apply(
            &state,
            &Command::SpendResource {
                entity: id,
                resource: slot,
                amount: -1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn spending_an_unknown_resource_is_not_found() {
        let id = EntityId::new();
        let ghost = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                ResourceId::new(),
                "Pact Magic",
                Formula::Const(2),
                2,
                Recharge::Short,
            )],
        )]);

        let result = apply(
            &state,
            &Command::SpendResource {
                entity: id,
                resource: ghost,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: Some(ElementRef::Resource(resource)),
            }) if entity == id && resource == ghost
        ));
    }

    #[test]
    fn restoring_clamps_to_a_constant_maximum() {
        let id = EntityId::new();
        let slot = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                slot,
                "Pact Magic",
                Formula::Const(2),
                1,
                Recharge::Short,
            )],
        )]);

        let transition = apply(
            &state,
            &Command::RestoreResource {
                entity: id,
                resource: slot,
                amount: 10,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("clamping to the maximum is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, 2),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn restoring_with_a_formula_maximum_is_unclamped_and_warns() {
        let id = EntityId::new();
        let slot = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                slot,
                "Arcane Recovery",
                Formula::Expr(FormulaExpr::ProficiencyBonus),
                1,
                Recharge::Long,
            )],
        )]);

        let transition = apply(
            &state,
            &Command::RestoreResource {
                entity: id,
                resource: slot,
                amount: 5,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("unclamped restores are warned, never blocked");

        assert_eq!(transition.warnings.len(), 1);
        let warning = &transition.warnings[0];
        assert_eq!(warning.code, WarningCode::FormulaUnevaluated);
        assert_eq!(warning.entity, Some(id));
        assert_eq!(warning.element, Some(ElementRef::Resource(slot)));
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, 6),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn restoring_a_negative_amount_is_malformed() {
        let id = EntityId::new();
        let slot = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![resource(
                slot,
                "Pact Magic",
                Formula::Const(2),
                1,
                Recharge::Short,
            )],
        )]);

        let result = apply(
            &state,
            &Command::RestoreResource {
                entity: id,
                resource: slot,
                amount: -1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn restoring_an_unknown_resource_is_not_found() {
        let id = EntityId::new();
        let ghost = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(id, Vec::new())]);

        let result = apply(
            &state,
            &Command::RestoreResource {
                entity: id,
                resource: ghost,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: Some(ElementRef::Resource(resource)),
            }) if entity == id && resource == ghost
        ));
    }

    #[test]
    fn an_advance_expires_a_status_softly_and_appends_the_owner_entry() {
        let owner = PlayerId::new();
        let id = EntityId::new();
        let status = timed_status(3_600);
        let state = state_with(vec![with_owner(
            wizard_with_statuses(id, vec![status.clone()]),
            owner,
        )]);

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(
            transition.events,
            vec![
                Event::CampaignChanged,
                Event::FeedAppended {
                    ids: vec![FeedEntryId::new(1)],
                },
            ]
        );
        let patch = transition.patch();
        assert_eq!(
            patch.campaign.expect("the campaign moved").clock.now_s,
            CampaignTime::from_seconds(3_600)
        );
        assert!(patch.upserts.is_empty(), "soft expiry rewrites nothing");
        assert_eq!(transition.feed_appends.len(), 1);
        let entry = &transition.feed_appends[0];
        assert_eq!(entry.id, FeedEntryId::new(1));
        assert_eq!(entry.ts, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(entry.clock_s, CampaignTime::from_seconds(3_600));
        assert_eq!(entry.actor, Some(owner));
        assert_eq!(entry.entity, Some(id));
        assert_eq!(entry.audience, Audience::RollerAndDm);
        assert_eq!(
            entry.payload,
            FeedPayload::StatusExpired {
                status: status.id,
                name: "Bleeding".to_owned(),
            }
        );
        assert!(transition.feed_keep_from.is_none());
    }

    #[test]
    fn a_zero_advance_is_an_empty_transition() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 0 },
            &mut FixedEnv,
        )
        .expect("a zero move is legal");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(transition.events, Vec::new());
        let patch = transition.patch();
        assert!(
            patch.campaign.is_none(),
            "even the campaign document stays untouched"
        );
        assert!(patch.upserts.is_empty());
        assert!(patch.removals.is_empty());
        assert!(patch.feed_appends.is_empty());
        assert!(patch.feed_keep_from.is_none());
    }

    #[test]
    fn an_advance_overflow_is_malformed() {
        let state = state_with_campaign(campaign_with(200, 6, 1), Vec::new());

        let result = apply(
            &state,
            &Command::AdvanceClock {
                by_seconds: i64::MAX,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn an_expiry_at_the_window_start_is_not_new() {
        let id = EntityId::new();
        let state = state_with_campaign(
            campaign_with(200, 6, 3_600),
            vec![wizard_with_statuses(id, vec![timed_status(3_600)])],
        );

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");

        assert!(
            transition.feed_appends.is_empty(),
            "the window excludes `old`"
        );
        assert_eq!(transition.events, vec![Event::CampaignChanged]);
    }

    #[test]
    fn a_rewind_warns_dawn_and_never_reverses_the_refill() {
        let dawn = ResourceId::new();
        let id = EntityId::new();
        let state = state_with_campaign(
            campaign_with(200, 6, 30_000),
            vec![wizard_with_resources(
                id,
                vec![resource(
                    dawn,
                    "Dawn Crystal",
                    Formula::Const(2),
                    2,
                    Recharge::Dawn,
                )],
            )],
        );

        let rewind = apply(
            &state,
            &Command::SetClock {
                to: CampaignTime::from_seconds(0),
            },
            &mut FixedEnv,
        )
        .expect("a rewind is legal");

        assert_eq!(rewind.warnings.len(), 1);
        let warning = &rewind.warnings[0];
        assert_eq!(warning.code, WarningCode::DawnRewind);
        assert_eq!(warning.entity, None, "the warning is campaign-scoped");
        assert_eq!(rewind.events, vec![Event::CampaignChanged]);
        let patch = rewind.patch();
        assert!(patch.upserts.is_empty(), "recharges are not reversed");
        assert_eq!(
            patch.campaign.expect("the campaign moved").clock.now_s,
            CampaignTime::from_seconds(0)
        );
    }

    #[test]
    fn a_non_constant_dawn_maximum_warns_without_rewriting() {
        let dawn = ResourceId::new();
        let id = EntityId::new();
        let state = state_with_campaign(
            campaign_with(200, 6, 0),
            vec![wizard_with_resources(
                id,
                vec![resource(
                    dawn,
                    "Dawn Crystal",
                    Formula::Expr(FormulaExpr::ProficiencyBonus),
                    0,
                    Recharge::Dawn,
                )],
            )],
        );

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 30_000 },
            &mut FixedEnv,
        )
        .expect("crossing dawn is bookkeeping");

        assert_eq!(transition.warnings.len(), 1);
        let warning = &transition.warnings[0];
        assert_eq!(warning.code, WarningCode::FormulaUnevaluated);
        assert_eq!(warning.entity, Some(id));
        assert_eq!(warning.element, Some(ElementRef::Resource(dawn)));
        assert!(
            transition.upserts.is_empty(),
            "a skipped refill writes nothing"
        );
        assert_eq!(transition.events, vec![Event::CampaignChanged]);
    }

    #[test]
    fn an_already_full_dawn_resource_causes_no_write() {
        let dawn = ResourceId::new();
        let id = EntityId::new();
        let state = state_with_campaign(
            campaign_with(200, 6, 0),
            vec![wizard_with_resources(
                id,
                vec![resource(
                    dawn,
                    "Dawn Crystal",
                    Formula::Const(2),
                    2,
                    Recharge::Dawn,
                )],
            )],
        );

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 30_000 },
            &mut FixedEnv,
        )
        .expect("crossing dawn is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert!(
            transition.upserts.is_empty(),
            "a full resource is not rewritten"
        );
        assert_eq!(transition.events, vec![Event::CampaignChanged]);
    }

    #[test]
    fn the_feed_cap_keeps_the_newest_entries() {
        let (kept, keep_from) = cap_feed(2, vec![feed(1), feed(2), feed(3)]);
        assert_eq!(
            kept.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![FeedEntryId::new(2), FeedEntryId::new(3)]
        );
        assert_eq!(keep_from, Some(FeedEntryId::new(2)));

        let (kept, keep_from) = cap_feed(0, vec![feed(1)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            keep_from,
            Some(FeedEntryId::new(1)),
            "capacity zero keeps exactly the latest entry"
        );

        let (kept, keep_from) = cap_feed(5, Vec::new());
        assert!(kept.is_empty());
        assert_eq!(keep_from, None);
    }

    #[test]
    fn a_transition_at_capacity_zero_keeps_the_latest_entry() {
        let first = EntityId::new();
        let second = EntityId::new();
        let state = state_with_campaign(
            campaign_with(0, 6, 0),
            vec![
                wizard_with_statuses(first, vec![timed_status(3_600)]),
                wizard_with_statuses(second, vec![timed_status(3_600)]),
            ],
        );

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");

        assert_eq!(transition.feed_appends.len(), 1);
        assert_eq!(
            transition.feed_appends[0].id,
            FeedEntryId::new(2),
            "the newest minted entry survives the ring"
        );
        assert_eq!(transition.feed_keep_from, Some(FeedEntryId::new(2)));
        assert_eq!(
            transition.events,
            vec![
                Event::CampaignChanged,
                Event::FeedAppended {
                    ids: vec![FeedEntryId::new(2)],
                },
            ]
        );
    }

    #[test]
    fn a_short_rest_clears_only_short_until_rest_statuses() {
        let id = EntityId::new();
        let state = state_with(vec![wizard_with_statuses(
            id,
            vec![
                until_rest_status("Winded", RestKind::Short),
                until_rest_status("Exhausted", RestKind::Long),
                timed_status(3_600),
            ],
        )]);

        let transition = apply(
            &state,
            &Command::ShortRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a rest is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(
            transition.events,
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }]
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                let names: Vec<&str> = document
                    .body
                    .statuses
                    .iter()
                    .map(|status| status.name.as_str())
                    .collect();
                assert_eq!(names, vec!["Exhausted", "Bleeding"]);
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
        assert!(
            transition.patch().campaign.is_none(),
            "no advance, no campaign write"
        );
    }

    #[test]
    fn a_long_rest_refills_long_resources_and_clears_both_rest_statuses() {
        let id = EntityId::new();
        let long_slot = ResourceId::new();
        let short_slot = ResourceId::new();
        let mut document = wizard_with_resources(
            id,
            vec![
                resource(
                    short_slot,
                    "Second Wind",
                    Formula::Const(2),
                    0,
                    Recharge::Short,
                ),
                resource(
                    long_slot,
                    "Spell Slots (1st)",
                    Formula::Const(3),
                    0,
                    Recharge::Long,
                ),
            ],
        );
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.statuses = vec![
            until_rest_status("Winded", RestKind::Short),
            until_rest_status("Exhausted", RestKind::Long),
            timed_status(3_600),
        ];
        let state = state_with(vec![document]);

        let transition = apply(
            &state,
            &Command::LongRest {
                entity: id,
                advance_seconds: Some(0),
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a long rest is bookkeeping");

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.resources[0].current, 2);
                assert_eq!(document.body.resources[1].current, 3);
                let names: Vec<&str> = document
                    .body
                    .statuses
                    .iter()
                    .map(|status| status.name.as_str())
                    .collect();
                assert_eq!(
                    names,
                    vec!["Bleeding"],
                    "a timed status is not a rest status"
                );
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
        assert!(
            transition.patch().campaign.is_none(),
            "advance zero is no move"
        );
    }

    #[test]
    fn a_rest_on_a_non_statblock_is_malformed() {
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
            &Command::LongRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn a_negative_rest_advance_is_malformed() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

        let result = apply(
            &state,
            &Command::ShortRest {
                entity: id,
                advance_seconds: Some(-60),
                expected_rev: None,
            },
            &mut FixedEnv,
        );

        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn a_rest_target_that_is_also_dawn_refilled_is_one_revision() {
        let id = EntityId::new();
        let dawn = ResourceId::new();
        let state = state_with_campaign(
            campaign_with(200, 6, 0),
            vec![wizard_with_resources(
                id,
                vec![resource(
                    dawn,
                    "Dawn Crystal",
                    Formula::Const(2),
                    0,
                    Recharge::Dawn,
                )],
            )],
        );

        let transition = apply(
            &state,
            &Command::ShortRest {
                entity: id,
                advance_seconds: Some(30_000),
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a rest across dawn");

        assert_eq!(
            transition.events.len(),
            2,
            "a campaign change plus one entity change"
        );
        let Some(Event::EntityChanged {
            id: changed,
            rev,
            kind,
        }) = transition.events.get(1)
        else {
            panic!(
                "the second event is the entity change, got {:?}",
                transition.events
            );
        };
        assert_eq!(*changed, id);
        assert_eq!(*kind, EntityKind::Pc);
        assert_eq!(*rev, 1, "dawn refill and rest stamp one revision");
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, 2),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }
}
