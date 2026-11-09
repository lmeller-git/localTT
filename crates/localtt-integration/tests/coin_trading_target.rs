//! Coin trading target test: exact purse transfers between statblocks, as a
//! client uses them.
//!
//! This is the frozen client contract for the coin-trading unit; it is green
//! and must keep compiling unchanged as the engine evolves.

use localtt_adapter::store::{memory::MemoryStore, sqlite::SqliteStore};
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{
        Abilities,
        AclEntry,
        AclLevel,
        AnyEntity,
        Campaign,
        Coins,
        Entity,
        EntityId,
        EntityKind,
        HandoutBody,
        HandoutContent,
        InlineItem,
        ItemKind,
        PlayerId,
        StatblockEntity,
        UnixTime,
    },
};
use localtt_engine::{
    commands::{Command, CommandEnv, Event},
    error::EngineError,
    executor::Executor,
    ports::store::{CampaignStore, StatePatch},
    state::{LoadedCampaign, WorkingSet},
};
use localtt_server::protocol::CommandDto;
use tempfile::TempDir;

/// The frozen wall clock; every command stamps this instant.
const FIXED_TIME: i64 = 1_750_000_000;

/// The environment seam: a fixed clock and no other capabilities.
struct Env;

impl CommandEnv for Env {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }
}

/// A campaign with one DM and three players. Carl deliberately starts with no
/// character, so "recipient has no character" is a real case.
fn campaign() -> (Campaign, PlayerId, PlayerId, PlayerId, PlayerId) {
    let mut builder = CampaignBuilder::new("Coin trading");
    let dm = builder.dm("Dungeon Master");
    let aria = builder.player("Aria");
    let bram = builder.player("Bram");
    let carl = builder.player("Carl");
    (builder.build(), dm, aria, bram, carl)
}

/// Seeds a store with the campaign singleton. `CreateEntity` seeds the rest.
fn seed(store: &mut MemoryStore, campaign: &Campaign) {
    store
        .apply(&StatePatch {
            campaign: Some(campaign),
            upserts: Vec::new(),
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: None,
        })
        .expect("the campaign seeds the store");
}

/// The fields needed to create one statblock with a known purse.
struct NewPurse<'a> {
    actor: PlayerId,
    owner: PlayerId,
    kind: EntityKind,
    name: &'a str,
    acl: Vec<AclEntry>,
    coins: Coins,
}

/// Creates one statblock through the real command path.
fn create_purse(executor: &mut Executor<'_>, new: NewPurse<'_>) -> EntityId {
    let mut builder = StatblockBuilder::character();
    builder
        .abilities(Abilities::all(12))
        .hp(12, 12)
        .coins(new.coins);
    let entity = builder
        .entity_builder(new.kind, EntityId::new(), new.name, new.owner)
        .expect("abilities and hp are set")
        .acl(new.acl)
        .build()
        .expect("a statblock kind pairs with a statblock body");
    let id = entity.id;
    executor
        .execute_as(
            new.actor,
            &Command::CreateEntity {
                entity: Box::new(AnyEntity::from(entity)),
            },
        )
        .expect("the actor creates the statblock");
    id
}

/// Creates one player character through the real command path.
fn create_pc(
    executor: &mut Executor<'_>,
    actor: PlayerId,
    owner: PlayerId,
    name: &str,
    coins: Coins,
) -> EntityId {
    create_purse(
        executor,
        NewPurse {
            actor,
            owner,
            kind: EntityKind::Pc,
            name,
            acl: Vec::new(),
            coins,
        },
    )
}

/// Creates one handout through the real command path.
fn create_handout(
    executor: &mut Executor<'_>,
    actor: PlayerId,
    owner: PlayerId,
    name: &str,
    content: HandoutContent,
) -> EntityId {
    let handout = Entity::builder(
        EntityKind::Handout,
        EntityId::new(),
        name,
        owner,
        HandoutBody { content },
    )
    .build()
    .expect("a handout kind pairs with a handout body");
    let id = handout.id;
    executor
        .execute_as(
            actor,
            &Command::CreateEntity {
                entity: Box::new(AnyEntity::from(handout)),
            },
        )
        .expect("the actor creates the handout");
    id
}

