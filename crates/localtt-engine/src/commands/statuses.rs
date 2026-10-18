//! Statuses: the provisional concentration helper and, from the U3 implement
//! phase, the status command effects (RFC `engine-executor.md` 5.4).

use localtt_core::types::{StatblockEntity, Status, StatusKind};

/// Begins concentration: replaces any existing concentration status
/// (RFC-0001 5.4, command-layer semantics).
#[inline]
pub fn begin_concentration(entity: &mut StatblockEntity, status: Status) {
    entity
        .body
        .statuses
        .retain(|s| s.kind != StatusKind::Concentration);
    entity.body.statuses.push(status);
}
