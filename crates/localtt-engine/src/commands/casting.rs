//! `CastSpell`, `BeginConcentration` and `EndConcentration` (RFC `dice.md`
//! 5.7 and 5.8).

use localtt_core::types::{
    Audience,
    CampaignTime,
    EntityId,
    FeedEntry,
    FeedEntryId,
    FeedPayload,
    PlayerId,
    ResourceId,
    ResourceKind,
    SpellEntryId,
    Status,
    StatusDuration,
    StatusId,
    StatusKind,
    UnixTime,
};

use super::{
    CommandEnv,
    StatusSpec,
    Transition,
    cap_feed,
    load_statblock,
    spend_resource,
    stamped_with_feed,
};
use crate::{
    content::ContentData,
    error::{ElementRef, EngineError, Warning, WarningCode},
    state::WorkingSet,
};

/// The `CastSpell` command (5.7): validates the spell and the slot, spends
/// one slot and replaces concentration when the spell (or the client)
/// concentrates. Never rolls.
pub(super) fn cast_spell_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    spell: SpellEntryId,
    slot: ResourceId,
    upcast_slots: u32,
    concentrate: bool,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let Some(known) = statblock
        .body
        .spellcasting
        .iter()
        .flat_map(|block| block.spells.iter())
        .find(|candidate| candidate.id == spell)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Spell(spell)),
        });
    };
    let known_name = known.name.clone();
    let known_reference = known.reference.clone();

    let Some(slot_index) = statblock
        .body
        .resources
        .iter()
        .position(|candidate| candidate.id == slot)
    else {
        return Err(EngineError::NotFound {
            entity,
            element: Some(ElementRef::Resource(slot)),
        });
    };
    let slot_level = match &statblock.body.resources[slot_index].kind {
        ResourceKind::SpellSlot { level } => Some(*level),
        ResourceKind::PactSlot => None,
        _ => {
            return Err(EngineError::Malformed {
                reason: format!("resource {slot} is not a spell slot"),
            });
        }
    };

    let mut warnings = Vec::new();
    let resolved = match &known_reference {
        Some(reference) => {
            match env
                .reference()
                .get(reference)
                .map_err(EngineError::Content)?
            {
                Some(entry) => match entry.data {
                    ContentData::Spell(data) => Some((data.level, data.concentration)),
                    _ => None,
                },
                None => None,
            }
        }
        None => None,
    };
    if resolved.is_none() {
        warnings.push(Warning {
            code: WarningCode::UnresolvedReference,
            message: format!("`{known_name}` does not resolve to spell content"),
            entity: Some(entity),
            element: Some(ElementRef::Spell(spell)),
        });
    }

    if matches!(resolved, Some((0, _))) {
        return Err(EngineError::Malformed {
            reason: format!("`{known_name}` is a cantrip and cannot be cast with a slot"),
        });
    }
    match (slot_level, resolved) {
        (Some(level), Some((spell_level, _))) => {
            let expected = i64::from(level) - i64::from(spell_level);
            if i64::from(upcast_slots) != expected {
                return Err(EngineError::Malformed {
                    reason: format!(
                        "upcast_slots {upcast_slots} does not match slot level {level} minus spell level {spell_level}"
                    ),
                });
            }
        }
        _ => {
            if upcast_slots != 0 {
                return Err(EngineError::Malformed {
                    reason: format!(
                        "upcast_slots {upcast_slots} cannot be validated without a known slot or spell level"
                    ),
                });
            }
        }
    }

    spend_resource(&mut statblock, slot, 1);
    let spent = &statblock.body.resources[slot_index];
    if spent.current < 0 {
        warnings.push(Warning {
            code: WarningCode::ResourceOverspent,
            message: format!(
                "{} has {} {} left",
                statblock.name, spent.current, spent.name
            ),
            entity: Some(entity),
            element: Some(ElementRef::Resource(slot)),
        });
    }

    let now = env.now();
    let clock_s = state.campaign().clock.now_s;
    let should_concentrate = concentrate || resolved.is_some_and(|(_, concentrates)| concentrates);
    let mut removed = Vec::new();
    if should_concentrate {
        if concentrate && matches!(resolved, Some((_, false))) {
            warnings.push(Warning {
                code: WarningCode::ConcentrationUnsupported,
                message: format!("`{known_name}` does not require concentration"),
                entity: Some(entity),
                element: Some(ElementRef::Spell(spell)),
            });
        }
        removed = statblock
            .body
            .statuses
            .iter()
            .filter(|status| status.kind == StatusKind::Concentration)
            .map(|status| status.name.clone())
            .collect();
        statblock
            .body
            .statuses
            .retain(|status| status.kind != StatusKind::Concentration);
        statblock.body.statuses.push(Status {
            id: StatusId::new(),
            kind: StatusKind::Concentration,
            name: known_name,
            reference: known_reference,
            text: None,
            started_at: clock_s,
            duration: StatusDuration::Indefinite,
            source: None,
            modifiers: Vec::new(),
        });
    }
    let minted = concentration_ends(state, entity, statblock.owner, now, clock_s, removed);
    let (feed_appends, feed_keep_from) = cap_feed(state.campaign().settings.feed_capacity, minted);
    Ok(stamped_with_feed(
        statblock,
        now,
        feed_appends,
        feed_keep_from,
        warnings,
    ))
}

