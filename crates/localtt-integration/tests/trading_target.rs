//! Trading target test: item transfers and entity ownership transfers, as a
//! client uses them.
//!
//! This file is the fixed point for `docs/rfcs/trading.md`: ordinary client
//! code written against the pinned API. The skeleton now provides every type
//! and signature, so the file compiles; the behavior tests stay red at
//! `not yet implemented` until the implement phase lands the handlers. The
//! protocol conversion tests already pass because those conversions are
//! implemented in the skeleton.

use localtt_adapter::store::{memory::MemoryStore, sqlite::SqliteStore};
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{
        Abilities,
        AclEntry,
        AclLevel,
        AnyEntity,
        Campaign,
        Entity,
        EntityId,
        EntityKind,
        Ext,
        Formula,
        HandoutBody,
        HandoutContent,
        InlineItem,
        InventoryEntryId,
        InventoryItem,
        ItemKind,
        ItemState,
        PlayerId,
        Recharge,
        Resource,
        ResourceId,
        ResourceKind,
        StatblockEntity,
        UnixTime,
    },
};
use localtt_engine::{
    commands::{Command, CommandEnv, Event, ItemMove},
    error::{ElementRef, EngineError},
    executor::Executor,
    ports::store::{CampaignStore, StatePatch},
    state::{LoadedCampaign, WorkingSet},
};
use localtt_server::protocol::{CommandDto, ItemMoveDto};
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
    let mut builder = CampaignBuilder::new("Trading");
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

/// One inline item with the fields the transfer cases care about.
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
        ext: Ext::new(),
    }
}

/// A plain inline item.
fn item(name: &str) -> InventoryItem {
    InventoryItem::Inline(inline(name))
}

/// An inline item carrying a nested tracked resource, e.g. a charged wand.
fn charged_item(name: &str, charges: i32) -> (InventoryItem, ResourceId) {
    let resource = Resource {
        id: ResourceId::new(),
        name: format!("{name} charges"),
        kind: ResourceKind::Item,
        max: Formula::Const(charges),
        current: charges,
        recharge: Recharge::Dawn,
    };
    let id = resource.id;
    (
        InventoryItem::Inline(InlineItem {
            resource: Some(resource),
            ..inline(name)
        }),
        id,
    )
}

