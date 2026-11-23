//! Event, warning and element DTOs (`docs/architecture.md` section 6).
//!
//! Engine events and warnings are output-only. Each DTO mirrors one engine
//! variant; an engine variant this build does not know maps to `Unknown` so a
//! future additive variant cannot silently disappear.

use localtt_core::types::{
    EntityId,
    EntityKind,
    FeatureEntryId,
    FeedEntryId,
    InventoryEntryId,
    ResourceId,
    SpellEntryId,
    SpellcastingBlockId,
    StatusId,
};
use serde::{Deserialize, Serialize};

/// One element inside an entity document, for scoped warnings and errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ElementRefDto {
    /// A status inside the entity's `statuses`.
    Status {
        /// The status id.
        id: StatusId,
    },
    /// A resource inside the entity's `resources`.
    Resource {
        /// The resource id.
        id: ResourceId,
    },
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
    /// One spell entry.
    Spell {
        /// The spell entry id.
        id: SpellEntryId,
    },
    /// One spellcasting block.
    SpellcastingBlock {
        /// The block id.
        id: SpellcastingBlockId,
    },
    /// An engine element kind this build does not know.
    #[serde(other)]
    Unknown,
}

/// Machine-readable warning kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WarningCodeDto {
    /// A spend left the resource below zero.
    ResourceOverspent,
    /// Damage left hit points below zero.
    NegativeHp,
    /// A concentration status was added while one was already active.
    MultipleConcentration,
    /// A clock move crossed at least one dawn instant backwards.
    DawnRewind,
    /// A formula was unevaluable where it had to be evaluated.
    FormulaUnevaluated,
    /// A dice leaf was flattened to its flat part in a static context.
    DiceInStaticContext,
    /// A content reference did not resolve to a usable entry.
    UnresolvedReference,
    /// More than one applicable body armor, or more than one shield.
    OverArmored,
    /// More than three attuned inventory entries.
    OverAttunement,
    /// An equipped armor's strength requirement is unmet.
    StrengthRequirement,
    /// An inventory entry with non-positive quantity was skipped.
    ZeroQuantity,
    /// A custom condition was not modeled.
    UnmodeledCondition,
    /// A class level named a class the static tables do not model.
    UnmodeledClass,
    /// A cast asked for concentration on a spell that does not concentrate.
    ConcentrationUnsupported,
    /// An engine warning kind this build does not know.
    #[serde(other)]
    Unknown,
}

/// A non-blocking rule outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningDto {
    /// The machine-readable kind.
    pub code: WarningCodeDto,
    /// A short human-readable sentence; display-only, not contractual.
    pub message: String,
    /// The affected entity, when entity-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// The affected element, when element-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementRefDto>,
}

/// One committed-change event from the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum EventDto {
    /// One entity's document was replaced.
    EntityChanged {
        /// The entity.
        id: EntityId,
        /// The new revision.
        rev: u64,
        /// The entity kind, for client routing.
        kind: EntityKind,
    },
    /// An entity was removed.
    EntityRemoved {
        /// The removed entity.
        id: EntityId,
    },
    /// The campaign document changed.
    CampaignChanged,
    /// Feed entries were appended; ascending ids, never empty.
    FeedAppended {
        /// The appended ids, ascending.
        ids: Vec<FeedEntryId>,
    },
    /// An engine event kind this build does not know.
    #[serde(other)]
    Unknown,
}
