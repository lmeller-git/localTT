//! Item, entity and coin transfer handlers .
//!
//! A transfer is one-way: the recipient gains the asset immediately and can
//! hand it back with the same commands. Item batches and coin purses are atomic
//! across the source and destination, and entity ownership moves change the
//! header only.

use std::collections::BTreeSet;

use localtt_core::types::{
    AnyEntity,
    Campaign,
    Coins,
    EntityId,
    EntityKind,
    InventoryEntry,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    Player,
    PlayerId,
    ResourceId,
    Role,
    StatblockEntity,
};

use super::{
    CommandEnv,
    Event,
    ItemMove,
    Transition,
    load_statblock,
    load_target,
    one_upsert,
    stamp,
    two_upserts,
};
use crate::{
    authz,
    error::{ElementRef, EngineError},
    state::WorkingSet,
};

/// The inputs of a `TransferItems` command, grouped to keep the handler small.
#[derive(Debug)]
pub(super) struct ItemTransfer<'a> {
    /// The statblock the items leave.
    pub(super) source: EntityId,
    /// The lines to move.
    pub(super) moves: &'a [ItemMove],
    /// The player receiving the items.
    pub(super) recipient: PlayerId,
    /// The recipient's statblock, when the DM pinned one.
    pub(super) destination: Option<EntityId>,
    /// Optional compare-and-swap on the source's revision.
    pub(super) expected_source_rev: Option<u64>,
    /// Optional compare-and-swap on the destination's revision.
    pub(super) expected_destination_rev: Option<u64>,
}

/// The inputs of a `TransferCoins` command.
#[derive(Debug)]
pub(super) struct CoinTransfer {
    /// The statblock the coins leave.
    pub(super) source: EntityId,
    /// The exact per-denomination amounts to move.
    pub(super) amount: Coins,
    /// The player receiving the coins.
    pub(super) recipient: PlayerId,
    /// The recipient's statblock, when the DM pinned one.
    pub(super) destination: Option<EntityId>,
    /// Optional compare-and-swap on the source's revision.
    pub(super) expected_source_rev: Option<u64>,
    /// Optional compare-and-swap on the destination's revision.
    pub(super) expected_destination_rev: Option<u64>,
}

/// The inputs of a `TransferEntity` command.
#[derive(Debug)]
pub(super) struct EntityTransfer {
    /// The entity whose owner changes.
    pub(super) entity: EntityId,
    /// The player who becomes the owner.
    pub(super) recipient: PlayerId,
    /// Optional compare-and-swap on the entity's revision.
    pub(super) expected_rev: Option<u64>,
}

/// Moves a batch of inventory lines to the recipient's statblock.
///
/// The whole batch lands on both sheets in one transition, or neither sheet
/// changes.
pub(super) fn transfer_items_command(
    state: &dyn WorkingSet,
    transfer: ItemTransfer<'_>,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let Some(actor) = actor else {
        return Err(EngineError::Malformed {
            reason: "an item transfer requires an actor".to_owned(),
        });
    };
    let recipient = recipient_seat(state.campaign(), transfer.recipient)?;
    check_recipient_rules(
        state.campaign(),
        actor,
        recipient,
        transfer.destination,
        "items",
    )?;

    let destination = resolve_destination(state, transfer.recipient, transfer.destination)?;
    if destination == transfer.source {
        return Err(EngineError::Malformed {
            reason: "an item transfer needs a different source and destination".to_owned(),
        });
    }
    let mut destination_sheet =
        load_statblock(state, destination, transfer.expected_destination_rev)?;
    if transfer.destination.is_some() && destination_sheet.owner != transfer.recipient {
        return Err(EngineError::Malformed {
            reason: format!(
                "destination {destination} is not owned by player {}",
                transfer.recipient
            ),
        });
    }
    let mut source_sheet = load_statblock(state, transfer.source, transfer.expected_source_rev)?;
    validate_moves(&source_sheet, transfer.moves)?;
    apply_moves(&mut source_sheet, &mut destination_sheet, transfer.moves);

    let now = env.now();
    Ok(two_upserts(
        AnyEntity::from(source_sheet),
        AnyEntity::from(destination_sheet),
        now,
        Vec::new(),
    ))
}

