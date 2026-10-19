//! The `Roll` command: specs, context, audience and feed entry (RFC
//! `dice.md` 5.6).

use localtt_core::types::{
    Audience,
    DiceExpr,
    EntityId,
    FeedEntry,
    FeedEntryId,
    FeedPayload,
    PlayerId,
    Roll,
    RollContext,
    RollId,
    RollKind,
    StatKey,
};

use super::{
    CommandEnv,
    Event,
    RollSpec,
    Transition,
    cap_feed,
    load_statblock,
    materialize_item_content,
};
use crate::{
    dice::{self, RollPlan, TermPlan},
    error::EngineError,
    rules::{self, RulesContext},
    state::WorkingSet,
};

/// The `Roll` command (RFC `dice.md` 5.6): validates the spec, rolls the plan
/// through the environment and appends exactly one feed entry. The actor is
/// required; a roll is always attributed.
pub(super) fn roll_command(
    state: &dyn WorkingSet,
    entity: Option<EntityId>,
    spec: &RollSpec,
    audience: Option<Audience>,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let Some(actor) = actor else {
        return Err(EngineError::Malformed {
            reason: "a roll requires an actor".to_owned(),
        });
    };
    if let Some(id) = entity
        && state.entity(id)?.is_none()
    {
        return Err(EngineError::NotFound {
            entity: id,
            element: None,
        });
    }
    let (plan, context) = match spec {
        RollSpec::Expression { expr, target } => {
            dice::validate(expr)?;
            let context = target.as_ref().map(|target| RollContext {
                kind: stat_family(target),
                stat: Some(target.clone()),
                target_dc: None,
            });
            let plan = RollPlan {
                terms: vec![TermPlan {
                    expr: expr.clone(),
                    source: None,
                }],
                expression: expr.clone(),
            };
            (plan, context)
        }
        RollSpec::Check { stat, target_dc } => {
            let Some(id) = entity else {
                return Err(EngineError::Malformed {
                    reason: "a check requires an entity to derive from".to_owned(),
                });
            };
            let statblock = load_statblock(state, id, None)?;
            let content = materialize_item_content(&statblock, env.reference())?;
            let parts = rules::roll_parts(
                &statblock,
                stat,
                &RulesContext {
                    now: state.campaign().clock.now_s,
                    content: &content,
                },
            )?;
            let plan = dice::compose(
                DiceExpr::Roll {
                    count: 1,
                    sides: 20,
                    keep: None,
                },
                &parts,
            );
            let context = RollContext {
                kind: stat_family(stat),
                stat: Some(stat.clone()),
                target_dc: *target_dc,
            };
            (plan, Some(context))
        }
    };
    let result = env.roll(&plan)?;
    let now = env.now();
    let audience = audience.unwrap_or(state.campaign().settings.player_roll_audience_default);
    let roll = Roll {
        id: RollId::new(),
        roller: actor,
        entity,
        ts: now,
        expr: plan.expression,
        result,
        context,
        audience,
    };
    let entry = FeedEntry {
        id: FeedEntryId::new(state.next_feed_id()),
        ts: now,
        clock_s: state.campaign().clock.now_s,
        actor: Some(actor),
        entity,
        audience,
        payload: FeedPayload::Roll { roll },
    };
    let (feed_appends, feed_keep_from) =
        cap_feed(state.campaign().settings.feed_capacity, vec![entry]);
    let events = vec![Event::FeedAppended {
        ids: feed_appends.iter().map(|entry| entry.id).collect(),
    }];
    Ok(Transition {
        campaign: None,
        upserts: Vec::new(),
        removals: Vec::new(),
        feed_appends,
        feed_keep_from,
        warnings: Vec::new(),
        events,
    })
}

/// The stat-family map (RFC `dice.md` 5.6.1): the key's prefix decides the
/// roll kind; anything else is `Other`.
fn stat_family(stat: &StatKey) -> RollKind {
    let key = stat.to_string();
    if key.starts_with("save.") {
        RollKind::Save
    } else if key.starts_with("skill.") || key.starts_with("ability.") {
        RollKind::Check
    } else if key.starts_with("attack.") {
        RollKind::Attack
    } else if key.starts_with("damage.") {
        RollKind::Damage
    } else {
        RollKind::Other
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::builders::stat;

    use super::*;

    #[test]
    fn the_stat_family_map_follows_the_prefixes() {
        let family = |key: &str| stat_family(&stat(key).expect("a literal stat key"));
        assert_eq!(family("save.wis"), RollKind::Save);
        assert_eq!(family("skill.arcana"), RollKind::Check);
        assert_eq!(family("ability.str"), RollKind::Check);
        assert_eq!(family("attack.spell"), RollKind::Attack);
        assert_eq!(family("damage.fire"), RollKind::Damage);
        assert_eq!(family("passive.perception"), RollKind::Other);
        assert_eq!(family("hp"), RollKind::Other);
        assert_eq!(family("save"), RollKind::Other);
    }
}
