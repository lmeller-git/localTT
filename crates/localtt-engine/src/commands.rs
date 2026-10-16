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
/// go negative (the engine warns, never blocks - RFC-0001 2.2).
#[inline]
pub fn apply_damage(entity: &mut StatblockEntity, amount: i32) {
    let absorbed = amount.min(entity.body.hp.temp);
    entity.body.hp.temp -= absorbed;
    entity.body.hp.current -= amount - absorbed;
}

/// Heals `amount`; healing does not restore temp HP.
#[inline]
pub fn heal(entity: &mut StatblockEntity, amount: i32) {
    let hp = &mut entity.body.hp;
    hp.current = (hp.current + amount).min(hp.max);
}

/// Sets temp HP; non-stacking (keeps the higher value).
#[inline]
pub fn set_temp_hp(entity: &mut StatblockEntity, amount: i32) {
    let hp = &mut entity.body.hp;
    hp.temp = hp.temp.max(amount);
}

/// Spends `amount` of a tracked resource; overspending is allowed and warned.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    if let Some(resource) = entity.body.resources.iter_mut().find(|r| r.id == id) {
        resource.current -= amount;
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

/// Levels up one class identified by its [`ClassLevelId`]: bumps the level,
/// optionally appending a feature, spell or resource. Overrides are untouched
/// (stale overrides stay; RFC-0001 5.1).
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
#[must_use]
#[inline]
pub fn summon_from_template(
    template: &StatblockEntity,
    caster: &StatblockEntity,
) -> StatblockEntity {
    let mut summon = template.clone();
    summon.id = EntityId::new();
    summon.kind = EntityKind::Summon;
    summon.owner = caster.owner;
    summon.parent = Some(caster.id);

    let mut inventory = BTreeMap::new();
    for entry in &mut summon.body.inventory {
        let old = entry.id;
        let fresh = InventoryEntryId::new();
        entry.id = fresh;
        inventory.insert(old, fresh);
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
    for status in &mut summon.body.statuses {
        status.id = StatusId::new();
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
