//! Hit points: the provisional arithmetic (RFC-0001 2.2) and, from the U3
//! implement phase, the hp command effects (RFC `engine-executor.md` 5.4).

use localtt_core::types::StatblockEntity;

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
