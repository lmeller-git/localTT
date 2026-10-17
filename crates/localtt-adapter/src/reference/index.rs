//! The runtime `ReferenceSource` over N snapshots
//! (`docs/rfcs/content.md` sections 4.2 and 6.2).

use std::{collections::BTreeSet, fs, path::PathBuf};

use localtt_core::types::{Namespace, RefId};
use localtt_engine::content::{
    ContentEntry,
    ContentFault,
    ContentKind,
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
    /// without `index.json` or without `entries.jsonl`, or roots that together
    /// yield no namespace directory is `Unavailable`.
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
        if readers.is_empty() {
            return Err(ContentFault::Unavailable {
                message: "no namespace directories were found under the configured roots"
                    .to_owned(),
            });
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

    /// Shared matching and ordering for `search` and `autocomplete`
    /// (RFC 5.3): filter by kind, match names, optionally scan descriptions,
    /// sort by `(tier, kind priority, namespace, name, id)` and truncate.
    ///
    /// `include_description` adds tier 4: the description contains the query.
    /// An entry already matched by name is never demoted to tier 4.
    fn rank(
        &self,
        query: &SearchQuery<'_>,
        include_description: bool,
    ) -> Result<Vec<ContentSummary>, ContentFault> {
        let text = query
            .text
            .trim_matches(|character: char| character.is_ascii_whitespace());
        if text.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let needle = text.to_lowercase();
        let mut hits = Vec::new();
        for source in &self.sources {
            let namespace = source.provenance.namespace.as_str();
            for summary in &source.summaries {
                let Some(kind_priority) = kind_priority(summary.kind, query.kinds) else {
                    continue;
                };
                let tier = match name_tier(&summary.name, &needle) {
                    Some(tier) => tier,
                    None if include_description => {
                        let Some(entry) = source.reader.entry(&summary.id)? else {
                            return Err(ContentFault::Corrupt {
                                message: format!("{} has no body behind its summary", summary.id),
                            });
                        };
                        if !entry.description.to_lowercase().contains(&needle) {
                            continue;
                        }
                        4
                    }
                    None => continue,
                };
                hits.push(Hit {
                    tier,
                    kind_priority,
                    namespace,
                    name: &summary.name,
                    id: &summary.id,
                    summary,
                });
            }
        }
        hits.sort_by(|left, right| {
            left.tier
                .cmp(&right.tier)
                .then_with(|| left.kind_priority.cmp(&right.kind_priority))
                .then_with(|| left.namespace.cmp(right.namespace))
                .then_with(|| left.name.cmp(right.name))
                .then_with(|| left.id.cmp(right.id))
        });
        hits.truncate(query.limit);
        Ok(hits.into_iter().map(|hit| hit.summary.clone()).collect())
    }
}

/// A matched summary with its sort key.
struct Hit<'a> {
    /// Match quality: 0 exact name through 4 description-only.
    tier: u8,
    /// The caller's kind priority ([`kind_priority`]).
    kind_priority: usize,
    /// The source namespace.
    namespace: &'a str,
    /// The display name.
    name: &'a str,
    /// The reference id.
    id: &'a RefId,
    /// The summary this hit is derived from.
    summary: &'a ContentSummary,
}

/// The caller's kind priority for a candidate, or `None` when the kind is
/// filtered out: first occurrence in `kinds`, or the declaration order of
/// [`ContentKind::ALL`] when unfiltered.
fn kind_priority(kind: ContentKind, kinds: &[ContentKind]) -> Option<usize> {
    if kinds.is_empty() {
        ContentKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
    } else {
        kinds.iter().position(|candidate| *candidate == kind)
    }
}