/// Moves coins to the recipient's statblock.
///
/// The whole purse lands on both sheets in one transition, or neither sheet
/// changes.
pub(super) fn transfer_coins_command(
    state: &dyn WorkingSet,
    transfer: CoinTransfer,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let Some(actor) = actor else {
        return Err(EngineError::Malformed {
            reason: "a coin transfer requires an actor".to_owned(),
        });
    };
    let recipient = recipient_seat(state.campaign(), transfer.recipient)?;
    check_recipient_rules(
        state.campaign(),
        actor,
        recipient,
        transfer.destination,
        "coins",
    )?;

    let destination = resolve_destination(state, transfer.recipient, transfer.destination)?;
    if destination == transfer.source {
        return Err(EngineError::Malformed {
            reason: "a coin transfer needs a different source and destination".to_owned(),
        });
    }
    let mut destination_sheet =
        load_statblock(state, destination, transfer.expected_destination_rev)?;
    if transfer.destination.is_some() && destination_sheet.owner != transfer.recipient {
        return Err(EngineError::Malformed {
            reason: format!(
                "destination {destination} is not owned by player {}",
                transfer.recipient
            ),
        });
    }
    let mut source_sheet = load_statblock(state, transfer.source, transfer.expected_source_rev)?;
    validate_amount(&source_sheet, &destination_sheet, transfer.amount)?;
    apply_amount(&mut source_sheet, &mut destination_sheet, transfer.amount);

    let now = env.now();
    Ok(two_upserts(
        AnyEntity::from(source_sheet),
        AnyEntity::from(destination_sheet),
        now,
        Vec::new(),
    ))
}

/// Rejects a negative, empty, oversized or overflowing purse before either
/// document is touched.
fn validate_amount(
    source: &StatblockEntity,
    destination: &StatblockEntity,
    amount: Coins,
) -> Result<(), EngineError> {
    let moving = [amount.cp, amount.sp, amount.ep, amount.gp, amount.pp];
    if moving.iter().any(|value| *value < 0) {
        return Err(EngineError::Malformed {
            reason: "coin amounts must not be negative".to_owned(),
        });
    }
    if moving.iter().all(|value| *value == 0) {
        return Err(EngineError::Malformed {
            reason: "a coin transfer needs at least one positive amount".to_owned(),
        });
    }

    let stored = source.body.coins;
    for (name, moving, stored) in [
        ("cp", amount.cp, stored.cp),
        ("sp", amount.sp, stored.sp),
        ("ep", amount.ep, stored.ep),
        ("gp", amount.gp, stored.gp),
        ("pp", amount.pp, stored.pp),
    ] {
        if moving > 0 && moving > stored {
            return Err(EngineError::Malformed {
                reason: format!("source has {stored} {name}, cannot move {moving}"),
            });
        }
    }

    let held = destination.body.coins;
    for (name, moving, held_field) in [
        ("cp", amount.cp, held.cp),
        ("sp", amount.sp, held.sp),
        ("ep", amount.ep, held.ep),
        ("gp", amount.gp, held.gp),
        ("pp", amount.pp, held.pp),
    ] {
        if held_field.checked_add(moving).is_none() {
            return Err(EngineError::Malformed {
                reason: format!("destination cannot hold {moving} more {name}"),
            });
        }
    }
    Ok(())
}

/// Applies a validated purse: the exact denominations leave the source and
/// arrive at the destination.
fn apply_amount(source: &mut StatblockEntity, destination: &mut StatblockEntity, amount: Coins) {
    source.body.coins.cp -= amount.cp;
    source.body.coins.sp -= amount.sp;
    source.body.coins.ep -= amount.ep;
    source.body.coins.gp -= amount.gp;
    source.body.coins.pp -= amount.pp;

    destination.body.coins.cp += amount.cp;
    destination.body.coins.sp += amount.sp;
    destination.body.coins.ep += amount.ep;
    destination.body.coins.gp += amount.gp;
    destination.body.coins.pp += amount.pp;
}