/// Creates one statblock through the real command path.
fn create_statblock(
    executor: &mut Executor<'_>,
    actor: PlayerId,
    owner: PlayerId,
    kind: EntityKind,
    name: &str,
    acl: Vec<AclEntry>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut builder = StatblockBuilder::character();
    builder.abilities(Abilities::all(12)).hp(12, 12);
    let mut header = builder
        .entity_builder(kind, EntityId::new(), name, owner)
        .expect("abilities and hp are set")
        .acl(acl);
    if let Some(parent) = parent {
        header = header.parent(parent);
    }
    let entity = header
        .build()
        .expect("a statblock kind pairs with a statblock body");
    let id = entity.id;
    executor
        .execute_as(
            actor,
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
) -> EntityId {
    create_statblock(
        executor,
        actor,
        owner,
        EntityKind::Pc,
        name,
        Vec::new(),
        None,
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

/// The inventory fields `add_item` sets; each test pins at least one.
struct NewItem {
    item: InventoryItem,
    qty: i32,
    state: ItemState,
    attuned: bool,
    notes: String,
}

impl NewItem {
    /// A carried, unattuned stack with no notes.
    fn carried(item: InventoryItem, qty: i32) -> Self {
        Self {
            item,
            qty,
            state: ItemState::Carried,
            attuned: false,
            notes: String::new(),
        }
    }

    /// An equipped stack, with the attunement flag and notes a test pins.
    fn equipped(item: InventoryItem, qty: i32, attuned: bool, notes: &str) -> Self {
        Self {
            item,
            qty,
            state: ItemState::Equipped,
            attuned,
            notes: notes.to_owned(),
        }
    }
}

/// Adds one item and returns the minted entry id.
fn add_item(
    executor: &mut Executor<'_>,
    actor: PlayerId,
    entity: EntityId,
    new: NewItem,
) -> InventoryEntryId {
    let rev = statblock_of(executor.state(), entity).rev;
    executor
        .execute_as(
            actor,
            &Command::AddInventoryItem {
                entity,
                item: Box::new(new.item),
                qty: new.qty,
                state: new.state,
                attuned: new.attuned,
                notes: new.notes,
                expected_rev: Some(rev),
            },
        )
        .expect("the actor adds the item");
    statblock_of(executor.state(), entity)
        .body
        .inventory
        .last()
        .expect("the item was added")
        .id
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

/// One entity as the working set holds it.
fn entity_of(state: &dyn WorkingSet, id: EntityId) -> AnyEntity {
    state
        .entity(id)
        .expect("the working set resolves its own headers")
        .expect("the entity exists")
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

#[test]
fn a_player_splits_a_stack_to_another_players_primary_character() {
    // Arrange: Aria, Bram and a stack of three ropes in Aria's pack.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::equipped(item("Rope"), 3, true, "50 feet"),
    );
    let aria_rev = statblock_of(executor.state(), aria_pc).rev;
    let bram_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act: Aria hands two of the three ropes to Bram. No destination is named,
    // so the engine resolves Bram's primary character.
    let executed = executor
        .execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 2,
                }],
                recipient: bram,
                destination: None,
                expected_source_rev: Some(aria_rev),
                expected_destination_rev: Some(bram_rev),
            },
        )
        .expect("a player hands part of a stack to another player");

    // Assert: the source keeps the remainder and its equipment state; the
    // destination gets one fresh entry that is carried and unattuned.
    assert_eq!(executed.warnings, Vec::new());
    assert!(executor.state().feed().is_empty(), "transfers add no feed");
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, aria_rev + 1);
    assert_eq!(bram_sheet.rev, bram_rev + 1);
    assert_eq!(aria_sheet.body.inventory.len(), 1);
    assert_eq!(aria_sheet.body.inventory[0].id, rope);
    assert_eq!(aria_sheet.body.inventory[0].qty, 1);
    assert_eq!(aria_sheet.body.inventory[0].state, ItemState::Equipped);
    assert!(aria_sheet.body.inventory[0].attuned);
    assert_eq!(bram_sheet.body.inventory.len(), 1);
    let moved = &bram_sheet.body.inventory[0];
    assert_ne!(
        moved.id, rope,
        "the destination always gets a fresh entry id"
    );
    assert_eq!(moved.item, item("Rope"));
    assert_eq!(moved.qty, 2);
    assert_eq!(moved.state, ItemState::Carried);
    assert!(!moved.attuned);
    assert_eq!(moved.notes, "50 feet");
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
    let changed_ids: Vec<EntityId> = executed
        .events
        .iter()
        .filter_map(|event| match event {
            Event::EntityChanged { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert!(
        changed_ids.windows(2).all(|pair| pair[0] < pair[1]),
        "the transition orders its upserts and events by entity id"
    );
}

#[test]
fn a_whole_stack_moves_and_the_source_entry_disappears() {
    // Arrange: one equipped, attuned potion.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let potion = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::equipped(item("Potion of Healing"), 1, true, "stoppered"),
    );

    // Act: transfer the only unit.
    executor
        .execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: potion,
                    qty: 1,
                }],
                recipient: bram,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        )
        .expect("the whole entry moves");

    // Assert: the source entry is gone, not left at quantity zero.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert!(aria_sheet.body.inventory.is_empty());
    assert_eq!(bram_sheet.body.inventory.len(), 1);
    let moved = &bram_sheet.body.inventory[0];
    assert_ne!(
        moved.id, potion,
        "the destination always gets a fresh entry id"
    );
    assert_eq!(moved.item, item("Potion of Healing"));
    assert_eq!(moved.qty, 1);
    assert_eq!(moved.state, ItemState::Carried);
    assert!(!moved.attuned);
    assert_eq!(moved.notes, "stoppered");
}

#[test]
fn a_dm_moves_a_character_and_the_old_owner_loses_write() {
    // Arrange: Aria owns her PC; Carl has a write grant on it.
    let (campaign, dm, aria, bram, carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_statblock(
        &mut executor,
        dm,
        aria,
        EntityKind::Pc,
        "Aria",
        vec![write_grant(carl)],
        None,
    );
    let rev = statblock_of(executor.state(), aria_pc).rev;

    // Act: the DM reassigns the character to Bram.
    executor
        .execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: bram,
                expected_rev: Some(rev),
            },
        )
        .expect("the DM moves the character");

    // Assert: ownership moved, the revision advanced, and the ACL is
    // untouched. Aria, the former owner, can no longer write; Bram and the
    // existing grantee can.
    let sheet = statblock_of(executor.state(), aria_pc);
    assert_eq!(sheet.owner, bram);
    assert_eq!(sheet.rev, rev + 1);
    assert_eq!(sheet.acl, vec![write_grant(carl)]);
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::ApplyDamage {
                entity: aria_pc,
                amount: 1,
                expected_rev: None,
            },
        ),
        Err(EngineError::Unauthorized {
            actor,
            entity: Some(id),
        }) if actor == aria && id == aria_pc
    ));
    executor
        .execute_as(
            bram,
            &Command::ApplyDamage {
                entity: aria_pc,
                amount: 1,
                expected_rev: None,
            },
        )
        .expect("the new owner writes the character");
    executor
        .execute_as(
            carl,
            &Command::ApplyDamage {
                entity: aria_pc,
                amount: 1,
                expected_rev: None,
            },
        )
        .expect("an explicit ACL grant survives an ownership transfer");
}

