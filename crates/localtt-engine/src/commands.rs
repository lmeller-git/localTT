//! The command surface (RFC-0001 section 5.2): the mutating operations a client
//! drives.
//!
//! These signatures are the provisional surface the integration scenario suite
//! calls (see `crates/localtt-integration/DESIGN.md` section 5); the catalogue
//! will move to the engine's eventual `(state, command) -> result` shape later.

use std::collections::BTreeMap;

use localtt_core::types::{
    AttackEntryId,
    ClassLevelId,
    EntityId,
    EntityKind,
    FeatureEntryId,
    HandoutContent,
    HandoutEntity,
    InventoryEntry,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    ResourceId,
    SpellEntryId,
    SpellcastingBlockId,
    StatblockEntity,
    Status,
    StatusId,
    StatusKind,
};

/// Applies `amount` damage: temp HP absorbs first, then current; `current` may
/// go negative (the engine warns, never blocks - RFC-0001 2.2). The arithmetic
/// saturates at the `i32` bounds rather than panicking in debug builds or
/// wrapping in release.
#[inline]
pub fn apply_damage(entity: &mut StatblockEntity, amount: i32) {
    let hp = &mut entity.body.hp;
    let absorbed = amount.min(hp.temp);
    hp.temp = hp.temp.saturating_sub(absorbed);
    hp.current = hp.current.saturating_sub(amount.saturating_sub(absorbed));
}

/// Heals `amount`; healing does not restore temp HP. Saturates at the `i32`
/// bounds before clamping to `max`.
#[inline]
pub fn heal(entity: &mut StatblockEntity, amount: i32) {
    let hp = &mut entity.body.hp;
    hp.current = hp.current.saturating_add(amount).min(hp.max);
}

/// Sets temp HP; non-stacking (keeps the higher value).
#[inline]
pub fn set_temp_hp(entity: &mut StatblockEntity, amount: i32) {
    let hp = &mut entity.body.hp;
    hp.temp = hp.temp.max(amount);
}

/// Spends `amount` of a tracked resource; overspending is allowed and warned.
/// Saturates at the `i32` bounds.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    if let Some(resource) = entity.body.resources.iter_mut().find(|r| r.id == id) {
        resource.current = resource.current.saturating_sub(amount);
    }
}

/// Begins concentration: replaces any existing concentration status
/// (RFC-0001 5.4, command-layer semantics).
#[inline]
pub fn begin_concentration(entity: &mut StatblockEntity, status: Status) {
    entity
        .body
        .statuses
        .retain(|s| s.kind != StatusKind::Concentration);
    entity.body.statuses.push(status);
}

/// Levels up one class identified by its [`ClassLevelId`]: sets that class's
/// level. Overrides are untouched (stale overrides stay; RFC-0001 5.1).
///
/// Keying on the id rather than a class name keeps it unambiguous when a class
/// is stored as a [`localtt_core::types::RefOrText::Ref`] (docs/decisions.md
/// ARCH-5).
#[inline]
pub fn level_up(entity: &mut StatblockEntity, class: ClassLevelId, to_level: i32) {
    if let Some(level) = entity.body.levels.iter_mut().find(|l| l.id == class) {
        level.level = to_level;
    }
}

/// Creates a summon from a template: a copy with fresh element ids, owned by
/// `caster`, linked via `parent` (RFC-0001 5.12).
///
/// Every element id is regenerated, including the charge resource nested in an
/// inline inventory item; references between elements (`from_inventory`,
/// `resource_id`, a status's `source.status`) are remapped to the new ids so
/// they keep resolving inside the copy. A `source.entity` is an entity
/// reference, not an element of this body, and is left as the template had it.
#[must_use]
#[inline]
pub fn summon_from_template(
    template: &StatblockEntity,
    caster: &StatblockEntity,
) -> StatblockEntity {
    let mut summon = template.clone();
    summon.id = EntityId::new();
    summon
        .set_kind(EntityKind::Summon)
        .expect("a summon is a statblock kind");
    summon.owner = caster.owner;
    summon.parent = Some(caster.id);

    let mut inventory = BTreeMap::new();
    for entry in &mut summon.body.inventory {
        let old = entry.id;
        let fresh = InventoryEntryId::new();
        entry.id = fresh;
        inventory.insert(old, fresh);
        if let InventoryItem::Inline(item) = &mut entry.item
            && let Some(resource) = &mut item.resource
        {
            resource.id = ResourceId::new();
        }
    }
    let mut resources = BTreeMap::new();
    for resource in &mut summon.body.resources {
        let old = resource.id;
        let fresh = ResourceId::new();
        resource.id = fresh;
        resources.insert(old, fresh);
    }
    for level in &mut summon.body.levels {
        level.id = ClassLevelId::new();
    }
    for attack in &mut summon.body.attacks {
        attack.id = AttackEntryId::new();
        if let Some(old) = attack.from_inventory {
            attack.from_inventory = inventory.get(&old).copied();
        }
    }
    for feature in &mut summon.body.features {
        feature.id = FeatureEntryId::new();
        if let Some(old) = feature.resource_id {
            feature.resource_id = resources.get(&old).copied();
        }
    }
    for block in &mut summon.body.spellcasting {
        block.id = SpellcastingBlockId::new();
        for spell in &mut block.spells {
            spell.id = SpellEntryId::new();
        }
    }
    let mut statuses = BTreeMap::new();
    for status in &mut summon.body.statuses {
        let old = status.id;
        let fresh = StatusId::new();
        status.id = fresh;
        statuses.insert(old, fresh);
    }
    for status in &mut summon.body.statuses {
        if let Some(source) = &mut status.source
            && let Some(old) = source.status
            && let Some(fresh) = statuses.get(&old)
        {
            source.status = Some(*fresh);
        }
    }
    summon
}

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
