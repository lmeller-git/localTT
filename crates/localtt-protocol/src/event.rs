//! Event, warning and element DTOs (`docs/rfcs/server.md` 4.3).
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
    StatusId,
};
use localtt_engine::{
    commands::Event,
    error::{ElementRef, Warning, WarningCode},
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
    /// An engine element kind this build does not know.
    #[serde(other)]
    Unknown,
}

impl From<ElementRef> for ElementRefDto {
    #[inline]
    fn from(element: ElementRef) -> Self {
        match element {
            ElementRef::Status(id) => Self::Status { id },
            ElementRef::Resource(id) => Self::Resource { id },
            ElementRef::Item(id) => Self::Item { id },
            ElementRef::Feature(id) => Self::Feature { id },
            ElementRef::Spell(id) => Self::Spell { id },
            _ => Self::Unknown,
        }
    }
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

impl From<WarningCode> for WarningCodeDto {
    #[inline]
    fn from(code: WarningCode) -> Self {
        match code {
            WarningCode::ResourceOverspent => Self::ResourceOverspent,
            WarningCode::NegativeHp => Self::NegativeHp,
            WarningCode::MultipleConcentration => Self::MultipleConcentration,
            WarningCode::DawnRewind => Self::DawnRewind,
            WarningCode::FormulaUnevaluated => Self::FormulaUnevaluated,
            WarningCode::DiceInStaticContext => Self::DiceInStaticContext,
            WarningCode::UnresolvedReference => Self::UnresolvedReference,
            WarningCode::OverArmored => Self::OverArmored,
            WarningCode::OverAttunement => Self::OverAttunement,
            WarningCode::StrengthRequirement => Self::StrengthRequirement,
            WarningCode::ZeroQuantity => Self::ZeroQuantity,
            WarningCode::UnmodeledCondition => Self::UnmodeledCondition,
            WarningCode::UnmodeledClass => Self::UnmodeledClass,
            WarningCode::ConcentrationUnsupported => Self::ConcentrationUnsupported,
            _ => Self::Unknown,
        }
    }
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

impl From<Warning> for WarningDto {
    #[inline]
    fn from(warning: Warning) -> Self {
        Self {
            code: warning.code.into(),
            message: warning.message,
            entity: warning.entity,
            element: warning.element.map(ElementRefDto::from),
        }
    }
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

impl From<Event> for EventDto {
    #[inline]
    fn from(event: Event) -> Self {
        match event {
            Event::EntityChanged { id, rev, kind } => Self::EntityChanged { id, rev, kind },
            Event::EntityRemoved { id } => Self::EntityRemoved { id },
            Event::CampaignChanged => Self::CampaignChanged,
            Event::FeedAppended { ids } => Self::FeedAppended { ids },
            _ => Self::Unknown,
        }
    }
}