#[test]
fn an_entity_transfer_stamps_the_header_and_emits_one_event() {
    // Arrange: Aria's PC with a known body, owner and revision.
    let (campaign, dm, aria, bram, carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let before = statblock_of(executor.state(), aria_pc);
    let rev = before.rev;

    // Act: the DM moves ownership to Bram.
    let executed = executor
        .execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: bram,
                expected_rev: Some(rev),
            },
        )
        .expect("the DM moves the character");

    // Assert: exactly the header stamp changed; one event, no feed entry.
    assert_eq!(executed.warnings, Vec::new());
    assert_eq!(
        executed.events,
        vec![Event::EntityChanged {
            id: aria_pc,
            rev: rev + 1,
            kind: EntityKind::Pc,
        }]
    );
    assert!(executor.state().feed().is_empty(), "transfers add no feed");
    let after = statblock_of(executor.state(), aria_pc);
    assert_eq!(after.owner, bram);
    assert_eq!(after.rev, rev + 1);
    assert_eq!(after.updated_at, UnixTime::from_seconds(FIXED_TIME));
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.parent, before.parent);
    assert_eq!(after.acl, before.acl);
    assert_eq!(after.body, before.body);

    // Act and assert: a stale revision is a conflict and changes nothing.
    assert!(matches!(
        executor.execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: carl,
                expected_rev: Some(rev),
            },
        ),
        Err(EngineError::Conflict {
            entity,
            expected,
            actual,
        }) if entity == aria_pc && expected == rev && actual == rev + 1
    ));
    let unchanged = statblock_of(executor.state(), aria_pc);
    assert_eq!(unchanged.owner, bram);
    assert_eq!(unchanged.rev, rev + 1);
}

#[test]
fn a_write_grantee_cannot_transfer_ownership() {
    // Arrange: Aria owns the PC; Bram has write access but not ownership.
    let (campaign, dm, aria, bram, carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_statblock(
        &mut executor,
        dm,
        aria,
        EntityKind::Pc,
        "Aria",
        vec![write_grant(bram)],
        None,
    );
    add_item(
        &mut executor,
        bram,
        aria_pc,
        NewItem::carried(item("Rope"), 1),
    );
    let rev = statblock_of(executor.state(), aria_pc).rev;

    // Act: Bram tries to give the character to Carl.
    let result = executor.execute_as(
        bram,
        &Command::TransferEntity {
            entity: aria_pc,
            recipient: carl,
            expected_rev: Some(rev),
        },
    );

    // Assert: write access is not ownership. Aria, the owner, can do it.
    assert!(matches!(
        result,
        Err(EngineError::Unauthorized {
            actor,
            entity: Some(id),
        }) if actor == bram && id == aria_pc
    ));
    executor
        .execute_as(
            aria,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: carl,
                expected_rev: Some(rev),
            },
        )
        .expect("the owner transfers the character");
    assert_eq!(statblock_of(executor.state(), aria_pc).owner, carl);
}

#[test]
fn a_write_grantee_can_transfer_items_off_a_shared_sheet() {
    // Arrange: the DM shares Aria's sheet with Bram; Bram has his own PC.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_statblock(
        &mut executor,
        dm,
        aria,
        EntityKind::Pc,
        "Aria",
        vec![write_grant(bram)],
        None,
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        bram,
        aria_pc,
        NewItem::carried(item("Rope"), 1),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act: write access, not ownership, is enough to move items off a sheet.
    executor
        .execute_as(
            bram,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        )
        .expect("a write grantee distributes the shared sheet");

    // Assert: both sheets advanced and the item moved to Bram's own PC.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev + 1);
    assert!(aria_sheet.body.inventory.is_empty());
    assert_eq!(bram_sheet.rev, destination_rev + 1);
    assert_eq!(bram_sheet.body.inventory.len(), 1);
    assert_eq!(bram_sheet.body.inventory[0].item, item("Rope"));
}

