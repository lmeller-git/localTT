//! Reference-content DTOs (`docs/architecture.md` section 6).
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

/// The spell fields a client needs without coupling to the engine payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellDataDto {
    /// Spell level; 0 for cantrips.
    pub level: u8,
    /// Whether the spell requires concentration.
    pub concentration: bool,
    /// School of magic, e.g. `"Evocation"`.
    pub school: String,
    /// Whether the spell can be cast as a ritual.
    pub ritual: bool,
}

impl ContentEntryDto {
    /// The structured spell payload, when this entry is a spell.
    ///
    /// Clients use this to prefill a spell form and to show concentration and
    /// ritual markers without importing engine types.
    #[must_use]
    #[inline]
    pub fn spell(&self) -> Option<SpellDataDto> {
        if self.kind != ContentKind::Spell {
            return None;
        }
        match &self.data {
            ContentData::Spell(data) => Some(SpellDataDto {
                level: data.level,
                concentration: data.concentration,
                school: data.school.clone(),
                ritual: data.ritual,
            }),
            _ => None,
        }
    }
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
