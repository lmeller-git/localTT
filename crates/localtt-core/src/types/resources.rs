//! Resources (slots, hit dice, charges) and statuses (conditions, effects, concentration).

use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::types::{CampaignTime, EntityId, Formula, Modifier, RefId, ResourceId, StatusId};

/// A tracked resource on a statblock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    /// Element id.
    pub id: ResourceId,
    /// Display name.
    pub name: String,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Maximum; a bare integer is allowed by the formula shorthand.
    pub max: Formula,
    /// Current value; signed, overspending is warned about by the engine.
    pub current: i32,
    /// Recharge trigger.
    pub recharge: Recharge,
}

/// Resource kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceKind {
    /// Spell slot of a level.
    SpellSlot {
        /// Spell level, 1 to 9.
        level: i32,
    },
    /// Warlock pact slot.
    PactSlot,
    /// Hit die.
    HitDie {
        /// Die sides, 4 to 12.
        sides: i32,
    },
    /// Generic feature resource.
    Feature,
    /// Item charge resource.
    Item,
    /// Anything else.
    Other,
}

/// Recharge trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recharge {
    /// On a short rest.
    Short,
    /// On a long rest.
    Long,
    /// At dawn.
    Dawn,
    /// Manually.
    Manual,
    /// Never.
    Never,
}

/// A status effect on an entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Element id.
    pub id: StatusId,
    /// Status kind.
    pub kind: StatusKind,
    /// Display name.
    pub name: String,
    /// Content reference, e.g. `srd2014:condition/prone`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Inline description for custom statuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// When the status started, on the campaign clock.
    pub started_at: CampaignTime,
    /// How the status ends.
    pub duration: StatusDuration,
    /// Who applied it and/or the concentration status it hangs off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<StatusSource>,
    /// Modifiers applied when the engine composes a value for this entity:
    /// static derived values use the flat part, rolls add dice terms
    /// attributed to this status.
    pub modifiers: Vec<Modifier>,
}

/// Status kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// A named condition such as prone.
    Condition,
    /// A spell or ability effect such as Bless.
    Effect,
    /// The one concentration effect an entity can hold.
    Concentration,
    /// Table-provided status.
    Custom,
}

/// How a status ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusDuration {
    /// Absolute expiry on the campaign clock: expired when `now >= expires_at`.
    Timed {
        /// Expiry time.
        expires_at: CampaignTime,
    },
    /// Cleared by a rest.
    UntilRest {
        /// The rest kind that clears it.
        rest: RestKind,
    },
    /// Never auto-expired.
    Indefinite,
    /// Cleared manually, with a note like "until the vault is opened".
    Manual {
        /// The reminder text.
        text: String,
    },
}

/// Rest kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestKind {
    /// Short rest.
    Short,
    /// Long rest.
    Long,
}

/// Who applied a status and/or the concentration status it hangs off; both
/// optional, absent means unknown or table-provided.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSource {
    /// The caster entity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// The concentration status this rider hangs off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusId>,
}
