//! Inventory helpers: the provisional handout conversion; the command variant
//! arrives in a later unit (RFC `engine-executor.md` 6.4).

use localtt_core::types::{
    HandoutContent,
    HandoutEntity,
    InventoryEntry,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    StatblockEntity,
};

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