/// One inline item with the fields a handout needs.
fn inline(name: &str) -> InlineItem {
    InlineItem {
        name: name.to_owned(),
        description: String::new(),
        kind: ItemKind::Gear,
        requires_attunement: false,
        weight_oz: None,
        value: None,
        properties: None,
        modifiers: Vec::new(),
        resource: None,
        ext: Default::default(),
    }
}

/// One statblock as the working set holds it.
fn statblock_of(state: &dyn WorkingSet, id: EntityId) -> StatblockEntity {
    match state
        .entity(id)
        .expect("the working set resolves its own headers")
        .expect("the entity exists")
    {
        AnyEntity::Statblock(document) => document,
        other => panic!("{id} is a statblock, got {other:?}"),
    }
}

/// One entity as a reloaded snapshot holds it.
fn snapshot_entity(entities: &[AnyEntity], id: EntityId) -> AnyEntity {
    entities
        .iter()
        .find(|entity| entity.id() == id)
        .unwrap_or_else(|| panic!("{id} is stored"))
        .clone()
}

/// A write grant with no field mask; write masks are ignored by authz.
fn write_grant(player: PlayerId) -> AclEntry {
    AclEntry {
        player,
        level: AclLevel::Write,
        field_mask: None,
    }
}

/// The changed entity ids of a transition, in delivery order.
fn changed_ids(events: &[Event]) -> Vec<EntityId> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::EntityChanged { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

#[test]
fn a_player_transfers_part_of_a_purse_to_another_players_primary_character() {
    // Arrange: Aria's purse and Bram's smaller purse.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            cp: 1,
            sp: 5,
            ep: 2,
            gp: 10,
            pp: 1,
        },
    );
    let bram_pc = create_pc(
        &mut executor,
        bram,
        bram,
        "Bram",
        Coins {
            cp: 2,
            sp: 0,
            ep: 0,
            gp: 1,
            pp: 0,
        },
    );
    let aria_rev = statblock_of(executor.state(), aria_pc).rev;
    let bram_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act: Aria hands over 3 gp and 2 sp. No destination is named, so the
    // engine resolves Bram's primary character.
    let executed = executor
        .execute_as(
            aria,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    gp: 3,
                    sp: 2,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: Some(aria_rev),
                expected_destination_rev: Some(bram_rev),
            },
        )
        .expect("a player hands part of a purse to another player");

    // Assert: only the moved denominations changed; both revisions advanced;
    // two events arrive in ascending id order; no feed entry was added.
    assert_eq!(executed.warnings, Vec::new());
    assert!(executor.state().feed().is_empty(), "transfers add no feed");
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, aria_rev + 1);
    assert_eq!(bram_sheet.rev, bram_rev + 1);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            cp: 1,
            sp: 3,
            ep: 2,
            gp: 7,
            pp: 1,
        }
    );
    assert_eq!(
        bram_sheet.body.coins,
        Coins {
            cp: 2,
            sp: 2,
            ep: 0,
            gp: 4,
            pp: 0,
        }
    );
    assert_eq!(executed.events.len(), 2);
    assert!(executed.events.contains(&Event::EntityChanged {
        id: aria_pc,
        rev: aria_rev + 1,
        kind: EntityKind::Pc,
    }));
    assert!(executed.events.contains(&Event::EntityChanged {
        id: bram_pc,
        rev: bram_rev + 1,
        kind: EntityKind::Pc,
    }));
    let ids = changed_ids(&executed.events);
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "the transition orders its upserts and events by entity id"
    );
}

#[test]
fn a_whole_denomination_transfer_leaves_zero_and_other_coins_untouched() {
    // Arrange: one purse with every denomination populated.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            cp: 1,
            sp: 2,
            ep: 3,
            gp: 4,
            pp: 5,
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());

    // Act: move the whole gp field.
    executor
        .execute_as(
            aria,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    gp: 4,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        )
        .expect("the whole denomination moves");

    // Assert: the source stores zero, not a missing field, and the other four
    // denominations are untouched.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            cp: 1,
            sp: 2,
            ep: 3,
            gp: 0,
            pp: 5,
        }
    );
    assert_eq!(
        bram_sheet.body.coins,
        Coins {
            gp: 4,
            ..Coins::default()
        }
    );
}

