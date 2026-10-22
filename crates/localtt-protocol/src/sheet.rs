//! The derived-sheet DTO (`docs/rfcs/server.md` 4.3 and 7).
//!
//! `DerivedSheet` is an engine value type without serde derives. This module
//! owns the transport form, including the contribution breakdown, so the web
//! client can show computed values and their sources without coupling to the
//! engine's Rust types.

use std::collections::BTreeMap;

use localtt_core::types::{
    EntityId,
    FeatureEntryId,
    InventoryEntryId,
    OverrideValue,
    ResourceId,
    StatKey,
    StatusId,
};
use localtt_engine::{
    rules::{
        Contribution,
        ContributionOp,
        ContributionSource,
        DerivedResource,
        DerivedSheet,
        DerivedValue,
        PactSlots,
        RollHint,
        SlotMaxima,
    },
    services::SheetView,
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

impl From<SheetView> for SheetViewDto {
    #[inline]
    fn from(view: SheetView) -> Self {
        Self {
            entity: view.entity,
            rev: view.rev,
            sheet: view.sheet.into(),
        }
    }
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
}

impl From<DerivedSheet> for DerivedSheetDto {
    #[inline]
    fn from(sheet: DerivedSheet) -> Self {
        Self {
            values: sheet
                .values
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
            resources: sheet
                .resources
                .into_iter()
                .map(|(id, resource)| (id, resource.into()))
                .collect(),
            slots: sheet.slots.into(),
            hit_dice: sheet.hit_dice,
            warnings: sheet.warnings.into_iter().map(WarningDto::from).collect(),
        }
    }
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

impl From<DerivedValue> for DerivedValueDto {
    #[inline]
    fn from(value: DerivedValue) -> Self {
        Self {
            computed: value.computed,
            override_value: value.override_value,
            effective: value.effective,
            breakdown: value.breakdown.into_iter().map(Into::into).collect(),
            hint: value.hint.map(RollHintDto::from),
        }
    }
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

impl From<Contribution> for ContributionDto {
    #[inline]
    fn from(contribution: Contribution) -> Self {
        Self {
            source: contribution.source.into(),
            label: contribution.label,
            op: contribution.op.into(),
            value: contribution.value,
        }
    }
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

impl From<ContributionSource> for ContributionSourceDto {
    #[inline]
    fn from(source: ContributionSource) -> Self {
        match source {
            ContributionSource::Base => Self::Base,
            ContributionSource::Item(id) => Self::Item { id },
            ContributionSource::Feature(id) => Self::Feature { id },
            ContributionSource::Status(id) => Self::Status { id },
        }
    }
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

impl From<ContributionOp> for ContributionOpDto {
    #[inline]
    fn from(op: ContributionOp) -> Self {
        match op {
            ContributionOp::Set => Self::Set,
            ContributionOp::Add => Self::Add,
            ContributionOp::Min => Self::Min,
            ContributionOp::Max => Self::Max,
            ContributionOp::Advantage => Self::Advantage,
            ContributionOp::Disadvantage => Self::Disadvantage,
        }
    }
}

/// A tracked resource's derived maximum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedResourceDto {
    /// The evaluated maximum; absent when unevaluable.
    pub max: Option<i32>,
    /// The evaluated formula as a contribution, or empty.
    pub breakdown: Vec<ContributionDto>,
}

impl From<DerivedResource> for DerivedResourceDto {
    #[inline]
    fn from(resource: DerivedResource) -> Self {
        Self {
            max: resource.max,
            breakdown: resource.breakdown.into_iter().map(Into::into).collect(),
        }
    }
}

/// Spell-slot capacity from the class tables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotMaximaDto {
    /// Regular slots by spell level, 1 to 9.
    pub spell: BTreeMap<u8, i32>,
    /// Pact Magic capacity, when present.
    pub pact: Option<PactSlotsDto>,
}

impl From<SlotMaxima> for SlotMaximaDto {
    #[inline]
    fn from(slots: SlotMaxima) -> Self {
        Self {
            spell: slots.spell,
            pact: slots.pact.map(PactSlotsDto::from),
        }
    }
}

/// Pact Magic capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PactSlotsDto {
    /// Slot level.
    pub level: u8,
    /// Number of slots.
    pub count: i32,
}

impl From<PactSlots> for PactSlotsDto {
    #[inline]
    fn from(slots: PactSlots) -> Self {
        Self {
            level: slots.level,
            count: slots.count,
        }
    }
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

impl From<RollHint> for RollHintDto {
    #[inline]
    fn from(hint: RollHint) -> Self {
        match hint {
            RollHint::Normal => Self::Normal,
            RollHint::Advantage => Self::Advantage,
            RollHint::Disadvantage => Self::Disadvantage,
        }
    }
}
