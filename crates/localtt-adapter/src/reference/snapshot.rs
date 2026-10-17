//! The on-disk snapshot format and its writer
//! (`docs/rfcs/content.md` sections 4.2, 5.1 and 5.6).
//!
//! One namespace directory holds `index.json` (version, provenance, name and
//! offset rows), `entries.jsonl` (one entry per line, sorted by `RefId`),
//! optional `aliases.json` and `report.json`. `index.json` is written last,
//! temp + rename: its presence marks the namespace complete.

use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufRead, BufReader, Seek as _, SeekFrom},
    path::{Path, PathBuf},
};

use localtt_core::types::{Ext, RefId};
use localtt_engine::content::{ContentData, ContentEntry, ContentFault, ContentKind, Provenance};
use serde::{Deserialize, Serialize};

use crate::reference::{aliases::AliasTable, import::ImportReport};

/// The current on-disk snapshot version; a newer file is refused at load.
pub const CURRENT_SNAPSHOT_VERSION: u32 = 1;

/// Everything [`write_source`] needs for one namespace.
#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    /// Namespace, licensing and pinned revision of the source.
    pub provenance: Provenance,
    /// The catalog; `write_source` sorts by `RefId` before writing.
    pub entries: Vec<ContentEntry>,
    /// `(alias, target)` rows; validated on write and on load.
    pub aliases: Vec<(RefId, RefId)>,
    /// Fidelity report written next to the snapshot.
    pub report: ImportReport,
}

/// Writes one namespace snapshot under `root/<namespace>/`: `entries.jsonl`,
/// `aliases.json` (when any, stale file removed otherwise), `report.json`,
/// then `index.json` last as the completeness marker; each file via temp +
/// rename.
///
/// Validates before touching the first file, with the same rules the loader
/// applies: id/kind and namespace/provenance agreement, no duplicate ids, a
/// valid alias table (namespace-scoped), and
/// `report.source_revision == provenance.source_revision`; a violation is
/// `Corrupt` and nothing is written.
///
/// The write is deterministic: the same snapshot produces byte-identical
/// files. It is not atomic as a whole; a crash can leave the namespace without
/// index (or torn), which reads report as `Unavailable`/`Corrupt`. Re-running
/// the tool repairs it, but only while no server holds the namespace: sources
/// are immutable per process (restart to swap).
#[inline]
pub fn write_source(root: &Path, snapshot: &SourceSnapshot) -> Result<(), ContentFault> {
    // Validate the whole snapshot before touching the first file (N6): nothing is
    // written unless every check passes.
    if snapshot.report.source_revision != snapshot.provenance.source_revision {
        return Err(corrupt(format!(
            "report revision {} disagrees with provenance revision {}",
            snapshot.report.source_revision, snapshot.provenance.source_revision
        )));
    }
    let mut entries: Vec<&ContentEntry> = snapshot.entries.iter().collect();
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    for entry in &entries {
        validate_entry(entry, &snapshot.provenance)?;
    }
    for pair in entries.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(corrupt(format!("duplicate entry id {}", pair[0].id)));
        }
    }
    let live: BTreeSet<RefId> = entries.iter().map(|entry| entry.id.clone()).collect();
    let _table = AliasTable::new(
        snapshot.aliases.clone(),
        &live,
        &snapshot.provenance.namespace,
    )?;

    let namespace_dir = root.join(snapshot.provenance.namespace.as_str());
    fs::create_dir_all(&namespace_dir).map_err(|error| io_fault(&namespace_dir, &error))?;

    let mut body = Vec::new();
    let mut rows = Vec::with_capacity(entries.len());
    for entry in &entries {
        let offset = u64::try_from(body.len())
            .map_err(|_| corrupt("entries.jsonl exceeds the u64 offset range".to_owned()))?;
        serde_json::to_writer(&mut body, &EntryRecord::from(*entry))
            .map_err(|error| json_fault(&namespace_dir, &error))?;
        body.push(b'\n');
        rows.push(IndexRow {
            id: entry.id.clone(),
            kind: entry.kind,
            name: entry.name.clone(),
            offset,
        });
    }
    write_atomically(&namespace_dir.join("entries.jsonl"), &body)?;

    let mut aliases = snapshot.aliases.clone();
    aliases.sort();
    let aliases_path = namespace_dir.join("aliases.json");
    if aliases.is_empty() {
        remove_if_present(&aliases_path)?;
    } else {
        let file = AliasFile {
            rows: aliases
                .iter()
                .map(|(alias, target)| AliasRow {
                    alias: alias.clone(),
                    target: target.clone(),
                })
                .collect(),
        };
        write_json(&aliases_path, &file)?;
    }

    write_json(&namespace_dir.join("report.json"), &snapshot.report)?;

    let index = IndexFile {
        snapshot_version: CURRENT_SNAPSHOT_VERSION,
        provenance: snapshot.provenance.clone(),
        rows,
    };
    write_json(&namespace_dir.join("index.json"), &index)?;
    Ok(())
}

