//! The snapshot reader seam and the directory-backed reader
//! (`docs/rfcs/content.md` sections 4.2 and 5.1).

use std::path::{Path, PathBuf};

use localtt_core::types::RefId;
use localtt_engine::content::{ContentEntry, ContentFault, ContentSummary, Provenance};

use crate::reference::snapshot::IndexRow;

/// One loaded snapshot, read for indexing.
///
/// This is the seam where a later *local* reader (the pack loader) plugs in:
/// [`crate::reference::IndexedSources`] only needs these four calls. A live
/// REST backend is not a reader of this trait; the runtime's async dispatcher
/// routes remote link loads (CONTENT-12), and nothing here changes when it
/// lands. Readers are immutable after construction; bodies are read lazily by
/// [`SnapshotReader::entry`].
pub trait SnapshotReader: Send + Sync {
    /// Namespace and licensing of the snapshot.
    fn provenance(&self) -> &Provenance;

    /// Every entry's summary (id, kind, name), eager; bodies stay behind
    /// [`SnapshotReader::entry`].
    fn summaries(&self) -> Result<Vec<ContentSummary>, ContentFault>;

    /// One entry body by id; no alias resolution here.
    fn entry(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault>;

    /// The `(alias, target)` rows of this snapshot.
    fn aliases(&self) -> Result<Vec<(RefId, RefId)>, ContentFault>;
}

/// One namespace directory on disk: `index.json` plus `entries.jsonl`, with
/// bodies read lazily by offset (RFC section 5.1).
pub(crate) struct DirSnapshot {
    /// The namespace directory, kept for lazy body reads.
    dir: PathBuf,
    /// Provenance from `index.json`.
    provenance: Provenance,
    /// Index rows, ascending by id, as read and validated at open.
    rows: Vec<IndexRow>,
}

impl DirSnapshot {
    /// Opens a namespace directory: reads and gates `index.json`, checks that
    /// `entries.jsonl` is present, and validates `aliases.json` when present.
    ///
    /// A missing required file is `Unavailable`; malformed content is
    /// `Corrupt`; a version this build does not know is
    /// `SnapshotVersionRefused`.
    pub(crate) fn open(dir: &Path) -> Result<Self, ContentFault> {
        todo!()
    }
}

impl SnapshotReader for DirSnapshot {
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
