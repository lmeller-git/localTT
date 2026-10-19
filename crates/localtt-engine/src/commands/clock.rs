//! Clock command effects: the campaign move, the expiry sweep and dawn
//! recharge (RFC `engine-executor.md` 5.2/5.3). Private to the command layer;
//! `crate::clock` holds the pure arithmetic.

use std::collections::BTreeMap;

use localtt_core::types::{
    AnyEntity,
    Audience,
    Campaign,
    CampaignTime,
    EntityId,
    FeedEntry,
    FeedEntryId,
    FeedPayload,
    Formula,
    PlayerId,
    Recharge,
    ResourceKind,
    StatblockEntity,
    StatusDuration,
    StatusId,
};

use super::{CommandEnv, Event, Transition, cap_feed, stamp};
use crate::{
    error::{ElementRef, EngineError, Warning, WarningCode},
    state::WorkingSet,
};

/// The `AdvanceClock` command (5.2): an overflow of the signed move is
/// `Malformed`.
pub(super) fn advance_clock_command(
    state: &dyn WorkingSet,
    by_seconds: i64,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let old = state.campaign().clock.now_s;
    let Some(seconds) = old.seconds().checked_add(by_seconds) else {
        return Err(EngineError::Malformed {
            reason: format!("moving the clock by {by_seconds} overflows campaign time"),
        });
    };
    let to = CampaignTime::from_seconds(seconds);
    match clock_move(state, old, to)? {
        Some(moved) => Ok(finish(state, moved, env)),
        None => Ok(Transition::empty()),
    }
}

/// The `SetClock` command (5.2): `to` is taken as given; pre-origin times are
/// legal.
pub(super) fn set_clock_command(
    state: &dyn WorkingSet,
    to: CampaignTime,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    let old = state.campaign().clock.now_s;
    match clock_move(state, old, to)? {
        Some(moved) => Ok(finish(state, moved, env)),
        None => Ok(Transition::empty()),
    }
}

/// The document-level effects of a clock move, before any stamping: a rest
/// merges its target into `touched` so one command stays one revision.
pub(super) struct ClockMove {
    /// The campaign document with the new clock time.
    pub(super) campaign: Campaign,
    /// Dawn-refilled documents by id, unstamped.
    pub(super) touched: BTreeMap<EntityId, StatblockEntity>,
    /// Warnings produced by the move itself.
    pub(super) warnings: Vec<Warning>,
    /// Newly expired statuses in assignment order.
    pub(super) expired: Vec<ExpiredStatus>,
}

/// One newly expired status, before feed ids are minted.
pub(super) struct ExpiredStatus {
    /// The in-game instant it expired at.
    pub(super) expires_at: CampaignTime,
    /// The entity that carried the status.
    pub(super) entity: EntityId,
    /// The entity owner; the `RollerAndDm` audience hangs on it.
    pub(super) owner: PlayerId,
    /// The status element.
    pub(super) status: StatusId,
    /// The live name snapshot.
    pub(super) name: String,
}

/// Runs the move from `old` to `to`; `None` when they are equal (5.2: an
/// empty transition). Errors only propagate from the working set.
pub(super) fn clock_move(
    state: &dyn WorkingSet,
    old: CampaignTime,
    to: CampaignTime,
) -> Result<Option<ClockMove>, EngineError> {
    if to == old {
        return Ok(None);
    }
    let mut campaign = state.campaign().clone();
    campaign.clock.now_s = to;
    let expired = if to > old {
        newly_expired(state, old, to)?
    } else {
        Vec::new()
    };
    let mut touched = BTreeMap::new();
    let mut warnings = Vec::new();
    let crossings = crate::clock::crossed_dawns(old, to, campaign.settings.dawn_hour);
    if crossings < 0 {
        warnings.push(Warning {
            code: WarningCode::DawnRewind,
            message: "the clock moved back across dawn; recharges are not reversed".to_owned(),
            entity: None,
            element: None,
        });
    } else if crossings > 0 {
        for header in state.headers() {
            let Some(document) = state.entity(header.id)? else {
                continue;
            };
            let AnyEntity::Statblock(mut statblock) = document else {
                continue;
            };
            if refill_resources(&mut statblock, Recharge::Dawn, &mut warnings) {
                touched.insert(header.id, statblock);
            }
        }
    }
    Ok(Some(ClockMove {
        campaign,
        touched,
        warnings,
        expired,
    }))
}

