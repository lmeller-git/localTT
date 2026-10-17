//! The on-disk snapshot format and its writer
//! (`docs/rfcs/content.md` sections 4.2, 5.1 and 5.6).
//!
//! One namespace directory holds `index.json` (version, provenance, name and
//! offset rows), `entries.jsonl` (one entry per line, sorted by `RefId`),
//! optional `aliases.json` and `report.json`. `index.json` is written last,
//! temp + rename: its presence marks the namespace complete.

use std::path::Path;

use localtt_core::types::{Ext, RefId};
use localtt_engine::content::{ContentData, ContentEntry, ContentFault, ContentKind, Provenance};
use serde::{Deserialize, Serialize};

use crate::reference::import::ImportReport;

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
    todo!()
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
    todo!()
}

/// Reads one entry body by its index row (lazy): seeks `entries.jsonl` to the
/// row's offset, parses the line, and validates it against the row and the
/// snapshot provenance.
pub(crate) fn read_entry(dir: &Path, row: &IndexRow) -> Result<ContentEntry, ContentFault> {
    todo!()
}

/// Reads `<dir>/aliases.json`: absent -> empty, malformed -> `Corrupt`.
pub(crate) fn read_aliases(dir: &Path) -> Result<Vec<AliasRow>, ContentFault> {
    todo!()
}

/// Checks an entry against its snapshot provenance: the id's kind segment and
/// namespace match, and the entry provenance equals the snapshot's.
pub(crate) fn validate_entry(
    entry: &ContentEntry,
    provenance: &Provenance,
) -> Result<(), ContentFault> {
    todo!()
}
