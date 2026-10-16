//! Deterministic identifiers for fixtures (RFC-0003 section 5.3).
//!
//! Fixtures must serialize to identical bytes across runs and machines, so they
//! use fixed UUIDs rather than [`localtt_core::types::Id::new`].
#![expect(
    clippy::todo,
    unused_variables,
    dead_code,
    reason = "skeleton phase: every body is `todo!()`; removed in the implement phase"
)]

use localtt_core::types::Id;
use uuid::Uuid;

/// A stable, v7-shaped UUID derived from `seed`.
#[must_use]
#[inline]
pub(crate) fn fixture_uuid(seed: u64) -> Uuid {
    todo!()
}

/// An [`Id`] of any domain, built from [`fixture_uuid`].
#[must_use]
#[inline]
pub(crate) fn fixture_id<T>(seed: u64) -> Id<T> {
    todo!()
}
