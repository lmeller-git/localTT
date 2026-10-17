//! One-hop alias resolution (`docs/rfcs/content.md` section 5.4).
//!
//! An alias is an old `RefId` that resolves to one live id of the same
//! snapshot. Alias names are not indexed: `get` is the only alias-aware call.

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::types::RefId;
use localtt_engine::content::ContentFault;

/// A validated `alias -> target` table.
pub(crate) struct AliasTable {
    /// Alias id to live target id; built once and then read-only.
    rows: BTreeMap<RefId, RefId>,
}

impl AliasTable {
    /// Validates and stores `(alias, target)` rows against the live ids of one
    /// snapshot.
    ///
    /// Refused as [`ContentFault::Corrupt`]: an alias that equals a live id, a
    /// duplicate alias, a target that is not live, and a target that is itself
    /// an alias (chains).
    pub(crate) fn new(
        rows: Vec<(RefId, RefId)>,
        live: &BTreeSet<RefId>,
    ) -> Result<Self, ContentFault> {
        todo!()
    }

    /// Resolves one id; `Some(target)` when `id` is an alias, `None` otherwise.
    #[inline]
    pub(crate) fn resolve(&self, id: &RefId) -> Option<&RefId> {
        todo!()
    }
}