#[test]
fn a_dm_can_pick_a_companions_sheet_explicitly() {
    // Arrange: Aria's PC and her wolf companion; Bram also has a PC.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            gp: 6,
            ..Coins::default()
        },
    );
    let wolf = create_purse(
        &mut executor,
        NewPurse {
            actor: dm,
            owner: aria,
            kind: EntityKind::Companion,
            name: "Wolf",
            acl: Vec::new(),
            coins: Coins::default(),
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let wolf_rev = statblock_of(executor.state(), wolf).rev;

    // Act: the DM names the companion as the destination.
    executor
        .execute_as(
            dm,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    gp: 2,
                    ..Coins::default()
                },
                recipient: aria,
                destination: Some(wolf),
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(wolf_rev),
            },
        )
        .expect("the DM targets the recipient's companion");

    // Assert: the wolf received the coins.
    assert_eq!(
        statblock_of(executor.state(), wolf).body.coins,
        Coins {
            gp: 2,
            ..Coins::default()
        }
    );
    assert_eq!(
        statblock_of(executor.state(), aria_pc).body.coins,
        Coins {
            gp: 4,
            ..Coins::default()
        }
    );

    // Act and assert: the destination must belong to the named recipient.
    assert!(matches!(
        executor.execute_as(
            dm,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    gp: 1,
                    ..Coins::default()
                },
                recipient: aria,
                destination: Some(bram_pc),
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        ),
        Err(EngineError::Malformed { .. })
    ));
}

#[test]
fn a_write_grantee_can_transfer_coins_off_a_shared_sheet() {
    // Arrange: the DM shares Aria's sheet with Bram; Bram has his own PC.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_purse(
        &mut executor,
        NewPurse {
            actor: dm,
            owner: aria,
            kind: EntityKind::Pc,
            name: "Aria",
            acl: vec![write_grant(bram)],
            coins: Coins {
                sp: 8,
                ..Coins::default()
            },
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act: write access, not ownership, is enough to move coins off a sheet.
    executor
        .execute_as(
            bram,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    sp: 3,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        )
        .expect("a write grantee distributes the shared sheet");

    // Assert: both sheets advanced and the coins moved to Bram's own PC.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev + 1);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            sp: 5,
            ..Coins::default()
        }
    );
    assert_eq!(bram_sheet.rev, destination_rev + 1);
    assert_eq!(
        bram_sheet.body.coins,
        Coins {
            sp: 3,
            ..Coins::default()
        }
    );
}

#[test]
fn a_non_dm_cannot_send_to_the_dm_or_choose_a_destination() {
    // Arrange: Aria has coins and Bram has a character; Carl has none.
    let (campaign, dm, aria, bram, carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            gp: 10,
            ..Coins::default()
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let _dm_pc = create_pc(&mut executor, dm, dm, "Dungeon Master", Coins::default());
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;
    let stranger = PlayerId::new();

    // Act and assert: each non-DM shortcut is refused.
    let transfer = |recipient: PlayerId, destination: Option<EntityId>| Command::TransferCoins {
        source: aria_pc,
        amount: Coins {
            gp: 1,
            ..Coins::default()
        },
        recipient,
        destination,
        expected_source_rev: Some(source_rev),
        expected_destination_rev: Some(destination_rev),
    };
    assert!(matches!(
        executor.execute_as(aria, &transfer(dm, None)),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        executor.execute_as(aria, &transfer(bram, Some(bram_pc))),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        executor.execute_as(aria, &transfer(carl, None)),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        executor.execute_as(aria, &transfer(stranger, None)),
        Err(EngineError::PlayerNotFound { player }) if player == stranger
    ));
    assert!(matches!(
        executor.execute_as(aria, &transfer(aria, None)),
        Err(EngineError::Malformed { .. })
    ));

    // None of the refusals touched either sheet.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            gp: 10,
            ..Coins::default()
        }
    );
    assert_eq!(bram_sheet.rev, destination_rev);
    assert_eq!(bram_sheet.body.coins, Coins::default());
}

