//! M5 slice 1 target test: roster commands as a client uses them.
//!
//! This file is the fixed point for `docs/plans/m5-identity.md`: it is written
//! against the pinned command surface before the implementation exists. It
//! intentionally fails at the first `todo!()` until the slice lands.

use localtt_adapter::store::memory::MemoryStore;
use localtt_core::types::{AclEntry, AclLevel, AnyEntity, PlayerId, Role, TokenHash, UnixTime};
use localtt_engine::{
    commands::{Command, CommandEnv, Event},
    error::EngineError,
    executor::Executor,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch},
    state::LoadedCampaign,
};
use localtt_integration::fixtures;

/// The fixed wall-clock stamp the fixture campaign uses.
const FIXED_TIME: i64 = 1_750_000_000;

/// Deterministic environment for `created_at` and `updated_at` stamps.
struct FixedEnv;

impl CommandEnv for FixedEnv {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// A full-length lowercase SHA-256-shaped hash for a distinct test token.
fn token_hash(byte: char) -> TokenHash {
    TokenHash::parse(&byte.to_string().repeat(64)).expect("lowercase hex is a valid hash")
}

/// Seeds a store with the party campaign and the supplied entities.
fn seeded_store(campaign: &localtt_core::types::Campaign, entities: &[AnyEntity]) -> MemoryStore {
    let mut store = MemoryStore::new();
    store
        .apply(&StatePatch {
            campaign: Some(campaign),
            upserts: entities.iter().collect(),
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: None,
        })
        .expect("a fresh store accepts the fixture campaign");
    store
}

fn executor<'a>(store: &'a mut MemoryStore) -> Executor<'a> {
    let snapshot: CampaignSnapshot = store.load().expect("the fixture campaign was loaded");
    Executor::new(
        Box::new(LoadedCampaign::from_snapshot(snapshot)),
        store,
        Box::new(FixedEnv),
    )
}

#[test]
fn add_player_appends_a_player_slot_and_emits_campaign_changed() {
    // Arrange: the fixture party has a DM and one player.
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let id = PlayerId::new();
    let hash = token_hash('a');

    // Act
    let executed = executor
        .execute_as(
            party.dm,
            &Command::AddPlayer {
                id,
                display_name: "Bea".to_owned(),
                token_hash: Some(hash.clone()),
            },
        )
        .expect("the DM may add a roster seat");

    // Assert: one campaign event, a Player role and the fixed stamps.
    assert_eq!(executed.events, vec![Event::CampaignChanged]);
    let campaign = executor.state().campaign();
    assert_eq!(campaign.players.len(), party.campaign.players.len() + 1);
    let player = campaign
        .players
        .iter()
        .find(|player| player.id == id)
        .expect("the new seat is on the roster");
    assert_eq!(player.display_name, "Bea");
    assert_eq!(player.role, Role::Player);
    assert_eq!(player.token_hash, Some(hash));
    assert_eq!(player.created_at, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(player.last_seen, None);

    // And the seat is durable.
    drop(executor);
    let loaded = store.load().expect("the roster change persisted");
    assert!(loaded.campaign.players.iter().any(|player| player.id == id));
}

#[test]
fn add_player_accepts_a_seat_without_an_invite() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let id = PlayerId::new();

    // Act
    executor
        .execute_as(
            party.dm,
            &Command::AddPlayer {
                id,
                display_name: "Uninvited".to_owned(),
                token_hash: None,
            },
        )
        .expect("an uninvited seat is legal");

    // Assert
    let player = executor
        .state()
        .campaign()
        .players
        .iter()
        .find(|player| player.id == id)
        .expect("the uninvited seat exists");
    assert_eq!(player.token_hash, None);
}

#[test]
fn add_player_rejects_a_duplicate_player_id() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::AddPlayer {
            id: party.player,
            display_name: "Copy".to_owned(),
            token_hash: None,
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerExists { player }) if player == party.player
    ));
}

