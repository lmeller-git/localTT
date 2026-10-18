//! Entity lifecycle helpers: the provisional level-up and summoning
//! arithmetic; the command variants arrive in later units (RFC
//! `engine-executor.md` 6.4).

use std::collections::BTreeMap;

use localtt_core::types::{
    AttackEntryId,
    ClassLevelId,
    EntityId,
    EntityKind,
    FeatureEntryId,
    InventoryEntryId,
    InventoryItem,
    ResourceId,
    SpellEntryId,
    SpellcastingBlockId,
    StatblockEntity,
    StatusId,
};

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
