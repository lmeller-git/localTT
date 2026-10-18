//! Tracked resources: the provisional spend arithmetic and, from the U3
//! implement phase, the spend/restore command effects (RFC
//! `engine-executor.md` 5.4).

use localtt_core::types::{ResourceId, StatblockEntity};

/// Spends `amount` of a tracked resource; overspending is allowed and warned.
/// Saturates at the `i32` bounds.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    if let Some(resource) = entity.body.resources.iter_mut().find(|r| r.id == id) {
        resource.current = resource.current.saturating_sub(amount);
    }
}
