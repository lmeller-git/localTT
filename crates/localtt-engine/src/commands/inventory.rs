//! Inventory: the item commands and the handout conversion (RFC
//! `engine-executor.md` 6.4, `docs/rfcs/server.md` milestone M3).
//!
//! Quantities are non-negative on add and edit; the derivation layer warns
//! about zero. A handout copy leaves the handout document untouched.

use localtt_core::types::{
    AnyEntity,
    EntityId,
    HandoutContent,
    HandoutEntity,
    InventoryEntry,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    StatblockEntity,
};

use super::{CommandEnv, Transition, load_statblock, load_target, one_upsert, stamp};
use crate::{error::EngineError, state::WorkingSet};

/// Gives a handout: a [`HandoutContent::Item`] becomes a fresh
/// [`InventoryItem::Inline`] entry owned by the recipient, and its id is
/// returned.
///
/// A [`HandoutContent::Text`] handout has no inventory effect (the client just
/// shows the text), so it returns `None`.
#[must_use]
#[inline]
pub fn give_handout(
    handout: &HandoutEntity,
    recipient: &mut StatblockEntity,
) -> Option<InventoryEntryId> {
    let HandoutContent::Item { item } = &handout.body.content else {
        return None;
    };
    let id = InventoryEntryId::new();
    recipient.body.inventory.push(InventoryEntry {
        id,
        item: InventoryItem::Inline(item.clone()),
        qty: 1,
        state: ItemState::Carried,
        attuned: false,
        notes: String::new(),
    });
    Some(id)
}
/// The fields of one [`super::Command::AddInventoryItem`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NewItem {
    pub(super) item: InventoryItem,
    pub(super) qty: i32,
    pub(super) state: ItemState,
    pub(super) attuned: bool,
    pub(super) notes: String,
}

/// The optional fields of one [`super::Command::EditInventoryItem`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ItemEdit {
    pub(super) qty: Option<i32>,
    pub(super) state: Option<ItemState>,
    pub(super) attuned: Option<bool>,
    pub(super) notes: Option<String>,
}

