//! Values the actor produces after a commit or a read.
//!
//! These are the owned results the actor hands to its drivers (the REST
//! adapter and the TUI). They are not transport shapes: the REST and WebSocket
//! adapters convert them into the protocol's `*Dto` types at the edge, and the
//! TUI reads them directly. [`StreamEvent`] carries ids so a subscriber
//! refetches through its own authorization; [`CommandOutcome`] and [`FeedPage`]
//! carry views already projected for the acting or reading viewer.

use localtt_core::types::{EntityId, FeedEntryId};
use localtt_engine::{
    authz::{CampaignView, EntityView, FeedEntryView},
    executor::Executed,
};
use localtt_protocol::{ActorStatsDto, CommandOutcomeDto, FeedPageDto, StreamEventDto};

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
    /// The credential behind this stream is no longer valid; stop reconnecting.
    Revoked,
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

/// Counters and queue state of the single-writer actor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorStats {
    /// Every actor request handled since startup.
    pub requests: u64,
    /// Requests that completed with an error.
    pub errors: u64,
    /// Command executions.
    pub commands: u64,
    /// Read requests, including campaign, entity, feed and sheet reads.
    pub reads: u64,
    /// Store durability flushes.
    pub saves: u64,
    /// Live projected-event subscribers.
    pub subscribers: usize,
    /// Requests waiting in the actor channel when the stats were sampled.
    pub queue_depth: usize,
    /// The actor channel's configured capacity.
    pub queue_capacity: usize,
}

impl From<StreamEvent> for StreamEventDto {
    #[inline]
    fn from(event: StreamEvent) -> Self {
        match event {
            StreamEvent::EntityChanged(id) => Self::EntityChanged { id },
            StreamEvent::Forgotten(id) => Self::Forgotten { id },
            StreamEvent::CampaignChanged => Self::CampaignChanged,
            StreamEvent::FeedAppended(id) => Self::FeedAppended { id },
            StreamEvent::Revoked => Self::Revoked,
            StreamEvent::Lagged => Self::Lagged,
        }
    }
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

impl From<FeedPage> for FeedPageDto {
    #[inline]
    fn from(page: FeedPage) -> Self {
        Self {
            entries: page.entries,
            trimmed_before: page.trimmed_before,
        }
    }
}

impl From<ActorStats> for ActorStatsDto {
    #[inline]
    fn from(stats: ActorStats) -> Self {
        Self {
            requests: stats.requests,
            errors: stats.errors,
            commands: stats.commands,
            reads: stats.reads,
            saves: stats.saves,
            subscribers: stats.subscribers,
            queue_depth: stats.queue_depth,
            queue_capacity: stats.queue_capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::types::{EntityKind, Id};
    use localtt_engine::{
        commands::Event,
        error::{Warning, WarningCode},
    };
    use serde_json::json;

    use super::*;

    fn entity_id() -> EntityId {
        Id::from_uuid(uuid::Uuid::from_u128(0x11))
    }

    #[test]
    fn outcome_feed_and_stream_golden_json() {
        let outcome = CommandOutcome {
            executed: Executed {
                warnings: vec![Warning {
                    code: WarningCode::MultipleConcentration,
                    message: "already concentrating".to_owned(),
                    entity: Some(entity_id()),
                    element: None,
                }],
                events: vec![Event::EntityChanged {
                    id: entity_id(),
                    rev: 3,
                    kind: EntityKind::Pc,
                }],
            },
            views: Vec::new(),
            campaign: None,
            feed: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(CommandOutcomeDto::from(outcome)).expect("the outcome serializes"),
            json!({
                "executed": {
                    "warnings": [{
                        "code": "multiple_concentration",
                        "message": "already concentrating",
                        "entity": entity_id()
                    }],
                    "events": [{
                        "type": "entity_changed",
                        "id": entity_id(),
                        "rev": 3,
                        "kind": "pc"
                    }]
                },
                "views": [],
                "feed": []
            })
        );

        let page = FeedPageDto::from(FeedPage {
            entries: Vec::new(),
            trimmed_before: Some(FeedEntryId::new(7)),
        });
        assert_eq!(
            serde_json::to_value(page).expect("the page serializes"),
            json!({"entries": [], "trimmed_before": 7})
        );

        let stream_cases = [
            (
                StreamEvent::EntityChanged(entity_id()),
                json!({"type": "entity_changed", "id": entity_id()}),
            ),
            (
                StreamEvent::Forgotten(entity_id()),
                json!({"type": "forgotten", "id": entity_id()}),
            ),
            (
                StreamEvent::CampaignChanged,
                json!({"type": "campaign_changed"}),
            ),
            (
                StreamEvent::FeedAppended(FeedEntryId::new(9)),
                json!({"type": "feed_appended", "id": 9}),
            ),
            (StreamEvent::Revoked, json!({"type": "revoked"})),
            (StreamEvent::Lagged, json!({"type": "lagged"})),
        ];
        for (event, expected) in stream_cases {
            assert_eq!(
                serde_json::to_value(StreamEventDto::from(event))
                    .expect("the stream event serializes"),
                expected
            );
        }
    }

    #[test]
    fn actor_stats_map_field_for_field() {
        let stats = ActorStats {
            requests: 12,
            errors: 1,
            commands: 4,
            reads: 6,
            saves: 1,
            subscribers: 2,
            queue_depth: 3,
            queue_capacity: 64,
        };

        let dto = ActorStatsDto::from(stats);

        assert_eq!(
            serde_json::to_value(dto).expect("the DTO serializes"),
            json!({
                "requests": 12,
                "errors": 1,
                "commands": 4,
                "reads": 6,
                "saves": 1,
                "subscribers": 2,
                "queue_depth": 3,
                "queue_capacity": 64
            })
        );
    }
}
