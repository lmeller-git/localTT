//! Provisional stand-ins for `engine::commands`, deleted as the engine lands.
//!
//! These mirror the engine command catalogue (RFC-0001 5.2) closely enough that
//! the scenario tests keep passing when the real commands land and the calls
//! are swapped. They are data only: no derived computation, no warnings, no
//! authorization.
#![expect(
    clippy::todo,
    unused_variables,
    reason = "skeleton phase: every body is `todo!()`; removed in the implement phase"
)]

use localtt_core::types::{HandoutEntity, InventoryEntryId, ResourceId, StatblockEntity, Status};

/// Mirror of `commands::apply_damage`: temp HP absorbs first, then current;
/// `current` may go negative (the engine warns, never blocks - RFC-0001 2.2).
#[inline]
pub fn apply_damage(entity: &mut StatblockEntity, amount: i32) {
    todo!()
}

/// Mirror of `commands::heal`; healing does not restore temp HP.
#[inline]
pub fn heal(entity: &mut StatblockEntity, amount: i32) {
    todo!()
}

/// Mirror of `commands::set_temp_hp`; non-stacking (take the higher).
#[inline]
pub fn set_temp_hp(entity: &mut StatblockEntity, amount: i32) {
    todo!()
}

/// Mirror of `commands::spend_resource`; overspending is allowed.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    todo!()
}

/// Mirror of `commands::begin_concentration`: replaces any existing
/// concentration status (RFC-0001 5.4, command-layer semantics).
#[inline]
pub fn begin_concentration(entity: &mut StatblockEntity, status: Status) {
    todo!()
}

/// Mirror of `commands::level_up` for one class: bumps the level, optionally
/// appending a feature, spell or resource. Overrides are untouched (stale
/// overrides stay; RFC-0001 5.1).
#[inline]
pub fn level_up(entity: &mut StatblockEntity, class: &str, to_level: i32) {
    todo!()
}

/// Mirror of `commands::create_summon_from_template`: a copy with fresh element
/// ids, owned by `caster`, linked via `parent` (RFC-0001 5.12).
#[must_use]
#[inline]
pub fn summon_from_template(
    template: &StatblockEntity,
    caster: &StatblockEntity,
) -> StatblockEntity {
    todo!()
}

/// Mirror of `commands::give_handout`: a `HandoutContent::Item` becomes a fresh
/// `InlineItem` inventory entry owned by the recipient.
#[must_use]
#[inline]
pub fn give_handout(handout: &HandoutEntity, recipient: &mut StatblockEntity) -> InventoryEntryId {
    todo!()
}
