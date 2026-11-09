//! Roster and identity commands (M5).
//!
//! These commands are DM-only. Token minting and verification stay in the
//! server; the engine stores only the hash on the player seat.

use localtt_core::types::{AnyEntity, Campaign, Player, PlayerId, Role, TokenHash};

use crate::{
    commands::{
        CommandEnv,
        Event,
        MAX_DISPLAY_NAME,
        MAX_PLAYERS,
        Transition,
        campaign_change,
        stamp,
    },
    error::EngineError,
    state::WorkingSet,
};

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
    if let Some(hash) = token_hash {
        ensure_sha256(hash)?;
    }
    let player_seats = state
        .campaign()
        .players
        .iter()
        .filter(|seat| seat.role == Role::Player)
        .count();
    if player_seats >= MAX_PLAYERS {
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
        && let Some(holder) = hash_holder(&campaign, hash, id)
    {
        return Err(EngineError::PlayerTokenInUse { player: holder });
    }
    campaign.players.push(Player {
        id,
        display_name: name.to_owned(),
        role: Role::Player,
        token_hash: token_hash.cloned(),
        created_at: env.now(),
        last_seen: None,
    });
    Ok(campaign_change(campaign))
}

/// Replaces or clears a player seat's invite hash.
///
/// Re-setting the seat's current hash is idempotent; only another seat's hash
/// is a conflict.
pub(crate) fn set_player_token_command(
    state: &dyn WorkingSet,
    player: PlayerId,
    token_hash: Option<&TokenHash>,
) -> Result<Transition, EngineError> {
    if let Some(hash) = token_hash {
        ensure_sha256(hash)?;
    }
    let mut campaign = state.campaign().clone();
    let seat = campaign
        .players
        .iter()
        .find(|seat| seat.id == player)
        .ok_or(EngineError::PlayerNotFound { player })?;
    if seat.role != Role::Player {
        return Err(EngineError::PlayerProtected { player });
    }
    if let Some(hash) = token_hash
        && let Some(holder) = hash_holder(&campaign, hash, player)
    {
        return Err(EngineError::PlayerTokenInUse { player: holder });
    }
    let seat = campaign
        .players
        .iter_mut()
        .find(|seat| seat.id == player)
        .expect("the seat was resolved above");
    seat.token_hash = token_hash.cloned();
    Ok(campaign_change(campaign))
}

/// Replaces a DM seat's bearer-token hash.
///
/// Re-setting the seat's current hash is idempotent; only another seat's hash
/// is a conflict.
pub(crate) fn rotate_dm_key_command(
    state: &dyn WorkingSet,
    player: PlayerId,
    token_hash: &TokenHash,
) -> Result<Transition, EngineError> {
    ensure_sha256(token_hash)?;
    let mut campaign = state.campaign().clone();
    let seat = campaign
        .players
        .iter()
        .find(|seat| seat.id == player)
        .ok_or(EngineError::PlayerNotFound { player })?;
    if seat.role != Role::Dm {
        return Err(EngineError::Malformed {
            reason: format!("player {player} is not a DM"),
        });
    }
    if let Some(holder) = hash_holder(&campaign, token_hash, player) {
        return Err(EngineError::PlayerTokenInUse { player: holder });
    }
    let seat = campaign
        .players
        .iter_mut()
        .find(|seat| seat.id == player)
        .expect("the seat was resolved above");
    seat.token_hash = Some(token_hash.clone());
    Ok(campaign_change(campaign))
}