/// Stamps the touched documents and assembles the transition (5.6/5.7): the
/// campaign change, the upserts in ascending id order, the capped expiry
/// appends and the events.
///
/// One `env.now()` reading per transition stamps every document and every feed
/// entry, so the whole commit shares a single wall-clock instant.
///
/// Minting saturates at the position ceiling: an exhausted feed can only
/// produce a duplicate top id, which the store's append validation refuses
/// loudly; the counter never wraps to zero.
pub(super) fn finish(
    state: &dyn WorkingSet,
    moved: ClockMove,
    env: &mut dyn CommandEnv,
) -> Transition {
    let ClockMove {
        campaign,
        touched,
        warnings,
        expired,
    } = moved;
    let now = env.now();
    let mut upserts = Vec::with_capacity(touched.len());
    for (_, document) in touched {
        let mut document = AnyEntity::from(document);
        stamp(&mut document, now);
        upserts.push(document);
    }
    let mut next = state.next_feed_id();
    let minted = expired
        .into_iter()
        .map(|expired| {
            let entry = FeedEntry {
                id: FeedEntryId::new(next),
                ts: now,
                clock_s: expired.expires_at,
                actor: Some(expired.owner),
                entity: Some(expired.entity),
                audience: Audience::RollerAndDm,
                payload: FeedPayload::StatusExpired {
                    status: expired.status,
                    name: expired.name,
                },
            };
            next = next.saturating_add(1);
            entry
        })
        .collect();
    let (feed_appends, feed_keep_from) = cap_feed(campaign.settings.feed_capacity, minted);
    let mut events = Vec::with_capacity(upserts.len() + 2);
    events.push(Event::CampaignChanged);
    events.extend(upserts.iter().map(|document| Event::EntityChanged {
        id: document.id(),
        rev: document.rev(),
        kind: document.kind(),
    }));
    if !feed_appends.is_empty() {
        events.push(Event::FeedAppended {
            ids: feed_appends.iter().map(|entry| entry.id).collect(),
        });
    }
    Transition {
        campaign: Some(campaign),
        upserts,
        removals: Vec::new(),
        feed_appends,
        feed_keep_from,
        warnings,
        events,
    }
}

/// Every status whose `Timed { expires_at }` falls in `(old, to]` (5.2), in
/// the deterministic `(expires_at, entity, status)` assignment order.
fn newly_expired(
    state: &dyn WorkingSet,
    old: CampaignTime,
    to: CampaignTime,
) -> Result<Vec<ExpiredStatus>, EngineError> {
    let mut expired = Vec::new();
    for (entity, status) in state.expiry_candidates_until(to) {
        let Some(document) = state.entity(entity)? else {
            continue;
        };
        let AnyEntity::Statblock(statblock) = document else {
            continue;
        };
        let Some(candidate) = statblock
            .body
            .statuses
            .iter()
            .find(|candidate| candidate.id == status)
        else {
            continue;
        };
        let StatusDuration::Timed { expires_at } = &candidate.duration else {
            continue;
        };
        if *expires_at > old {
            expired.push(ExpiredStatus {
                expires_at: *expires_at,
                entity,
                owner: statblock.owner,
                status,
                name: candidate.name.clone(),
            });
        }
    }
    expired.sort_by_key(|expired| (expired.expires_at, expired.entity, expired.status));
    Ok(expired)
}

/// Refills one recharge class of `statblock` to its maximum; returns whether
/// anything changed. A non-constant maximum is skipped and warns
/// `FormulaUnevaluated` (OQ-2). Long-recharge hit dice are skipped: they
/// regain half on a long rest (`dice.md` 5.9), not to full.
pub(super) fn refill_resources(
    statblock: &mut StatblockEntity,
    recharge: Recharge,
    warnings: &mut Vec<Warning>,
) -> bool {
    let id = statblock.id;
    let mut changed = false;
    for resource in &mut statblock.body.resources {
        if resource.recharge != recharge {
            continue;
        }
        if recharge == Recharge::Long && matches!(resource.kind, ResourceKind::HitDie { .. }) {
            continue;
        }
        match resource.max {
            Formula::Const(max) => {
                if resource.current != max {
                    resource.current = max;
                    changed = true;
                }
            }
            Formula::Expr(_) => warnings.push(Warning {
                code: WarningCode::FormulaUnevaluated,
                message: format!(
                    "{} was not refilled: its maximum needs the rules unit",
                    resource.name
                ),
                entity: Some(id),
                element: Some(ElementRef::Resource(resource.id)),
            }),
        }
    }
    changed
}
