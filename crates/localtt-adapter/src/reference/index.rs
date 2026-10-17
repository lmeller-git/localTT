//! The runtime `ReferenceSource` over N snapshots
//! (`docs/rfcs/content.md` sections 4.2 and 6.2).

use std::path::PathBuf;

use localtt_core::types::RefId;
use localtt_engine::content::{
    ContentEntry,
    ContentFault,
    ContentSummary,
    Provenance,
    ReferenceSource,
    SearchQuery,
};

use crate::reference::{aliases::AliasTable, source::SnapshotReader};

/// One loaded snapshot, indexed for lookup.
struct LoadedSource {
    /// Reads bodies and metadata lazily.
    reader: Box<dyn SnapshotReader>,
    /// Provenance read eagerly at construction.
    provenance: Provenance,
    /// Name summaries, eager.
    summaries: Vec<ContentSummary>,
    /// Validated alias table.
    aliases: AliasTable,
}

/// The runtime [`ReferenceSource`]: N loaded snapshots composed by namespace.
///
/// Namespaces are the join key; a ref whose namespace is not loaded answers
/// `Ok(None)`. Construction validates the snapshots (duplicate namespaces,
/// alias tables); bodies stay on disk until `get` or `search` needs them.
pub struct IndexedSources {
    /// Sources in load order (root order, ascending directory names).
    sources: Vec<LoadedSource>,
}

impl IndexedSources {
    /// Loads every namespace directory under each root, in root order and
    /// ascending directory-name order.
    ///
    /// At least one root is required; a missing root, a namespace directory
    /// without `index.json` or without `entries.jsonl` is `Unavailable`.
    /// `index.json` is read and gated before `entries.jsonl` is checked.
    /// Malformed content is `Corrupt`; a directory whose name differs from its
    /// `provenance.namespace` is `Corrupt`; a version this build does not know
    /// is `SnapshotVersionRefused`; two sources with the same namespace are
    /// `NamespaceConflict`. Names that are not valid namespaces (`*.bak`,
    /// `*.tmp`, `*.old` backups included) and entries that are not directories
    /// are pruned silently.
    #[inline]
    pub fn from_dirs(roots: &[PathBuf]) -> Result<Self, ContentFault> {
        todo!()
    }

    /// Composes already-open readers, running the same composition checks as
    /// [`IndexedSources::from_dirs`] without a directory layout.
    #[inline]
    pub fn from_readers(readers: Vec<Box<dyn SnapshotReader>>) -> Result<Self, ContentFault> {
        todo!()
    }
}

impl ReferenceSource for IndexedSources {
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
