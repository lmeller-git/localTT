//! The command surface (RFC-0001 5.2): the mutating operations a client
//! drives.
//!
//! The U3 slice (RFC `engine-executor.md` 4.3) is clock, statuses, hp,
//! resources and rests; U5 adds dice, casting and hit dice (RFC `dice.md`
//! 4.3); later units add inventory, entities and seats. The provisional free
//! helpers of the pre-U3 surface are re-exported unchanged (6.4), so the
//! frozen scenario suite keeps its import paths while the command effects move
//! through the executor (5.1).

mod casting;
mod clock;
mod creation;
mod dice_cmd;
mod entities;
mod hp;
mod inventory;
mod resources;
mod rests;
mod statuses;

use std::collections::BTreeMap;

use localtt_core::types::{
    AnyEntity,
    Audience,
    Campaign,
    CampaignTime,
    DeathSaves,
    DiceExpr,
    DiceResult,
    EntityId,
    EntityKind,
    FeedEntry,
    FeedEntryId,
    InventoryItem,
    Modifier,
    PlayerId,
    RefId,
    ResourceId,
    SpellEntryId,
    StatKey,
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
    content::{
        ContentEntry,
        ContentFault,
        ContentSummary,
        Provenance,
        ReferenceSource,
        SearchQuery,
    },
    dice::RollPlan,
    error::{EngineError, Warning},
    ports::store::StatePatch,
    state::WorkingSet,
};