/// Moves one entity's ownership to the recipient.
///
/// The owner header, revision and update stamp change, and a queued initiative
/// entry for the entity has its owner snapshot refreshed.
pub(super) fn transfer_entity_command(
    state: &dyn WorkingSet,
    transfer: EntityTransfer,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let Some(actor) = actor else {
        return Err(EngineError::Malformed {
            reason: "an entity transfer requires an actor".to_owned(),
        });
    };
    let mut document = load_target(state, transfer.entity, transfer.expected_rev)?;
    let recipient = recipient_seat(state.campaign(), transfer.recipient)?;
    if document.owner() == transfer.recipient {
        return Err(EngineError::Malformed {
            reason: format!(
                "entity {} is already owned by player {}",
                transfer.entity, transfer.recipient
            ),
        });
    }
    if !authz::dm(state.campaign(), actor) && recipient.role != Role::Player {
        return Err(EngineError::Malformed {
            reason: "only the DM may transfer an entity to another DM".to_owned(),
        });
    }
    match &mut document {
        AnyEntity::Statblock(document) => document.owner = transfer.recipient,
        AnyEntity::Note(document) => document.owner = transfer.recipient,
        AnyEntity::Handout(document) => document.owner = transfer.recipient,
    }
    stamp(&mut document, env.now());
    let mut campaign = state.campaign().clone();
    let queued = campaign
        .initiative
        .entries
        .iter_mut()
        .find(|entry| entry.entity == transfer.entity);
    let mut transition = one_upsert(document, Vec::new());
    if let Some(entry) = queued {
        entry.owner = transfer.recipient;
        transition.campaign = Some(campaign);
        transition.events.push(Event::CampaignChanged);
    }
    Ok(transition)
}

/// Resolves the recipient's seat, or reports it missing.
fn recipient_seat(campaign: &Campaign, player: PlayerId) -> Result<&Player, EngineError> {
    campaign
        .player(player)
        .ok_or(EngineError::PlayerNotFound { player })
}

/// Rejects a non-DM recipient that is not a player, or a non-DM explicit
/// destination.
fn check_recipient_rules(
    campaign: &Campaign,
    actor: PlayerId,
    recipient: &Player,
    destination: Option<EntityId>,
    asset: &str,
) -> Result<(), EngineError> {
    if authz::dm(campaign, actor) {
        return Ok(());
    }
    if recipient.role != Role::Player {
        return Err(EngineError::Malformed {
            reason: format!("only the DM may transfer {asset} to another DM"),
        });
    }
    if destination.is_some() {
        return Err(EngineError::Malformed {
            reason: "only the DM may pick the recipient's statblock".to_owned(),
        });
    }
    Ok(())
}

/// Resolves the destination statblock: the pinned id, or the recipient's
/// primary character.
fn resolve_destination(
    state: &dyn WorkingSet,
    recipient: PlayerId,
    destination: Option<EntityId>,
) -> Result<EntityId, EngineError> {
    if let Some(destination) = destination {
        return Ok(destination);
    }
    primary_character(state, recipient)?.ok_or_else(|| EngineError::Malformed {
        reason: format!("player {recipient} has no player character to receive the transfer"),
    })
}

/// The `Pc` owned by `recipient` with the earliest `created_at`, breaking ties
/// on the lowest entity id.
fn primary_character(
    state: &dyn WorkingSet,
    recipient: PlayerId,
) -> Result<Option<EntityId>, EngineError> {
    Ok(state
        .headers()
        .into_iter()
        .filter(|header| header.kind == EntityKind::Pc && header.owner == recipient)
        .map(|header| (header.created_at, header.id))
        .min()
        .map(|(_, id)| id))
}