/// The `BeginConcentration` command (5.8): the spec must be a concentration
/// status; every existing concentration is replaced and announced.
pub(super) fn begin_concentration_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    spec: &StatusSpec,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    if spec.kind != StatusKind::Concentration {
        return Err(EngineError::Malformed {
            reason: "BeginConcentration requires a concentration status".to_owned(),
        });
    }
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let now = env.now();
    let clock_s = state.campaign().clock.now_s;
    let removed: Vec<String> = statblock
        .body
        .statuses
        .iter()
        .filter(|status| status.kind == StatusKind::Concentration)
        .map(|status| status.name.clone())
        .collect();
    statblock
        .body
        .statuses
        .retain(|status| status.kind != StatusKind::Concentration);
    statblock.body.statuses.push(Status {
        id: StatusId::new(),
        kind: StatusKind::Concentration,
        name: spec.name.clone(),
        reference: spec.reference.clone(),
        text: spec.text.clone(),
        started_at: clock_s,
        duration: spec.duration.clone(),
        source: spec.source.clone(),
        modifiers: spec.modifiers.clone(),
    });
    let minted = concentration_ends(state, entity, statblock.owner, now, clock_s, removed);
    let (feed_appends, feed_keep_from) = cap_feed(state.campaign().settings.feed_capacity, minted);
    Ok(stamped_with_feed(
        statblock,
        now,
        feed_appends,
        feed_keep_from,
        Vec::new(),
    ))
}

/// The `EndConcentration` command (5.8): removes every concentration status
/// and announces each; with none present the command is `NotFound`.
pub(super) fn end_concentration_command(
    state: &dyn WorkingSet,
    entity: EntityId,
    expected_rev: Option<u64>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let mut statblock = load_statblock(state, entity, expected_rev)?;
    let removed: Vec<String> = statblock
        .body
        .statuses
        .iter()
        .filter(|status| status.kind == StatusKind::Concentration)
        .map(|status| status.name.clone())
        .collect();
    if removed.is_empty() {
        return Err(EngineError::NotFound {
            entity,
            element: None,
        });
    }
    statblock
        .body
        .statuses
        .retain(|status| status.kind != StatusKind::Concentration);
    let now = env.now();
    let clock_s = state.campaign().clock.now_s;
    let minted = concentration_ends(state, entity, statblock.owner, now, clock_s, removed);
    let (feed_appends, feed_keep_from) = cap_feed(state.campaign().settings.feed_capacity, minted);
    Ok(stamped_with_feed(
        statblock,
        now,
        feed_appends,
        feed_keep_from,
        Vec::new(),
    ))
}

/// One `ConcentrationEnded` entry per removed status, in document order
/// (5.7); ids come from the campaign's feed counter.
fn concentration_ends(
    state: &dyn WorkingSet,
    entity: EntityId,
    owner: PlayerId,
    now: UnixTime,
    clock_s: CampaignTime,
    names: Vec<String>,
) -> Vec<FeedEntry> {
    let mut next = state.next_feed_id();
    names
        .into_iter()
        .map(|name| {
            let entry = FeedEntry {
                id: FeedEntryId::new(next),
                ts: now,
                clock_s,
                actor: Some(owner),
                entity: Some(entity),
                audience: Audience::RollerAndDm,
                payload: FeedPayload::ConcentrationEnded { name },
            };
            next = next.saturating_add(1);
            entry
        })
        .collect()
}