/// The best name-match tier, 0 (exact) through 3 (substring).
fn name_tier(name: &str, needle: &str) -> Option<u8> {
    let lowered = name.to_lowercase();
    if lowered == needle {
        return Some(0);
    }
    if lowered.starts_with(needle) {
        return Some(1);
    }
    if lowered
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| word.starts_with(needle))
    {
        return Some(2);
    }
    if lowered.contains(needle) {
        Some(3)
    } else {
        None
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
        self.rank(query, true)
    }

    #[inline]
    fn autocomplete(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        self.rank(query, false)
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
    use localtt_core::types::{Ext, Namespace, RefId};
    use localtt_engine::content::{
        ContentData,
        ContentEntry,
        ContentFault,
        ContentKind,
        Provenance,
        ReferenceSource,
        SearchQuery,
    };
    use tempfile::TempDir;

    use super::IndexedSources;
    use crate::reference::{
        fixture::MemorySnapshot,
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
            report: ImportReport::new("test".to_owned(), "test-revision".to_owned()),
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

    #[test]
    fn a_root_without_namespaces_is_unavailable() {
        let dir = TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(dir.path().join("srd2014.bak")).expect("a pruned name");

        match IndexedSources::from_dirs(&[dir.path().to_path_buf()]) {
            Err(ContentFault::Unavailable { .. }) => {}
            Err(other) => panic!("an empty root must be unavailable, got {other:?}"),
            Ok(_) => panic!("an empty root must not load"),
        }
    }

    fn memory_source(entries: Vec<ContentEntry>) -> IndexedSources {
        IndexedSources::from_readers(vec![Box::new(MemorySnapshot::new(
            provenance("fixture"),
            entries,
            Vec::new(),
        ))])
        .expect("the fixture composes")
    }

    fn named(kind: ContentKind, slug: &str, name: &str, description: &str) -> ContentEntry {
        ContentEntry {
            id: ref_id(&format!("fixture:{}/{slug}", kind.as_str())),
            kind,
            name: name.to_owned(),
            description: description.to_owned(),
            data: ContentData::Text,
            provenance: provenance("fixture"),
            needs_review: false,
            review_notes: Vec::new(),
            ext: Ext::default(),
        }
    }

    fn autocomplete_ids(source: &IndexedSources, text: &str, kinds: &[ContentKind]) -> Vec<String> {
        source
            .autocomplete(&SearchQuery {
                text,
                kinds,
                limit: 100,
            })
            .expect("the source answers")
            .into_iter()
            .map(|hit| hit.id.as_str().to_owned())
            .collect()
    }

    fn search_ids(
        source: &IndexedSources,
        text: &str,
        kinds: &[ContentKind],
        limit: usize,
    ) -> Vec<String> {
        source
            .search(&SearchQuery { text, kinds, limit })
            .expect("the source answers")
            .into_iter()
            .map(|hit| hit.id.as_str().to_owned())
            .collect()
    }

    fn fire_catalog() -> IndexedSources {
        memory_source(vec![
            named(
                ContentKind::Spell,
                "fire-bolt",
                "Fire Bolt",
                "A mote of fire streaks toward a creature.",
            ),
            named(
                ContentKind::Item,
                "wand-of-fire",
                "Wand of Fire",
                "A gnarled wand.",
            ),
            named(ContentKind::Feat, "bonfire", "Bonfire", "A bonfire burns."),
            named(
                ContentKind::Background,
                "acolyte",
                "Acolyte",
                "You tend a temple's sacred fire.",
            ),
        ])
    }

    #[test]
    fn name_tiers_rank_exact_prefix_word_and_substring_before_descriptions() {
        let source = fire_catalog();

        assert_eq!(
            search_ids(&source, "fire", &[], 100),
            [
                "fixture:spell/fire-bolt",
                "fixture:item/wand-of-fire",
                "fixture:feat/bonfire",
                "fixture:background/acolyte",
            ]
        );
        assert_eq!(
            autocomplete_ids(&source, "fire", &[]),
            [
                "fixture:spell/fire-bolt",
                "fixture:item/wand-of-fire",
                "fixture:feat/bonfire",
            ],
            "autocomplete ignores descriptions"
        );
        assert_eq!(
            autocomplete_ids(&source, "bonfire", &[]),
            ["fixture:feat/bonfire"]
        );
    }

    #[test]
    fn kind_filter_order_decides_exact_name_ties() {
        let source = memory_source(vec![
            named(ContentKind::Spell, "shield", "Shield", "A barrier."),
            named(ContentKind::Item, "shield", "Shield", "A board."),
        ]);

        assert_eq!(
            autocomplete_ids(&source, "shield", &[ContentKind::Item, ContentKind::Spell]),
            ["fixture:item/shield", "fixture:spell/shield"]
        );
        assert_eq!(
            autocomplete_ids(&source, "shield", &[ContentKind::Spell, ContentKind::Item]),
            ["fixture:spell/shield", "fixture:item/shield"]
        );
        assert_eq!(
            autocomplete_ids(&source, "shield", &[]),
            ["fixture:spell/shield", "fixture:item/shield"],
            "unfiltered priority follows ContentKind::ALL order"
        );
    }

    #[test]
    fn empty_queries_and_zero_limits_yield_no_hits() {
        let source = fire_catalog();

        assert!(autocomplete_ids(&source, "", &[]).is_empty());
        assert!(autocomplete_ids(&source, "   ", &[]).is_empty());
        assert!(search_ids(&source, "fire", &[], 0).is_empty());
    }

    #[test]
    fn limit_truncates_after_ranking() {
        let source = fire_catalog();

        assert_eq!(
            search_ids(&source, "fire", &[], 2),
            ["fixture:spell/fire-bolt", "fixture:item/wand-of-fire"]
        );
    }

    #[test]
    fn reruns_are_stable() {
        let source = fire_catalog();

        assert_eq!(
            search_ids(&source, "fire", &[], 100),
            search_ids(&source, "fire", &[], 100)
        );
    }

    #[test]
    fn unicode_names_match_case_insensitively() {
        let source = memory_source(vec![named(
            ContentKind::Feat,
            "elan",
            "Élan",
            "A surge of vitality.",
        )]);

        assert_eq!(
            autocomplete_ids(&source, "ÉLAN", &[]),
            ["fixture:feat/elan"]
        );
        assert_eq!(autocomplete_ids(&source, "él", &[]), ["fixture:feat/elan"]);
    }
}
