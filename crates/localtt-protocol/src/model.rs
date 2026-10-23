//! Projected values shared by the server and its clients.
//!
//! These are not transport shapes: they are the owned results the actor produces
//! after a commit. [`StreamEvent`] carries ids so a client refetches through its
//! own authorization; [`CommandOutcome`] and [`FeedPage`] carry already-projected
//! views for the acting or reading client.

use localtt_core::types::{EntityId, FeedEntryId};
use localtt_engine::{
    authz::{CampaignView, EntityView, FeedEntryView},
    executor::Executed,
};

/// One event on a viewer's projected stream.
///
/// Events carry ids, not documents: the client refetches with its own
/// authorization. [`Self::Forgotten`] tells a viewer that an entity it
/// previously knew about is now removed or no longer readable.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamEvent {
    /// The entity changed; refetch its view.
    EntityChanged(EntityId),
    /// The entity is gone or no longer visible to this viewer.
    Forgotten(EntityId),
    /// The campaign document changed; refetch the campaign view.
    CampaignChanged,
    /// New feed entries are visible; fetch them after the client's cursor.
    FeedAppended(FeedEntryId),
    /// The subscriber fell behind; drop local projections and resync.
    Lagged,
}

/// The acting client's post-commit projection.
///
/// Views are scoped to the actor. Other subscribers get id-only
/// [`StreamEvent`]s and refetch through their own viewer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    /// Warnings and delivery hints returned by the engine.
    pub executed: Executed,
    /// Fresh views of entities the actor can still read.
    pub views: Vec<EntityView>,
    /// The fresh campaign view when the campaign document changed.
    pub campaign: Option<CampaignView>,
    /// Fresh views of the visible entries appended by this command.
    pub feed: Vec<FeedEntryView>,
}

/// One ascending feed page plus the ring-trim signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedPage {
    /// Visible entries after the cursor, ascending.
    pub entries: Vec<FeedEntryView>,
    /// Oldest retained id when the cursor sits below the ring, so the client
    /// knows entries between its cursor and the retained window were lost.
    pub trimmed_before: Option<FeedEntryId>,
}