/// Removes a player seat and transfers its owned entities and ACL grants.
///
/// A missing `reassign_to` seat is [`EngineError::PlayerNotFound`]; an existing
/// non-DM target is [`EngineError::Malformed`]. Every entity document is
/// visited once, so the cost is linear in the working set.
pub(crate) fn remove_player_command(
    state: &dyn WorkingSet,
    player: PlayerId,
    reassign_to: PlayerId,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut campaign = state.campaign().clone();
    let seat = campaign
        .players
        .iter()
        .find(|seat| seat.id == player)
        .ok_or(EngineError::PlayerNotFound { player })?;
    if seat.role != Role::Player {
        return Err(EngineError::PlayerProtected { player });
    }
    let target = campaign
        .players
        .iter()
        .find(|seat| seat.id == reassign_to)
        .ok_or(EngineError::PlayerNotFound {
            player: reassign_to,
        })?;
    if target.role != Role::Dm {
        return Err(EngineError::Malformed {
            reason: format!("player {reassign_to} is not a DM"),
        });
    }
    campaign.players.retain(|seat| seat.id != player);

    let now = env.now();
    let mut upserts = Vec::new();
    for header in state.headers() {
        let Some(mut entity) = state.entity(header.id)? else {
            continue;
        };
        let owned = entity.owner() == player;
        let granted = entity.acl().iter().any(|entry| entry.player == player);
        if !owned && !granted {
            continue;
        }
        if owned {
            set_owner(&mut entity, reassign_to);
        }
        if granted {
            remove_acl_player(&mut entity, player);
        }
        stamp(&mut entity, now);
        upserts.push(entity);
    }

    let mut events = Vec::with_capacity(upserts.len() + 1);
    events.extend(upserts.iter().map(|entity| Event::EntityChanged {
        id: entity.id(),
        rev: entity.rev(),
        kind: entity.kind(),
    }));
    events.push(Event::CampaignChanged);
    Ok(Transition {
        campaign: Some(campaign),
        upserts,
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
        warnings: Vec::new(),
        events,
    })
}

/// The seat that already owns `hash`, ignoring `except`.
fn hash_holder(campaign: &Campaign, hash: &TokenHash, except: PlayerId) -> Option<PlayerId> {
    campaign
        .players
        .iter()
        .find(|seat| seat.id != except && seat.token_hash.as_ref() == Some(hash))
        .map(|seat| seat.id)
}

/// Sets the owning player on any entity kind.
fn set_owner(entity: &mut AnyEntity, owner: PlayerId) {
    match entity {
        AnyEntity::Statblock(document) => document.owner = owner,
        AnyEntity::Note(document) => document.owner = owner,
        AnyEntity::Handout(document) => document.owner = owner,
    }
}

/// Drops every ACL entry naming `player` from any entity kind.
fn remove_acl_player(entity: &mut AnyEntity, player: PlayerId) {
    match entity {
        AnyEntity::Statblock(document) => {
            document.acl.retain(|entry| entry.player != player);
        }
        AnyEntity::Note(document) => {
            document.acl.retain(|entry| entry.player != player);
        }
        AnyEntity::Handout(document) => {
            document.acl.retain(|entry| entry.player != player);
        }
    }
}

/// Rejects a digest that the M5 auth index could never match.
fn ensure_sha256(hash: &TokenHash) -> Result<(), EngineError> {
    if hash.is_sha256() {
        Ok(())
    } else {
        Err(EngineError::Malformed {
            reason: "a token hash must be a full SHA-256 digest".to_owned(),
        })
    }
}

