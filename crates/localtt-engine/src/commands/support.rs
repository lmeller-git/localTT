//! Helpers shared by the command handlers: target loading, revision stamping
//! and transition assembly.

use localtt_core::types::{
    AnyEntity,
    Campaign,
    EntityId,
    FeedEntry,
    FeedEntryId,
    StatblockEntity,
    UnixTime,
};

use crate::{
    commands::{Event, Transition},
    error::{EngineError, Warning},
    state::WorkingSet,
};

/// The common prelude (5.4): the target document must exist, and an explicit
/// `expected_rev` must match the authoritative revision.
pub(super) fn load_target(
    state: &dyn WorkingSet,
    entity: EntityId,
    expected_rev: Option<u64>,
) -> Result<AnyEntity, EngineError> {
    let document = state.entity(entity)?.ok_or(EngineError::NotFound {
        entity,
        element: None,
    })?;
    if let Some(expected) = expected_rev {
        let actual = document.rev();
        if actual != expected {
            return Err(EngineError::Conflict {
                entity,
                expected,
                actual,
            });
        }
    }
    Ok(document)
}

/// Loads a target of a command that addresses statblock-only fields (hit
/// points, statuses, resources). Other entity kinds cannot carry those
/// fields, so the command is `Malformed` (5.4).
pub(super) fn load_statblock(
    state: &dyn WorkingSet,
    entity: EntityId,
    expected_rev: Option<u64>,
) -> Result<StatblockEntity, EngineError> {
    match load_target(state, entity, expected_rev)? {
        AnyEntity::Statblock(document) => Ok(document),
        AnyEntity::Note(_) | AnyEntity::Handout(_) => Err(EngineError::Malformed {
            reason: format!("entity {entity} is not a statblock"),
        }),
    }
}

/// Stamps a touched copy: `rev = old.rev + 1`, `updated_at = now` (5.5).
///
/// The revision saturates at `u64::MAX`; a document at the ceiling has no next
/// revision, and the executor's monotonicity assertion fails fast on it instead
/// of a wrapped revision slipping into the store.
pub(super) fn stamp(entity: &mut AnyEntity, now: UnixTime) {
    let rev = entity.rev().saturating_add(1);
    match entity {
        AnyEntity::Statblock(document) => {
            document.rev = rev;
            document.updated_at = now;
        }
        AnyEntity::Note(document) => {
            document.rev = rev;
            document.updated_at = now;
        }
        AnyEntity::Handout(document) => {
            document.rev = rev;
            document.updated_at = now;
        }
    }
}

/// A transition that only changes the campaign document.
pub(super) fn campaign_change(campaign: Campaign) -> Transition {
    Transition {
        campaign: Some(campaign),
        upserts: Vec::new(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
        warnings: Vec::new(),
        events: vec![Event::CampaignChanged],
    }
}

/// Wraps one stamped document into the transition a single-upsert command
/// produces: one `EntityChanged`, no campaign or feed change.
pub(super) fn one_upsert(entity: AnyEntity, warnings: Vec<Warning>) -> Transition {
    let events = vec![Event::EntityChanged {
        id: entity.id(),
        rev: entity.rev(),
        kind: entity.kind(),
    }];
    Transition {
        campaign: None,
        upserts: vec![entity],
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
        warnings,
        events,
    }
}

/// Wraps two stamped documents into the transition a transfer produces:
/// upserts and events ascending by id, no campaign or feed change.
pub(super) fn two_upserts(
    first: AnyEntity,
    second: AnyEntity,
    now: UnixTime,
    warnings: Vec<Warning>,
) -> Transition {
    let mut first = first;
    let mut second = second;
    stamp(&mut first, now);
    stamp(&mut second, now);
    let mut upserts = vec![first, second];
    upserts.sort_by_key(AnyEntity::id);
    let events = upserts
        .iter()
        .map(|document| Event::EntityChanged {
            id: document.id(),
            rev: document.rev(),
            kind: document.kind(),
        })
        .collect();
    Transition {
        campaign: None,
        upserts,
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
        warnings,
        events,
    }
}

/// Wraps one stamped document plus capped feed appends into a transition:
/// `EntityChanged` first, then `FeedAppended` when anything was appended.
pub(super) fn stamped_with_feed(
    statblock: StatblockEntity,
    now: UnixTime,
    feed_appends: Vec<FeedEntry>,
    feed_keep_from: Option<FeedEntryId>,
    warnings: Vec<Warning>,
) -> Transition {
    let mut document = AnyEntity::from(statblock);
    stamp(&mut document, now);
    let mut events = vec![Event::EntityChanged {
        id: document.id(),
        rev: document.rev(),
        kind: document.kind(),
    }];
    if !feed_appends.is_empty() {
        events.push(Event::FeedAppended {
            ids: feed_appends.iter().map(|entry| entry.id).collect(),
        });
    }
    Transition {
        campaign: None,
        upserts: vec![document],
        removals: Vec::new(),
        feed_appends,
        feed_keep_from,
        warnings,
        events,
    }
}

/// Caps one transition's minted feed entries to the ring (5.6): the newest
/// `c = capacity.max(1)` survive, and the returned cutoff drops stored entries
/// below them.
///
/// The cutoff is spelled `last - (c - 1)` rather than `last + 1 - c`, so it
/// stays exact at the position ceiling (`last + 1` cannot overflow).
pub(super) fn cap_feed(
    capacity: u32,
    entries: Vec<FeedEntry>,
) -> (Vec<FeedEntry>, Option<FeedEntryId>) {
    let ring = capacity.max(1);
    let ring_u64 = u64::from(ring);
    let ring_usize = usize::try_from(ring).expect("a u32 feed capacity fits usize");
    let Some(last) = entries.last() else {
        return (entries, None);
    };
    let keep_from = if last.id.get() >= ring_u64 {
        Some(FeedEntryId::new(last.id.get() - (ring_u64 - 1)))
    } else {
        None
    };
    if entries.len() <= ring_usize {
        return (entries, keep_from);
    }
    let dropped = entries.len() - ring_usize;
    (entries.into_iter().skip(dropped).collect(), keep_from)
}
