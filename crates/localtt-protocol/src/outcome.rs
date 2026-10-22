//! Response and subscription DTOs (`docs/rfcs/server.md` 4.3, 4.4 and 7).
//!
//! These map the actor's owned results to the HTTP/WebSocket shapes. Per-viewer
//! entity and feed views are already `Serialize`; only the engine's warning and
//! event values need their own DTOs.

use localtt_core::types::{EntityId, FeedEntryId};
use localtt_engine::{
    authz::{CampaignView, EntityView, FeedEntryView},
    executor::Executed,
};
use serde::{Deserialize, Serialize};

use super::event::{EventDto, WarningDto};
use crate::model::{CommandOutcome, FeedPage, StreamEvent};

/// The warning and event parts of a command result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutedDto {
    /// Non-blocking rule outcomes.
    pub warnings: Vec<WarningDto>,
    /// The ordered delivery hint.
    pub events: Vec<EventDto>,
}

impl From<Executed> for ExecutedDto {
    #[inline]
    fn from(executed: Executed) -> Self {
        Self {
            warnings: executed
                .warnings
                .into_iter()
                .map(WarningDto::from)
                .collect(),
            events: executed.events.into_iter().map(EventDto::from).collect(),
        }
    }
}

/// The acting client's post-commit projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcomeDto {
    /// Warnings and delivery hints.
    pub executed: ExecutedDto,
    /// Fresh views of entities the actor can still read.
    pub views: Vec<EntityView>,
    /// The fresh campaign view when the campaign changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub campaign: Option<CampaignView>,
    /// Fresh views of the visible entries appended by this command.
    pub feed: Vec<FeedEntryView>,
}

impl From<CommandOutcome> for CommandOutcomeDto {
    #[inline]
    fn from(outcome: CommandOutcome) -> Self {
        Self {
            executed: outcome.executed.into(),
            views: outcome.views,
            campaign: outcome.campaign,
            feed: outcome.feed,
        }
    }
}

/// One ascending feed page plus the ring-trim signal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPageDto {
    /// Visible entries after the cursor, ascending.
    pub entries: Vec<FeedEntryView>,
    /// Oldest retained id when the cursor sits below the ring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trimmed_before: Option<FeedEntryId>,
}

impl From<FeedPage> for FeedPageDto {
    #[inline]
    fn from(page: FeedPage) -> Self {
        Self {
            entries: page.entries,
            trimmed_before: page.trimmed_before,
        }
    }
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
    /// The subscriber fell behind; drop local projections and resync.
    Lagged,
    /// A stream event kind this build does not know.
    #[serde(other)]
    Unknown,
}

impl From<StreamEvent> for StreamEventDto {
    #[inline]
    fn from(event: StreamEvent) -> Self {
        match event {
            StreamEvent::EntityChanged(id) => Self::EntityChanged { id },
            StreamEvent::Forgotten(id) => Self::Forgotten { id },
            StreamEvent::CampaignChanged => Self::CampaignChanged,
            StreamEvent::FeedAppended(id) => Self::FeedAppended { id },
            StreamEvent::Lagged => Self::Lagged,
        }
    }
}
