//! Tracked resources: the provisional spend arithmetic and, from the U3
//! implement phase, the spend/restore command effects (RFC
//! `engine-executor.md` 5.4).

use localtt_core::types::{AnyEntity, EntityId, Formula, ResourceId, StatblockEntity};

use super::{CommandEnv, Transition, load_statblock, one_upsert, stamp};
use crate::{
    error::{ElementRef, EngineError, Warning, WarningCode},
    state::WorkingSet,
};

/// Spends `amount` of a tracked resource; overspending is allowed and warned.
/// Saturates at the `i32` bounds.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    if let Some(resource) = entity.body.resources.iter_mut().find(|r| r.id == id) {
        resource.current = resource.current.saturating_sub(amount);
    }
}

/// The `SpendResource` command (5.4): the resource must be on the sheet;
/// overspending goes below zero and warns.
pub(super) fn spend_resource_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    resource: ResourceId,
    amount: i32,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some(index) = statblock
        .body
        .resources
        .iter()
        .position(|candidate| candidate.id == resource)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Resource(resource)),
        });
    };
    if amount < 0 {
        return Err(EngineError::Malformed {
            reason: format!("resource amount {amount} is negative"),
        });
    }
    spend_resource(&mut statblock, resource, amount);
    let mut warnings = Vec::new();
    let spent = &statblock.body.resources[index];
    if spent.current < 0 {
        warnings.push(Warning {
            code: WarningCode::ResourceOverspent,
            message: format!(
                "{} has {} {} left",
                statblock.name, spent.current, spent.name
            ),
            entity: Some(entity),
            element: Some(ElementRef::Resource(resource)),
        });
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, warnings))
}

/// The `RestoreResource` command (5.4): clamped to the maximum when it is a
/// constant; a non-constant maximum restores unclamped and warns
/// `FormulaUnevaluated` (OQ-2).
pub(super) fn restore_resource_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    resource: ResourceId,
    amount: i32,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some(restored) = statblock
        .body
        .resources
        .iter_mut()
        .find(|candidate| candidate.id == resource)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Resource(resource)),
        });
    };
    if amount < 0 {
        return Err(EngineError::Malformed {
            reason: format!("resource amount {amount} is negative"),
        });
    }
    let mut warnings = Vec::new();
    match restored.max {
        Formula::Const(max) => {
            restored.current = restored.current.saturating_add(amount).min(max);
        }
        Formula::Expr(_) => {
            restored.current = restored.current.saturating_add(amount);
            warnings.push(Warning {
                code: WarningCode::FormulaUnevaluated,
                message: format!(
                    "{} was not clamped: its maximum needs the rules unit",
                    restored.name
                ),
                entity: Some(entity),
                element: Some(ElementRef::Resource(resource)),
            });
        }
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, warnings))
}
