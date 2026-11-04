//! Item and entity transfer handlers (RFC `trading.md`).
//!
//! A transfer is one-way: the recipient gains the asset immediately and can
//! hand it back with the same commands. Item batches are atomic across the
//! source and destination, and entity ownership moves change the header only.

use std::collections::BTreeSet;

use localtt_core::types::{
    AnyEntity,
    Campaign,
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
    let actor_is_dm = authz::dm(state.campaign(), actor);
    if !actor_is_dm {
        if recipient.role != Role::Player {
            return Err(EngineError::Malformed {
                reason: "only the DM may transfer items to another DM".to_owned(),
            });
        }
        if transfer.destination.is_some() {
            return Err(EngineError::Malformed {
                reason: "only the DM may pick the recipient's statblock".to_owned(),
            });
        }
    }

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

/// Moves one entity's ownership to the recipient.
///
/// Only the owner header, revision and update stamp change.
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
    Ok(one_upsert(document, Vec::new()))
}

/// Resolves the recipient's seat, or reports it missing.
fn recipient_seat(campaign: &Campaign, player: PlayerId) -> Result<&Player, EngineError> {
    campaign
        .player(player)
        .ok_or(EngineError::PlayerNotFound { player })
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
        reason: format!("player {recipient} has no player character to receive the items"),
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
            EntityId,
            EntityKind,
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
    fn an_entity_transfer_moves_only_the_owner_header() {
        let (campaign, dm, aria, bram) = party();
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
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }]
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
}
