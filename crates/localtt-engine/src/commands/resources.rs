//! Tracked resources: the provisional spend arithmetic and, from the U3
//! implement phase, the spend/restore command effects (see `docs/engine.md` section 2).

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
    Recharge,
    Resource,
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

/// The optional fields of one [`super::Command::EditResource`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResourceEdit {
    pub(super) name: Option<String>,
    pub(super) max: Option<Formula>,
    pub(super) current: Option<i32>,
    pub(super) recharge: Option<Recharge>,
}

/// The fields of one [`super::Command::AddResource`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NewResource<'a> {
    pub(super) name: &'a str,
    pub(super) kind: ResourceKind,
    pub(super) max: &'a Formula,
    pub(super) current: Option<i32>,
    pub(super) recharge: Recharge,
}

/// The `AddResource` command: mints one resource and stamps the statblock.
pub(super) fn add_resource_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    new: NewResource<'_>,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let name = validated_resource_name(new.name)?;
    validate_resource_kind(new.kind)?;
    validate_resource_max(new.max)?;
    let current = new.current.unwrap_or_else(|| default_current(new.max));
    statblock.body.resources.push(Resource {
        id: ResourceId::new(),
        name,
        kind: new.kind,
        max: new.max.clone(),
        current,
        recharge: new.recharge,
    });
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `EditResource` command: at least one field must be set. Lowering a
/// constant maximum clamps the current value unless `current` is also set.
pub(super) fn edit_resource_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    resource: ResourceId,
    edit: ResourceEdit,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    if edit.name.is_none()
        && edit.max.is_none()
        && edit.current.is_none()
        && edit.recharge.is_none()
    {
        return Err(EngineError::Malformed {
            reason: "editing a resource needs at least one field".to_owned(),
        });
    }
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    if let Some(name) = &edit.name {
        validated_resource_name(name)?;
    }
    if let Some(max) = &edit.max {
        validate_resource_max(max)?;
    }
    let Some(stored) = statblock
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
    if let Some(name) = edit.name {
        stored.name = name.trim().to_owned();
    }
    if let Some(max) = edit.max {
        stored.max = max;
        if edit.current.is_none()
            && let Formula::Const(maximum) = &stored.max
        {
            stored.current = stored.current.min(*maximum);
        }
    }
    if let Some(current) = edit.current {
        stored.current = current;
    }
    if let Some(recharge) = edit.recharge {
        stored.recharge = recharge;
    }
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// The `RemoveResource` command.
pub(super) fn remove_resource_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    resource: ResourceId,
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
    statblock.body.resources.remove(index);
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, env.now());
    Ok(one_upsert(document, Vec::new()))
}

/// Trims a resource name and rejects an empty one.
fn validated_resource_name(name: &str) -> Result<String, EngineError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(EngineError::Malformed {
            reason: "a resource name must not be empty".to_owned(),
        });
    }
    Ok(name.to_owned())
}

/// Rejects a spell-slot level outside 1 to 9 and hit-die sides outside 4 to 12.
fn validate_resource_kind(kind: ResourceKind) -> Result<(), EngineError> {
    match kind {
        ResourceKind::SpellSlot { level } if !(1..=9).contains(&level) => {
            Err(EngineError::Malformed {
                reason: format!("spell slot level {level} is outside 1..=9"),
            })
        }
        ResourceKind::HitDie { sides } if !(4..=12).contains(&sides) => {
            Err(EngineError::Malformed {
                reason: format!("hit die sides {sides} are outside 4..=12"),
            })
        }
        _ => Ok(()),
    }
}

/// Rejects a negative constant maximum; expression maxima are evaluated by
/// the rules unit.
fn validate_resource_max(max: &Formula) -> Result<(), EngineError> {
    if let Formula::Const(maximum) = max
        && *maximum < 0
    {
        return Err(EngineError::Malformed {
            reason: format!("resource maximum {maximum} is negative"),
        });
    }
    Ok(())
}

/// The starting value for a new resource: full for a constant maximum, zero
/// for an expression maximum the engine cannot evaluate here.
fn default_current(max: &Formula) -> i32 {
    match max {
        Formula::Const(maximum) => *maximum,
        Formula::Expr(_) => 0,
    }
}

/// The `SpendHitDie` command (see `docs/engine.md` section 4): requires an actor, rolls
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