/// A `Corrupt` fault from a diagnostic message.
fn corrupt(message: String) -> ContentFault {
    ContentFault::Corrupt { message }
}

/// An IO error naming the path it happened on.
fn io_fault(path: &Path, error: &io::Error) -> ContentFault {
    ContentFault::Unavailable {
        message: format!("{}: {error}", path.display()),
    }
}

/// A JSON error naming the file it happened on.
fn json_fault(path: &Path, error: &serde_json::Error) -> ContentFault {
    corrupt(format!("{}: {error}", path.display()))
}

/// Writes `bytes` to `path` via a sibling temp file and a rename, so a reader
/// never observes a torn file.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), ContentFault> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, bytes).map_err(|error| io_fault(&temp, &error))?;
    fs::rename(&temp, path).map_err(|error| io_fault(path, &error))?;
    Ok(())
}

/// Serializes `value` and writes it via [`write_atomically`].
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ContentFault> {
    let bytes = serde_json::to_vec(value).map_err(|error| json_fault(path, &error))?;
    write_atomically(path, &bytes)
}

/// Removes `path` when it exists; absence is success (stale alias files).
fn remove_if_present(path: &Path) -> Result<(), ContentFault> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_fault(path, &error)),
    }
}

/// One `entries.jsonl` line (crate-private wire type).
///
/// Mirrors [`ContentEntry`], which is deliberately serde-free (section 4.1):
/// this struct is the only entry parse path, and its field names are the
/// on-disk contract for `CURRENT_SNAPSHOT_VERSION`. The engine payload types
/// are the wire contract, so field additions must be `Option` +
/// `#[serde(default)]` and anything else bumps the version (CONTENT-13).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EntryRecord {
    /// The reference id.
    pub(crate) id: RefId,
    /// The content kind; must match the id's kind segment.
    pub(crate) kind: ContentKind,
    /// Display name.
    pub(crate) name: String,
    /// Markdown description.
    pub(crate) description: String,
    /// Structured payload, typed per kind.
    pub(crate) data: ContentData,
    /// Source and licensing of this entry.
    pub(crate) provenance: Provenance,
    /// Conversion fidelity: true when the importer had to guess.
    pub(crate) needs_review: bool,
    /// Why the entry needs review, in human-readable form.
    pub(crate) review_notes: Vec<String>,
    /// Unmapped source fields, under `ext["src.<format>"]`.
    pub(crate) ext: Ext,
}

impl From<&ContentEntry> for EntryRecord {
    #[inline]
    fn from(entry: &ContentEntry) -> Self {
        Self {
            id: entry.id.clone(),
            kind: entry.kind,
            name: entry.name.clone(),
            description: entry.description.clone(),
            data: entry.data.clone(),
            provenance: entry.provenance.clone(),
            needs_review: entry.needs_review,
            review_notes: entry.review_notes.clone(),
            ext: entry.ext.clone(),
        }
    }
}

impl From<EntryRecord> for ContentEntry {
    #[inline]
    fn from(record: EntryRecord) -> Self {
        Self {
            id: record.id,
            kind: record.kind,
            name: record.name,
            description: record.description,
            data: record.data,
            provenance: record.provenance,
            needs_review: record.needs_review,
            review_notes: record.review_notes,
            ext: record.ext,
        }
    }
}

/// One `index.json` (crate-private wire type).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IndexFile {
    /// The on-disk version, gated against [`CURRENT_SNAPSHOT_VERSION`].
    pub(crate) snapshot_version: u32,
    /// Namespace, licensing and pinned revision.
    pub(crate) provenance: Provenance,
    /// Name and offset rows, ascending by id and offset.
    pub(crate) rows: Vec<IndexRow>,
}

/// One name/offset row of `index.json`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IndexRow {
    /// The entry id.
    pub(crate) id: RefId,
    /// The entry kind; must match the id's kind segment.
    pub(crate) kind: ContentKind,
    /// Display name, eager.
    pub(crate) name: String,
    /// Byte offset of the entry's line in `entries.jsonl`.
    pub(crate) offset: u64,
}

/// One `aliases.json` (crate-private wire type).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AliasFile {
    /// The alias rows.
    pub(crate) rows: Vec<AliasRow>,
}

/// One `(alias, target)` row of `aliases.json`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AliasRow {
    /// The old id.
    pub(crate) alias: RefId,
    /// The live id it now resolves to.
    pub(crate) target: RefId,
}

