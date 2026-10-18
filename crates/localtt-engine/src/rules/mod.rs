//! The rules unit (RFC `rules.md`): static derivation of a sheet and the
//! roll-parts seam U5 consumes.
//!
//! [`derive()`] and [`roll_parts()`] are pure: the entity, the context and the
//! campaign instant are the only inputs, and nothing here does I/O
//! (RFC `rules.md` 5.1).

mod armor;
mod collect;
mod formula;
mod tables;

use std::collections::BTreeMap;

use localtt_core::types::{
    CampaignTime,
    DiceExpr,
    FeatureEntryId,
    InventoryEntryId,
    OverrideValue,
    RefId,
    ResourceId,
    StatKey,
    StatblockEntity,
    StatusId,
};

use crate::{
    content::ContentEntry,
    error::{EngineError, Warning},
};

/// Inputs a derivation needs beyond the entity: the campaign instant and the
/// materialized content entries the entity's references resolve to.
#[derive(Clone, Debug)]
pub struct RulesContext<'a> {
    /// Campaign instant used for status expiry and time-gated rules.
    pub now: CampaignTime,
    /// Materialized entries keyed by reference id; the caller resolves them
    /// (engine architecture 7.4). Missing entries degrade with warnings.
    pub content: &'a BTreeMap<RefId, ContentEntry>,
}

/// Derives the full sheet: values, resources, class tables, warnings.
#[must_use]
#[inline]
pub fn derive(entity: &StatblockEntity, ctx: &RulesContext<'_>) -> DerivedSheet {
    todo!()
}

/// Builds the flat part, attributed dice and hint for one stat roll (U5).
///
/// Errors with `Malformed` when the stat has no integer effective value (no
/// derived value, or an override that is not an integer) or when a dice leaf
/// sits under an operation the roller cannot represent (section 5.8).
#[inline]
pub fn roll_parts(
    entity: &StatblockEntity,
    stat: &StatKey,
    ctx: &RulesContext<'_>,
) -> Result<RollParts, EngineError> {
    todo!()
}

/// The static sheet of one entity at one instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedSheet {
    /// Derived stat values by canonical key.
    pub values: BTreeMap<StatKey, DerivedValue>,
    /// Derived maxima for the entity's tracked resources, keyed by element id.
    pub resources: BTreeMap<ResourceId, DerivedResource>,
    /// Class-table spell-slot capacity (not the tracked resources).
    pub slots: SlotMaxima,
    /// Hit-dice pools by die sides: how many dice of that size the entity has.
    pub hit_dice: BTreeMap<u8, i32>,
    /// Non-blocking rule outcomes, entity-scoped.
    pub warnings: Vec<Warning>,
}

/// One derived stat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedValue {
    /// The computed value, when the rules know how to compute one.
    pub computed: Option<i32>,
    /// The sheet's sparse override, when present.
    pub override_value: Option<OverrideValue>,
    /// `override_value` when present, else the computed integer; `None` when
    /// neither exists. The only value a client shows as "the" value.
    pub effective: Option<OverrideValue>,
    /// The contributions that produced `computed`, in application order.
    pub breakdown: Vec<Contribution>,
    /// Net advantage/disadvantage from contributions; `None` when no hint
    /// contribution applied, `Some(RollHint::Normal)` when they cancelled.
    pub hint: Option<RollHint>,
}

/// One contribution to a derived value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contribution {
    /// Where the contribution came from.
    pub source: ContributionSource,
    /// Display attribution: `modifier.note`, else the source's name.
    pub label: String,
    /// The operation the source applied.
    pub op: ContributionOp,
    /// The operand; `None` for advantage/disadvantage.
    pub value: Option<i32>,
}

/// The origin of one contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContributionSource {
    /// The rules' own base value.
    Base,
    /// One inventory entry.
    Item(InventoryEntryId),
    /// One feature entry.
    Feature(FeatureEntryId),
    /// One status.
    Status(StatusId),
}

/// What a contribution does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContributionOp {
    /// Replace the running value.
    Set,
    /// Add to the running value.
    Add,
    /// Clamp to at least the operand.
    Min,
    /// Clamp to at most the operand.
    Max,
    /// Grant advantage (a roll hint, not a number).
    Advantage,
    /// Grant disadvantage (a roll hint, not a number).
    Disadvantage,
}

/// A tracked resource's derived maximum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedResource {
    /// The evaluated maximum; `None` when the formula is unevaluable here.
    pub max: Option<i32>,
    /// The evaluated formula as a single `Set` contribution, or empty when
    /// unevaluable.
    pub breakdown: Vec<Contribution>,
}

/// Spell-slot capacity from the class tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotMaxima {
    /// Regular slots by spell level, 1 to 9.
    pub spell: BTreeMap<u8, i32>,
    /// Pact Magic, when the entity has warlock levels.
    pub pact: Option<PactSlots>,
}

/// Pact Magic capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PactSlots {
    /// Slot level.
    pub level: u8,
    /// Number of slots.
    pub count: i32,
}

/// Net advantage or disadvantage on a roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollHint {
    /// Neither, or a pairwise cancellation.
    Normal,
    /// Net advantage.
    Advantage,
    /// Net disadvantage.
    Disadvantage,
}

/// The flat part, attributed dice and hint for one roll of a stat (U5 seam).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollParts {
    /// The integer the roll starts from.
    pub flat: i32,
    /// Dice terms from applicable modifiers, in application order.
    pub dice: Vec<DiceTerm>,
    /// The net hint.
    pub hint: RollHint,
}

/// One attributed dice term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiceTerm {
    /// Display attribution from the contributing modifier.
    pub source: Option<String>,
    /// The dice expression.
    pub expr: DiceExpr,
}