#[test]
fn add_player_rejects_a_duplicate_token_hash() {
    // Arrange: one seat already owns the hash.
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let first = PlayerId::new();
    let hash = token_hash('b');
    executor
        .execute_as(
            party.dm,
            &Command::AddPlayer {
                id: first,
                display_name: "First".to_owned(),
                token_hash: Some(hash.clone()),
            },
        )
        .expect("the first invite is unique");

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::AddPlayer {
            id: PlayerId::new(),
            display_name: "Second".to_owned(),
            token_hash: Some(hash),
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerTokenInUse { player }) if player == first
    ));
}

#[test]
fn add_player_rejects_a_blank_display_name() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::AddPlayer {
            id: PlayerId::new(),
            display_name: "   ".to_owned(),
            token_hash: None,
        },
    );

    // Assert
    assert!(matches!(result, Err(EngineError::Malformed { .. })));
}

#[test]
fn add_player_rejects_an_oversized_display_name() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::AddPlayer {
            id: PlayerId::new(),
            display_name: "x".repeat(10_000),
            token_hash: None,
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::TooLarge { what, .. }) if what == "display name"
    ));
}

#[test]
fn set_player_token_sets_then_clears_the_hash() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let hash = token_hash('c');

    // Act: issue, then revoke.
    let issued = executor
        .execute_as(
            party.dm,
            &Command::SetPlayerToken {
                player: party.player,
                token_hash: Some(hash.clone()),
            },
        )
        .expect("the DM may issue a player token");
    let revoked = executor
        .execute_as(
            party.dm,
            &Command::SetPlayerToken {
                player: party.player,
                token_hash: None,
            },
        )
        .expect("the DM may revoke a player token");

    // Assert
    assert_eq!(issued.events, vec![Event::CampaignChanged]);
    assert_eq!(revoked.events, vec![Event::CampaignChanged]);
    assert_eq!(
        executor
            .state()
            .campaign()
            .players
            .iter()
            .find(|player| player.id == party.player)
            .expect("the player exists")
            .token_hash,
        None
    );
}

#[test]
fn set_player_token_refuses_the_dm_seat() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::SetPlayerToken {
            player: party.dm,
            token_hash: Some(token_hash('d')),
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerProtected { player }) if player == party.dm
    ));
}

#[test]
fn set_player_token_rejects_an_unknown_token_holder() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let ghost = PlayerId::new();

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::SetPlayerToken {
            player: ghost,
            token_hash: Some(token_hash('e')),
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerNotFound { player }) if player == ghost
    ));
}

#[test]
fn rotate_dm_key_replaces_the_selected_seat_hash() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let first = token_hash('f');
    let second = token_hash('1');

    // Act
    executor
        .execute_as(
            party.dm,
            &Command::RotateDmKey {
                player: party.dm,
                token_hash: first.clone(),
            },
        )
        .expect("the DM key can be seeded");
    executor
        .execute_as(
            party.dm,
            &Command::RotateDmKey {
                player: party.dm,
                token_hash: second.clone(),
            },
        )
        .expect("the DM key can be rotated");

    // Assert
    assert_eq!(
        executor
            .state()
            .campaign()
            .players
            .iter()
            .find(|player| player.id == party.dm)
            .expect("the DM exists")
            .token_hash,
        Some(second)
    );
}

#[test]
fn rotate_dm_key_refuses_a_player_seat() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::RotateDmKey {
            player: party.player,
            token_hash: token_hash('2'),
        },
    );

    // Assert
    assert!(matches!(result, Err(EngineError::Malformed { .. })));
}

#[test]
fn rotate_dm_key_rejects_a_hash_used_by_a_player() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let hash = token_hash('3');
    executor
        .execute_as(
            party.dm,
            &Command::SetPlayerToken {
                player: party.player,
                token_hash: Some(hash.clone()),
            },
        )
        .expect("the player token is set");

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::RotateDmKey {
            player: party.dm,
            token_hash: hash,
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerTokenInUse { player }) if player == party.player
    ));
}