/// Reads and gates `<dir>/index.json`: missing -> `Unavailable`, malformed ->
/// `Corrupt`, unknown version -> `SnapshotVersionRefused`, rows validated.
pub(crate) fn read_index(dir: &Path) -> Result<IndexFile, ContentFault> {
    let path = dir.join("index.json");
    let bytes = fs::read(&path).map_err(|error| io_fault(&path, &error))?;
    let file: IndexFile =
        serde_json::from_slice(&bytes).map_err(|error| json_fault(&path, &error))?;
    if file.snapshot_version != CURRENT_SNAPSHOT_VERSION {
        return Err(ContentFault::SnapshotVersionRefused {
            found: file.snapshot_version,
            supported: CURRENT_SNAPSHOT_VERSION,
        });
    }
    for row in &file.rows {
        if row.id.kind() != row.kind.as_str() {
            return Err(corrupt(format!(
                "index row {} disagrees with its kind {}",
                row.id,
                row.kind.as_str()
            )));
        }
        if row.id.namespace() != file.provenance.namespace.as_str() {
            return Err(corrupt(format!(
                "index row {} is outside namespace {}",
                row.id, file.provenance.namespace
            )));
        }
    }
    for pair in file.rows.windows(2) {
        if pair[0].id >= pair[1].id {
            return Err(corrupt(format!(
                "index rows {} and {} are not strictly ascending",
                pair[0].id, pair[1].id
            )));
        }
        if pair[0].offset >= pair[1].offset {
            return Err(corrupt(format!(
                "index offsets for {} and {} are not strictly ascending",
                pair[0].id, pair[1].id
            )));
        }
    }
    Ok(file)
}

/// Reads one entry body by its index row (lazy): seeks `entries.jsonl` to the
/// row's offset, parses the line, and validates it against the row and the
/// snapshot provenance.
pub(crate) fn read_entry(
    dir: &Path,
    row: &IndexRow,
    provenance: &Provenance,
) -> Result<ContentEntry, ContentFault> {
    let path = dir.join("entries.jsonl");
    let mut file = fs::File::open(&path).map_err(|error| io_fault(&path, &error))?;
    file.seek(SeekFrom::Start(row.offset))
        .map_err(|error| io_fault(&path, &error))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    reader
        .read_until(b'\n', &mut line)
        .map_err(|error| io_fault(&path, &error))?;
    if line.is_empty() {
        return Err(corrupt(format!(
            "{}: no entry line at offset {}",
            path.display(),
            row.offset
        )));
    }
    let record: EntryRecord =
        serde_json::from_slice(&line).map_err(|error| json_fault(&path, &error))?;
    if record.id != row.id || record.kind != row.kind || record.name != row.name {
        return Err(corrupt(format!(
            "entry at offset {} disagrees with its index row {}",
            row.offset, row.id
        )));
    }
    let entry: ContentEntry = record.into();
    validate_entry(&entry, provenance)?;
    Ok(entry)
}