#[test]
fn a_non_dm_cannot_send_to_the_dm_or_choose_a_destination() {
    // Arrange: Aria has a stack and Bram has a character; Carl has none.
    let (campaign, dm, aria, bram, carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let _dm_pc = create_pc(&mut executor, dm, dm, "Dungeon Master");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 3),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;
    let stranger = PlayerId::new();

    // Act and assert: each non-DM shortcut is refused.
    let transfer = |recipient: PlayerId, destination: Option<EntityId>| Command::TransferItems {
        source: aria_pc,
        moves: vec![ItemMove {
            entry: rope,
            qty: 1,
        }],
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
    assert_eq!(aria_sheet.body.inventory[0].qty, 3);
    assert_eq!(bram_sheet.rev, destination_rev);
    assert!(bram_sheet.body.inventory.is_empty());
}

#[test]
fn a_non_dm_cannot_transfer_an_entity_to_the_dm() {
    // Arrange: Aria owns a PC and the DM has a PC of their own.
    let (campaign, dm, aria, _bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let _dm_pc = create_pc(&mut executor, dm, dm, "Dungeon Master");
    let rev = statblock_of(executor.state(), aria_pc).rev;
    let stranger = PlayerId::new();

    // Act and assert: a non-DM may address only a player seat.
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: dm,
                expected_rev: Some(rev),
            },
        ),
        Err(EngineError::Malformed { .. })
    ));
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: stranger,
                expected_rev: Some(rev),
            },
        ),
        Err(EngineError::PlayerNotFound { player }) if player == stranger
    ));

    // Assert: the refusals changed nothing.
    let sheet = statblock_of(executor.state(), aria_pc);
    assert_eq!(sheet.owner, aria);
    assert_eq!(sheet.rev, rev);
}

#[test]
fn stale_revisions_and_bad_moves_are_refused_atomically() {
    // Arrange: Aria has three ropes; Bram has an empty pack.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 3),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;
    let ghost = InventoryEntryId::new();

    let transfer =
        |moves: Vec<ItemMove>,
         expected_source_rev: Option<u64>,
         expected_destination_rev: Option<u64>| Command::TransferItems {
            source: aria_pc,
            moves,
            recipient: bram,
            destination: None,
            expected_source_rev,
            expected_destination_rev,
        };

    // Act and assert: stale source revision.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(
                vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
                Some(source_rev + 1),
                Some(destination_rev),
            ),
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
            &transfer(
                vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
                Some(source_rev),
                Some(destination_rev + 1),
            ),
        ),
        Err(EngineError::Conflict {
            entity,
            expected,
            actual,
        }) if entity == bram_pc && expected == destination_rev + 1 && actual == destination_rev
    ));

    // Act and assert: empty moves.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(Vec::new(), Some(source_rev), Some(destination_rev)),
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Act and assert: non-positive and oversized quantities.
    for qty in [0, -1, 4] {
        assert!(matches!(
            executor.execute_as(
                aria,
                &transfer(
                    vec![ItemMove { entry: rope, qty }],
                    Some(source_rev),
                    Some(destination_rev),
                ),
            ),
            Err(EngineError::Malformed { .. })
        ));
    }

    // Act and assert: the same entry twice.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(
                vec![
                    ItemMove {
                        entry: rope,
                        qty: 1,
                    },
                    ItemMove {
                        entry: rope,
                        qty: 1,
                    },
                ],
                Some(source_rev),
                Some(destination_rev),
            ),
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Act and assert: an entry that is not on the source sheet.
    assert!(matches!(
        executor.execute_as(
            aria,
            &transfer(
                vec![ItemMove {
                    entry: ghost,
                    qty: 1,
                }],
                Some(source_rev),
                Some(destination_rev),
            ),
        ),
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Item(missing)),
        }) if entity == aria_pc && missing == ghost
    ));

    // Assert: every failure left both sheets exactly as they were.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev);
    assert_eq!(aria_sheet.body.inventory.len(), 1);
    assert_eq!(aria_sheet.body.inventory[0].qty, 3);
    assert_eq!(bram_sheet.rev, destination_rev);
    assert!(bram_sheet.body.inventory.is_empty());
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
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 3),
    );
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
    let ghost_entity = EntityId::new();
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    let transfer =
        |source: EntityId, recipient: PlayerId, destination: Option<EntityId>| -> Command {
            Command::TransferItems {
                source,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
                recipient,
                destination,
                expected_source_rev: None,
                expected_destination_rev: None,
            }
        };

    // Act and assert: a missing source and a missing destination are both the
    // command's own NotFound, not a missing-entry NotFound.
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
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferEntity {
                entity: ghost_entity,
                recipient: bram,
                expected_rev: None,
            },
        ),
        Err(EngineError::NotFound {
            entity,
            element: None,
        }) if entity == ghost_entity
    ));

    // Act and assert: neither side of an item move may be a non-statblock.
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
    assert_eq!(aria_sheet.body.inventory[0].qty, 3);
    assert_eq!(bram_sheet.rev, destination_rev);
    assert!(bram_sheet.body.inventory.is_empty());
}

