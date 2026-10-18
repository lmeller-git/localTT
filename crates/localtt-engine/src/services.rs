//! Read-only services over the working set (RFC `authz.md` 4.2, 5.5, 5.6).

use localtt_core::types::{EntityId, FeedEntryId, PlayerId};

use crate::{
    authz::{EntityView, FeedEntryView},
    error::EngineError,
    state::WorkingSet,
};

/// Every entity the viewer may read, ascending id.
///
/// Infallible: ids come from `WorkingSet::headers`, and an id its own working
/// set cannot materialize is skipped (it cannot happen; engine unit tests pin
/// the behavior).
#[inline]
pub fn list_entities(state: &dyn WorkingSet, viewer: PlayerId) -> Vec<EntityView> {
    todo!()
}

/// One entity view for one viewer: missing before unauthorized.
///
/// `NotFound { entity, element: None }` when the id is absent;
/// `Unauthorized { actor: viewer, entity: Some(id) }` when present but
/// unreadable; otherwise the same view `list_entities` would return.
#[inline]
pub fn get_entity(
    state: &dyn WorkingSet,
    id: EntityId,
    viewer: PlayerId,
) -> Result<EntityView, EngineError> {
    todo!()
}

/// One ascending page of feed entries the viewer may see.
///
/// `cursor` is exclusive (`id > cursor`), the cursor may name an invisible
/// entry, `limit == 0` is an empty page, and the returned views drop the
/// audience (section 5.5).
#[inline]
pub fn feed_after(
    state: &dyn WorkingSet,
    viewer: PlayerId,
    cursor: Option<FeedEntryId>,
    limit: usize,
) -> Vec<FeedEntryView> {
    todo!()
}