/// Rejects an empty, duplicate, non-positive or oversized batch before any
/// document is touched.
fn validate_moves(source: &StatblockEntity, moves: &[ItemMove]) -> Result<(), EngineError> {
    if moves.is_empty() {
        return Err(EngineError::Malformed {
            reason: "an item transfer needs at least one move".to_owned(),
        });
    }
    let mut seen = BTreeSet::new();
    for line in moves {
        if line.qty <= 0 {
            return Err(EngineError::Malformed {
                reason: format!("inventory quantity {} is not positive", line.qty),
            });
        }
        if !seen.insert(line.entry) {
            return Err(EngineError::Malformed {
                reason: format!("entry {} appears twice in one batch", line.entry),
            });
        }
        let Some(stored) = source
            .body
            .inventory
            .iter()
            .find(|stored| stored.id == line.entry)
        else {
            return Err(EngineError::NotFound {
                entity: source.id,
                element: Some(ElementRef::Item(line.entry)),
            });
        };
        if line.qty > stored.qty {
            return Err(EngineError::Malformed {
                reason: format!(
                    "entry {} has {} units, cannot move {}",
                    line.entry, stored.qty, line.qty
                ),
            });
        }
    }
    Ok(())
}

/// Applies a validated batch in move order: full moves remove the source line,
/// partial moves decrement it, and every destination line is fresh, carried,
/// unattuned and owns a new nested resource id.
fn apply_moves(
    source: &mut StatblockEntity,
    destination: &mut StatblockEntity,
    moves: &[ItemMove],
) {
    for line in moves {
        let index = source
            .body
            .inventory
            .iter()
            .position(|stored| stored.id == line.entry)
            .expect("validate_moves checked every entry");
        let mut item = source.body.inventory[index].item.clone();
        if let InventoryItem::Inline(inline) = &mut item
            && let Some(resource) = &mut inline.resource
        {
            resource.id = ResourceId::new();
        }
        let notes = source.body.inventory[index].notes.clone();
        if line.qty == source.body.inventory[index].qty {
            source.body.inventory.remove(index);
        } else {
            source.body.inventory[index].qty -= line.qty;
        }
        destination.body.inventory.push(InventoryEntry {
            id: InventoryEntryId::new(),
            item,
            qty: line.qty,
            state: ItemState::Carried,
            attuned: false,
            notes,
        });
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            AclEntry,
            AclLevel,
            AnyEntity,
            Campaign,
            Coins,
            EntityId,
            EntityKind,
            InitiativeEntry,
            PlayerId,
            UnixTime,
        },
    };

    use super::*;
    use crate::{commands::Event, ports::store::CampaignSnapshot, state::LoadedCampaign};

    const NOW: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(NOW)
        }
    }

    fn campaign() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Trading");
        let dm = builder.dm("DM");
        let player = builder.player("Aria");
        (builder.build(), dm, player)
    }

    fn statblock(owner: PlayerId, kind: EntityKind, created_at: UnixTime) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(12)).hp(12, 12);
        AnyEntity::from(
            builder
                .entity_builder(kind, EntityId::new(), "Aria", owner)
                .expect("abilities and hp are set")
                .created_at(created_at)
                .build()
                .expect("a statblock kind pairs with a statblock body"),
        )
    }

    fn state_with(campaign: Campaign, entities: Vec<AnyEntity>) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign,
            entities,
            feed: Vec::new(),
        })
    }

    #[test]
    fn primary_character_uses_created_at_then_the_lowest_id() {
        let (campaign, dm, player) = campaign();
        let early = statblock(player, EntityKind::Pc, UnixTime::from_seconds(50));
        let tied_a = statblock(player, EntityKind::Pc, UnixTime::from_seconds(100));
        let tied_b = statblock(player, EntityKind::Pc, UnixTime::from_seconds(100));
        let companion = statblock(player, EntityKind::Companion, UnixTime::from_seconds(1));
        let foreign = statblock(dm, EntityKind::Pc, UnixTime::from_seconds(1));

        let state = state_with(
            campaign.clone(),
            vec![early.clone(), tied_a, tied_b, companion, foreign],
        );
        assert_eq!(
            primary_character(&state, player).expect("the working set loads"),
            Some(early.id()),
            "the earliest created_at wins"
        );

        let tied_a = statblock(player, EntityKind::Pc, UnixTime::from_seconds(100));
        let tied_b = statblock(player, EntityKind::Pc, UnixTime::from_seconds(100));
        let expected = tied_a.id().min(tied_b.id());
        let state = state_with(campaign, vec![tied_a, tied_b]);
        assert_eq!(
            primary_character(&state, player).expect("the working set loads"),
            Some(expected),
            "equal created_at falls back to the lowest id"
        );
        assert_eq!(
            primary_character(&state, PlayerId::new()).expect("the working set loads"),
            None,
            "a player with no PC has no primary character"
        );
    }

    fn party() -> (Campaign, PlayerId, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Trading");
        let dm = builder.dm("DM");
        let aria = builder.player("Aria");
        let bram = builder.player("Bram");
        (builder.build(), dm, aria, bram)
    }

    #[test]
    fn an_entity_transfer_moves_the_owner_header_and_refreshes_initiative() {
        let (mut campaign, dm, aria, bram) = party();
        let mut pc = statblock(aria, EntityKind::Pc, UnixTime::from_seconds(100));
        if let AnyEntity::Statblock(document) = &mut pc {
            document.acl = vec![AclEntry {
                player: bram,
                level: AclLevel::Write,
                field_mask: None,
            }];
            document.parent = Some(EntityId::new());
        }
        let id = pc.id();
        let before = pc.clone();
        campaign.initiative.entries.push(InitiativeEntry {
            entity: id,
            name: "Aria".to_owned(),
            owner: aria,
            value: 15,
        });
        let state = state_with(campaign, vec![pc]);

        let transition = transfer_entity_command(
            &state,
            EntityTransfer {
                entity: id,
                recipient: bram,
                expected_rev: Some(0),
            },
            Some(dm),
            &mut FixedEnv,
        )
        .expect("the DM transfers the character");

        assert_eq!(
            transition.events,
            vec![
                Event::EntityChanged {
                    id,
                    rev: 1,
                    kind: EntityKind::Pc,
                },
                Event::CampaignChanged,
            ]
        );
        assert_eq!(
            transition
                .campaign
                .as_ref()
                .expect("the queued row changed owner")
                .initiative
                .entries[0]
                .owner,
            bram
        );
        let after = &transition.upserts[0];
        assert_eq!(after.owner(), bram);
        assert_eq!(after.rev(), 1);
        assert_eq!(after.updated_at(), UnixTime::from_seconds(NOW));
        assert_eq!(after.created_at(), before.created_at());
        assert_eq!(after.parent(), before.parent());
        assert_eq!(after.acl(), before.acl());
        let (AnyEntity::Statblock(after), AnyEntity::Statblock(before)) = (after, &before) else {
            panic!("both documents are statblocks");
        };
        assert_eq!(after.body, before.body);
    }

    #[test]
    fn an_entity_transfer_refuses_self_missing_and_non_player_targets() {
        let (campaign, dm, aria, bram) = party();
        let pc = statblock(aria, EntityKind::Pc, UnixTime::from_seconds(100));
        let id = pc.id();
        let state = state_with(campaign, vec![pc]);
        let transfer = |recipient| EntityTransfer {
            entity: id,
            recipient,
            expected_rev: None,
        };

        assert!(matches!(
            transfer_entity_command(&state, transfer(aria), Some(dm), &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            transfer_entity_command(&state, transfer(dm), Some(aria), &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            transfer_entity_command(&state, transfer(PlayerId::new()), Some(aria), &mut FixedEnv),
            Err(EngineError::PlayerNotFound { .. })
        ));
        let ghost = EntityTransfer {
            entity: EntityId::new(),
            recipient: bram,
            expected_rev: None,
        };
        assert!(matches!(
            transfer_entity_command(&state, ghost, Some(dm), &mut FixedEnv),
            Err(EngineError::NotFound { element: None, .. })
        ));
        assert!(matches!(
            transfer_entity_command(&state, transfer(bram), None, &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
    }

    fn purse(owner: PlayerId, kind: EntityKind, coins: Coins) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder
            .abilities(Abilities::all(12))
            .hp(12, 12)
            .coins(coins);
        AnyEntity::from(
            builder
                .entity_builder(kind, EntityId::new(), "Purse", owner)
                .expect("abilities and hp are set")
                .build()
                .expect("a statblock kind pairs with a statblock body"),
        )
    }

    fn coins_of(transition: &Transition, id: EntityId) -> Coins {
        match transition.upserts.iter().find(|entity| entity.id() == id) {
            Some(AnyEntity::Statblock(document)) => document.body.coins,
            other => panic!("{id} is a statblock, got {other:?}"),
        }
    }

    #[test]
    fn a_coin_transfer_moves_the_named_denominations_and_stamps_both_sheets() {
        let (campaign, _dm, aria, bram) = party();
        let source = purse(
            aria,
            EntityKind::Pc,
            Coins {
                cp: 1,
                sp: 2,
                ep: 3,
                gp: 4,
                pp: 5,
            },
        );
        let destination = purse(bram, EntityKind::Pc, Coins::default());
        let source_id = source.id();
        let destination_id = destination.id();
        let state = state_with(campaign, vec![source, destination]);

        let transition = transfer_coins_command(
            &state,
            CoinTransfer {
                source: source_id,
                amount: Coins {
                    sp: 1,
                    gp: 3,
                    ..Coins::default()
                },
                recipient: bram,
                destination: None,
                expected_source_rev: Some(0),
                expected_destination_rev: Some(0),
            },
            Some(aria),
            &mut FixedEnv,
        )
        .expect("the purse moves");

        assert!(transition.feed_appends.is_empty(), "transfers add no feed");
        assert_eq!(transition.upserts.len(), 2);
        assert!(transition.upserts[0].id() < transition.upserts[1].id());
        assert!(
            transition.upserts.iter().all(|entity| entity.rev() == 1),
            "each touched sheet stamps exactly one revision"
        );
        assert_eq!(transition.events.len(), 2);
        assert!(
            transition
                .events
                .iter()
                .all(|event| matches!(event, Event::EntityChanged { rev: 1, .. }))
        );
        assert_eq!(
            coins_of(&transition, source_id),
            Coins {
                cp: 1,
                sp: 1,
                ep: 3,
                gp: 1,
                pp: 5,
            }
        );
        assert_eq!(
            coins_of(&transition, destination_id),
            Coins {
                sp: 1,
                gp: 3,
                ..Coins::default()
            }
        );
    }

    #[test]
    fn a_coin_transfer_requires_an_actor_and_the_recipient_rules() {
        let (campaign, dm, aria, bram) = party();
        let source = purse(
            aria,
            EntityKind::Pc,
            Coins {
                gp: 1,
                ..Coins::default()
            },
        );
        let source_id = source.id();
        let state = state_with(campaign, vec![source]);
        let transfer = |recipient, destination| CoinTransfer {
            source: source_id,
            amount: Coins {
                gp: 1,
                ..Coins::default()
            },
            recipient,
            destination,
            expected_source_rev: None,
            expected_destination_rev: None,
        };

        assert!(matches!(
            transfer_coins_command(&state, transfer(bram, None), None, &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            transfer_coins_command(
                &state,
                transfer(PlayerId::new(), None),
                Some(aria),
                &mut FixedEnv
            ),
            Err(EngineError::PlayerNotFound { .. })
        ));
        assert!(matches!(
            transfer_coins_command(&state, transfer(dm, None), Some(aria), &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            transfer_coins_command(
                &state,
                transfer(bram, Some(source_id)),
                Some(aria),
                &mut FixedEnv
            ),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            transfer_coins_command(&state, transfer(aria, None), Some(aria), &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
    }

    #[test]
    fn a_non_dm_can_move_coins_from_their_companion_to_their_primary_character() {
        let (campaign, _dm, aria, _bram) = party();
        let pc = purse(aria, EntityKind::Pc, Coins::default());
        let companion = purse(
            aria,
            EntityKind::Companion,
            Coins {
                gp: 2,
                ..Coins::default()
            },
        );
        let pc_id = pc.id();
        let companion_id = companion.id();
        let state = state_with(campaign, vec![pc, companion]);

        let transition = transfer_coins_command(
            &state,
            CoinTransfer {
                source: companion_id,
                amount: Coins {
                    gp: 1,
                    ..Coins::default()
                },
                recipient: aria,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
            Some(aria),
            &mut FixedEnv,
        )
        .expect("a non-DM moves coins from their companion to their PC");

        assert_eq!(
            coins_of(&transition, companion_id),
            Coins {
                gp: 1,
                ..Coins::default()
            }
        );
        assert_eq!(
            coins_of(&transition, pc_id),
            Coins {
                gp: 1,
                ..Coins::default()
            }
        );
    }

    #[test]
    fn a_dm_can_move_coins_to_another_dm() {
        let mut builder = CampaignBuilder::new("Trading");
        let dm = builder.dm("DM");
        let other_dm = builder.dm("Other DM");
        let campaign = builder.build();
        let source = purse(
            dm,
            EntityKind::Pc,
            Coins {
                gp: 3,
                ..Coins::default()
            },
        );
        let destination = purse(other_dm, EntityKind::Pc, Coins::default());
        let source_id = source.id();
        let destination_id = destination.id();
        let state = state_with(campaign, vec![source, destination]);

        let transition = transfer_coins_command(
            &state,
            CoinTransfer {
                source: source_id,
                amount: Coins {
                    gp: 2,
                    ..Coins::default()
                },
                recipient: other_dm,
                destination: None,
                expected_source_rev: None,
                expected_destination_rev: None,
            },
            Some(dm),
            &mut FixedEnv,
        )
        .expect("the DM sends coins to another DM");

        assert_eq!(
            coins_of(&transition, source_id),
            Coins {
                gp: 1,
                ..Coins::default()
            }
        );
        assert_eq!(
            coins_of(&transition, destination_id),
            Coins {
                gp: 2,
                ..Coins::default()
            }
        );
    }

    #[test]
    fn a_coin_transfer_refuses_bad_amounts_before_touching_either_sheet() {
        let (campaign, _dm, aria, bram) = party();
        let source = purse(
            aria,
            EntityKind::Pc,
            Coins {
                gp: 3,
                ..Coins::default()
            },
        );
        let destination = purse(
            bram,
            EntityKind::Pc,
            Coins {
                gp: i32::MAX - 1,
                ..Coins::default()
            },
        );
        let source_id = source.id();
        let destination_id = destination.id();
        let state = state_with(campaign, vec![source, destination]);
        let transfer = |amount, expected_source_rev, expected_destination_rev| CoinTransfer {
            source: source_id,
            amount,
            recipient: bram,
            destination: None,
            expected_source_rev,
            expected_destination_rev,
        };

        for amount in [
            Coins::default(),
            Coins {
                gp: -1,
                ..Coins::default()
            },
            Coins {
                gp: 4,
                ..Coins::default()
            },
            Coins {
                gp: 2,
                ..Coins::default()
            },
        ] {
            assert!(matches!(
                transfer_coins_command(
                    &state,
                    transfer(amount, None, None),
                    Some(aria),
                    &mut FixedEnv
                ),
                Err(EngineError::Malformed { .. })
            ));
        }
        assert!(matches!(
            transfer_coins_command(
                &state,
                transfer(
                    Coins {
                        gp: 1,
                        ..Coins::default()
                    },
                    Some(1),
                    None,
                ),
                Some(aria),
                &mut FixedEnv
            ),
            Err(EngineError::Conflict { entity, .. }) if entity == source_id
        ));
        assert!(matches!(
            transfer_coins_command(
                &state,
                transfer(
                    Coins {
                        gp: 1,
                        ..Coins::default()
                    },
                    None,
                    Some(1),
                ),
                Some(aria),
                &mut FixedEnv
            ),
            Err(EngineError::Conflict { entity, .. }) if entity == destination_id
        ));

        let source_sheet = match state
            .entity(source_id)
            .expect("the working set loads")
            .expect("the source exists")
        {
            AnyEntity::Statblock(document) => document,
            other => panic!("the source is a statblock, got {other:?}"),
        };
        let destination_sheet = match state
            .entity(destination_id)
            .expect("the working set loads")
            .expect("the destination exists")
        {
            AnyEntity::Statblock(document) => document,
            other => panic!("the destination is a statblock, got {other:?}"),
        };
        assert_eq!(
            source_sheet.body.coins,
            Coins {
                gp: 3,
                ..Coins::default()
            }
        );
        assert_eq!(
            destination_sheet.body.coins,
            Coins {
                gp: i32::MAX - 1,
                ..Coins::default()
            }
        );
        assert_eq!(source_sheet.rev, 0);
        assert_eq!(destination_sheet.rev, 0);
    }
}