#[test]
fn a_batch_moves_two_entries_in_one_transition() {
    // Arrange: a stack and a separate whole entry on Aria's sheet.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 3),
    );
    let torch = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Torch"), 2),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;

    // Act: one partial and one full move land together.
    let executed = executor
        .execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![
                    ItemMove {
                        entry: rope,
                        qty: 1,
                    },
                    ItemMove {
                        entry: torch,
                        qty: 2,
                    },
                ],
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        )
        .expect("the batch lands as one transition");

    // Assert: the source keeps the rope remainder and loses the torch; the
    // destination appends the two moved lines in move order. The batch is one
    // revision per sheet and one event per sheet.
    assert!(executor.state().feed().is_empty(), "transfers add no feed");
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev + 1);
    assert_eq!(bram_sheet.rev, destination_rev + 1);
    assert_eq!(aria_sheet.body.inventory.len(), 1);
    assert_eq!(aria_sheet.body.inventory[0].id, rope);
    assert_eq!(aria_sheet.body.inventory[0].qty, 2);
    assert_eq!(bram_sheet.body.inventory.len(), 2);
    assert_eq!(bram_sheet.body.inventory[0].item, item("Rope"));
    assert_eq!(bram_sheet.body.inventory[0].qty, 1);
    assert_eq!(bram_sheet.body.inventory[1].item, item("Torch"));
    assert_eq!(bram_sheet.body.inventory[1].qty, 2);
    assert_ne!(bram_sheet.body.inventory[0].id, rope);
    assert_ne!(bram_sheet.body.inventory[1].id, torch);
    assert_eq!(executed.events.len(), 2);
    assert!(executed.events.contains(&Event::EntityChanged {
        id: aria_pc,
        rev: source_rev + 1,
        kind: EntityKind::Pc,
    }));
    assert!(executed.events.contains(&Event::EntityChanged {
        id: bram_pc,
        rev: destination_rev + 1,
        kind: EntityKind::Pc,
    }));
}

#[test]
fn a_bad_move_in_a_batch_applies_none_of_the_moves() {
    // Arrange: two valid stacks and one missing entry id.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 3),
    );
    let torch = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Torch"), 2),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let destination_rev = statblock_of(executor.state(), bram_pc).rev;
    let ghost = InventoryEntryId::new();

    // Act: the first move is valid, the second names no entry.
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![
                    ItemMove {
                        entry: rope,
                        qty: 1,
                    },
                    ItemMove {
                        entry: ghost,
                        qty: 1,
                    },
                ],
                recipient: bram,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(destination_rev),
            },
        ),
        Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Item(missing)),
        }) if entity == aria_pc && missing == ghost
    ));

    // Assert: the valid first move was not applied either.
    let aria_sheet = statblock_of(executor.state(), aria_pc);
    let bram_sheet = statblock_of(executor.state(), bram_pc);
    assert_eq!(aria_sheet.rev, source_rev);
    assert_eq!(aria_sheet.body.inventory.len(), 2);
    assert_eq!(aria_sheet.body.inventory[0].id, rope);
    assert_eq!(aria_sheet.body.inventory[0].qty, 3);
    assert_eq!(aria_sheet.body.inventory[1].id, torch);
    assert_eq!(aria_sheet.body.inventory[1].qty, 2);
    assert_eq!(bram_sheet.rev, destination_rev);
    assert!(bram_sheet.body.inventory.is_empty());
}

