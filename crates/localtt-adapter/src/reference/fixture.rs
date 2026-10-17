//! The in-memory fixture source (`docs/rfcs/content.md` section 4.2).
//!
//! The fixture is the "every port ships an in-memory implementation" side of
//! U2: a fixed catalog covering every `ContentKind`, duplicate names across
//! kinds, a unicode name and one `needs_review` entry. It composes the same
//! index as the directory source, so ranking, aliasing and validation have one
//! implementation.

use localtt_core::types::RefId;
use localtt_engine::content::{
    ContentEntry,
    ContentFault,
    ContentSummary,
    Provenance,
    ReferenceSource,
    SearchQuery,
};

use crate::reference::{index::IndexedSources, source::SnapshotReader};

/// An in-memory [`ReferenceSource`] over the fixed fixture catalog.
pub struct FixtureSource {
    /// The shared index over the fixture snapshot.
    index: IndexedSources,
}

impl FixtureSource {
    /// The fixture catalog as a source.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        todo!()
    }

    /// The fixture catalog plus an explicit alias table.
    ///
    /// An invalid table is refused as [`ContentFault::Corrupt`]: an alias
    /// outside the fixture namespace, an alias that collides with a live id,
    /// a duplicate, a dead target or a chain.
    #[inline]
    pub fn with_aliases(aliases: Vec<(RefId, RefId)>) -> Result<Self, ContentFault> {
        todo!()
    }

    /// A copy of the fixture entries, for expectations in the contract suite.
    #[must_use]
    #[inline]
    pub fn catalog() -> Vec<ContentEntry> {
        todo!()
    }
}

impl Default for FixtureSource {
    #[inline]
    fn default() -> Self {
        todo!()
    }
}

impl ReferenceSource for FixtureSource {
    #[inline]
    fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        todo!()
    }

    #[inline]
    fn search(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        todo!()
    }

    #[inline]
    fn autocomplete(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        todo!()
    }

    #[inline]
    fn sources(&self) -> Vec<Provenance> {
        todo!()
    }
}

/// An in-memory snapshot over explicit entries (crate-private).
pub(crate) struct MemorySnapshot {
    /// Namespace, licensing and pinned revision.
    provenance: Provenance,
    /// The entries, as given.
    entries: Vec<ContentEntry>,
    /// `(alias, target)` rows, as given.
    aliases: Vec<(RefId, RefId)>,
}

impl MemorySnapshot {
    /// Builds a snapshot over `entries` and `aliases`.
    #[inline]
    pub(crate) fn new(
        provenance: Provenance,
        entries: Vec<ContentEntry>,
        aliases: Vec<(RefId, RefId)>,
    ) -> Self {
        todo!()
    }
}

impl SnapshotReader for MemorySnapshot {
    #[inline]
    fn provenance(&self) -> &Provenance {
        todo!()
    }

    #[inline]
    fn summaries(&self) -> Result<Vec<ContentSummary>, ContentFault> {
        todo!()
    }

    #[inline]
    fn entry(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        todo!()
    }

    #[inline]
    fn aliases(&self) -> Result<Vec<(RefId, RefId)>, ContentFault> {
        todo!()
    }
}
