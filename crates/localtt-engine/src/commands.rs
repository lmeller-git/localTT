//! The command surface (RFC-0001 section 5.2): the mutating operations a client
//! drives.
//!
//! Skeleton: every body is `todo!()`. These signatures are the provisional
//! surface RFC-0003's scenario suite calls; the catalogue will move to the
//! engine's eventual `(state, command) -> result` shape in the implement phase.
#![expect(
    clippy::todo,
    unused_variables,
    reason = "skeleton phase: every body is `todo!()`; removed in the implement phase"
)]

use localtt_core::types::{
    ClassLevelId,
    HandoutEntity,
    InventoryEntryId,
    ResourceId,
    StatblockEntity,
    Status,
};

/// Applies `amount` damage: temp HP absorbs first, then current; `current` may
/// go negative (the engine warns, never blocks - RFC-0001 2.2).
#[inline]
pub fn apply_damage(entity: &mut StatblockEntity, amount: i32) {
    todo!()
}

/// Heals `amount`; healing does not restore temp HP.
#[inline]
pub fn heal(entity: &mut StatblockEntity, amount: i32) {
    todo!()
}

/// Sets temp HP; non-stacking (keeps the higher value).
#[inline]
pub fn set_temp_hp(entity: &mut StatblockEntity, amount: i32) {
    todo!()
}

/// Spends `amount` of a tracked resource; overspending is allowed and warned.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    todo!()
}

/// Begins concentration: replaces any existing concentration status
/// (RFC-0001 5.4, command-layer semantics).
#[inline]
pub fn begin_concentration(entity: &mut StatblockEntity, status: Status) {
    todo!()
}

/// Levels up one class identified by its [`ClassLevelId`]: bumps the level,
/// optionally appending a feature, spell or resource. Overrides are untouched
/// (stale overrides stay; RFC-0001 5.1).
///
/// Keying on the id rather than a class name keeps it unambiguous when a class
/// is stored as a [`localtt_core::types::RefOrText::Ref`] (RFC-0003 B1).
#[inline]
pub fn level_up(entity: &mut StatblockEntity, class: ClassLevelId, to_level: i32) {
    todo!()
}

/// Creates a summon from a template: a copy with fresh element ids, owned by
/// `caster`, linked via `parent` (RFC-0001 5.12).
#[must_use]
#[inline]
pub fn summon_from_template(
    template: &StatblockEntity,
    caster: &StatblockEntity,
) -> StatblockEntity {
    todo!()
}

/// Gives a handout: a `HandoutContent::Item` becomes a fresh `InlineItem`
/// inventory entry owned by the recipient.
#[must_use]
#[inline]
pub fn give_handout(handout: &HandoutEntity, recipient: &mut StatblockEntity) -> InventoryEntryId {
    todo!()
}