#[test]
fn the_dm_can_pick_a_companions_sheet_explicitly() {
    // Arrange: Aria's PC and her wolf companion; Bram also has a PC.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let wolf = create_statblock(
        &mut executor,
        dm,
        aria,
        EntityKind::Companion,
        "Wolf",
        Vec::new(),
        None,
    );
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 2),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;
    let wolf_rev = statblock_of(executor.state(), wolf).rev;

    // Act: the DM names the companion as the destination.
    executor
        .execute_as(
            dm,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
                recipient: aria,
                destination: Some(wolf),
                expected_source_rev: Some(source_rev),
                expected_destination_rev: Some(wolf_rev),
            },
        )
        .expect("the DM targets the recipient's companion");

    // Assert: the wolf received the item.
    assert_eq!(statblock_of(executor.state(), wolf).body.inventory.len(), 1);
    assert_eq!(
        statblock_of(executor.state(), aria_pc).body.inventory[0].qty,
        1
    );

    // Act and assert: the destination must belong to the named recipient.
    assert!(matches!(
        executor.execute_as(
            dm,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
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
fn transfers_to_self_and_to_the_current_owner_are_malformed() {
    // Arrange: Aria owns a PC with one rope.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::carried(item("Rope"), 1),
    );
    let source_rev = statblock_of(executor.state(), aria_pc).rev;

    // Act and assert: an item cannot be "transferred" to its own sheet.
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 1,
                }],
                recipient: aria,
                destination: None,
                expected_source_rev: Some(source_rev),
                expected_destination_rev: None,
            },
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Act and assert: an entity cannot be transferred to its current owner.
    assert!(matches!(
        executor.execute_as(
            aria,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: aria,
                expected_rev: Some(source_rev),
            },
        ),
        Err(EngineError::Malformed { .. })
    ));

    // Act: the DM moves it to Bram, then a repeat transfer to Bram is refused.
    executor
        .execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: bram,
                expected_rev: Some(source_rev),
            },
        )
        .expect("the DM moves the character");
    assert!(matches!(
        executor.execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: bram,
                expected_rev: None,
            },
        ),
        Err(EngineError::Malformed { .. })
    ));
}

#[test]
fn a_split_charged_item_gets_a_fresh_resource_id_on_the_destination() {
    // Arrange: two charged wands; one nested resource id.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let (wand, original_resource) = charged_item("Wand of Magic Missiles", 7);
    let entry = add_item(&mut executor, aria, aria_pc, NewItem::carried(wand, 2));

    // Act: split the stack; each copy keeps its own charge tracker.
    executor
        .execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove { entry, qty: 1 }],
                recipient: bram,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        )
        .expect("the charged stack splits");

    // Assert: the moved item has a new nested resource id, never a duplicate
    // of the source's.
    let source_sheet = statblock_of(executor.state(), aria_pc);
    let destination_sheet = statblock_of(executor.state(), bram_pc);
    let source = &source_sheet.body.inventory[0];
    let destination = &destination_sheet.body.inventory[0];
    let InventoryItem::Inline(source_item) = &source.item else {
        panic!("the source item is inline");
    };
    let InventoryItem::Inline(destination_item) = &destination.item else {
        panic!("the destination item is inline");
    };
    let source_resource = source_item
        .resource
        .as_ref()
        .expect("the source keeps its tracker");
    let destination_resource = destination_item
        .resource
        .as_ref()
        .expect("the destination gets a tracker");
    assert_eq!(source_resource.id, original_resource);
    assert_ne!(destination_resource.id, original_resource);
    assert_eq!(destination_resource.current, 7);
    assert_eq!(destination_resource.max, Formula::Const(7));
}

#[test]
fn a_whole_charged_item_gets_a_fresh_resource_id_on_the_destination() {
    // Arrange: one charged wand moved as a whole entry.
    let (campaign, _dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let (wand, original_resource) = charged_item("Wand of Magic Missiles", 7);
    let entry = add_item(&mut executor, aria, aria_pc, NewItem::carried(wand, 1));

    // Act: move the whole entry.
    executor
        .execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove { entry, qty: 1 }],
                recipient: bram,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        )
        .expect("the whole charged entry moves");

    // Assert: the source entry is gone and the destination copy has a new
    // nested tracker, not the source's id.
    let source_sheet = statblock_of(executor.state(), aria_pc);
    let destination_sheet = statblock_of(executor.state(), bram_pc);
    assert!(source_sheet.body.inventory.is_empty());
    let destination = &destination_sheet.body.inventory[0];
    let InventoryItem::Inline(destination_item) = &destination.item else {
        panic!("the destination item is inline");
    };
    let destination_resource = destination_item
        .resource
        .as_ref()
        .expect("the destination gets a tracker");
    assert_ne!(destination_resource.id, original_resource);
    assert_eq!(destination_resource.current, 7);
    assert_eq!(destination_resource.max, Formula::Const(7));
}

