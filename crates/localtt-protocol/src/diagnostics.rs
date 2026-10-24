//! The gated diagnostics DTO (`docs/rfcs/server.md` 7).
//!
//! Diagnostics are authenticated and opt-in. The shape names the campaign and
//! exposes actor counters without leaking tokens, paths or command payloads.

use localtt_core::types::{CampaignId, CampaignTime};
use localtt_engine::authz::CampaignView;
use serde::{Deserialize, Serialize};

/// Counters and queue state for the single-writer actor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorStatsDto {
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
    /// Requests waiting in the actor channel when diagnostics were sampled.
    pub queue_depth: usize,
    /// The actor channel's configured capacity.
    pub queue_capacity: usize,
}

/// Server version, campaign identity, uptime and actor counters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsDto {
    /// The server build version.
    pub version: String,
    /// Campaign id.
    pub campaign_id: CampaignId,
    /// Campaign display name.
    pub campaign_name: String,
    /// Campaign clock.
    pub clock: CampaignTime,
    /// Whole seconds since the actor started.
    pub uptime_s: u64,
    /// Actor counters sampled for this response.
    pub actor: ActorStatsDto,
}

impl DiagnosticsDto {
    /// Builds the diagnostics shape from one authenticated campaign view.
    #[must_use]
    #[inline]
    pub fn new(campaign: &CampaignView, uptime_s: u64, actor: ActorStatsDto) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            campaign_id: campaign.id,
            campaign_name: campaign.name.clone(),
            clock: campaign.clock,
            uptime_s,
            actor,
        }
    }
}
