//! Roll and feed audiences and the persisted feed ring-buffer entries.

use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::types::{CampaignTime, EntityId, FeedEntryId, PlayerId, Roll, StatusId, UnixTime};

/// Role-agnostic audience of a roll or feed entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// Visible to everyone.
    Public,
    /// Visible to the roller and the DM.
    RollerAndDm,
    /// Visible to the roller only.
    RollerOnly,
}

/// One entry of the persisted feed ring buffer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedEntry {
    /// Feed position.
    pub id: FeedEntryId,
    /// Wall-clock time.
    pub ts: UnixTime,
    /// Campaign-clock time.
    pub clock_s: CampaignTime,
    /// Acting player.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<PlayerId>,
    /// Related entity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// Who sees the entry.
    pub audience: Audience,
    /// What happened.
    pub payload: FeedPayload,
}

/// Feed payload kinds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedPayload {
    /// A roll; the entry audience mirrors the roll audience.
    Roll {
        /// The roll.
        roll: Roll,
    },
    /// A timed status expired.
    StatusExpired {
        /// The status.
        status: StatusId,
        /// Status name snapshot.
        name: String,
    },
    /// Concentration ended.
    ConcentrationEnded {
        /// The concentration status name snapshot.
        name: String,
    },
    /// A system hint, e.g. a suggested concentration check.
    Hint {
        /// The hint text.
        text: String,
    },
}