#[test]
fn stale_revisions_and_bad_amounts_are_refused_atomically() {
    // Arrange: every denomination on Aria's sheet; Bram has an empty purse.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            cp: 3,
            sp: 3,
            ep: 3,
            gp: 3,
            pp: 3,
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    let transfer =
        |amount: Coins, expected_source_rev: Option<u64>, expected_destination_rev: Option<u64>| {
            Command::TransferCoins {
                source: aria_pc,
                amount,
                recipient: bram,
                destination: None,
                expected_source_rev,
                expected_destination_rev,
            }
        };
    let one = Coins {
        gp: 1,
        ..Coins::default()
    };

    // Act and assert: stale source revision.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(one, Some(source_rev + 1), Some(destination_rev)),
        ),
        Err(EngineError::Conflict {
            entity,
            expected,
            actual,
        }) if entity == aria_pc && expected == source_rev + 1 && actual == source_rev
    ));

    // Act and assert: stale destination revision.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(one, Some(source_rev), Some(destination_rev + 1)),
        ),
        Err(EngineError::Conflict {
            entity,
            expected,
            actual,
        }) if entity == bram_pc && expected == destination_rev + 1 && actual == destination_rev
    ));

    // Act and assert: an all-zero amount.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(Coins::default(), Some(source_rev), Some(destination_rev)),
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Act and assert: each negative denomination.
    let negative = [
        Coins {
            cp: -1,
            ..Coins::default()
        },
        Coins {
            sp: -1,
            ..Coins::default()
        },
        Coins {
            ep: -1,
            ..Coins::default()
        },
        Coins {
            gp: -1,
            ..Coins::default()
        },
        Coins {
            pp: -1,
            ..Coins::default()
        },
    ];
    for amount in negative {
        assert!(matches!(
            executor.execute_as(
                aria,
                &transfer(amount, Some(source_rev), Some(destination_rev)),
            ),
            Err(EngineError::Malformed { .. })
        ));
    }

    // Act and assert: more than the source stores.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(
                Coins {
                    gp: 4,
                    ..Coins::default()
                },
                Some(source_rev),
                Some(destination_rev),
            ),
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Assert: every failure left both sheets exactly as they were.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            cp: 3,
            sp: 3,
            ep: 3,
            gp: 3,
            pp: 3,
        }
    );
    assert_eq!(bram_sheet.rev, destination_rev);
    assert_eq!(bram_sheet.body.coins, Coins::default());
}

#[test]
fn missing_and_non_statblock_endpoints_are_refused() {
    // Arrange: two PCs, one handout per player, and fresh ids that resolve to
    // nothing.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            gp: 3,
            ..Coins::default()
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let aria_handout = create_handout(
        &mut executor,
        aria,
        aria,
        "Aria's notes",
        HandoutContent::Item {
            item: inline("A letter"),
        },
    );
    let bram_handout = create_handout(
        &mut executor,
        bram,
        bram,
        "Bram's notes",
        HandoutContent::Item {
            item: inline("A map"),
        },
    );
    let ghost_source = EntityId::new();
    let ghost_destination = EntityId::new();
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    let transfer =
        |source: EntityId, recipient: PlayerId, destination: Option<EntityId>| -> Command {
            Command::TransferCoins {
                source,
                amount: Coins {
                    gp: 1,
                    ..Coins::default()
                },
                recipient,
                destination,
                expected_source_rev: None,
                expected_destination_rev: None,
            }
        };

    // Act and assert: a missing source and a missing destination are both the
    // command's own NotFound.
    assert!(matches!(
        executor.execute_as(aria, &transfer(ghost_source, bram, None)),
        Err(EngineError::NotFound {
            entity,
            element: None,
        }) if entity == ghost_source
    ));
    assert!(matches!(
        executor.execute_as(dm, &transfer(aria_pc, bram, Some(ghost_destination))),
        Err(EngineError::NotFound {
            entity,
            element: None,
        }) if entity == ghost_destination
    ));

    // Act and assert: neither side of a coin transfer may be a non-statblock.
    assert!(matches!(
        executor.execute_as(aria, &transfer(aria_handout, bram, None)),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        executor.execute_as(dm, &transfer(aria_pc, bram, Some(bram_handout))),
        Err(EngineError::Malformed { .. })
    ));

    // Assert: none of the refusals changed either sheet.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            gp: 3,
            ..Coins::default()
        }
    );
    assert_eq!(bram_sheet.rev, destination_rev);
    assert_eq!(bram_sheet.body.coins, Coins::default());
}

