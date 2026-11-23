//! The health DTO (`docs/architecture.md` section 6).
//!
//! Health is authenticated like every other API route: it names the campaign
//! and its clock, so it is not a public liveness probe.

use localtt_core::types::{CampaignId, CampaignTime};
use serde::{Deserialize, Serialize};

use crate::view::CampaignViewDto;

/// The version of the HTTP/WebSocket contract this build speaks.
///
/// Bumped when a request or response shape changes in a way an older client
/// cannot read. A server reports it in [`HealthDto::protocol_version`] and a
/// client compares it with its own copy at sign-in, so a stale browser bundle
/// is noticed instead of failing field by field.
pub const PROTOCOL_VERSION: u32 = 1;

/// Server version, protocol version, plus the campaign's id, name and clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthDto {
    /// The server build version.
    pub version: String,
    /// The contract version the server speaks ([`PROTOCOL_VERSION`]); `0` when
    /// an older server did not report one.
    #[serde(default)]
    pub protocol_version: u32,
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
    pub fn new(campaign: &CampaignViewDto) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol_version: PROTOCOL_VERSION,
            campaign_id: campaign.id,
            campaign_name: campaign.name.clone(),
            clock: campaign.clock,
        }
    }
}