#[test]
fn remove_player_transfers_owned_entities_and_scrubs_acl_entries() {
    // Arrange: the player owns the PC, companion and familiar; the note is
    // DM-owned but shared to the player with a read grant.
    let party = fixtures::party();
    let mut note = party.note.clone();
    note.acl.push(AclEntry {
        player: party.player,
        level: AclLevel::Read,
        field_mask: None,
    });
    let entities = [
        AnyEntity::from(party.pc.clone()),
        AnyEntity::from(party.companion.clone()),
        AnyEntity::from(party.familiar.clone()),
        AnyEntity::from(note.clone()),
        AnyEntity::from(party.handout.clone()),
    ];
    let mut store = seeded_store(&party.campaign, &entities);
    let mut executor = executor(&mut store);

    // Act
    let executed = executor
        .execute_as(
            party.dm,
            &Command::RemovePlayer {
                player: party.player,
                reassign_to: party.dm,
            },
        )
        .expect("the DM may remove a player");

    // Assert: the seat is gone, ownership moved, the ACL grant is gone.
    let campaign = executor.state().campaign();
    assert!(
        !campaign
            .players
            .iter()
            .any(|player| player.id == party.player)
    );
    for id in [party.pc.id, party.companion.id, party.familiar.id] {
        let entity = executor
            .state()
            .entity(id)
            .expect("the entity is loaded")
            .expect("the entity exists");
        assert_eq!(entity.owner(), party.dm, "{id} must transfer to the DM");
        assert_eq!(entity.rev(), 1, "{id} must be stamped");
    }
    let note_after = executor
        .state()
        .entity(note.id)
        .expect("the note is loaded")
        .expect("the note exists");
    assert_eq!(note_after.owner(), party.dm);
    assert_eq!(note_after.rev(), 1, "the ACL scrub stamps the note");
    assert!(
        note_after
            .acl()
            .iter()
            .all(|entry| entry.player != party.player),
        "the removed player's ACL entries are gone"
    );
    let handout = executor
        .state()
        .entity(party.handout.id)
        .expect("the handout is loaded")
        .expect("the handout exists");
    assert_eq!(handout.rev(), 0, "an untouched entity keeps its revision");

    let changed: Vec<_> = executed
        .events
        .iter()
        .filter_map(|event| match event {
            Event::EntityChanged { id, rev, .. } => Some((*id, *rev)),
            _ => None,
        })
        .collect();
    assert_eq!(changed.len(), 4, "PC, companion, familiar and note changed");
    assert!(changed.contains(&(party.pc.id, 1)));
    assert!(changed.contains(&(party.companion.id, 1)));
    assert!(changed.contains(&(party.familiar.id, 1)));
    assert!(changed.contains(&(note.id, 1)));
    assert_eq!(executed.events.last(), Some(&Event::CampaignChanged));

    // And the transfer is durable.
    drop(executor);
    let loaded = store.load().expect("the removal persisted");
    let stored_pc = loaded
        .entities
        .iter()
        .find(|entity| entity.id() == party.pc.id)
        .expect("the PC persists");
    assert_eq!(stored_pc.owner(), party.dm);
}

#[test]
fn remove_player_refuses_a_dm_seat() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::RemovePlayer {
            player: party.dm,
            reassign_to: party.dm,
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerProtected { player }) if player == party.dm
    ));
}

#[test]
fn remove_player_rejects_an_unknown_seat() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);
    let ghost = PlayerId::new();

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::RemovePlayer {
            player: ghost,
            reassign_to: party.dm,
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(EngineError::PlayerNotFound { player }) if player == ghost
    ));
}

#[test]
fn remove_player_rejects_a_non_dm_reassign_target() {
    // Arrange
    let party = fixtures::party();
    let mut store = seeded_store(&party.campaign, &[]);
    let mut executor = executor(&mut store);

    // Act
    let result = executor.execute_as(
        party.dm,
        &Command::RemovePlayer {
            player: party.player,
            reassign_to: party.player,
        },
    );

    // Assert
    assert!(matches!(result, Err(EngineError::Malformed { .. })));
}