/// Narrows a compile-time roster cap to the error taxonomy's `u32`.
fn limit(cap: usize) -> u32 {
    u32::try_from(cap).expect("a roster cap fits u32")
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::CampaignBuilder,
        types::{AclEntry, AclLevel, Entity, EntityId, EntityKind, NoteBody, UnixTime},
    };

    use super::*;
    use crate::{ports::store::CampaignSnapshot, state::LoadedCampaign};

    const FIXED_TIME: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(FIXED_TIME)
        }
    }

    fn party() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("DM");
        let player = builder.player("Alex");
        (builder.build(), dm, player)
    }

    fn state(campaign: Campaign, entities: Vec<AnyEntity>) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign,
            entities,
            feed: Vec::new(),
        })
    }

    fn hash(byte: char) -> TokenHash {
        TokenHash::parse(&byte.to_string().repeat(64)).expect("lowercase hex is a valid hash")
    }

    fn short_hash() -> TokenHash {
        TokenHash::parse("0a1b").expect("lowercase hex is valid at any length")
    }

    fn note(id: EntityId, owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                id,
                "Session 4",
                owner,
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .acl(acl)
            .build()
            .expect("a note kind pairs with a note body"),
        )
    }

    #[test]
    fn add_player_trims_the_name_and_stamps_the_seat() {
        // Arrange
        let (campaign, _, _) = party();
        let state = state(campaign, Vec::new());
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
        let (campaign, _, _) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = add_player_command(&state, PlayerId::new(), "   ", None, &mut FixedEnv);

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn add_player_refuses_a_non_sha256_hash() {
        // Arrange
        let (campaign, _, _) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = add_player_command(
            &state,
            PlayerId::new(),
            "Bea",
            Some(&short_hash()),
            &mut FixedEnv,
        );

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn max_players_counts_player_seats_only() {
        // Arrange: one short of the cap, plus extra DM seats.
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        for seat in 0..5 {
            builder.dm(format!("DM {seat}"));
        }
        for seat in 0..(MAX_PLAYERS - 1) {
            builder.player(format!("Player {seat}"));
        }
        let state = state(builder.build(), Vec::new());

        // Act
        let result = add_player_command(&state, PlayerId::new(), "Last", None, &mut FixedEnv);

        // Assert
        assert!(result.is_ok(), "DM seats do not consume the player cap");
    }

    #[test]
    fn set_player_token_sets_then_clears_the_hash() {
        // Arrange
        let (campaign, _, player) = party();
        let state = state(campaign, Vec::new());
        let token = hash('b');

        // Act
        let issued = set_player_token_command(&state, player, Some(&token))
            .expect("a player token can be issued");
        let revoked =
            set_player_token_command(&state, player, None).expect("a player token can be revoked");

        // Assert
        let issued = issued.campaign.expect("the roster changed");
        assert_eq!(
            issued
                .players
                .iter()
                .find(|seat| seat.id == player)
                .expect("the player exists")
                .token_hash,
            Some(token)
        );
        let revoked = revoked.campaign.expect("the roster changed");
        assert_eq!(
            revoked
                .players
                .iter()
                .find(|seat| seat.id == player)
                .expect("the player exists")
                .token_hash,
            None
        );
    }

    #[test]
    fn set_player_token_refuses_the_dm_seat() {
        // Arrange
        let (campaign, dm, _) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = set_player_token_command(&state, dm, Some(&hash('c')));

        // Assert
        assert!(matches!(
            result,
            Err(EngineError::PlayerProtected { player }) if player == dm
        ));
    }

    #[test]
    fn set_player_token_refuses_a_non_sha256_hash() {
        // Arrange
        let (campaign, _, player) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = set_player_token_command(&state, player, Some(&short_hash()));

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn set_player_token_allows_reissuing_the_same_hash() {
        // Arrange: the seat already owns the hash.
        let (mut campaign, _, player) = party();
        let token = hash('c');
        campaign
            .players
            .iter_mut()
            .find(|seat| seat.id == player)
            .expect("the player exists")
            .token_hash = Some(token.clone());
        let state = state(campaign, Vec::new());

        // Act
        let result = set_player_token_command(&state, player, Some(&token));

        // Assert
        assert!(result.is_ok(), "the seat's own hash is not a conflict");
    }

    #[test]
    fn rotate_dm_key_replaces_the_hash() {
        // Arrange
        let (campaign, dm, _) = party();
        let state = state(campaign, Vec::new());
        let token = hash('d');

        // Act
        let transition =
            rotate_dm_key_command(&state, dm, &token).expect("a DM key can be rotated");

        // Assert
        let campaign = transition.campaign.expect("the roster changed");
        assert_eq!(
            campaign
                .players
                .iter()
                .find(|seat| seat.id == dm)
                .expect("the DM exists")
                .token_hash,
            Some(token)
        );
    }

    #[test]
    fn rotate_dm_key_refuses_a_player_seat() {
        // Arrange
        let (campaign, _, player) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = rotate_dm_key_command(&state, player, &hash('e'));

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn rotate_dm_key_refuses_a_non_sha256_hash() {
        // Arrange
        let (campaign, dm, _) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = rotate_dm_key_command(&state, dm, &short_hash());

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn rotate_dm_key_allows_reissuing_the_same_hash() {
        // Arrange: the DM seat already owns the hash.
        let (mut campaign, dm, _) = party();
        let token = hash('f');
        campaign
            .players
            .iter_mut()
            .find(|seat| seat.id == dm)
            .expect("the DM exists")
            .token_hash = Some(token.clone());
        let state = state(campaign, Vec::new());

        // Act
        let result = rotate_dm_key_command(&state, dm, &token);

        // Assert
        assert!(result.is_ok(), "the seat's own hash is not a conflict");
    }

    #[test]
    fn remove_player_transfers_owned_entities_and_scrubs_acls() {
        // Arrange: one owned note and one shared note.
        let (campaign, dm, player) = party();
        let owned = EntityId::new();
        let shared = EntityId::new();
        let untouched = EntityId::new();
        let state = state(
            campaign,
            vec![
                note(owned, player, Vec::new()),
                note(
                    shared,
                    dm,
                    vec![AclEntry {
                        player,
                        level: AclLevel::Read,
                        field_mask: None,
                    }],
                ),
                note(untouched, dm, Vec::new()),
            ],
        );

        // Act
        let transition = remove_player_command(&state, player, dm, &mut FixedEnv)
            .expect("the DM may remove a player");

        // Assert
        let campaign = transition.campaign.expect("the roster changed");
        assert!(!campaign.players.iter().any(|seat| seat.id == player));
        let by_id = |id: EntityId| {
            transition
                .upserts
                .iter()
                .find(|entity| entity.id() == id)
                .expect("the entity was upserted")
        };
        assert_eq!(by_id(owned).owner(), dm);
        assert_eq!(by_id(owned).rev(), 1);
        assert_eq!(by_id(shared).owner(), dm);
        assert_eq!(by_id(shared).rev(), 1);
        assert!(
            by_id(shared)
                .acl()
                .iter()
                .all(|entry| entry.player != player)
        );
        assert!(
            !transition
                .upserts
                .iter()
                .any(|entity| entity.id() == untouched)
        );
        assert_eq!(transition.events.len(), 3);
        assert_eq!(transition.events.last(), Some(&Event::CampaignChanged));
    }

    #[test]
    fn remove_player_refuses_a_dm_seat() {
        // Arrange
        let (campaign, dm, _) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = remove_player_command(&state, dm, dm, &mut FixedEnv);

        // Assert
        assert!(matches!(
            result,
            Err(EngineError::PlayerProtected { player }) if player == dm
        ));
    }

    #[test]
    fn remove_player_refuses_a_non_dm_reassign_target() {
        // Arrange
        let (campaign, _, player) = party();
        let state = state(campaign, Vec::new());

        // Act
        let result = remove_player_command(&state, player, player, &mut FixedEnv);

        // Assert
        assert!(matches!(result, Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn remove_player_reports_a_missing_reassign_target() {
        // Arrange
        let (campaign, _, player) = party();
        let state = state(campaign, Vec::new());
        let ghost = PlayerId::new();

        // Act
        let result = remove_player_command(&state, player, ghost, &mut FixedEnv);

        // Assert
        assert!(matches!(
            result,
            Err(EngineError::PlayerNotFound { player }) if player == ghost
        ));
    }
}
