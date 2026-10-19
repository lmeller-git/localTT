//! Tracked resources: the provisional spend arithmetic and, from the U3
//! implement phase, the spend/restore command effects (RFC
//! `engine-executor.md` 5.4).

use localtt_core::types::{
    Ability,
    AnyEntity,
    DiceExpr,
    EntityId,
    FeedEntry,
    FeedEntryId,
    FeedPayload,
    Formula,
    KnownStat,
    OverrideValue,
    PlayerId,
    ResourceId,
    ResourceKind,
    Roll,
    RollContext,
    RollId,
    RollKind,
    StatKey,
    StatblockEntity,
};

use super::{
    CommandEnv,
    Transition,
    cap_feed,
    heal,
    load_statblock,
    materialize_item_content,
    one_upsert,
    stamp,
    stamped_with_feed,
};
use crate::{
    dice,
    error::{ElementRef, EngineError, Warning, WarningCode},
    rules::{self, RollHint, RollParts, RulesContext},
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

/// The `SpendHitDie` command (RFC `dice.md` 5.9): requires an actor, rolls
/// one hit die plus the CON modifier (derived, 0 when absent), spends one
/// die and heals by `max(0, total)`, clamped to the HP maximum.
pub(super) fn spend_hit_die_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    resource: ResourceId,
    expected_rev: Option<u64>,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let Some(actor) = actor else {
        return Err(EngineError::Malformed {
            reason: "spending a hit die requires an actor".to_owned(),
        });
    };
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
    let ResourceKind::HitDie { sides } = statblock.body.resources[index].kind else {
        return Err(EngineError::Malformed {
            reason: format!("resource {resource} is not a hit die"),
        });
    };

    let content = materialize_item_content(&statblock, env.reference())?;
    let sheet = rules::derive(
        &statblock,
        &RulesContext {
            now: state.campaign().clock.now_s,
            content: &content,
        },
    );
    let con_modifier = match sheet
        .values
        .get(&StatKey::Known(KnownStat::AbilityModifier(
            Ability::Constitution,
        )))
        .and_then(|value| value.effective.as_ref())
    {
        Some(OverrideValue::Int(modifier)) => *modifier,
        _ => 0,
    };
    let plan = dice::compose(
        DiceExpr::Roll {
            count: 1,
            sides,
            keep: None,
        },
        &RollParts {
            flat: con_modifier,
            dice: Vec::new(),
            hint: RollHint::Normal,
        },
    );
    // The persisted `sides` is a sheet value, not client input; the caps
    // still gate it before any draw reaches the port.
    dice::validate(&plan.expression)?;
    let result = env.roll(&plan)?;

    let mut warnings = Vec::new();
    spend_resource(&mut statblock, resource, 1);
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
    heal(&mut statblock, result.total.max(0));

    let now = env.now();
    let audience = state.campaign().settings.player_roll_audience_default;
    let roll = Roll {
        id: RollId::new(),
        roller: actor,
        entity: Some(entity),
        ts: now,
        expr: plan.expression,
        result,
        context: Some(RollContext {
            kind: RollKind::Other,
            stat: None,
            target_dc: None,
        }),
        audience,
    };
    let entry = FeedEntry {
        id: FeedEntryId::new(state.next_feed_id()),
        ts: now,
        clock_s: state.campaign().clock.now_s,
        actor: Some(actor),
        entity: Some(entity),
        audience,
        payload: FeedPayload::Roll { roll },
    };
    let (feed_appends, feed_keep_from) =
        cap_feed(state.campaign().settings.feed_capacity, vec![entry]);
    Ok(stamped_with_feed(
        statblock,
        now,
        feed_appends,
        feed_keep_from,
        warnings,
    ))
}
