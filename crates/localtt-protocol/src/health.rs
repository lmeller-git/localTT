//! The health DTO (`docs/rfcs/server.md` 7).
//!
//! Health is authenticated like every other API route: it names the campaign
//! and its clock, so it is not a public liveness probe.

use localtt_core::types::{CampaignId, CampaignTime};
use localtt_engine::authz::CampaignView;
use serde::{Deserialize, Serialize};

/// Server version plus the campaign's id, name and clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthDto {
    /// The server build version.
    pub version: String,
    /// Campaign id.
    pub campaign_id: CampaignId,
    /// Campaign display name.
    pub campaign_name: String,
    /// Campaign clock.
    pub clock: CampaignTime,
}

impl HealthDto {
    /// Builds the health shape from one authenticated campaign view.
    #[must_use]
    #[inline]
    pub fn new(campaign: &CampaignView) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            campaign_id: campaign.id,
            campaign_name: campaign.name.clone(),
            clock: campaign.clock,
        }
    }
}