#[test]
fn a_handout_moves_but_its_content_is_unchanged() {
    // Arrange: a DM-owned handout given to Aria.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let content = HandoutContent::Item {
        item: inline("Potion of Healing"),
    };
    let handout = create_handout(&mut executor, dm, aria, "Loot", content.clone());
    let rev = entity_of(executor.state(), handout).rev();

    // Act: the owner hands the handout itself to Bram.
    executor
        .execute_as(
            aria,
            &Command::TransferEntity {
                entity: handout,
                recipient: bram,
                expected_rev: Some(rev),
            },
        )
        .expect("the owner transfers the handout");

    // Assert: ownership moved, the document body did not.
    let moved = entity_of(executor.state(), handout);
    assert_eq!(moved.owner(), bram);
    assert_eq!(moved.rev(), rev + 1);
    let AnyEntity::Handout(moved) = moved else {
        panic!("the transferred document is a handout");
    };
    assert_eq!(moved.body.content, content);
}

#[test]
fn entity_transfers_do_not_cascade_to_a_summon() {
    // Arrange: Aria's PC and a summon linked to it.
    let (campaign, dm, aria, bram, _carl) = campaign();
    let mut store = MemoryStore::new();
    seed(&mut store, &campaign);
    let state = LoadedCampaign::from_snapshot(store.load().expect("the campaign was just seeded"));
    let mut executor = Executor::new(Box::new(state), &mut store, Box::new(Env));
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let summon = create_statblock(
        &mut executor,
        dm,
        aria,
        EntityKind::Summon,
        "Echo",
        Vec::new(),
        Some(aria_pc),
    );
    let rev = statblock_of(executor.state(), aria_pc).rev;

    // Act: transfer the character, not the summon.
    executor
        .execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: bram,
                expected_rev: Some(rev),
            },
        )
        .expect("the DM moves the character");

    // Assert: the summon stays with Aria and keeps its parent link.
    let summon = statblock_of(executor.state(), summon);
    assert_eq!(summon.owner, aria);
    assert_eq!(summon.parent, Some(aria_pc));
}

