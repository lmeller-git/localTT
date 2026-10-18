//! Statuses: the provisional concentration helper and, from the U3 implement
//! phase, the status command effects (RFC `engine-executor.md` 5.4).

use localtt_core::types::{
    AnyEntity,
    CampaignTime,
    EntityId,
    StatblockEntity,
    Status,
    StatusDuration,
    StatusId,
    StatusKind,
};

use super::{CommandEnv, StatusSpec, Transition, load_statblock, one_upsert, stamp};
use crate::{
    error::{ElementRef, EngineError, Warning, WarningCode},
    state::WorkingSet,
};

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

/// The `AddStatus` command (5.4): mints the id, stamps `started_at` from the
/// campaign clock, and warns when a concentration status is already active.
pub(super) fn add_status_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    spec: &StatusSpec,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let now = state.campaign().clock.now_s;
    let mut warnings = Vec::new();
    if spec.kind == StatusKind::Concentration && has_active_concentration(&statblock, now) {
        warnings.push(Warning {
            code: WarningCode::MultipleConcentration,
            message: format!("{} is already concentrating", statblock.name),
            entity: Some(entity),
            element: None,
        });
    }
    statblock.body.statuses.push(Status {
        id: StatusId::new(),
        kind: spec.kind,
        name: spec.name.clone(),
        reference: spec.reference.clone(),
        text: spec.text.clone(),
        started_at: now,
        duration: spec.duration.clone(),
        source: spec.source.clone(),
        modifiers: spec.modifiers.clone(),
    });
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, warnings))
}

/// A concentration status is active while the soft-expiry predicate
/// (`now < expires_at`, 5.2) holds: an expired status kept on the sheet does
/// not block a new concentration.
fn has_active_concentration(statblock: &StatblockEntity, now: CampaignTime) -> bool {
    statblock.body.statuses.iter().any(|status| {
        status.kind == StatusKind::Concentration
            && !matches!(
                &status.duration,
                StatusDuration::Timed { expires_at } if now >= *expires_at
            )
    })
}

/// The `RemoveStatus` command (5.4): the status must be on the sheet.
pub(super) fn remove_status_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    status: StatusId,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    if !statblock
        .body
        .statuses
        .iter()
        .any(|candidate| candidate.id == status)
    {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Status(status)),
        });
    }
    statblock
        .body
        .statuses
        .retain(|candidate| candidate.id != status);
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `EditStatusDuration` command (5.4): replaces the duration in place.
pub(super) fn edit_status_duration_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    status: StatusId,
    duration: &StatusDuration,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some(found) = statblock
        .body
        .statuses
        .iter_mut()
        .find(|candidate| candidate.id == status)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Status(status)),
        });
    };
    found.duration = duration.clone();
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}