#[test]
fn a_destination_overflow_is_refused_atomically() {
    // Arrange: Bram's purse sits at the i32 ceiling for gp.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            gp: 5,
            ..Coins::default()
        },
    );
    let bram_pc = create_pc(
        &mut executor,
        bram,
        bram,
        "Bram",
        Coins {
            gp: i32::MAX,
            ..Coins::default()
        },
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act and assert: the destination cannot hold one more gp.
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    gp: 1,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Assert: the source was not debited.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            gp: 5,
            ..Coins::default()
        }
    );
    assert_eq!(bram_sheet.rev, destination_rev);
    assert_eq!(
        bram_sheet.body.coins,
        Coins {
            gp: i32::MAX,
            ..Coins::default()
        }
    );
}

#[test]
fn negative_stored_coins_are_not_repaired() {
    // Arrange: Aria's stored gp is negative; her sp is normal.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            gp: -2,
            sp: 5,
            ..Coins::default()
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act and assert: a positive move of the negative field is insufficient.
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    gp: 1,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Act: a zero move of the negative field alongside a positive other field
    // is allowed and leaves the negative value untouched.
    executor
        .execute_as(
            aria,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    sp: 1,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        )
        .expect("a zero move of the negative denomination is harmless");

    // Assert: the negative value was not repaired.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            gp: -2,
            sp: 4,
            ..Coins::default()
        }
    );
    assert_eq!(
        bram_sheet.body.coins,
        Coins {
            sp: 1,
            ..Coins::default()
        }
    );
}