#[test]
fn transfers_survive_a_store_reopen() {
    // Arrange: a real SQLite campaign with two characters and three ropes.
    let dir = TempDir::new().expect("a temporary directory");
    let path = dir.path().join("campaign.sqlite");
    let mut store = SqliteStore::open(&path).expect("the store opens");
    let (campaign, dm, aria, bram, _carl) = campaign();
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
    let aria_pc = create_pc(&mut executor, aria, aria, "Aria");
    let bram_pc = create_pc(&mut executor, bram, bram, "Bram");
    let rope = add_item(
        &mut executor,
        aria,
        aria_pc,
        NewItem::equipped(item("Rope"), 3, true, "50 feet"),
    );

    // Act: split the stack, then move the character itself.
    executor
        .execute_as(
            aria,
            &Command::TransferItems {
                source: aria_pc,
                moves: vec![ItemMove {
                    entry: rope,
                    qty: 2,
                }],
                recipient: bram,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
        )
        .expect("the item transfer persists");
    executor
        .execute_as(
            dm,
            &Command::TransferEntity {
                entity: aria_pc,
                recipient: bram,
                expected_rev: None,
            },
        )
        .expect("the ownership transfer persists");
    drop(executor);
    drop(store);

    // Assert: both transfers are on disk.
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
    assert_eq!(aria_sheet.owner, bram);
    assert_eq!(aria_sheet.body.inventory.len(), 1);
    assert_eq!(aria_sheet.body.inventory[0].qty, 1);
    assert_eq!(bram_sheet.body.inventory.len(), 1);
    assert_eq!(bram_sheet.body.inventory[0].qty, 2);
    assert_eq!(bram_sheet.body.inventory[0].state, ItemState::Carried);
    assert!(!bram_sheet.body.inventory[0].attuned);
}

#[test]
fn the_wire_dto_round_trips_and_converts_to_the_command() {
    // Arrange: one transfer over the wire, with one revision pinned and the
    // destination left implicit.
    let source = EntityId::new();
    let entry = InventoryEntryId::new();
    let recipient = PlayerId::new();
    let dto = CommandDto::TransferItems {
        source,
        moves: vec![ItemMoveDto { entry, qty: 2 }],
        recipient,
        destination: None,
        expected_source_rev: Some(4),
        expected_destination_rev: None,
    };

    // Act: serialize, read the shape back, deserialize, convert.
    let value = serde_json::to_value(&dto).expect("the DTO serializes");
    assert_eq!(value["type"], "transfer_items");
    assert_eq!(value["source"], source.to_string());
    assert_eq!(value["moves"][0]["entry"], entry.to_string());
    assert_eq!(value["moves"][0]["qty"], 2);
    assert!(value.get("destination").is_none());
    assert_eq!(value["expected_source_rev"], 4);
    assert!(value.get("expected_destination_rev").is_none());
    let decoded: CommandDto = serde_json::from_value(value).expect("the DTO round-trips");
    assert_eq!(decoded, dto);
    let command: Command = decoded.try_into().expect("this build knows the command");

    // Assert: the engine command carries the same move list.
    assert_eq!(
        command,
        Command::TransferItems {
            source,
            moves: vec![ItemMove { entry, qty: 2 }],
            recipient,
            destination: None,
            expected_source_rev: Some(4),
            expected_destination_rev: None,
        }
    );

    // And the ownership-transfer variant has the same shape.
    let entity = EntityId::new();
    let dto = CommandDto::TransferEntity {
        entity,
        recipient,
        expected_rev: Some(7),
    };
    let value = serde_json::to_value(&dto).expect("the DTO serializes");
    assert_eq!(value["type"], "transfer_entity");
    assert_eq!(value["expected_rev"], 7);
    let command: Command = dto.try_into().expect("this build knows the command");
    assert_eq!(
        command,
        Command::TransferEntity {
            entity,
            recipient,
            expected_rev: Some(7),
        }
    );
}

#[test]
fn the_wire_dto_preserves_an_explicit_destination_and_revisions() {
    // Arrange: a DM move with both the destination and its revision pinned,
    // and two lines in the batch.
    let source = EntityId::new();
    let destination = EntityId::new();
    let recipient = PlayerId::new();
    let first = InventoryEntryId::new();
    let second = InventoryEntryId::new();
    let dto = CommandDto::TransferItems {
        source,
        moves: vec![
            ItemMoveDto {
                entry: first,
                qty: 1,
            },
            ItemMoveDto {
                entry: second,
                qty: 2,
            },
        ],
        recipient,
        destination: Some(destination),
        expected_source_rev: Some(4),
        expected_destination_rev: Some(9),
    };

    // Act: serialize and read every pinned field back.
    let value = serde_json::to_value(&dto).expect("the DTO serializes");
    assert_eq!(value["type"], "transfer_items");
    assert_eq!(value["source"], source.to_string());
    assert_eq!(value["destination"], destination.to_string());
    assert_eq!(value["expected_source_rev"], 4);
    assert_eq!(value["expected_destination_rev"], 9);
    assert_eq!(value["moves"][0]["entry"], first.to_string());
    assert_eq!(value["moves"][0]["qty"], 1);
    assert_eq!(value["moves"][1]["entry"], second.to_string());
    assert_eq!(value["moves"][1]["qty"], 2);

    // Assert: the round-trip and the engine command keep both fields.
    let decoded: CommandDto = serde_json::from_value(value).expect("the DTO round-trips");
    assert_eq!(decoded, dto);
    let command: Command = decoded.try_into().expect("this build knows the command");
    assert_eq!(
        command,
        Command::TransferItems {
            source,
            moves: vec![
                ItemMove {
                    entry: first,
                    qty: 1,
                },
                ItemMove {
                    entry: second,
                    qty: 2,
                },
            ],
            recipient,
            destination: Some(destination),
            expected_source_rev: Some(4),
            expected_destination_rev: Some(9),
        }
    );

    // And an omitted entity revision stays omitted on the wire.
    let entity = EntityId::new();
    let dto = CommandDto::TransferEntity {
        entity,
        recipient,
        expected_rev: None,
    };
    let value = serde_json::to_value(&dto).expect("the DTO serializes");
    assert!(value.get("expected_rev").is_none());
    let command: Command = dto.try_into().expect("this build knows the command");
    assert_eq!(
        command,
        Command::TransferEntity {
            entity,
            recipient,
            expected_rev: None,
        }
    );
}
