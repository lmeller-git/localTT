//! Roster administration request and response DTOs (`docs/rfcs/server.md` 7).
//!
//! These shapes are DM-only. Invite responses are the only place a raw player
//! token crosses the wire; the server persists only its hash.

use localtt_core::types::PlayerId;
use serde::{Deserialize, Serialize};

/// The body of a create-seat request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRosterEntryRequest {
    /// Display name for the new seat; the engine trims and bounds it.
    pub display_name: String,
}

/// A freshly minted player invite.
///
/// The raw token appears only in `invite_url`; the server keeps just its hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteDto {
    /// The invited seat.
    pub player: PlayerId,
    /// The join URL, with the raw token in the fragment.
    pub invite_url: String,
}
