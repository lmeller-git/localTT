//! Reference-content DTOs (`docs/rfcs/server.md` 4.3).
//!
//! `ContentSummary`, `ContentEntry` and `Provenance` are engine types;
//! `ContentSummary` and `ContentEntry` are deliberately not `Serialize`
//! (content.rs). The server maps them to these transport shapes without
//! changing the engine.

use localtt_core::types::{Ext, Namespace, RefId};
use localtt_engine::content::{ContentData, ContentEntry, ContentKind, ContentSummary, Provenance};
use serde::{Deserialize, Serialize};

/// One reference hit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSummaryDto {
    /// The reference id.
    pub id: RefId,
    /// The content kind.
    pub kind: ContentKind,
    /// Display name.
    pub name: String,
}

impl From<ContentSummary> for ContentSummaryDto {
    #[inline]
    fn from(summary: ContentSummary) -> Self {
        Self {
            id: summary.id,
            kind: summary.kind,
            name: summary.name,
        }
    }
}

/// Source and licensing information for one loaded reference source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfoDto {
    /// Source namespace, e.g. `srd2014`.
    pub namespace: Namespace,
    /// Human-readable source document.
    pub source: String,
    /// Pinned source revision.
    pub source_revision: String,
    /// License identifier.
    pub license: String,
    /// Attribution text to surface in the UI.
    pub attribution: String,
    /// Canonical source URL, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl From<Provenance> for SourceInfoDto {
    #[inline]
    fn from(provenance: Provenance) -> Self {
        Self {
            namespace: provenance.namespace,
            source: provenance.source,
            source_revision: provenance.source_revision,
            license: provenance.license,
            attribution: provenance.attribution,
            url: provenance.url,
        }
    }
}

/// One full reference entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentEntryDto {
    /// The reference id.
    pub id: RefId,
    /// The content kind.
    pub kind: ContentKind,
    /// Display name.
    pub name: String,
    /// Markdown; frontends sanitize before rendering.
    pub description: String,
    /// Structured payload, typed per kind.
    pub data: ContentData,
    /// Source and licensing.
    pub provenance: SourceInfoDto,
    /// Conversion fidelity: true when the importer had to guess.
    pub needs_review: bool,
    /// Why the entry needs review.
    pub review_notes: Vec<String>,
    /// Unmapped source fields, verbatim.
    pub ext: Ext,
}

impl From<ContentEntry> for ContentEntryDto {
    #[inline]
    fn from(entry: ContentEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind,
            name: entry.name,
            description: entry.description,
            data: entry.data,
            provenance: entry.provenance.into(),
            needs_review: entry.needs_review,
            review_notes: entry.review_notes,
            ext: entry.ext,
        }
    }
}
