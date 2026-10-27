//! Roster and identity commands (M5).
//!
//! These commands are DM-only. Token minting and verification stay in the
//! server; the engine stores only the hash on the player seat.

use localtt_core::types::{PlayerId, TokenHash};

use crate::{
    commands::{CommandEnv, Transition},
    error::EngineError,
    state::WorkingSet,
};

/// Adds a player seat; the id must be fresh and the name trimmed and bounded.
pub(crate) fn add_player_command(
    _state: &dyn WorkingSet,
    _id: PlayerId,
    _display_name: &str,
    _token_hash: Option<&TokenHash>,
    _env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    todo!()
}

/// Replaces or clears a player seat's invite hash.
pub(crate) fn set_player_token_command(
    _state: &dyn WorkingSet,
    _player: PlayerId,
    _token_hash: Option<&TokenHash>,
) -> Result<Transition, EngineError> {
    todo!()
}

/// Replaces a DM seat's bearer-token hash.
pub(crate) fn rotate_dm_key_command(
    _state: &dyn WorkingSet,
    _player: PlayerId,
    _token_hash: &TokenHash,
) -> Result<Transition, EngineError> {
    todo!()
}

/// Removes a player seat and transfers its owned entities and ACL grants.
pub(crate) fn remove_player_command(
    _state: &dyn WorkingSet,
    _player: PlayerId,
    _reassign_to: PlayerId,
    _env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    todo!()
}
