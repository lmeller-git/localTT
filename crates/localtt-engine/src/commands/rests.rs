//! Rest bookkeeping (RFC `engine-executor.md` 5.3): short and long rests are
//! pure effects over a statblock plus the optional clock move.
//!
//! The rest target is always rewritten (a rest is an explicit checkpoint), and
//! a target that is also dawn-refilled is merged into a single new revision.

use localtt_core::types::{
    AnyEntity,
    CampaignTime,
    DeathSaves,
    EntityId,
    Recharge,
    RestKind,
    StatblockEntity,
    StatusDuration,
};

use super::{CommandEnv, Transition, clock, load_statblock, one_upsert, stamp};
use crate::{
    error::{EngineError, Warning},
    state::WorkingSet,
};

/// The `ShortRest` command (5.3): refills `Recharge::Short` resources and
/// clears `UntilRest { rest: Short }` statuses.
pub(super) fn short_rest_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    advance_seconds: Option<i64>,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    rest_command(
        state,
        RestCommand {
            entity,
            rest: RestKind::Short,
            advance_seconds,
            expected_rev,
        },
        env,
    )
}

/// The `LongRest` command (5.3): short plus `Recharge::Long` refills,
/// `UntilRest { rest: Long }` clears, and the `rules.md` 5.10 recovery
/// (hit points, death saves, one exhaustion level).
pub(super) fn long_rest_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    advance_seconds: Option<i64>,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    rest_command(
        state,
        RestCommand {
            entity,
            rest: RestKind::Long,
            advance_seconds,
            expected_rev,
        },
        env,
    )
}

/// The shared arguments of the two rest commands.
struct RestCommand {
    entity: EntityId,
    rest: RestKind,
    advance_seconds: Option<i64>,
    expected_rev: Option<u64>,
}

/// The shared `ShortRest` / `LongRest` command (5.3): the prelude runs first,
/// then the optional clock move through the same machinery as 5.2/5.3, then
/// the rest effect merged into the target's single new revision.
fn rest_command(
    state: &dyn WorkingSet,
    command: RestCommand,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let RestCommand {
        entity,
        rest,
        advance_seconds,
        expected_rev,
    } = command;
    let target = load_statblock(state, entity, expected_rev)?;
    if advance_seconds.is_some_and(|seconds| seconds < 0) {
        return Err(EngineError::Malformed {
            reason: format!("rest advance {advance_seconds:?} is negative"),
        });
    }
    let old = state.campaign().clock.now_s;
    let to = match advance_seconds {
        Some(seconds) if seconds > 0 => {
            let Some(moved) = old.seconds().checked_add(seconds) else {
                return Err(EngineError::Malformed {
                    reason: format!("rest advance {seconds} overflows campaign time"),
                });
            };
            CampaignTime::from_seconds(moved)
        }
        Some(_) | None => old,
    };
    match clock::clock_move(state, old, to)? {
        Some(mut moved) => {
            let mut document = moved.touched.remove(&entity).unwrap_or(target);
            rest_effect(&mut document, rest, &mut moved.warnings);
            moved.touched.insert(entity, document);
            Ok(clock::finish(state, moved, env))
        }
        None => {
            let mut document = target;
            let mut warnings = Vec::new();
            rest_effect(&mut document, rest, &mut warnings);
            let mut document = AnyEntity::from(document);
            stamp(&mut document, env.now());
            Ok(one_upsert(document, warnings))
        }
    }
}

/// Applies one rest to one document (5.3): refills by recharge class and
/// clears the covered `UntilRest` statuses. A long rest additionally recovers
/// hit points, death saves and one exhaustion level (`rules.md` 5.10); hit
/// dice are untouched (U5).
fn rest_effect(statblock: &mut StatblockEntity, rest: RestKind, warnings: &mut Vec<Warning>) {
    clock::refill_resources(statblock, Recharge::Short, warnings);
    if rest == RestKind::Long {
        clock::refill_resources(statblock, Recharge::Long, warnings);
        recover_long_rest(statblock);
    }
    statblock.body.statuses.retain(|status| {
        let StatusDuration::UntilRest { rest: attached } = &status.duration else {
            return true;
        };
        !covers(rest, *attached)
    });
}

/// The deterministic long-rest recovery (`rules.md` 5.10): hit points back to
/// the maximum (temporary hit points are untouched), death saves reset to
/// zero, and one exhaustion level is shed.
fn recover_long_rest(statblock: &mut StatblockEntity) {
    statblock.body.hp.current = statblock.body.hp.max;
    statblock.body.death_saves = DeathSaves::default();
    statblock.body.exhaustion = statblock.body.exhaustion.saturating_sub(1);
}

/// A long rest includes a short rest (5.3): short clears only `Short`,
/// long clears both.
fn covers(rest: RestKind, attached: RestKind) -> bool {
    match rest {
        RestKind::Short => attached == RestKind::Short,
        RestKind::Long => true,
    }
}
