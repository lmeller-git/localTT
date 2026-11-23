//! The derived-sheet DTO (`docs/architecture.md` section 6).
//!
//! `DerivedSheet` is an engine value type without serde derives. This module
//! owns the transport form, including the contribution breakdown, so the web
//! client can show computed values and their sources without coupling to the
//! engine's Rust types.

use std::collections::BTreeMap;

use localtt_core::types::{
    Ability,
    DamageType,
    DiceExpr,
    EntityId,
    FeatureEntryId,
    InventoryEntryId,
    OverrideValue,
    ResourceId,
    StatKey,
    StatusId,
};
use serde::{Deserialize, Serialize};

use super::event::WarningDto;

/// One statblock's derived sheet at the campaign instant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetViewDto {
    /// The entity the sheet belongs to.
    pub entity: EntityId,
    /// The entity revision the derivation read.
    pub rev: u64,
    /// Computed values, resources, slots, hit dice and warnings.
    pub sheet: DerivedSheetDto,
}

/// The static sheet of one entity at one instant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedSheetDto {
    /// Derived stat values by canonical key.
    pub values: BTreeMap<StatKey, DerivedValueDto>,
    /// Derived maxima for tracked resources, keyed by element id.
    pub resources: BTreeMap<ResourceId, DerivedResourceDto>,
    /// Spell-slot capacity from the class tables.
    pub slots: SlotMaximaDto,
    /// Hit-dice pools by die sides.
    pub hit_dice: BTreeMap<u8, i32>,
    /// Non-blocking rule outcomes, entity-scoped.
    pub warnings: Vec<WarningDto>,
    /// Equipped weapon attacks in inventory order.
    #[serde(default)]
    pub attacks: Vec<DerivedAttackDto>,
}

/// One derived stat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedValueDto {
    /// The computed value, when the rules know how to compute one.
    pub computed: Option<i32>,
    /// The sheet's sparse override, when present.
    pub override_value: Option<OverrideValue>,
    /// The value a client shows as "the" value.
    pub effective: Option<OverrideValue>,
    /// The contributions that produced `computed`, in application order.
    pub breakdown: Vec<ContributionDto>,
    /// Net advantage or disadvantage from contributions.
    pub hint: Option<RollHintDto>,
}

/// One contribution to a derived value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContributionDto {
    /// Where the contribution came from.
    pub source: ContributionSourceDto,
    /// Display attribution.
    pub label: String,
    /// The operation the source applied.
    pub op: ContributionOpDto,
    /// The operand; absent for advantage or disadvantage.
    pub value: Option<i32>,
}

/// The origin of one contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContributionSourceDto {
    /// The rules' own base value.
    Base,
    /// One inventory entry.
    Item {
        /// The inventory entry id.
        id: InventoryEntryId,
    },
    /// One feature entry.
    Feature {
        /// The feature entry id.
        id: FeatureEntryId,
    },
    /// One status.
    Status {
        /// The status id.
        id: StatusId,
    },
}

/// What a contribution does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionOpDto {
    /// Replace the running value.
    Set,
    /// Add to the running value.
    Add,
    /// Clamp to at least the operand.
    Min,
    /// Clamp to at most the operand.
    Max,
    /// Grant advantage.
    Advantage,
    /// Grant disadvantage.
    Disadvantage,
}

/// A tracked resource's derived maximum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedResourceDto {
    /// The evaluated maximum; absent when unevaluable.
    pub max: Option<i32>,
    /// The evaluated formula as a contribution, or empty.
    pub breakdown: Vec<ContributionDto>,
}

/// Spell-slot capacity from the class tables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotMaximaDto {
    /// Regular slots by spell level, 1 to 9.
    pub spell: BTreeMap<u8, i32>,
    /// Pact Magic capacity, when present.
    pub pact: Option<PactSlotsDto>,
}

/// Pact Magic capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PactSlotsDto {
    /// Slot level.
    pub level: u8,
    /// Number of slots.
    pub count: i32,
}

/// Net advantage or disadvantage on a roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollHintDto {
    /// Neither, or a pairwise cancellation.
    Normal,
    /// Net advantage.
    Advantage,
    /// Net disadvantage.
    Disadvantage,
}

/// The wire form of a weapon attack's melee/ranged kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponAttackKindDto {
    /// A melee attack.
    Melee,
    /// A ranged attack.
    Ranged,
}

/// One equipped weapon's derived attack line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedAttackDto {
    /// The inventory entry that produced this line.
    pub entry: InventoryEntryId,
    /// The resolved item's display name.
    pub name: String,
    /// Melee or ranged.
    pub kind: WeaponAttackKindDto,
    /// The ability that adds its modifier to attack and damage.
    pub ability: Ability,
    /// Ability modifier plus proficiency plus applicable attack modifiers.
    pub attack_bonus: i32,
    /// The weapon's damage dice, without the ability modifier.
    pub damage: DiceExpr,
    /// The ability modifier added to each damage line.
    pub damage_bonus: i32,
    /// The versatile damage dice, when the weapon has them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versatile_damage: Option<DiceExpr>,
    /// The weapon's damage type, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<DamageType>,
    /// Normal and long range in feet, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_ft: Option<(i32, i32)>,
    /// Net advantage or disadvantage from attack contributions.
    pub hint: RollHintDto,
    /// The attack-bonus contributions in application order.
    pub breakdown: Vec<ContributionDto>,
}
