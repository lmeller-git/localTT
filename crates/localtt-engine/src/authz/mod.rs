//! Authorization, entity views and feed visibility (RFC `authz.md`).
//!
//! The module is pure over core documents: campaign and entity are arguments;
//! no ports, no working set, no I/O (section 6).

mod access;
mod view;

use localtt_core::types::{AnyEntity, Campaign, FeedEntry, PlayerId};

pub use self::view::{
    EntityView,
    EntityViewBody,
    FeedEntryView,
    HandoutView,
    NoteView,
    ProficienciesView,
    StatblockView,
};

/// True when `viewer` may read at least one section of `entity`: roster
/// membership is required first, then DM, owner, any `Write` grant, or at
/// least one visible field from the `Read` grants (section 5.2).
#[must_use]
#[inline]
pub fn can_read(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> bool {
    todo!()
}

/// True when `viewer` may write `entity`: roster membership, then DM, owner
/// or any `Write` grant; masks are ignored (section 5.2).
#[must_use]
#[inline]
pub fn can_write(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> bool {
    todo!()
}

/// The viewer's mirror of one entity: mirrored headers, the visible sections,
/// the hidden set and the write flag. Never a redacted clone (ENG-8).
#[must_use]
#[inline]
pub fn view_of(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> EntityView {
    todo!()
}

/// True when `viewer` may see the feed entry (section 5.5).
#[must_use]
#[inline]
pub fn entry_visible(campaign: &Campaign, entry: &FeedEntry, viewer: PlayerId) -> bool {
    todo!()
}