/// Reads `<dir>/aliases.json`: absent -> empty, malformed -> `Corrupt`.
pub(crate) fn read_aliases(dir: &Path) -> Result<Vec<AliasRow>, ContentFault> {
    let path = dir.join("aliases.json");
    match fs::read(&path) {
        Ok(bytes) => {
            let file: AliasFile =
                serde_json::from_slice(&bytes).map_err(|error| json_fault(&path, &error))?;
            Ok(file.rows)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(io_fault(&path, &error)),
    }
}

/// Checks an entry against its snapshot provenance: the id's kind segment and
/// namespace match, and the entry provenance equals the snapshot's.
pub(crate) fn validate_entry(
    entry: &ContentEntry,
    provenance: &Provenance,
) -> Result<(), ContentFault> {
    if entry.id.kind() != entry.kind.as_str() {
        return Err(corrupt(format!(
            "entry {} disagrees with its kind {}",
            entry.id,
            entry.kind.as_str()
        )));
    }
    if entry.id.namespace() != provenance.namespace.as_str() {
        return Err(corrupt(format!(
            "entry {} is outside namespace {}",
            entry.id, provenance.namespace
        )));
    }
    if entry.provenance != *provenance {
        return Err(corrupt(format!(
            "entry {} provenance disagrees with its snapshot",
            entry.id
        )));
    }
    Ok(())
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
    };
    use tempfile::TempDir;

    use super::{
        CURRENT_SNAPSHOT_VERSION,
        SourceSnapshot,
        read_aliases,
        read_entry,
        read_index,
        write_source,
    };
    use crate::reference::import::ImportReport;

    fn ref_id(raw: &str) -> RefId {
        RefId::parse(raw).expect("a literal reference")
    }

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

    fn entry(id: &str, name: &str) -> ContentEntry {
        ContentEntry {
            id: ref_id(id),
            kind: ContentKind::Item,
            name: name.to_owned(),
            description: "A test entry.".to_owned(),
            data: ContentData::Text,
            provenance: provenance(),
            needs_review: false,
            review_notes: Vec::new(),
            ext: Ext::default(),
        }
    }

    fn snapshot(entries: Vec<ContentEntry>, aliases: &[(&str, &str)]) -> SourceSnapshot {
        SourceSnapshot {
            provenance: provenance(),
            entries,
            aliases: aliases
                .iter()
                .map(|(alias, target)| (ref_id(alias), ref_id(target)))
                .collect(),
            report: ImportReport::new("test".to_owned(), "test-revision".to_owned()),
        }
    }

    #[test]
    fn write_then_read_round_trips_and_is_deterministic() {
        let dir = TempDir::new().expect("a temp dir");
        let snapshot = snapshot(
            vec![
                entry("pack.test:item/zeta", "Zeta"),
                entry("pack.test:item/alpha", "Alpha"),
            ],
            &[("pack.test:item/alpha-old", "pack.test:item/alpha")],
        );
        write_source(dir.path(), &snapshot).expect("the snapshot writes");
        let namespace_dir = dir.path().join("pack.test");

        let index = read_index(&namespace_dir).expect("the index reads");
        assert_eq!(index.snapshot_version, CURRENT_SNAPSHOT_VERSION);
        let ids: Vec<&str> = index.rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["pack.test:item/alpha", "pack.test:item/zeta"]);

        let first = read_entry(&namespace_dir, &index.rows[0], &provenance())
            .expect("the first entry reads");
        assert_eq!(
            first, snapshot.entries[1],
            "entries round-trip in sorted order"
        );

        let aliases = read_aliases(&namespace_dir).expect("the aliases read");
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].alias.as_str(), "pack.test:item/alpha-old");
        assert_eq!(aliases[0].target.as_str(), "pack.test:item/alpha");

        let body = std::fs::read(namespace_dir.join("entries.jsonl")).expect("the body reads");
        write_source(dir.path(), &snapshot).expect("the snapshot rewrites");
        assert_eq!(
            std::fs::read(namespace_dir.join("entries.jsonl")).expect("the body reads"),
            body,
            "a rewrite is byte-identical"
        );
    }

    #[test]
    fn a_revision_disagreement_refuses_to_write() {
        let dir = TempDir::new().expect("a temp dir");
        let mut snapshot = snapshot(vec![entry("pack.test:item/alpha", "Alpha")], &[]);
        snapshot.report.source_revision = "other-revision".to_owned();

        assert!(matches!(
            write_source(dir.path(), &snapshot),
            Err(ContentFault::Corrupt { .. })
        ));
        assert!(!dir.path().join("pack.test").exists(), "nothing is written");
    }

    #[test]
    fn a_foreign_entry_provenance_refuses_to_write() {
        let dir = TempDir::new().expect("a temp dir");
        let mut snapshot = snapshot(vec![entry("pack.test:item/alpha", "Alpha")], &[]);
        snapshot.entries[0].provenance.source = "Elsewhere".to_owned();

        assert!(matches!(
            write_source(dir.path(), &snapshot),
            Err(ContentFault::Corrupt { .. })
        ));
        assert!(!dir.path().join("pack.test").exists(), "nothing is written");
    }

    #[test]
    fn duplicate_ids_refuse_to_write() {
        let dir = TempDir::new().expect("a temp dir");
        let snapshot = snapshot(
            vec![
                entry("pack.test:item/alpha", "Alpha"),
                entry("pack.test:item/alpha", "Alpha Again"),
            ],
            &[],
        );

        assert!(matches!(
            write_source(dir.path(), &snapshot),
            Err(ContentFault::Corrupt { .. })
        ));
        assert!(!dir.path().join("pack.test").exists(), "nothing is written");
    }

    #[test]
    fn any_snapshot_version_mismatch_is_refused() {
        for version in [0, CURRENT_SNAPSHOT_VERSION + 1] {
            let dir = TempDir::new().expect("a temp dir");
            let namespace_dir = dir.path().join("pack.test");
            std::fs::create_dir_all(&namespace_dir).expect("the namespace dir");
            let index = format!(
                r#"{{"snapshot_version": {version}, "provenance": {}, "rows": []}}"#,
                serde_json::to_string(&provenance()).expect("the provenance serializes")
            );
            std::fs::write(namespace_dir.join("index.json"), index).expect("the index writes");

            match read_index(&namespace_dir) {
                Err(ContentFault::SnapshotVersionRefused { found, supported }) => {
                    assert_eq!(found, version);
                    assert_eq!(supported, CURRENT_SNAPSHOT_VERSION);
                }
                other => panic!("version {version} must be refused, got {other:?}"),
            }
        }
    }
}
