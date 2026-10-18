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
    todo!()
}

/// One command's owned effect: the durable delta plus the delivery hints.
///
/// Fields are crate-private; clients see warnings and events through
/// [`Executed`](crate::executor::Executed) only.
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
        todo!()
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
