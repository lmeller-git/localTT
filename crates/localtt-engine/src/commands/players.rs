//! Roster and identity commands (M5).
//!
//! These commands are DM-only. Token minting and verification stay in the
//! server; the engine stores only the hash on the player seat.

use localtt_core::types::{Player, PlayerId, Role, TokenHash};

use crate::{
    commands::{CommandEnv, Event, Transition},
    error::EngineError,
    state::WorkingSet,
};

/// The largest roster the engine accepts, counting every seat.
const MAX_PLAYERS: usize = 64;

/// The longest display name, in characters after trimming.
const MAX_DISPLAY_NAME: usize = 80;

/// Adds a player seat; the id must be fresh and the name trimmed and bounded.
pub(crate) fn add_player_command(
    state: &dyn WorkingSet,
    id: PlayerId,
    display_name: &str,
    token_hash: Option<&TokenHash>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let name = display_name.trim();
    if name.is_empty() {
        return Err(EngineError::Malformed {
            reason: "a player display name must not be blank".to_owned(),
        });
    }
    if name.chars().count() > MAX_DISPLAY_NAME {
        return Err(EngineError::TooLarge {
            what: "display name",
            limit: limit(MAX_DISPLAY_NAME),
        });
    }
    if state.campaign().players.len() >= MAX_PLAYERS {
        return Err(EngineError::TooLarge {
            what: "players",
            limit: limit(MAX_PLAYERS),
        });
    }
    let mut campaign = state.campaign().clone();
    if campaign.players.iter().any(|player| player.id == id) {
        return Err(EngineError::PlayerExists { player: id });
    }
    if let Some(hash) = token_hash
        && let Some(holder) = campaign
            .players
            .iter()
            .find(|player| player.token_hash.as_ref() == Some(hash))
    {
        return Err(EngineError::PlayerTokenInUse { player: holder.id });
    }
    campaign.players.push(Player {
        id,
        display_name: name.to_owned(),
        role: Role::Player,
        token_hash: token_hash.cloned(),
        created_at: env.now(),
        last_seen: None,
    });
    Ok(Transition {
        campaign: Some(campaign),
        upserts: Vec::new(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
        warnings: Vec::new(),
        events: vec![Event::CampaignChanged],
    })
}

/// Narrows a compile-time roster cap to the error taxonomy's `u32`.
fn limit(cap: usize) -> u32 {
    u32::try_from(cap).expect("a roster cap fits u32")
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

#[cfg(test)]
mod tests {
    use localtt_core::{builders::CampaignBuilder, types::UnixTime};

    use super::*;
    use crate::{ports::store::CampaignSnapshot, state::LoadedCampaign};

    const FIXED_TIME: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }
    }

    fn state() -> LoadedCampaign {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        builder.dm("DM");
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign: builder.build(),
            entities: Vec::new(),
            feed: Vec::new(),
        })
    }

    fn hash(byte: char) -> TokenHash {
        TokenHash::parse(&byte.to_string().repeat(64)).expect("lowercase hex is a valid hash")
    }

    #[test]
    fn add_player_trims_the_name_and_stamps_the_seat() {
        // Arrange
        let state = state();
        let id = PlayerId::new();
        let token = hash('a');

        // Act
        let transition = add_player_command(&state, id, "  Bea  ", Some(&token), &mut FixedEnv)
            .expect("a fresh seat with a unique hash is legal");

        // Assert
        let campaign = transition.campaign.expect("the roster changed");
        let player = campaign
            .players
            .iter()
            .find(|player| player.id == id)
            .expect("the seat was appended");
        assert_eq!(player.display_name, "Bea");
        assert_eq!(player.role, Role::Player);
        assert_eq!(player.token_hash, Some(token));
        assert_eq!(player.created_at, UnixTime::from_seconds(FIXED_TIME));
        assert_eq!(player.last_seen, None);
        assert_eq!(transition.events, vec![Event::CampaignChanged]);
    }

    #[test]
    fn add_player_refuses_a_blank_name() {
        // Arrange
        let state = state();

        // Act
        let result = add_player_command(&state, PlayerId::new(), "   ", None, &mut FixedEnv);

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }
}
