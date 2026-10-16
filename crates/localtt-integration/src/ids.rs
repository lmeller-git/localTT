//! Deterministic identifiers for fixtures (RFC-0003 section 5.3).
//!
//! Fixtures must serialize to identical bytes across runs and machines, so they
//! use fixed UUIDs rather than [`localtt_core::types::Id::new`].

use localtt_core::types::Id;
use uuid::Uuid;

/// A stable, v7-shaped UUID derived from `seed`.
#[must_use]
#[inline]
pub(crate) fn fixture_uuid(seed: u64) -> Uuid {
    let mut bytes = [0u8; 16];
    bytes[8..].copy_from_slice(&seed.to_be_bytes());
    // v7 shape: version nibble 7 and the RFC 4122 variant bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// An [`Id`] of any domain, built from [`fixture_uuid`].
#[must_use]
#[inline]
pub(crate) fn fixture_id<T>(seed: u64) -> Id<T> {
    Id::from_uuid(fixture_uuid(seed))
}
