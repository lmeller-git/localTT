//! One-time converters into the snapshot format
//! (`docs/rfcs/content.md` section 4.3).
//!
//! Ingestion is separate from serving: a converter reads some external format,
//! emits a [`SourceSnapshot`], and never runs at play time. Hand-authored packs
//! (U8) normalize into the same model through the pack loader instead.

use std::{collections::BTreeMap, path::Path};

use localtt_core::types::RefId;
use localtt_engine::content::ContentKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::reference::snapshot::SourceSnapshot;

pub mod open5e;

pub use open5e::Open5eImporter;

/// One-time conversion from an external source format to a [`SourceSnapshot`].
pub trait Importer {
    /// The format identifier recorded in the report, e.g. `"open5e"`.
    fn format(&self) -> &'static str;

    /// Reads the source tree and produces a snapshot; never touches the
    /// network.
    fn import(&self, input: &Path) -> Result<SourceSnapshot, ImportError>;
}

/// What an import produced.
///
/// Written as `report.json` next to the snapshot, so fidelity is auditable
/// (CONTENT-7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    /// The importer's [`Importer::format`].
    pub format: String,
    /// The pinned source revision, recorded verbatim.
    pub source_revision: String,
    /// Entries per kind.
    pub counts: BTreeMap<ContentKind, usize>,
    /// Review notes per entry id.
    pub review: BTreeMap<RefId, Vec<String>>,
}

/// Import failure.
///
/// `file` is dataset-relative; messages are diagnostic, not user-facing.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The input tree or a required file is missing or unreadable.
    #[error("the source tree is unavailable: {message}")]
    Unavailable {
        /// Diagnostic detail (path, IO error).
        message: String,
    },
    /// A source file did not parse, or a record cannot be mapped.
    #[error("{file} is corrupt: {message}")]
    Corrupt {
        /// Dataset-relative file name.
        file: String,
        /// Diagnostic detail (parse error, unmappable record).
        message: String,
    },
    /// Two source records produce the same `RefId`; the converter resolves
    /// nothing silently.
    #[error("two records collide on {id}: {first} and {second}")]
    Collision {
        /// The colliding reference id.
        id: RefId,
        /// The first record, as `<file>: <name>`.
        first: String,
        /// The second record, as `<file>: <name>`.
        second: String,
    },
}
