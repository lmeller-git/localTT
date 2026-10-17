//! The snapshot reader seam and the directory-backed reader
//! (`docs/rfcs/content.md` sections 4.2 and 5.1).

use std::path::{Path, PathBuf};

use localtt_core::types::RefId;
use localtt_engine::content::{ContentEntry, ContentFault, ContentSummary, Provenance};

use crate::reference::snapshot::{IndexRow, read_aliases, read_entry, read_index, validate_entry};

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
    /// `(alias, target)` rows from `aliases.json`, validated for parsing.
    aliases: Vec<(RefId, RefId)>,
}

impl DirSnapshot {
    /// Opens a namespace directory: reads and gates `index.json`, checks that
    /// `entries.jsonl` is present, and validates `aliases.json` when present.
    ///
    /// A missing required file is `Unavailable`; malformed content is
    /// `Corrupt`; a version this build does not know is
    /// `SnapshotVersionRefused`. `index.json` is read and gated before
    /// `entries.jsonl` presence is checked, so a malformed index is `Corrupt`
    /// even when `entries.jsonl` is also missing.
    pub(crate) fn open(dir: &Path) -> Result<Self, ContentFault> {
        let index = read_index(dir)?;
        let entries_path = dir.join("entries.jsonl");
        if !entries_path.is_file() {
            return Err(ContentFault::Unavailable {
                message: format!("{}: entries.jsonl is missing", entries_path.display()),
            });
        }
        let aliases = read_aliases(dir)?
            .into_iter()
            .map(|row| (row.alias, row.target))
            .collect();
        Ok(Self {
            dir: dir.to_path_buf(),
            provenance: index.provenance,
            rows: index.rows,
            aliases,
        })
    }
}

impl SnapshotReader for DirSnapshot {
    #[inline]
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    #[inline]
    fn summaries(&self) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(self
            .rows
            .iter()
            .map(|row| ContentSummary {
                id: row.id.clone(),
                kind: row.kind,
                name: row.name.clone(),
            })
            .collect())
    }

    #[inline]
    fn entry(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        let Ok(position) = self.rows.binary_search_by(|row| row.id.cmp(id)) else {
            return Ok(None);
        };
        let entry = read_entry(&self.dir, &self.rows[position])?;
        validate_entry(&entry, &self.provenance)?;
        Ok(Some(entry))
    }

    #[inline]
    fn aliases(&self) -> Result<Vec<(RefId, RefId)>, ContentFault> {
        Ok(self.aliases.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use localtt_core::types::{Ext, Namespace, RefId};
    use localtt_engine::content::{
        ContentData,
        ContentEntry,
        ContentFault,
        ContentKind,
        Provenance,
    };
    use tempfile::TempDir;

    use super::{DirSnapshot, SnapshotReader};
    use crate::reference::{
        import::ImportReport,
        snapshot::{SourceSnapshot, write_source},
    };

    fn provenance() -> Provenance {
        Provenance {
            namespace: Namespace::parse("pack.test").expect("a literal namespace"),
            source: "Test pack".to_owned(),
            source_revision: "test-revision".to_owned(),
            license: "CC0".to_owned(),
            attribution: "Test fixture".to_owned(),
            url: None,
        }
    }

    fn snapshot() -> SourceSnapshot {
        SourceSnapshot {
            provenance: provenance(),
            entries: vec![ContentEntry {
                id: RefId::parse("pack.test:item/alpha").expect("a literal reference"),
                kind: ContentKind::Item,
                name: "Alpha".to_owned(),
                description: "A test entry.".to_owned(),
                data: ContentData::Text,
                provenance: provenance(),
                needs_review: false,
                review_notes: Vec::new(),
                ext: Ext::default(),
            }],
            aliases: Vec::new(),
            report: ImportReport {
                format: "test".to_owned(),
                source_revision: "test-revision".to_owned(),
                counts: BTreeMap::new(),
                review: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn a_namespace_directory_without_entries_is_unavailable() {
        let dir = TempDir::new().expect("a temp dir");
        write_source(dir.path(), &snapshot()).expect("the snapshot writes");
        let namespace_dir = dir.path().join("pack.test");
        std::fs::remove_file(namespace_dir.join("entries.jsonl")).expect("the body is removed");

        match DirSnapshot::open(&namespace_dir) {
            Err(ContentFault::Unavailable { .. }) => {}
            Err(other) => panic!("a missing entries.jsonl must be unavailable, got {other:?}"),
            Ok(_) => panic!("a missing entries.jsonl must not open"),
        }
    }

    #[test]
    fn a_valid_namespace_directory_serves_summaries_and_lazy_bodies() {
        let dir = TempDir::new().expect("a temp dir");
        write_source(dir.path(), &snapshot()).expect("the snapshot writes");
        let namespace_dir = dir.path().join("pack.test");
        let reader = DirSnapshot::open(&namespace_dir).expect("the snapshot opens");

        assert_eq!(reader.provenance().namespace.as_str(), "pack.test");
        assert_eq!(reader.summaries().expect("summaries").len(), 1);

        let id = RefId::parse("pack.test:item/alpha").expect("a literal reference");
        assert!(reader.entry(&id).expect("a lookup").is_some());
        let missing = RefId::parse("pack.test:item/missing").expect("a literal reference");
        assert!(reader.entry(&missing).expect("a lookup").is_none());
        assert!(reader.aliases().expect("aliases").is_empty());
    }
}