#[test]
fn coin_transfers_survive_a_store_reopen() {
    // Arrange: a real SQLite campaign with two characters and two purses.
    let dir = TempDir::new().expect("a temporary directory");
    let path = dir.path().join("campaign.sqlite");
    let mut store = SqliteStore::open(&path).expect("the store opens");
    let (campaign, _dm, aria, bram, _carl) = campaign();
    store
        .apply(&StatePatch {
            campaign: Some(&campaign),
            upserts: Vec::new(),
            removals: Vec::new(),
            feed_appends: Vec::new(),
            feed_keep_from: None,
        })
        .expect("the campaign seeds the store");
    let snapshot = store.load().expect("the campaign loads");
    let mut executor = Executor::new(
        Box::new(LoadedCampaign::from_snapshot(snapshot)),
        &mut store,
        Box::new(Env),
    );
    let aria_pc = create_pc(
        &mut executor,
        aria,
        aria,
        "Aria",
        Coins {
            cp: 4,
            sp: 6,
            gp: 8,
            ..Coins::default()
        },
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram", Coins::default());
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act: move two denominations and persist.
    executor
        .execute_as(
            aria,
            &Command::TransferCoins {
                source: aria_pc,
                amount: Coins {
                    cp: 1,
                    gp: 3,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        )
        .expect("the coin transfer persists");
    drop(executor);
    drop(store);

    // Assert: both purses and both revisions are on disk.
    let reopened = SqliteStore::open(&path).expect("the store reopens");
    let loaded = reopened.load().expect("the campaign reloads");
    let aria_sheet = snapshot_entity(&loaded.entities, aria_pc);
    let bram_sheet = snapshot_entity(&loaded.entities, bram_pc);
    let AnyEntity::Statblock(aria_sheet) = aria_sheet else {
        panic!("Aria's character is a statblock");
    };
    let AnyEntity::Statblock(bram_sheet) = bram_sheet else {
        panic!("Bram's character is a statblock");
    };
    assert_eq!(aria_sheet.rev, source_rev + 1);
    assert_eq!(bram_sheet.rev, destination_rev + 1);
    assert_eq!(
        aria_sheet.body.coins,
        Coins {
            cp: 3,
            sp: 6,
            gp: 5,
            ..Coins::default()
        }
    );
    assert_eq!(
        bram_sheet.body.coins,
        Coins {
            cp: 1,
            gp: 3,
            ..Coins::default()
        }
    );
}

#[test]
fn the_wire_dto_round_trips_and_converts_to_the_command() {
    // Arrange: one transfer over the wire, with one revision pinned and the
    // destination left implicit.
    let source = EntityId::new();
    let recipient = PlayerId::new();
    let amount = Coins {
        cp: 1,
        sp: 2,
        ep: 3,
        gp: 4,
        pp: 5,
    };
    let dto = CommandDto::TransferCoins {
        source,
        amount,
        recipient,
        destination: None,
        expected_source_rev: Some(4),
        expected_destination_rev: None,
    };

    // Act: serialize, read the shape back, deserialize, convert.
    let value = serde_json::to_value(&dto).expect("the DTO serializes");
    assert_eq!(value["type"], "transfer_coins");
    assert_eq!(value["source"], source.to_string());
    assert_eq!(value["amount"]["cp"], 1);
    assert_eq!(value["amount"]["sp"], 2);
    assert_eq!(value["amount"]["ep"], 3);
    assert_eq!(value["amount"]["gp"], 4);
    assert_eq!(value["amount"]["pp"], 5);
    assert!(value.get("destination").is_none());
    assert_eq!(value["expected_source_rev"], 4);
    assert!(value.get("expected_destination_rev").is_none());
    let decoded: CommandDto = serde_json::from_value(value).expect("the DTO round-trips");
    assert_eq!(decoded, dto);
    let command: Command = decoded.try_into().expect("this build knows the command");

    // Assert: the engine command carries the same purse.
    assert_eq!(
        command,
        Command::TransferCoins {
            source,
            amount,
            recipient,
            destination: None,
            expected_source_rev: Some(4),
            expected_destination_rev: None,
        }
    );
}

#[test]
fn the_wire_dto_preserves_an_explicit_destination_and_revisions() {
    // Arrange: a DM move with both the destination and its revision pinned.
    let source = EntityId::new();
    let destination = EntityId::new();
    let recipient = PlayerId::new();
    let amount = Coins {
        cp: 5,
        sp: 4,
        ep: 3,
        gp: 2,
        pp: 1,
    };
    let dto = CommandDto::TransferCoins {
        source,
        amount,
        recipient,
        destination: Some(destination),
        expected_source_rev: Some(4),
        expected_destination_rev: Some(9),
    };

    // Act: serialize and read every pinned field back.
    let value = serde_json::to_value(&dto).expect("the DTO serializes");
    assert_eq!(value["type"], "transfer_coins");
    assert_eq!(value["source"], source.to_string());
    assert_eq!(value["destination"], destination.to_string());
    assert_eq!(value["amount"]["cp"], 5);
    assert_eq!(value["amount"]["sp"], 4);
    assert_eq!(value["amount"]["ep"], 3);
    assert_eq!(value["amount"]["gp"], 2);
    assert_eq!(value["amount"]["pp"], 1);
    assert_eq!(value["expected_source_rev"], 4);
    assert_eq!(value["expected_destination_rev"], 9);

    // Assert: the round-trip and the engine command keep both fields.
    let decoded: CommandDto = serde_json::from_value(value).expect("the DTO round-trips");
    assert_eq!(decoded, dto);
    let command: Command = decoded.try_into().expect("this build knows the command");
    assert_eq!(
        command,
        Command::TransferCoins {
            source,
            amount,
            recipient,
            destination: Some(destination),
            expected_source_rev: Some(4),
            expected_destination_rev: Some(9),
        }
    );
}
