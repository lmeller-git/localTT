//! The runtime `ReferenceSource` over N snapshots
//! (`docs/rfcs/content.md` sections 4.2 and 6.2).

use std::{collections::BTreeSet, fs, path::PathBuf};

use localtt_core::types::{Namespace, RefId};
use localtt_engine::content::{
    ContentEntry,
    ContentFault,
    ContentSummary,
    Provenance,
    ReferenceSource,
    SearchQuery,
};

use crate::reference::{
    aliases::AliasTable,
    source::{DirSnapshot, SnapshotReader},
};

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
        if roots.is_empty() {
            return Err(ContentFault::Unavailable {
                message: "no content roots are configured".to_owned(),
            });
        }
        let mut readers: Vec<Box<dyn SnapshotReader>> = Vec::new();
        for root in roots {
            if !root.is_dir() {
                return Err(ContentFault::Unavailable {
                    message: format!("{}: not a directory", root.display()),
                });
            }
            let dir = fs::read_dir(root).map_err(|error| ContentFault::Unavailable {
                message: format!("{}: {error}", root.display()),
            })?;
            let mut names = Vec::new();
            for item in dir {
                let item = item.map_err(|error| ContentFault::Unavailable {
                    message: format!("{}: {error}", root.display()),
                })?;
                names.push(item.file_name());
            }
            names.sort();
            for name in names {
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !is_load_candidate(name) {
                    continue;
                }
                let namespace_dir = root.join(name);
                if !namespace_dir.is_dir() {
                    continue;
                }
                let snapshot = DirSnapshot::open(&namespace_dir)?;
                if snapshot.provenance().namespace.as_str() != name {
                    return Err(ContentFault::Corrupt {
                        message: format!(
                            "{}: directory name disagrees with provenance namespace {}",
                            namespace_dir.display(),
                            snapshot.provenance().namespace
                        ),
                    });
                }
                readers.push(Box::new(snapshot));
            }
        }
        Self::from_readers(readers)
    }

    /// Composes already-open readers, running the same composition checks as
    /// [`IndexedSources::from_dirs`] without a directory layout.
    #[inline]
    pub fn from_readers(readers: Vec<Box<dyn SnapshotReader>>) -> Result<Self, ContentFault> {
        if readers.is_empty() {
            return Err(ContentFault::Unavailable {
                message: "no content readers are configured".to_owned(),
            });
        }
        let mut sources = Vec::with_capacity(readers.len());
        let mut namespaces = BTreeSet::new();
        for reader in readers {
            let provenance = reader.provenance().clone();
            if !namespaces.insert(provenance.namespace.clone()) {
                return Err(ContentFault::NamespaceConflict {
                    namespace: provenance.namespace,
                });
            }
            let summaries = reader.summaries()?;
            let live: BTreeSet<RefId> =
                summaries.iter().map(|summary| summary.id.clone()).collect();
            let aliases = AliasTable::new(reader.aliases()?, &live, &provenance.namespace)?;
            sources.push(LoadedSource {
                reader,
                provenance,
                summaries,
                aliases,
            });
        }
        Ok(Self { sources })
    }
}

/// True when a directory name is a load candidate: a valid namespace and not a
/// backup copy (`*.bak`, `*.tmp`, `*.old`).
fn is_load_candidate(name: &str) -> bool {
    if name.ends_with(".bak") || name.ends_with(".tmp") || name.ends_with(".old") {
        return false;
    }
    Namespace::parse(name).is_ok()
}

impl ReferenceSource for IndexedSources {
    #[inline]
    fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        let Some(source) = self
            .sources
            .iter()
            .find(|source| source.provenance.namespace.as_str() == id.namespace())
        else {
            return Ok(None);
        };
        if let Some(entry) = source.reader.entry(id)? {
            return Ok(Some(entry));
        }
        match source.aliases.resolve(id) {
            Some(target) => source.reader.entry(target),
            None => Ok(None),
        }
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
        self.sources
            .iter()
            .map(|source| source.provenance.clone())
            .collect()
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
        ReferenceSource,
    };
    use tempfile::TempDir;

    use super::IndexedSources;
    use crate::reference::{
        import::ImportReport,
        snapshot::{SourceSnapshot, write_source},
    };

    fn ref_id(raw: &str) -> RefId {
        RefId::parse(raw).expect("a literal reference")
    }

    fn provenance(namespace: &str) -> Provenance {
        Provenance {
            namespace: Namespace::parse(namespace).expect("a literal namespace"),
            source: "Test pack".to_owned(),
            source_revision: "test-revision".to_owned(),
            license: "CC0".to_owned(),
            attribution: "Test fixture".to_owned(),
            url: None,
        }
    }

    fn snapshot(namespace: &str) -> SourceSnapshot {
        SourceSnapshot {
            provenance: provenance(namespace),
            entries: vec![ContentEntry {
                id: ref_id(&format!("{namespace}:item/alpha")),
                kind: ContentKind::Item,
                name: "Alpha".to_owned(),
                description: "A test entry.".to_owned(),
                data: ContentData::Text,
                provenance: provenance(namespace),
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
    fn backup_copies_and_non_namespace_names_are_pruned() {
        let dir = TempDir::new().expect("a temp dir");
        write_source(dir.path(), &snapshot("pack.keep")).expect("the kept snapshot writes");
        write_source(dir.path(), &snapshot("pack.drop")).expect("the dropped snapshot writes");
        std::fs::rename(
            dir.path().join("pack.drop"),
            dir.path().join("pack.drop.bak"),
        )
        .expect("the backup rename");
        std::fs::create_dir_all(dir.path().join("NotANamespace")).expect("a foreign dir");
        std::fs::write(dir.path().join("plainfile"), b"not a directory").expect("a foreign file");

        let source =
            IndexedSources::from_dirs(&[dir.path().to_path_buf()]).expect("the root loads");
        let sources = source.sources();
        let namespaces: Vec<&str> = sources.iter().map(|item| item.namespace.as_str()).collect();

        assert_eq!(namespaces, ["pack.keep"]);
        assert!(
            source
                .get(&ref_id("pack.drop:item/alpha"))
                .expect("a lookup")
                .is_none(),
            "the pruned namespace is not served"
        );
    }

    #[test]
    fn a_directory_name_disagreeing_with_provenance_is_corrupt() {
        let dir = TempDir::new().expect("a temp dir");
        write_source(dir.path(), &snapshot("pack.test")).expect("the snapshot writes");
        std::fs::rename(dir.path().join("pack.test"), dir.path().join("pack.other"))
            .expect("the rename");

        match IndexedSources::from_dirs(&[dir.path().to_path_buf()]) {
            Err(ContentFault::Corrupt { .. }) => {}
            Err(other) => panic!("a name mismatch must be corrupt, got {other:?}"),
            Ok(_) => panic!("a name mismatch must not load"),
        }
    }

    #[test]
    fn no_roots_is_unavailable() {
        match IndexedSources::from_dirs(&[]) {
            Err(ContentFault::Unavailable { .. }) => {}
            Err(other) => panic!("zero roots must be unavailable, got {other:?}"),
            Ok(_) => panic!("zero roots must not load"),
        }
    }
}