/// The command surface: U3's clock, statuses, hp, resources and rests, plus
/// U5's dice, casting and hit-die commands. Later units add variants
/// (inventory, entities, seats).
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
    /// Creates one entity from a client-supplied document.
    ///
    /// The command layer rejects a nonzero revision, stamps revision 1 and
    /// both timestamps from the environment, and sets the schema version to
    /// the current build. A player may create only their own entities and may
    /// set no ACL grants; the DM may create for any roster member and set
    /// grants. The entity id must be fresh.
    CreateEntity {
        /// The draft document; its header fields are validated and stamped.
        entity: Box<AnyEntity>,
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
    /// A long rest: short plus `Recharge::Long` refills, `UntilRest { rest:
    /// Long }` clears, and the `rules.md` 5.10 recovery (hit points, death
    /// saves, one exhaustion level).
    LongRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are `Malformed`.
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Rolls dice; the actor comes from [`Executor::execute_as`].
    ///
    /// [`Executor::execute_as`]: crate::executor::Executor::execute_as
    Roll {
        /// The entity the roll is for; may be absent for a free expression.
        entity: Option<EntityId>,
        /// What to roll.
        spec: RollSpec,
        /// Audience override; `None` means the campaign default.
        audience: Option<Audience>,
    },
    /// Casts a known spell from a tracked slot.
    CastSpell {
        /// The caster.
        entity: EntityId,
        /// The known spell entry inside one of the entity's blocks.
        spell: SpellEntryId,
        /// The slot resource to spend.
        slot: ResourceId,
        /// The declared upcast in slot levels above the spell's level.
        upcast_slots: u32,
        /// The client's concentration request; the resolved spell may
        /// concentrate on its own.
        concentrate: bool,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Starts (or replaces) concentration from a client-supplied status.
    BeginConcentration {
        /// The concentrating entity.
        entity: EntityId,
        /// The new status; `kind` must be `Concentration`.
        status: StatusSpec,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Ends every concentration status on the entity.
    EndConcentration {
        /// The entity.
        entity: EntityId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Spends one hit die and rolls its healing; the actor comes from
    /// [`Executor::execute_as`].
    ///
    /// [`Executor::execute_as`]: crate::executor::Executor::execute_as
    SpendHitDie {
        /// The creature spending the die.
        entity: EntityId,
        /// The `ResourceKind::HitDie` resource to spend.
        resource: ResourceId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
}

/// What a `Roll` command rolls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RollSpec {
    /// Rolls a client-supplied expression.
    Expression {
        /// The expression; validated before rolling.
        expr: DiceExpr,
        /// The stat the roll is for; drives `RollContext.kind` and the
        /// context's `stat`, and has no mechanical effect on the dice.
        target: Option<StatKey>,
    },
    /// Rolls a check for one stat: the engine derives the flat part, the
    /// attributed dice and the hint, then rolls `1d20` composed with them.
    Check {
        /// The stat, e.g. `"save.wis"`.
        stat: StatKey,
        /// The DC, when the client compares one.
        target_dc: Option<i32>,
    },
}

/// The client-supplied part of a new status; the command layer mints the id
/// and stamps `started_at` from the campaign clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSpec {
    /// The status kind; adding a `Concentration` status while one is already
    /// active warns `MultipleConcentration` (5.4) and appends the new status.
    /// Contrast [`begin_concentration`], which replaces the active
    /// concentration instead of appending.
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
/// `reference` and `roll` are defaulted (RFC `dice.md` 8, D4): a test
/// environment over the defaults fails closed on dice and sees an empty
/// content source, so the frozen U3 envs compile unchanged.
pub trait CommandEnv: Send {
    /// Wall-clock time for `updated_at` and feed `ts` stamps.
    fn now(&self) -> UnixTime;

    /// Read-only content access.
    ///
    /// The default is an empty source: `get` answers `Ok(None)`, search and
    /// autocomplete answer no hits, `sources` is empty.
    #[inline]
    fn reference(&self) -> &dyn ReferenceSource {
        &EMPTY_REFERENCE
    }

    /// Rolls one plan with the environment's die source.
    ///
    /// The default fails with `Environment`: a command cannot conjure an
    /// RNG, and a missing port is an environment fault, not bad input.
    #[inline]
    fn roll(&mut self, _plan: &RollPlan) -> Result<DiceResult, EngineError> {
        Err(EngineError::Environment {
            reason: "no dice source is configured".to_owned(),
        })
    }
}

/// The process-wide empty reference source behind [`CommandEnv::reference`]'s
/// default (RFC `dice.md` 8, D4).
struct EmptyReference;

static EMPTY_REFERENCE: EmptyReference = EmptyReference;

impl ReferenceSource for EmptyReference {
    #[inline]
    fn get(&self, _id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        Ok(None)
    }

    #[inline]
    fn search(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(Vec::new())
    }

    #[inline]
    fn autocomplete(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(Vec::new())
    }

    #[inline]
    fn sources(&self) -> Vec<Provenance> {
        Vec::new()
    }
}

/// Pure with respect to `state`: computes what should happen and what it
/// costs. State is not modified; the executor persists and then applies.
#[inline]
pub fn apply(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    apply_as(state, command, env, None)
}

/// The actor-carrying form of [`apply`] (RFC `dice.md` section 6): U5 records
/// the actor for roll attribution, and the actor-requiring handlers reject
/// `None`.
pub(crate) fn apply_as(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
    actor: Option<PlayerId>,
) -> Result<Transition, EngineError> {
    match command {
        Command::AdvanceClock { by_seconds } => {
            clock::advance_clock_command(state, *by_seconds, env)
        }
        Command::SetClock { to } => clock::set_clock_command(state, *to, env),
        Command::CreateEntity { entity } => {
            creation::create_entity_command(state, entity.as_ref().clone(), actor, env.now())
        }
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
        Command::Roll {
            entity,
            spec,
            audience,
        } => dice_cmd::roll_command(state, *entity, spec, *audience, actor, env),
        Command::CastSpell {
            entity,
            spell,
            slot,
            upcast_slots,
            concentrate,
            expected_rev,
        } => casting::cast_spell_command(
            state,
            *entity,
            *spell,
            *slot,
            casting::CastOptions {
                upcast_slots: *upcast_slots,
                concentrate: *concentrate,
            },
            *expected_rev,
            env,
        ),
        Command::BeginConcentration {
            entity,
            status,
            expected_rev,
        } => casting::begin_concentration_command(state, *entity, status, *expected_rev, env),
        Command::EndConcentration {
            entity,
            expected_rev,
        } => casting::end_concentration_command(state, *entity, *expected_rev, env),
        Command::SpendHitDie {
            entity,
            resource,
            expected_rev,
        } => resources::spend_hit_die_command(state, *entity, *resource, *expected_rev, actor, env),
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

/// Materializes the entries an entity's item references resolve to (RFC
/// `dice.md` 5.6): one `ReferenceSource::get` per `InventoryItem::Ref`
/// entry. A source fault fails the command with `Content`; a missing entry
/// is omitted, because derivation warns `UnresolvedReference` itself.
fn materialize_item_content(
    entity: &StatblockEntity,
    reference: &dyn ReferenceSource,
) -> Result<BTreeMap<RefId, ContentEntry>, EngineError> {
    let mut content = BTreeMap::new();
    for entry in &entity.body.inventory {
        let InventoryItem::Ref(item) = &entry.item else {
            continue;
        };
        if let Some(resolved) = reference.get(&item.id).map_err(EngineError::Content)? {
            content.insert(item.id.clone(), resolved);
        }
    }
    Ok(content)
}

/// Stamps a touched copy: `rev = old.rev + 1`, `updated_at = now` (5.5).
///
/// The revision saturates at `u64::MAX`; a document at the ceiling has no next
/// revision, and the executor's monotonicity assertion fails fast on it instead
/// of a wrapped revision slipping into the store.
fn stamp(entity: &mut AnyEntity, now: UnixTime) {
    let rev = entity.rev().saturating_add(1);
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

/// Wraps one stamped document plus capped feed appends into a transition:
/// `EntityChanged` first, then `FeedAppended` when anything was appended.
fn stamped_with_feed(
    statblock: StatblockEntity,
    now: UnixTime,
    feed_appends: Vec<FeedEntry>,
    feed_keep_from: Option<FeedEntryId>,
    warnings: Vec<Warning>,
) -> Transition {
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, now);
    let mut events = vec![Event::EntityChanged {
        id: document.id(),
        rev: document.rev(),
        kind: document.kind(),
    }];
    if !feed_appends.is_empty() {
        events.push(Event::FeedAppended {
            ids: feed_appends.iter().map(|entry| entry.id).collect(),
        });
    }
    Transition {
        campaign: None,
        upserts: vec![document],
        removals: Vec::new(),
        feed_appends,
        feed_keep_from,
        warnings,
        events,
    }
}

/// Caps one transition's minted feed entries to the ring (5.6): the newest
/// `c = capacity.max(1)` survive, and the returned cutoff drops stored entries
/// below them.
///
/// The cutoff is spelled `last - (c - 1)` rather than `last + 1 - c`, so it
/// stays exact at the position ceiling (`last + 1` cannot overflow).
fn cap_feed(capacity: u32, entries: Vec<FeedEntry>) -> (Vec<FeedEntry>, Option<FeedEntryId>) {
    let ring = capacity.max(1);
    let ring_u64 = u64::from(ring);
    let ring_usize = usize::try_from(ring).expect("a u32 feed capacity fits usize");
    let Some(last) = entries.last() else {
        return (entries, None);
    };
    let keep_from = if last.id.get() >= ring_u64 {
        Some(FeedEntryId::new(last.id.get() - (ring_u64 - 1)))
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
        builders::{CampaignBuilder, StatblockBuilder, dice_pool, stat},
        types::{
            Abilities,
            Ability,
            Audience,
            ContentRef,
            DiceExpr,
            DieResult,
            Entity,
            FeedPayload,
            Formula,
            FormulaExpr,
            InventoryEntry,
            InventoryEntryId,
            InventoryItem,
            ItemState,
            Keep,
            ModifierOp,
            Namespace,
            NoteBody,
            PlayerId,
            Recharge,
            RefOrText,
            Resource,
            ResourceId,
            ResourceKind,
            RestKind,
            RollContext,
            RollKind,
            SpellEntry,
            SpellEntryId,
            SpellcastingBlock,
            SpellcastingBlockId,
            Status,
        },
    };

    use super::*;
    use crate::{
        content::{ContentData, ContentKind, SpellData},
        error::{ElementRef, WarningCode},
        ports::store::CampaignSnapshot,
        state::{LoadedCampaign, WorkingSetMut},
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

    fn hit_die(id: ResourceId, sides: i32, max: i32, current: i32) -> Resource {
        hit_die_with_recharge(id, sides, max, current, Recharge::Long)
    }

    fn hit_die_with_recharge(
        id: ResourceId,
        sides: i32,
        max: i32,
        current: i32,
        recharge: Recharge,
    ) -> Resource {
        Resource {
            id,
            name: format!("Hit Dice (d{sides})"),
            kind: ResourceKind::HitDie { sides },
            max: Formula::Const(max),
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
    fn a_revision_at_the_ceiling_saturates_on_the_next_write() {
        let id = EntityId::new();
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(32, 32);
        let entity = AnyEntity::from(
            Entity::builder(
                EntityKind::Pc,
                id,
                "Aria",
                PlayerId::new(),
                builder.build().expect("abilities and hp are set"),
            )
            .rev(u64::MAX)
            .build()
            .expect("a character kind pairs with a statblock body"),
        );
        let state = state_with(vec![entity]);

        let transition = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 1,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("damage is bookkeeping");

        assert_eq!(
            transition.upserts[0].rev(),
            u64::MAX,
            "the exhausted revision counter saturates; the executor fails fast on it"
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.hp.current, 31),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
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
    fn damage_to_a_concentrating_entity_suggests_the_save() {
        let id = EntityId::new();
        let document = wizard_with_statuses(id, vec![concentration_status(3_600)]);
        let AnyEntity::Statblock(statblock) = &document else {
            panic!("the wizard is a statblock");
        };
        let owner = statblock.owner;
        let state = state_with(vec![document]);

        // 24 damage asks for DC 12.
        let transition = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 24,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("damage is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(
            transition.events,
            vec![
                Event::EntityChanged {
                    id,
                    rev: 1,
                    kind: EntityKind::Pc,
                },
                Event::FeedAppended {
                    ids: vec![FeedEntryId::new(1)],
                },
            ]
        );
        let entry = &transition.feed_appends[0];
        assert_eq!(entry.id, FeedEntryId::new(state.next_feed_id()));
        assert_eq!(entry.ts, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(entry.clock_s, CampaignTime::from_seconds(0));
        assert_eq!(entry.actor, Some(owner));
        assert_eq!(entry.entity, Some(id));
        assert_eq!(entry.audience, Audience::RollerAndDm);
        assert_eq!(
            entry.payload,
            FeedPayload::Hint {
                text: "Concentration: CON save DC 12".to_owned(),
            }
        );

        // 5 damage asks for the DC 10 floor.
        let transition = apply(
            &state,
            &Command::ApplyDamage {
                entity: id,
                amount: 5,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("damage is bookkeeping");
        assert_eq!(
            transition.feed_appends[0].payload,
            FeedPayload::Hint {
                text: "Concentration: CON save DC 10".to_owned(),
            }
        );
    }

    #[test]
    fn damage_without_an_unexpired_concentration_appends_no_hint() {
        let expired = EntityId::new();
        let plain = EntityId::new();
        let state = state_with(vec![
            wizard_with_statuses(expired, vec![concentration_status(0)]),
            wizard(plain, 32, 32, 0),
        ]);

        for (entity, amount) in [(expired, 24), (plain, 24), (expired, 0)] {
            let transition = apply(
                &state,
                &Command::ApplyDamage {
                    entity,
                    amount,
                    expected_rev: None,
                },
                &mut FixedEnv,
            )
            .expect("damage is bookkeeping");

            assert!(
                transition.feed_appends.is_empty(),
                "entity {entity:?} took {amount} damage: no hint expected"
            );
            assert_eq!(
                transition.events,
                vec![Event::EntityChanged {
                    id: entity,
                    rev: 1,
                    kind: EntityKind::Pc,
                }]
            );
        }
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
    fn started_at_comes_from_a_non_zero_campaign_clock() {
        let id = EntityId::new();
        let state = state_with_campaign(campaign_with(200, 6, 7_200), vec![wizard(id, 32, 32, 0)]);

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

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(
                document.body.statuses[0].started_at,
                CampaignTime::from_seconds(7_200),
                "the campaign clock, not zero, stamps the start"
            ),
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
    fn spending_a_hit_die_rolls_heals_and_deducts() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let die = ResourceId::new();
        let mut document = wizard_with_resources(id, vec![hit_die(die, 6, 5, 5)]);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.abilities.constitution = 14;
        statblock.body.hp.current = 20;
        let state = state_with(vec![document]);
        let mut env = RollingEnv {
            rng: crate::dice::tests::ScriptedRng::new(&[5]),
        };

        let transition = apply_as(
            &state,
            &Command::SpendHitDie {
                entity: id,
                resource: die,
                expected_rev: None,
            },
            &mut env,
            Some(actor),
        )
        .expect("spending a hit die is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(
            transition.events,
            vec![
                Event::EntityChanged {
                    id,
                    rev: 1,
                    kind: EntityKind::Pc,
                },
                Event::FeedAppended {
                    ids: vec![FeedEntryId::new(1)],
                },
            ]
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.resources[0].current, 4);
                assert_eq!(document.body.hp.current, 27, "5 on the die plus the +2 CON");
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
        let entry = &transition.feed_appends[0];
        assert_eq!(entry.id, FeedEntryId::new(state.next_feed_id()));
        assert_eq!(entry.ts, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(entry.clock_s, CampaignTime::from_seconds(0));
        assert_eq!(entry.actor, Some(actor));
        assert_eq!(entry.entity, Some(id));
        assert_eq!(
            entry.audience,
            state.campaign().settings.player_roll_audience_default
        );
        let FeedPayload::Roll { roll } = &entry.payload else {
            panic!("the entry is a roll, got {:?}", entry.payload);
        };
        assert_eq!(roll.roller, actor);
        assert_eq!(roll.entity, Some(id));
        assert_eq!(
            roll.expr,
            DiceExpr::Add {
                lhs: Box::new(DiceExpr::Roll {
                    count: 1,
                    sides: 6,
                    keep: None,
                }),
                rhs: Box::new(DiceExpr::Const { value: 2 }),
            }
        );
        assert_eq!(roll.result.total, 7);
        assert_eq!(
            roll.context,
            Some(RollContext {
                kind: RollKind::Other,
                stat: None,
                target_dc: None,
            })
        );
    }

    #[test]
    fn hit_die_overspending_warns_and_bad_requests_fail() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let die = ResourceId::new();
        let feature = ResourceId::new();
        let mut document = wizard_with_resources(
            id,
            vec![
                hit_die(die, 6, 5, 0),
                resource(
                    feature,
                    "Arcane Recovery",
                    Formula::Const(1),
                    1,
                    Recharge::Long,
                ),
            ],
        );
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        // A -5 Constitution floors the heal at zero.
        statblock.body.abilities.constitution = 1;
        statblock.body.hp.current = 20;
        let state = state_with(vec![document]);
        let command = Command::SpendHitDie {
            entity: id,
            resource: die,
            expected_rev: None,
        };

        let transition = apply_as(
            &state,
            &command,
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[1]),
            },
            Some(actor),
        )
        .expect("overspending is warned, never blocked");
        assert_eq!(transition.warnings.len(), 1);
        assert_eq!(transition.warnings[0].code, WarningCode::ResourceOverspent);
        assert_eq!(transition.warnings[0].entity, Some(id));
        assert_eq!(
            transition.warnings[0].element,
            Some(ElementRef::Resource(die))
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.resources[0].current, -1);
                assert_eq!(
                    document.body.hp.current, 20,
                    "a negative total heals nothing"
                );
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }

        // A resource that is not a hit die cannot be spent as one.
        let result = apply_as(
            &state,
            &Command::SpendHitDie {
                entity: id,
                resource: feature,
                expected_rev: None,
            },
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[]),
            },
            Some(actor),
        );
        assert!(matches!(result, Err(EngineError::Malformed { .. })));

        // An unknown resource is missing, not malformed.
        let ghost = ResourceId::new();
        let result = apply_as(
            &state,
            &Command::SpendHitDie {
                entity: id,
                resource: ghost,
                expected_rev: None,
            },
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[]),
            },
            Some(actor),
        );
        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: Some(ElementRef::Resource(resource)),
            }) if entity == id && resource == ghost
        ));

        // The trusted path carries no actor.
        let result = apply(&state, &command, &mut FixedEnv);
        assert!(matches!(result, Err(EngineError::Malformed { .. })));

        // A broken content source fails the command before any roll.
        let mut document = wizard_with_resources(id, vec![hit_die(die, 6, 5, 5)]);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.inventory = vec![inventory_entry(
            InventoryEntryId::new(),
            "test:item/ring",
            "Ring",
        )];
        let state = state_with(vec![document]);
        let result = apply_as(
            &state,
            &command,
            &mut ContentEnv {
                reference: MapReference {
                    entries: BTreeMap::new(),
                    broken: true,
                },
            },
            Some(actor),
        );
        assert!(matches!(result, Err(EngineError::Content(_))));
    }

    #[test]
    fn spending_a_hit_die_refuses_sides_outside_the_caps() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let zero = ResourceId::new();
        let one = ResourceId::new();
        let negative = ResourceId::new();
        let oversized = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![
                hit_die(zero, 0, 3, 3),
                hit_die(one, 1, 3, 3),
                hit_die(negative, -3, 3, 3),
                hit_die(oversized, 1001, 3, 3),
            ],
        )]);

        // The empty script would panic on any draw: the caps must refuse the
        // persisted sides before the port is touched.
        for resource in [zero, one, negative] {
            let result = apply_as(
                &state,
                &Command::SpendHitDie {
                    entity: id,
                    resource,
                    expected_rev: None,
                },
                &mut RollingEnv {
                    rng: crate::dice::tests::ScriptedRng::new(&[]),
                },
                Some(actor),
            );
            assert!(
                matches!(result, Err(EngineError::Malformed { .. })),
                "sides of resource {resource:?} must be malformed"
            );
        }
        let result = apply_as(
            &state,
            &Command::SpendHitDie {
                entity: id,
                resource: oversized,
                expected_rev: None,
            },
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[]),
            },
            Some(actor),
        );
        assert!(matches!(
            result,
            Err(EngineError::TooLarge {
                what: "dice sides",
                limit: 1000
            })
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
    fn an_added_timed_status_is_swept_by_a_crossing() {
        let id = EntityId::new();
        let mut state = state_with(vec![wizard(id, 32, 32, 0)]);

        let added = apply(
            &state,
            &Command::AddStatus {
                entity: id,
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
            },
            &mut FixedEnv,
        )
        .expect("statuses are bookkeeping");
        state.apply(&added.patch());

        let moved = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");

        assert_eq!(
            moved.feed_appends.len(),
            1,
            "the added status was indexed by the upsert"
        );
        assert!(matches!(
            moved.feed_appends[0].payload,
            FeedPayload::StatusExpired { .. }
        ));
    }

    #[test]
    fn an_edited_timed_status_moves_its_expiry() {
        let id = EntityId::new();
        let status = timed_status(3_600);
        let mut state = state_with(vec![wizard_with_statuses(id, vec![status.clone()])]);

        let edited = apply(
            &state,
            &Command::EditStatusDuration {
                entity: id,
                status: status.id,
                duration: StatusDuration::Timed {
                    expires_at: CampaignTime::from_seconds(7_200),
                },
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("editing a status is bookkeeping");
        state.apply(&edited.patch());

        let old_instant = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");
        assert!(
            old_instant.feed_appends.is_empty(),
            "the old expiry no longer fires"
        );
        state.apply(&old_instant.patch());

        let new_instant = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");
        assert_eq!(new_instant.feed_appends.len(), 1, "the new expiry fires");
    }

    #[test]
    fn a_removed_timed_status_leaves_no_candidate() {
        let id = EntityId::new();
        let status = timed_status(3_600);
        let mut state = state_with(vec![wizard_with_statuses(id, vec![status.clone()])]);

        let removed = apply(
            &state,
            &Command::RemoveStatus {
                entity: id,
                status: status.id,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("removing a status is bookkeeping");
        state.apply(&removed.patch());

        let moved = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move is bookkeeping");
        assert!(
            moved.feed_appends.is_empty(),
            "a removed status is no longer a candidate"
        );
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
    fn a_rewind_to_the_time_domain_floor_warns_and_never_refills() {
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
                    1,
                    Recharge::Dawn,
                )],
            )],
        );

        let transition = apply(
            &state,
            &Command::SetClock {
                to: CampaignTime::from_seconds(i64::MIN),
            },
            &mut FixedEnv,
        )
        .expect("a rewind to the domain floor is legal");

        assert_eq!(transition.warnings.len(), 1);
        assert_eq!(transition.warnings[0].code, WarningCode::DawnRewind);
        assert!(
            transition.upserts.is_empty(),
            "a backwards crossing never refills, even at the domain floor"
        );
        assert_eq!(
            transition
                .patch()
                .campaign
                .expect("the campaign moved")
                .clock
                .now_s,
            CampaignTime::from_seconds(i64::MIN)
        );
        assert_eq!(transition.events, vec![Event::CampaignChanged]);
    }

    #[test]
    fn an_advance_to_the_time_domain_ceiling_still_refills() {
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
                    0,
                    Recharge::Dawn,
                )],
            )],
        );

        let transition = apply(
            &state,
            &Command::AdvanceClock {
                by_seconds: i64::MAX,
            },
            &mut FixedEnv,
        )
        .expect("advancing to the domain ceiling is legal from zero");

        assert_eq!(transition.warnings, Vec::new());
        assert_eq!(transition.upserts.len(), 1, "the crossing refills once");
        assert_eq!(
            transition
                .patch()
                .campaign
                .expect("the campaign moved")
                .clock
                .now_s,
            CampaignTime::from_seconds(i64::MAX)
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
    fn a_dawn_refill_stamps_updated_at() {
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

        assert_eq!(transition.upserts.len(), 1);
        assert_eq!(transition.upserts[0].rev(), 1);
        assert_eq!(
            transition.upserts[0].updated_at(),
            UnixTime::from_seconds(FIXED_TIME),
            "a refill is stamped with the environment clock"
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, 2),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
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
    fn the_feed_cap_is_exact_at_the_position_ceiling() {
        let (kept, keep_from) = cap_feed(2, vec![feed(u64::MAX - 1), feed(u64::MAX)]);
        assert_eq!(kept.len(), 2);
        assert_eq!(
            keep_from,
            Some(FeedEntryId::new(u64::MAX - 1)),
            "the cutoff never forms `last + 1`"
        );

        let (kept, keep_from) = cap_feed(1, vec![feed(u64::MAX)]);
        assert_eq!(kept.len(), 1);
        assert_eq!(keep_from, Some(FeedEntryId::new(u64::MAX)));
    }

    #[test]
    fn minting_at_the_position_ceiling_saturates_without_wrapping() {
        let id = EntityId::new();
        let state = LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: campaign_with(2, 6, 0),
            entities: vec![wizard_with_statuses(
                id,
                vec![timed_status(3_600), timed_status(3_600)],
            )],
            feed: vec![feed(u64::MAX - 1)],
        });

        let transition = apply(
            &state,
            &Command::AdvanceClock { by_seconds: 3_600 },
            &mut FixedEnv,
        )
        .expect("a clock move at the position ceiling is bookkeeping");

        assert_eq!(transition.feed_appends.len(), 2);
        assert!(
            transition
                .feed_appends
                .iter()
                .all(|entry| entry.id == FeedEntryId::new(u64::MAX)),
            "an exhausted position domain saturates instead of wrapping to zero"
        );
        assert_eq!(
            transition.feed_keep_from,
            Some(FeedEntryId::new(u64::MAX - 1)),
            "the cutoff is exact at the ceiling"
        );
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
    fn a_rest_with_nothing_to_change_still_rewrites_the_target() {
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 32, 32, 0)]);

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

        assert_eq!(
            transition.events,
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }],
            "a rest is an explicit checkpoint, even with nothing to change"
        );
        assert_eq!(transition.upserts.len(), 1);
        assert_eq!(transition.upserts[0].rev(), 1);
        assert_eq!(
            transition.upserts[0].updated_at(),
            UnixTime::from_seconds(FIXED_TIME)
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.hp.current, 32);
                assert!(document.body.statuses.is_empty());
                assert!(document.body.resources.is_empty());
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
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
    fn a_long_rest_regains_half_the_hit_dice() {
        let id = EntityId::new();
        let partly = ResourceId::new();
        let spent = ResourceId::new();
        let minimum = ResourceId::new();
        let feature = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![
                hit_die(partly, 6, 5, 3),
                hit_die(spent, 8, 5, 0),
                hit_die(minimum, 4, 1, 0),
                resource(
                    feature,
                    "Arcane Recovery",
                    Formula::Const(2),
                    0,
                    Recharge::Long,
                ),
            ],
        )]);

        let transition = apply(
            &state,
            &Command::LongRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a long rest is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(
                    document.body.resources[0].current, 5,
                    "3 dice plus half of 5 is clamped at 5"
                );
                assert_eq!(document.body.resources[1].current, 2, "half of 5");
                assert_eq!(
                    document.body.resources[2].current, 1,
                    "a maximum of 1 still regains one die"
                );
                assert_eq!(
                    document.body.resources[3].current, 2,
                    "a Long feature still refills to full"
                );
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_long_rest_skips_unevaluated_hit_dice_and_warns() {
        let id = EntityId::new();
        let die = ResourceId::new();
        let mut document = wizard(id, 32, 32, 0);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.resources = vec![Resource {
            id: die,
            name: "Hit Dice (d6)".to_owned(),
            kind: ResourceKind::HitDie { sides: 6 },
            max: Formula::Expr(FormulaExpr::AbilityMod {
                ability: Ability::Constitution,
            }),
            current: 1,
            recharge: Recharge::Long,
        }];
        let state = state_with(vec![document]);

        let transition = apply(
            &state,
            &Command::LongRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a long rest is bookkeeping");

        assert_eq!(
            transition.warnings,
            vec![Warning {
                code: WarningCode::FormulaUnevaluated,
                message: "Hit Dice (d6) regained nothing: its maximum needs the rules unit"
                    .to_owned(),
                entity: Some(id),
                element: Some(ElementRef::Resource(die)),
            }],
            "an unevaluated hit-die maximum regains nothing and warns exactly once"
        );
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => assert_eq!(document.body.resources[0].current, 1),
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_long_rest_regains_manual_hit_dice_and_refills_short_ones() {
        let id = EntityId::new();
        let manual = ResourceId::new();
        let short = ResourceId::new();
        let state = state_with(vec![wizard_with_resources(
            id,
            vec![
                hit_die_with_recharge(manual, 6, 5, 0, Recharge::Manual),
                hit_die_with_recharge(short, 6, 5, 1, Recharge::Short),
            ],
        )]);

        let transition = apply(
            &state,
            &Command::LongRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a long rest is bookkeeping");

        assert_eq!(transition.warnings, Vec::new());
        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(
                    document.body.resources[0].current, 2,
                    "a Manual pool is untouched by refills and regains half"
                );
                assert_eq!(
                    document.body.resources[1].current, 5,
                    "a Short pool is refilled by the short pass before the half-regain clamps"
                );
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_long_rest_recovers_hp_death_saves_and_exhaustion() {
        let id = EntityId::new();
        let mut document = wizard(id, 32, 11, 5);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.death_saves = DeathSaves {
            successes: 2,
            failures: 1,
        };
        statblock.body.exhaustion = 2;
        let state = state_with(vec![document]);

        let transition = apply(
            &state,
            &Command::LongRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a long rest is bookkeeping");

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.hp.current, 32, "hit points recover fully");
                assert_eq!(
                    document.body.hp.temp, 5,
                    "temporary hit points are untouched"
                );
                assert_eq!(document.body.death_saves, DeathSaves::default());
                assert_eq!(document.body.exhaustion, 1, "one level is shed");
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_short_rest_leaves_hp_death_saves_and_exhaustion_alone() {
        let id = EntityId::new();
        let mut document = wizard(id, 32, 11, 0);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.death_saves = DeathSaves {
            successes: 0,
            failures: 2,
        };
        statblock.body.exhaustion = 3;
        let state = state_with(vec![document]);

        let transition = apply(
            &state,
            &Command::ShortRest {
                entity: id,
                advance_seconds: None,
                expected_rev: None,
            },
            &mut FixedEnv,
        )
        .expect("a short rest is bookkeeping");

        match &transition.upserts[0] {
            AnyEntity::Statblock(document) => {
                assert_eq!(document.body.hp.current, 11);
                assert_eq!(
                    document.body.death_saves,
                    DeathSaves {
                        successes: 0,
                        failures: 2,
                    }
                );
                assert_eq!(
                    document.body.exhaustion, 3,
                    "exhaustion survives a short rest"
                );
            }
            other => panic!("the wizard is a statblock, got {other:?}"),
        }
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

    /// A `CommandEnv` with a scripted die source, for roll commands.
    struct RollingEnv {
        rng: crate::dice::tests::ScriptedRng,
    }

    impl CommandEnv for RollingEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }

        fn roll(&mut self, plan: &RollPlan) -> Result<DiceResult, EngineError> {
            Ok(crate::dice::roll(plan, &mut self.rng))
        }
    }

    #[test]
    fn an_expression_roll_is_attributed_and_fed_once() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 12, 12, 0)]);
        let target = stat("save.wis").expect("a literal stat key");
        let command = Command::Roll {
            entity: Some(id),
            spec: RollSpec::Expression {
                expr: crate::dice::parse("1d20+3").expect("a literal expression"),
                target: Some(target.clone()),
            },
            audience: None,
        };
        let mut env = RollingEnv {
            rng: crate::dice::tests::ScriptedRng::new(&[17]),
        };

        let transition =
            apply_as(&state, &command, &mut env, Some(actor)).expect("a roll is bookkeeping");

        assert!(transition.upserts.is_empty(), "a roll mutates nothing");
        assert!(transition.removals.is_empty());
        assert!(transition.campaign.is_none());
        assert!(transition.warnings.is_empty());
        assert_eq!(transition.feed_appends.len(), 1);
        let entry = &transition.feed_appends[0];
        assert_eq!(entry.id, FeedEntryId::new(state.next_feed_id()));
        assert_eq!(entry.ts, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(entry.clock_s, CampaignTime::from_seconds(0));
        assert_eq!(entry.actor, Some(actor));
        assert_eq!(entry.entity, Some(id));
        assert_eq!(
            entry.audience,
            state.campaign().settings.player_roll_audience_default
        );
        assert_eq!(
            transition.events,
            vec![Event::FeedAppended {
                ids: vec![entry.id],
            }]
        );
        let FeedPayload::Roll { roll } = &entry.payload else {
            panic!("the entry is a roll, got {:?}", entry.payload);
        };
        assert_eq!(roll.roller, actor);
        assert_eq!(roll.entity, Some(id));
        assert_eq!(roll.ts, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(
            roll.expr,
            crate::dice::parse("1d20+3").expect("a literal expression")
        );
        assert_eq!(roll.result.total, 20, "a scripted 17 plus the flat 3");
        assert_eq!(
            roll.result.terms[0].dice,
            vec![DieResult {
                value: 17,
                kept: true
            }]
        );
        assert_eq!(
            roll.context,
            Some(RollContext {
                kind: RollKind::Save,
                stat: Some(target),
                target_dc: None,
            })
        );
    }

    #[test]
    fn a_roll_needs_an_actor_a_real_entity_and_a_port() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let ghost = EntityId::new();
        let state = state_with(vec![wizard(id, 12, 12, 0)]);
        let expression = || RollSpec::Expression {
            expr: crate::dice::parse("1d20").expect("a literal expression"),
            target: None,
        };

        // The trusted path carries no actor.
        let result = apply(
            &state,
            &Command::Roll {
                entity: Some(id),
                spec: expression(),
                audience: None,
            },
            &mut FixedEnv,
        );
        assert!(matches!(result, Err(EngineError::Malformed { .. })));

        // An addressed entity must exist, before any spec work.
        let result = apply_as(
            &state,
            &Command::Roll {
                entity: Some(ghost),
                spec: expression(),
                audience: None,
            },
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[]),
            },
            Some(actor),
        );
        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: None,
            }) if entity == ghost
        ));

        // A check needs an entity to derive from.
        let result = apply_as(
            &state,
            &Command::Roll {
                entity: None,
                spec: RollSpec::Check {
                    stat: stat("save.wis").expect("a literal stat key"),
                    target_dc: None,
                },
                audience: None,
            },
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[]),
            },
            Some(actor),
        );
        assert!(matches!(result, Err(EngineError::Malformed { .. })));

        // A wire AST meets the caps before any draw happens: the empty script
        // would panic on a draw.
        let result = apply_as(
            &state,
            &Command::Roll {
                entity: Some(id),
                spec: RollSpec::Expression {
                    expr: DiceExpr::Roll {
                        count: 101,
                        sides: 6,
                        keep: None,
                    },
                    target: None,
                },
                audience: None,
            },
            &mut RollingEnv {
                rng: crate::dice::tests::ScriptedRng::new(&[]),
            },
            Some(actor),
        );
        assert!(matches!(
            result,
            Err(EngineError::TooLarge {
                what: "dice count",
                limit: 100
            })
        ));

        // The trait default fails closed instead of conjuring a die source.
        let result = apply_as(
            &state,
            &Command::Roll {
                entity: Some(id),
                spec: expression(),
                audience: None,
            },
            &mut FixedEnv,
            Some(actor),
        );
        assert!(matches!(result, Err(EngineError::Environment { .. })));
    }

    #[test]
    fn a_check_roll_derives_its_plan_from_the_sheet() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let state = state_with(vec![wizard(id, 12, 12, 0)]);
        let target = stat("save.wis").expect("a literal stat key");
        let command = Command::Roll {
            entity: Some(id),
            spec: RollSpec::Check {
                stat: target.clone(),
                target_dc: Some(15),
            },
            audience: None,
        };
        let mut env = RollingEnv {
            rng: crate::dice::tests::ScriptedRng::new(&[17]),
        };

        let transition =
            apply_as(&state, &command, &mut env, Some(actor)).expect("a check is bookkeeping");

        assert!(transition.upserts.is_empty(), "a check mutates nothing");
        let FeedPayload::Roll { roll } = &transition.feed_appends[0].payload else {
            panic!(
                "the entry is a roll, got {:?}",
                transition.feed_appends[0].payload
            );
        };
        assert_eq!(
            roll.expr,
            crate::dice::parse("1d20").expect("a literal expression"),
            "an untrained save on a 10 is a flat 0"
        );
        assert_eq!(roll.result.total, 17);
        assert_eq!(
            roll.context,
            Some(RollContext {
                kind: RollKind::Save,
                stat: Some(target),
                target_dc: Some(15),
            })
        );
    }

    #[test]
    fn a_check_refuses_derived_pools_outside_the_caps() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let sheet_with_dice = |expr: DiceExpr| {
            let status = Status {
                id: StatusId::new(),
                kind: StatusKind::Effect,
                name: "Overreach".to_owned(),
                reference: None,
                text: None,
                started_at: CampaignTime::from_seconds(0),
                duration: StatusDuration::Indefinite,
                source: None,
                modifiers: vec![Modifier {
                    target: stat("save.wis").expect("a literal stat key"),
                    op: ModifierOp::Add {
                        value: Formula::dice(expr),
                    },
                    when: None,
                    note: None,
                }],
            };
            state_with(vec![wizard_with_statuses(id, vec![status])])
        };
        let check = || Command::Roll {
            entity: Some(id),
            spec: RollSpec::Check {
                stat: stat("save.wis").expect("a literal stat key"),
                target_dc: None,
            },
            audience: None,
        };

        // The empty script would panic on any draw: the caps must refuse the
        // derived pool before the port is touched.
        for expr in [
            dice_pool(0, 6),
            dice_pool(1, 1),
            DiceExpr::Roll {
                count: 3,
                sides: 6,
                keep: Some(Keep::Highest { count: -1 }),
            },
        ] {
            let state = sheet_with_dice(expr.clone());
            let result = apply_as(
                &state,
                &check(),
                &mut RollingEnv {
                    rng: crate::dice::tests::ScriptedRng::new(&[]),
                },
                Some(actor),
            );
            assert!(
                matches!(result, Err(EngineError::Malformed { .. })),
                "a derived {expr:?} must be malformed"
            );
        }

        for (expr, what, limit) in [
            (dice_pool(400, 6), "dice count", 100_u32),
            (dice_pool(1, 5000), "dice sides", 1000_u32),
        ] {
            let state = sheet_with_dice(expr.clone());
            let result = apply_as(
                &state,
                &check(),
                &mut RollingEnv {
                    rng: crate::dice::tests::ScriptedRng::new(&[]),
                },
                Some(actor),
            );
            assert!(
                matches!(
                    result,
                    Err(EngineError::TooLarge {
                        what: seen,
                        limit: seen_limit
                    }) if seen == what && seen_limit == limit
                ),
                "a derived {expr:?} must exceed {what} {limit}"
            );
        }
    }

    /// A content source over a literal map, optionally broken.
    struct MapReference {
        entries: BTreeMap<RefId, ContentEntry>,
        broken: bool,
    }

    impl ReferenceSource for MapReference {
        fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
            if self.broken {
                return Err(ContentFault::Unavailable {
                    message: "the fixture source is down".to_owned(),
                });
            }
            Ok(self.entries.get(id).cloned())
        }

        fn search(&self, _query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
            Ok(Vec::new())
        }

        fn autocomplete(
            &self,
            _query: &SearchQuery<'_>,
        ) -> Result<Vec<ContentSummary>, ContentFault> {
            Ok(Vec::new())
        }

        fn sources(&self) -> Vec<Provenance> {
            Vec::new()
        }
    }

    /// One item-shaped `ContentEntry`, just enough for materialization.
    fn item_entry(reference: &str, name: &str) -> ContentEntry {
        ContentEntry {
            id: RefId::parse(reference).expect("a literal reference"),
            kind: ContentKind::Item,
            name: name.to_owned(),
            description: String::new(),
            data: ContentData::Text,
            provenance: Provenance {
                namespace: Namespace::parse("test").expect("a literal namespace"),
                source: String::new(),
                source_revision: String::new(),
                license: String::new(),
                attribution: String::new(),
                url: None,
            },
            needs_review: false,
            review_notes: Vec::new(),
            ext: BTreeMap::new(),
        }
    }

    /// One inventory entry holding an item reference.
    fn inventory_entry(id: InventoryEntryId, reference: &str, name: &str) -> InventoryEntry {
        InventoryEntry {
            id,
            item: InventoryItem::Ref(ContentRef {
                id: RefId::parse(reference).expect("a literal reference"),
                name: name.to_owned(),
            }),
            qty: 1,
            state: ItemState::Carried,
            attuned: false,
            notes: String::new(),
        }
    }

    #[test]
    fn item_content_materializes_resolved_references_and_skips_missing_ones() {
        let id = EntityId::new();
        let resolved = RefId::parse("test:item/ring").expect("a literal reference");
        let missing = RefId::parse("test:item/ghost").expect("a literal reference");
        let mut document = wizard(id, 12, 12, 0);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.inventory = vec![
            inventory_entry(InventoryEntryId::new(), "test:item/ring", "Ring"),
            inventory_entry(InventoryEntryId::new(), "test:item/ghost", "Ghost"),
        ];
        let source = MapReference {
            entries: BTreeMap::from([(resolved.clone(), item_entry("test:item/ring", "Ring"))]),
            broken: false,
        };

        let content = materialize_item_content(statblock, &source).expect("the source answers");

        assert_eq!(content.len(), 1, "only the resolved entry is carried");
        assert_eq!(
            content.get(&resolved).map(|entry| entry.name.as_str()),
            Some("Ring")
        );
        assert!(!content.contains_key(&missing));

        // A source that fails while resolving is a content fault.
        let broken = MapReference {
            entries: BTreeMap::new(),
            broken: true,
        };
        let result = materialize_item_content(statblock, &broken);
        assert!(matches!(result, Err(EngineError::Content(_))));
    }

    /// A `CommandEnv` over a literal content source.
    struct ContentEnv {
        reference: MapReference,
    }

    impl CommandEnv for ContentEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }

        fn reference(&self) -> &dyn ReferenceSource {
            &self.reference
        }
    }

    fn map_reference(entries: Vec<ContentEntry>) -> MapReference {
        MapReference {
            entries: entries
                .into_iter()
                .map(|entry| (entry.id.clone(), entry))
                .collect(),
            broken: false,
        }
    }

    fn named_concentration(name: &str) -> Status {
        Status {
            id: StatusId::new(),
            kind: StatusKind::Concentration,
            name: name.to_owned(),
            reference: None,
            text: None,
            started_at: CampaignTime::from_seconds(0),
            duration: StatusDuration::Indefinite,
            source: None,
            modifiers: Vec::new(),
        }
    }

    fn spell_entry(id: SpellEntryId, name: &str, reference: Option<&str>) -> SpellEntry {
        SpellEntry {
            id,
            name: name.to_owned(),
            reference: reference.map(|raw| RefId::parse(raw).expect("a literal reference")),
            prepared: true,
        }
    }

    fn spell_slot(id: ResourceId, name: &str, level: i32, current: i32) -> Resource {
        Resource {
            id,
            name: name.to_owned(),
            kind: ResourceKind::SpellSlot { level },
            max: Formula::Const(4),
            current,
            recharge: Recharge::Long,
        }
    }

    fn pact_slot(id: ResourceId, current: i32) -> Resource {
        Resource {
            id,
            name: "Pact Magic".to_owned(),
            kind: ResourceKind::PactSlot,
            max: Formula::Const(2),
            current,
            recharge: Recharge::Short,
        }
    }

    fn spell_content(reference: &str, name: &str, level: u8, concentration: bool) -> ContentEntry {
        ContentEntry {
            id: RefId::parse(reference).expect("a literal reference"),
            kind: ContentKind::Spell,
            name: name.to_owned(),
            description: String::new(),
            data: ContentData::Spell(SpellData {
                level,
                school: "Evocation".to_owned(),
                concentration,
                ritual: false,
            }),
            provenance: Provenance {
                namespace: Namespace::parse("test").expect("a literal namespace"),
                source: String::new(),
                source_revision: String::new(),
                license: String::new(),
                attribution: String::new(),
                url: None,
            },
            needs_review: false,
            review_notes: Vec::new(),
            ext: BTreeMap::new(),
        }
    }

    fn caster(id: EntityId, resources: Vec<Resource>, spells: Vec<SpellEntry>) -> AnyEntity {
        let mut document = wizard_with_resources(id, resources);
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        statblock.body.spellcasting = vec![SpellcastingBlock {
            id: SpellcastingBlockId::new(),
            source: RefOrText::Text("Wizard".to_owned()),
            ability: Ability::Intelligence,
            spells,
        }];
        document
    }

    #[test]
    fn casting_resolves_spends_and_replaces_concentration() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let slot = ResourceId::new();
        let hex = SpellEntryId::new();
        let mut document = caster(
            id,
            vec![spell_slot(slot, "Spell Slots (1st)", 1, 4)],
            vec![spell_entry(hex, "Hex", Some("test:spell/hex"))],
        );
        let AnyEntity::Statblock(statblock) = &mut document else {
            panic!("the wizard is a statblock");
        };
        let owner = statblock.owner;
        statblock
            .body
            .statuses
            .push(named_concentration("Hunter's Mark"));
        let state = state_with(vec![document]);
        let mut env = ContentEnv {
            reference: map_reference(vec![spell_content("test:spell/hex", "Hex", 1, true)]),
        };

        let transition = apply_as(
            &state,
            &Command::CastSpell {
                entity: id,
                spell: hex,
                slot,
                upcast_slots: 0,
                concentrate: false,
                expected_rev: None,
            },
            &mut env,
            Some(actor),
        )
        .expect("casting a known spell is bookkeeping");

        assert!(transition.warnings.is_empty());
        let AnyEntity::Statblock(stamped) = &transition.upserts[0] else {
            panic!("the caster is a statblock");
        };
        assert_eq!(stamped.body.resources[0].current, 3);
        assert_eq!(stamped.body.statuses.len(), 1, "concentration is replaced");
        let status = &stamped.body.statuses[0];
        assert_eq!(status.kind, StatusKind::Concentration);
        assert_eq!(status.name, "Hex");
        assert_eq!(
            status.reference,
            Some(RefId::parse("test:spell/hex").expect("a literal reference"))
        );
        assert_eq!(status.started_at, state.campaign().clock.now_s);
        assert_eq!(transition.feed_appends.len(), 1);
        assert_eq!(
            transition.feed_appends[0].payload,
            FeedPayload::ConcentrationEnded {
                name: "Hunter's Mark".to_owned()
            }
        );
        assert_eq!(transition.feed_appends[0].actor, Some(owner));
        assert_eq!(transition.feed_appends[0].audience, Audience::RollerAndDm);
        assert!(matches!(
            transition.events.as_slice(),
            [Event::EntityChanged { .. }, Event::FeedAppended { .. }]
        ));
    }

    #[test]
    fn casting_rejects_cantrips_bad_slots_and_upcasts() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let slot1 = ResourceId::new();
        let slot3 = ResourceId::new();
        let pact = ResourceId::new();
        let feature = ResourceId::new();
        let cantrip = SpellEntryId::new();
        let fireball = SpellEntryId::new();
        let hex = SpellEntryId::new();
        let unresolved = SpellEntryId::new();
        let ghost_spell = SpellEntryId::new();
        let ghost_slot = ResourceId::new();
        let document = caster(
            id,
            vec![
                spell_slot(slot1, "Spell Slots (1st)", 1, 4),
                spell_slot(slot3, "Spell Slots (3rd)", 3, 2),
                pact_slot(pact, 2),
                resource(
                    feature,
                    "Arcane Recovery",
                    Formula::Const(1),
                    1,
                    Recharge::Long,
                ),
            ],
            vec![
                spell_entry(cantrip, "Spark", Some("test:spell/spark")),
                spell_entry(fireball, "Fireball", Some("test:spell/fireball")),
                spell_entry(hex, "Hex", Some("test:spell/hex")),
                spell_entry(unresolved, "Bolt", None),
            ],
        );
        let state = state_with(vec![document]);
        let mut env = ContentEnv {
            reference: map_reference(vec![
                spell_content("test:spell/spark", "Spark", 0, false),
                spell_content("test:spell/fireball", "Fireball", 3, false),
                spell_content("test:spell/hex", "Hex", 1, true),
            ]),
        };
        let mut try_cast = |spell: SpellEntryId, slot: ResourceId, upcast_slots: u32| {
            apply_as(
                &state,
                &Command::CastSpell {
                    entity: id,
                    spell,
                    slot,
                    upcast_slots,
                    concentrate: false,
                    expected_rev: None,
                },
                &mut env,
                Some(actor),
            )
        };

        // A cantrip is not cast with a slot.
        assert!(matches!(
            try_cast(cantrip, slot1, 0),
            Err(EngineError::Malformed { .. })
        ));
        // A spell needs a slot at least its level.
        assert!(matches!(
            try_cast(fireball, slot1, 0),
            Err(EngineError::Malformed { .. })
        ));
        // Upcasting must be declared: three minus three is zero, not one.
        assert!(matches!(
            try_cast(fireball, slot3, 1),
            Err(EngineError::Malformed { .. })
        ));
        // A pact slot's level lives in the class table: no level check, but
        // any declared upcast is still nonsense.
        assert!(matches!(
            try_cast(hex, pact, 1),
            Err(EngineError::Malformed { .. })
        ));
        // Only spell slots and pact slots pay for spells.
        assert!(matches!(
            try_cast(fireball, feature, 0),
            Err(EngineError::Malformed { .. })
        ));
        // Without a known spell level there is nothing to validate against.
        assert!(matches!(
            try_cast(unresolved, slot1, 1),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            try_cast(ghost_spell, slot1, 0),
            Err(EngineError::NotFound {
                element: Some(ElementRef::Spell(spell)),
                ..
            }) if spell == ghost_spell
        ));
        assert!(matches!(
            try_cast(fireball, ghost_slot, 0),
            Err(EngineError::NotFound {
                element: Some(ElementRef::Resource(resource)),
                ..
            }) if resource == ghost_slot
        ));
    }

    #[test]
    fn casting_warns_for_unresolved_and_forced_concentration() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let slot3 = ResourceId::new();
        let fireball = SpellEntryId::new();
        let unresolved = SpellEntryId::new();
        let document = caster(
            id,
            vec![spell_slot(slot3, "Spell Slots (3rd)", 3, 2)],
            vec![
                spell_entry(fireball, "Fireball", Some("test:spell/fireball")),
                spell_entry(unresolved, "Bolt", None),
            ],
        );
        let state = state_with(vec![document]);
        let mut env = ContentEnv {
            reference: map_reference(vec![spell_content(
                "test:spell/fireball",
                "Fireball",
                3,
                false,
            )]),
        };
        let mut cast = |spell: SpellEntryId, concentrate: bool| {
            apply_as(
                &state,
                &Command::CastSpell {
                    entity: id,
                    spell,
                    slot: slot3,
                    upcast_slots: 0,
                    concentrate,
                    expected_rev: None,
                },
                &mut env,
                Some(actor),
            )
        };

        // Resolved but not a concentration spell: warn, still concentrate.
        let forced = cast(fireball, true).expect("fireball is bookkeeping");
        assert_eq!(forced.warnings.len(), 1);
        assert_eq!(
            forced.warnings[0].code,
            WarningCode::ConcentrationUnsupported
        );
        assert_eq!(forced.warnings[0].entity, Some(id));
        assert_eq!(
            forced.warnings[0].element,
            Some(ElementRef::Spell(fireball))
        );
        let AnyEntity::Statblock(stamped) = &forced.upserts[0] else {
            panic!("the caster is a statblock");
        };
        assert_eq!(stamped.body.statuses.len(), 1);
        assert_eq!(stamped.body.statuses[0].name, "Fireball");
        assert!(forced.feed_appends.is_empty());

        // Unresolved: warn and cast anyway; concentration starts from the
        // client's request with the name snapshot.
        let unresolved_roll = cast(unresolved, true).expect("bolt is bookkeeping");
        assert_eq!(unresolved_roll.warnings.len(), 1);
        assert_eq!(
            unresolved_roll.warnings[0].code,
            WarningCode::UnresolvedReference
        );
        assert_eq!(
            unresolved_roll.warnings[0].element,
            Some(ElementRef::Spell(unresolved))
        );
        let AnyEntity::Statblock(stamped) = &unresolved_roll.upserts[0] else {
            panic!("the caster is a statblock");
        };
        assert_eq!(stamped.body.statuses.len(), 1);
        let status = &stamped.body.statuses[0];
        assert_eq!(status.name, "Bolt");
        assert_eq!(status.reference, None);
        assert_eq!(status.duration, StatusDuration::Indefinite);
    }

    #[test]
    fn begin_and_end_concentration_replace_and_clear() {
        let actor = PlayerId::new();
        let id = EntityId::new();
        let state = state_with(vec![wizard_with_statuses(
            id,
            vec![named_concentration("Hunter's Mark")],
        )]);
        let mut env = FixedEnv;

        // A begin must actually be a concentration status.
        let result = apply_as(
            &state,
            &Command::BeginConcentration {
                entity: id,
                status: StatusSpec {
                    kind: StatusKind::Effect,
                    name: "Bless".to_owned(),
                    reference: None,
                    text: None,
                    duration: StatusDuration::Indefinite,
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
            &mut env,
            Some(actor),
        );
        assert!(matches!(result, Err(EngineError::Malformed { .. })));

        // Replacing announces the end of the old status.
        let began = apply_as(
            &state,
            &Command::BeginConcentration {
                entity: id,
                status: StatusSpec {
                    kind: StatusKind::Concentration,
                    name: "Bless".to_owned(),
                    reference: None,
                    text: None,
                    duration: StatusDuration::Indefinite,
                    source: None,
                    modifiers: Vec::new(),
                },
                expected_rev: None,
            },
            &mut env,
            Some(actor),
        )
        .expect("beginning concentration is bookkeeping");
        assert!(began.warnings.is_empty());
        assert_eq!(
            began.feed_appends[0].payload,
            FeedPayload::ConcentrationEnded {
                name: "Hunter's Mark".to_owned()
            }
        );
        let AnyEntity::Statblock(stamped) = &began.upserts[0] else {
            panic!("the wizard is a statblock");
        };
        assert_eq!(stamped.body.statuses.len(), 1);
        assert_eq!(stamped.body.statuses[0].name, "Bless");

        // Ending announces and clears every concentration status.
        let ended = apply_as(
            &state,
            &Command::EndConcentration {
                entity: id,
                expected_rev: None,
            },
            &mut env,
            Some(actor),
        )
        .expect("ending concentration is bookkeeping");
        assert!(ended.warnings.is_empty());
        assert_eq!(
            ended.feed_appends[0].payload,
            FeedPayload::ConcentrationEnded {
                name: "Hunter's Mark".to_owned()
            }
        );
        let AnyEntity::Statblock(stamped) = &ended.upserts[0] else {
            panic!("the wizard is a statblock");
        };
        assert!(stamped.body.statuses.is_empty());

        // Nothing left to end.
        let empty_state = state_with(vec![wizard(id, 32, 32, 0)]);
        let result = apply_as(
            &empty_state,
            &Command::EndConcentration {
                entity: id,
                expected_rev: None,
            },
            &mut env,
            Some(actor),
        );
        assert!(matches!(
            result,
            Err(EngineError::NotFound {
                entity,
                element: None,
            }) if entity == id
        ));
    }
}