/// The `AddInventoryItem` command: mints one entry and stamps the statblock.
pub(super) fn add_inventory_item_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    new_item: NewItem,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    if new_item.qty < 0 {
        return Err(EngineError::Malformed {
            reason: format!("inventory quantity {} is negative", new_item.qty),
        });
    }
    validate_item(&new_item.item)?;
    statblock.body.inventory.push(InventoryEntry {
        id: InventoryEntryId::new(),
        item: new_item.item,
        qty: new_item.qty,
        state: new_item.state,
        attuned: new_item.attuned,
        notes: new_item.notes,
    });
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `EditInventoryItem` command: at least one field must be set.
pub(super) fn edit_inventory_item_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    entry: InventoryEntryId,
    edit: ItemEdit,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some(stored) = statblock
        .body
        .inventory
        .iter_mut()
        .find(|stored| stored.id == entry)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(crate::error::ElementRef::Item(entry)),
        });
    };
    if edit.qty.is_none() && edit.state.is_none() && edit.attuned.is_none() && edit.notes.is_none()
    {
        return Err(EngineError::Malformed {
            reason: "editing an inventory entry needs at least one field".to_owned(),
        });
    }
    if let Some(qty) = edit.qty
        && qty < 0
    {
        return Err(EngineError::Malformed {
            reason: format!("inventory quantity {qty} is negative"),
        });
    }
    if let Some(qty) = edit.qty {
        stored.qty = qty;
    }
    if let Some(item_state) = edit.state {
        stored.state = item_state;
    }
    if let Some(attuned) = edit.attuned {
        stored.attuned = attuned;
    }
    if let Some(notes) = edit.notes {
        stored.notes = notes;
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `RemoveInventoryItem` command.
pub(super) fn remove_inventory_item_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    entry: InventoryEntryId,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some(index) = statblock
        .body
        .inventory
        .iter()
        .position(|stored| stored.id == entry)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(crate::error::ElementRef::Item(entry)),
        });
    };
    statblock.body.inventory.remove(index);
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `GiveHandout` command: copies a handout item into a recipient and
/// stamps the recipient; the handout document is not modified.
pub(super) fn give_handout_command(
    state: &dyn WorkingSet,
    handout: EntityId,
    recipient: EntityId,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let handout_document = load_target(state, handout, None)?;
    let AnyEntity::Handout(handout_entity) = &handout_document else {
        return Err(EngineError::Malformed {
            reason: format!("entity {handout} is not a handout"),
        });
    };
    let mut statblock = load_statblock(state, recipient, expected_rev)?;
    if give_handout(handout_entity, &mut statblock).is_none() {
        return Err(EngineError::Malformed {
            reason: format!("handout {handout} has no item to give"),
        });
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

fn validate_item(item: &InventoryItem) -> Result<(), EngineError> {
    let name = match item {
        InventoryItem::Ref(reference) => &reference.name,
        InventoryItem::Inline(item) => &item.name,
    };
    if name.trim().is_empty() {
        return Err(EngineError::Malformed {
            reason: "an item name must not be empty".to_owned(),
        });
    }
    if name.chars().count() > 200 {
        return Err(EngineError::Malformed {
            reason: "an item name must be at most 200 characters".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            Campaign,
            Entity,
            EntityKind,
            Ext,
            HandoutBody,
            InlineItem,
            ItemKind,
            PlayerId,
            UnixTime,
        },
    };

    use super::*;
    use crate::{
        commands::Event,
        error::ElementRef,
        ports::store::CampaignSnapshot,
        state::LoadedCampaign,
    };

    const NOW: i64 = 1_750_000_000;

    struct FixedEnv;

    impl CommandEnv for FixedEnv {
        fn now(&self) -> UnixTime {
            UnixTime::from_seconds(NOW)
        }
    }

    fn campaign() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Inventory");
        let dm = builder.dm("DM");
        let player = builder.player("Player");
        (builder.build(), dm, player)
    }

    fn state_with(campaign: Campaign, entities: Vec<AnyEntity>) -> LoadedCampaign {
        LoadedCampaign::from_snapshot(CampaignSnapshot {
            campaign,
            entities,
            feed: Vec::new(),
        })
    }

    fn item(name: &str) -> InventoryItem {
        InventoryItem::Inline(InlineItem {
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
        })
    }

    fn new_item(name: &str, qty: i32, state: ItemState) -> NewItem {
        NewItem {
            item: item(name),
            qty,
            state,
            attuned: false,
            notes: String::new(),
        }
    }

    fn character(owner: PlayerId) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(12)).hp(12, 12);
        AnyEntity::from(
            builder
                .build_entity(EntityKind::Pc, EntityId::new(), "Aria", owner)
                .expect("abilities and hp are set"),
        )
    }

    fn handout(owner: PlayerId, content: HandoutContent) -> AnyEntity {
        AnyEntity::from(
            Entity::builder(
                EntityKind::Handout,
                EntityId::new(),
                "Loot",
                owner,
                HandoutBody { content },
            )
            .build()
            .expect("a handout kind pairs with a handout body"),
        )
    }

    #[test]
    fn an_item_is_added_with_a_minted_id_and_a_stamped_revision() {
        let (campaign, _dm, player) = campaign();
        let entity = character(player);
        let id = entity.id();
        let state = state_with(campaign, vec![entity]);

        let transition = add_inventory_item_command(
            &state,
            id,
            NewItem {
                item: item("Rope"),
                qty: 2,
                state: ItemState::Carried,
                attuned: false,
                notes: "50 feet".to_owned(),
            },
            None,
            &mut FixedEnv,
        )
        .expect("a player adds to their own sheet");

        assert_eq!(transition.upserts.len(), 1);
        let document = &transition.upserts[0];
        assert_eq!(document.rev(), 1);
        assert_eq!(document.updated_at(), UnixTime::from_seconds(NOW));
        assert_eq!(
            transition.events,
            vec![Event::EntityChanged {
                id,
                rev: 1,
                kind: EntityKind::Pc,
            }]
        );
        let inventory = match document {
            AnyEntity::Statblock(statblock) => &statblock.body.inventory,
            other => panic!("the target is a statblock, got {other:?}"),
        };
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].qty, 2);
        assert_eq!(inventory[0].state, ItemState::Carried);
        assert_eq!(inventory[0].notes, "50 feet");
    }

    #[test]
    fn negative_quantities_and_empty_names_are_refused() {
        let (campaign, _dm, player) = campaign();
        let entity = character(player);
        let id = entity.id();
        let state = state_with(campaign, vec![entity]);

        assert!(matches!(
            add_inventory_item_command(
                &state,
                id,
                NewItem {
                    item: item("Rope"),
                    qty: -1,
                    state: ItemState::Carried,
                    attuned: false,
                    notes: String::new(),
                },
                None,
                &mut FixedEnv,
            ),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            add_inventory_item_command(
                &state,
                id,
                NewItem {
                    item: item("   "),
                    qty: 1,
                    state: ItemState::Carried,
                    attuned: false,
                    notes: String::new(),
                },
                None,
                &mut FixedEnv,
            ),
            Err(EngineError::Malformed { .. })
        ));
    }

    #[test]
    fn editing_requires_a_field_and_reports_a_missing_entry() {
        let (campaign, _dm, player) = campaign();
        let entity = character(player);
        let id = entity.id();
        let state = state_with(campaign, vec![entity]);
        let added = add_inventory_item_command(
            &state,
            id,
            new_item("Rope", 1, ItemState::Carried),
            None,
            &mut FixedEnv,
        )
        .expect("the item is added");
        let AnyEntity::Statblock(statblock) = &added.upserts[0] else {
            panic!("the target is a statblock");
        };
        let entry = statblock.body.inventory[0].id;
        let state = state_with(state.campaign().clone(), added.upserts.clone());

        assert!(matches!(
            edit_inventory_item_command(
                &state,
                id,
                entry,
                ItemEdit {
                    qty: None,
                    state: None,
                    attuned: None,
                    notes: None,
                },
                None,
                &mut FixedEnv,
            ),
            Err(EngineError::Malformed { .. })
        ));
        let missing = InventoryEntryId::new();
        assert!(matches!(
            edit_inventory_item_command(
                &state,
                id,
                missing,
                ItemEdit {
                    qty: Some(3),
                    state: None,
                    attuned: None,
                    notes: None,
                },
                None,
                &mut FixedEnv,
            ),
            Err(EngineError::NotFound {
                entity: missing_entity,
                element: Some(ElementRef::Item(element)),
            }) if missing_entity == id && element == missing
        ));

        let transition = edit_inventory_item_command(
            &state,
            id,
            entry,
            ItemEdit {
                qty: Some(3),
                state: Some(ItemState::Equipped),
                attuned: Some(true),
                notes: Some("strapped".to_owned()),
            },
            None,
            &mut FixedEnv,
        )
        .expect("the entry edits");
        let AnyEntity::Statblock(statblock) = &transition.upserts[0] else {
            panic!("the target is a statblock");
        };
        assert_eq!(statblock.body.inventory[0].qty, 3);
        assert_eq!(statblock.body.inventory[0].state, ItemState::Equipped);
        assert!(statblock.body.inventory[0].attuned);
        assert_eq!(statblock.body.inventory[0].notes, "strapped");
    }

    #[test]
    fn removing_one_entry_keeps_the_rest() {
        let (campaign, _dm, player) = campaign();
        let entity = character(player);
        let id = entity.id();
        let state = state_with(campaign, vec![entity]);
        let first = add_inventory_item_command(
            &state,
            id,
            new_item("Rope", 1, ItemState::Carried),
            None,
            &mut FixedEnv,
        )
        .expect("the first item is added");
        let state = state_with(state.campaign().clone(), first.upserts.clone());
        let second = add_inventory_item_command(
            &state,
            id,
            new_item("Torch", 5, ItemState::Stowed),
            None,
            &mut FixedEnv,
        )
        .expect("the second item is added");
        let AnyEntity::Statblock(statblock) = &second.upserts[0] else {
            panic!("the target is a statblock");
        };
        assert_eq!(statblock.body.inventory.len(), 2);
        let removed = statblock.body.inventory[0].id;
        let state = state_with(state.campaign().clone(), second.upserts.clone());

        let transition = remove_inventory_item_command(&state, id, removed, None, &mut FixedEnv)
            .expect("the entry removes");
        let AnyEntity::Statblock(statblock) = &transition.upserts[0] else {
            panic!("the target is a statblock");
        };
        assert_eq!(statblock.body.inventory.len(), 1);
        assert_eq!(statblock.body.inventory[0].qty, 5);
        let state = state_with(state.campaign().clone(), transition.upserts.clone());

        assert!(matches!(
            remove_inventory_item_command(&state, id, removed, None, &mut FixedEnv),
            Err(EngineError::NotFound { .. })
        ));
    }

    #[test]
    fn giving_a_handout_copies_the_item_and_leaves_the_handout_alone() {
        let (campaign, dm, player) = campaign();
        let recipient = character(player);
        let recipient_id = recipient.id();
        let handout_item = handout(
            dm,
            HandoutContent::Item {
                item: match item("Potion of Healing") {
                    InventoryItem::Inline(item) => item,
                    InventoryItem::Ref(_) => unreachable!("the fixture is inline"),
                },
            },
        );
        let handout_id = handout_item.id();
        let state = state_with(campaign, vec![recipient, handout_item.clone()]);

        let transition =
            give_handout_command(&state, handout_id, recipient_id, None, &mut FixedEnv)
                .expect("the DM gives the item");
        let AnyEntity::Statblock(statblock) = &transition.upserts[0] else {
            panic!("the recipient is a statblock");
        };
        assert_eq!(statblock.body.inventory.len(), 1);
        assert_eq!(statblock.body.inventory[0].qty, 1);
        assert_eq!(statblock.body.inventory[0].item, item("Potion of Healing"));
        assert_eq!(
            state.entity(handout_id).expect("loaded").expect("present"),
            handout_item
        );

        let text_handout = handout(
            dm,
            HandoutContent::Text {
                text: "A map".to_owned(),
            },
        );
        let text_id = text_handout.id();
        let other_recipient = character(player);
        let other_id = other_recipient.id();
        let state = state_with(
            state.campaign().clone(),
            vec![other_recipient, text_handout],
        );
        assert!(matches!(
            give_handout_command(&state, text_id, other_id, None, &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            give_handout_command(&state, other_id, other_id, None, &mut FixedEnv),
            Err(EngineError::Malformed { .. })
        ));
    }
}
