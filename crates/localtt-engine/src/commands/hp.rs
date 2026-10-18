//! Hit points: the provisional arithmetic (RFC-0001 2.2) and, from the U3
//! implement phase, the hp command effects (RFC `engine-executor.md` 5.4).

use localtt_core::types::{AnyEntity, DeathSaves, EntityId, StatblockEntity};

use super::{CommandEnv, Transition, load_statblock, one_upsert, stamp};
use crate::{
    error::{EngineError, Warning, WarningCode},
    state::WorkingSet,
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

/// The `ApplyDamage` command (5.4): the prelude runs first, then the
/// provisional arithmetic; a result below zero warns instead of blocking.
pub(super) fn apply_damage_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    amount: i32,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    // The prelude runs before request validation (5.4): a missing target is
    // `NotFound` even when the amount is also negative.
    if amount < 0 {
        return Err(EngineError::Malformed {
            reason: format!("damage amount {amount} is negative"),
        });
    }
    apply_damage(&mut statblock, amount);
    let mut warnings = Vec::new();
    if statblock.body.hp.current < 0 {
        warnings.push(Warning {
            code: WarningCode::NegativeHp,
            message: format!(
                "{} has {} hit points left",
                statblock.name, statblock.body.hp.current
            ),
            entity: Some(entity),
            element: None,
        });
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, warnings))
}

/// The `Heal` command (5.4): clamping at max is not an error and temp HP is
/// untouched.
pub(super) fn heal_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    amount: i32,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    if amount < 0 {
        return Err(EngineError::Malformed {
            reason: format!("healing amount {amount} is negative"),
        });
    }
    heal(&mut statblock, amount);
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `SetTempHp` command (5.4): non-stacking; a lower amount keeps the
/// higher existing value.
pub(super) fn set_temp_hp_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    amount: i32,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    if amount < 0 {
        return Err(EngineError::Malformed {
            reason: format!("temp hit points {amount} are negative"),
        });
    }
    set_temp_hp(&mut statblock, amount);
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `SetDeathSaves` command (5.4): replaces the tally; a field above 3 is
/// `Malformed`.
pub(super) fn set_death_saves_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    value: DeathSaves,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    if value.successes > 3 || value.failures > 3 {
        return Err(EngineError::Malformed {
            reason: format!(
                "death saves go up to 3, got {} successes and {} failures",
                value.successes, value.failures
            ),
        });
    }
    statblock.body.death_saves = value;
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}
