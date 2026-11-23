//! Response and subscription DTOs (`docs/architecture.md` section 6).
//!
//! The actor's owned results (`CommandOutcome`, `FeedPage`, `StreamEvent`) live
//! in `localtt-server`, which converts them into these shapes at the edge.

use localtt_core::types::{EntityId, FeedEntryId};
use serde::{Deserialize, Serialize};

use super::{
    event::{EventDto, WarningDto},
    view::{CampaignViewDto, EntityViewDto, FeedEntryViewDto},
};

/// The warning and event parts of a command result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutedDto {
    /// Non-blocking rule outcomes.
    pub warnings: Vec<WarningDto>,
    /// The ordered delivery hint.
    pub events: Vec<EventDto>,
}

/// The acting client's post-commit projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcomeDto {
    /// Warnings and delivery hints.
    pub executed: ExecutedDto,
    /// Fresh views of entities the actor can still read.
    pub views: Vec<EntityViewDto>,
    /// The fresh campaign view when the campaign changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub campaign: Option<CampaignViewDto>,
    /// Fresh views of the visible entries appended by this command.
    pub feed: Vec<FeedEntryViewDto>,
}

/// One ascending feed page plus the ring-trim signal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPageDto {
    /// Visible entries after the cursor, ascending.
    pub entries: Vec<FeedEntryViewDto>,
    /// Oldest retained id when the cursor sits below the ring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trimmed_before: Option<FeedEntryId>,
}

/// One event on a viewer's projected WebSocket stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum StreamEventDto {
    /// The entity changed; refetch its view.
    EntityChanged {
        /// The entity.
        id: EntityId,
    },
    /// The entity is gone or no longer visible to this viewer.
    Forgotten {
        /// The entity.
        id: EntityId,
    },
    /// The campaign document changed; refetch the campaign view.
    CampaignChanged,
    /// New feed entries are visible.
    FeedAppended {
        /// The newest appended feed id.
        id: FeedEntryId,
    },
    /// The credential behind this stream is no longer valid; stop reconnecting.
    Revoked,
    /// The subscriber fell behind; drop local projections and resync.
    Lagged,
    /// A stream event kind this build does not know.
    #[serde(other)]
    Unknown,
}
