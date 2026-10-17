//! The content model and the read port over reference data
//! (`docs/rfcs/content.md` section 4.1).
//!
//! The engine owns the model; adapters produce it from snapshots (Open5e today,
//! packs and future importers later) and serve it behind [`ReferenceSource`].
//! Entries are never stored inside campaign documents: a campaign keeps a
//! `RefId` plus a name snapshot (RFC-0001 DATA-7).

use localtt_core::types::{
    Abilities,
    ChallengeRating,
    Ext,
    FeatureCategory,
    InlineItem,
    Namespace,
    RefId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Closed set of reference kinds (the `<kind>` segment of a `RefId`).
///
/// Declaration order is the fixed priority order used when a caller ranks
/// unfiltered results; [`ContentKind::ALL`] exposes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    /// A spell.
    Spell,
    /// An item, magic or mundane.
    Item,
    /// A creature stat block.
    Monster,
    /// A condition.
    Condition,
    /// A class.
    Class,
    /// A subclass.
    Subclass,
    /// A race (including subraces).
    Race,
    /// A background.
    Background,
    /// A feat.
    Feat,
    /// A class, race, feat or creature feature.
    Feature,
    /// A rules section.
    Rule,
}

impl ContentKind {
    /// Every kind, in the fixed priority order used for unfiltered ranking.
    pub const ALL: [ContentKind; 11] = [
        ContentKind::Spell,
        ContentKind::Item,
        ContentKind::Monster,
        ContentKind::Condition,
        ContentKind::Class,
        ContentKind::Subclass,
        ContentKind::Race,
        ContentKind::Background,
        ContentKind::Feat,
        ContentKind::Feature,
        ContentKind::Rule,
    ];

    /// The `<kind>` segment spelling, e.g. `"spell"`.
    #[must_use]
    #[inline]
    pub fn as_str(self) -> &'static str {
        todo!()
    }
}

/// A source record adapted to the localtt model. This is what `get` returns and
/// what snapshots carry; never stored inside campaign documents.
///
/// Deliberately serde-free: the adapter owns the crate-private wire types and
/// the only parse path goes through them, gated (the `ExportBundle` mechanism,
/// STORE-9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentEntry {
    /// The reference id, `<namespace>:<kind>/<slug>`.
    pub id: RefId,
    /// The content kind; must match the `id`'s kind segment.
    pub kind: ContentKind,
    /// Display name.
    pub name: String,
    /// Markdown; opaque (sanitization is a frontend concern, RFC-0001 8.7).
    pub description: String,
    /// Structured payload, typed per kind.
    pub data: ContentData,
    /// Source and licensing of this entry.
    pub provenance: Provenance,
    /// Conversion fidelity: true when the importer had to guess.
    pub needs_review: bool,
    /// Why the entry needs review, in human-readable form.
    pub review_notes: Vec<String>,
    /// Unmapped source fields, verbatim, under `ext["src.<format>"]`.
    pub ext: Ext,
}

/// Structured payload, typed per kind.
///
/// The U2 payloads are minimal; the rest of each record lives in the
/// description text and in `ext`. Widening a payload is additive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (core's InventoryItem has the same shape); boxing the item payload would ripple through every construction site and the target test constructs ContentData::Item(InlineItem { .. }) directly"
)]
pub enum ContentData {
    /// A spell.
    Spell(SpellData),
    /// An item, reusing the core item shape including modifiers.
    Item(InlineItem),
    /// A creature stat block.
    Monster(MonsterData),
    /// A class, race, feat or creature feature.
    Feature(FeatureData),
    /// Text-only content: conditions, rules, classes, races, backgrounds,
    /// feats.
    Text,
}

/// The minimal U2 spell payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellData {
    /// Spell level; 0 for cantrips.
    pub level: u8,
    /// School of magic, e.g. `"Evocation"`.
    pub school: String,
    /// Whether the spell requires concentration.
    pub concentration: bool,
    /// Whether the spell can be cast as a ritual.
    pub ritual: bool,
}

/// The minimal U2 monster payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterData {
    /// The six ability scores.
    pub abilities: Abilities,
    /// Challenge rating.
    pub challenge_rating: ChallengeRating,
    /// Armor class.
    pub armor_class: i32,
    /// Hit points.
    pub hit_points: i32,
}

/// A feature-shaped payload for class, race, feat and creature features.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureData {
    /// Feature category.
    pub category: FeatureCategory,
    /// The feature text, markdown.
    pub text: String,
}

/// Provenance of one imported snapshot, recorded per entry and reported by
/// [`ReferenceSource::sources`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Source namespace, e.g. `srd2014` or `pack.mytome`.
    pub namespace: Namespace,
    /// Human-readable source document, e.g. `"SRD 5.1 (CC-BY-4.0), via Open5e"`.
    pub source: String,
    /// Pinned source revision, e.g. a commit SHA plus dataset version.
    pub source_revision: String,
    /// License identifier, e.g. `"CC-BY-4.0"`.
    pub license: String,
    /// Attribution text to surface in the UI.
    pub attribution: String,
    /// Canonical source URL, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Lightweight hit for lists and autocomplete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentSummary {
    /// The reference id.
    pub id: RefId,
    /// The content kind.
    pub kind: ContentKind,
    /// Display name.
    pub name: String,
}

/// One search or autocomplete request.
///
/// `text` is trimmed and matched case-insensitively; an empty text yields no
/// hits. `kinds` filters before ranking and its order is the kind priority;
/// empty means all kinds ([`ContentKind::ALL`] order). `limit` of 0 yields no
/// hits.
#[derive(Clone, Copy, Debug)]
pub struct SearchQuery<'a> {
    /// The query text.
    pub text: &'a str,
    /// Kinds to include; empty means all.
    pub kinds: &'a [ContentKind],
    /// Maximum number of hits.
    pub limit: usize,
}

/// Failure of a content operation.
///
/// `message` fields are diagnostic, not user-facing; the server maps variants
/// to status codes and must not forward the messages (the `StoreFault` rule).
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ContentFault {
    /// The source root or a required file is missing or unreadable.
    #[error("the content source is unavailable: {message}")]
    Unavailable {
        /// Diagnostic detail (path, IO error).
        message: String,
    },
    /// A snapshot file, index row or entry body failed to parse or validate.
    #[error("the content snapshot is corrupt: {message}")]
    Corrupt {
        /// Diagnostic detail (path, parse error, invariant).
        message: String,
    },
    /// A snapshot's on-disk version is newer than this build supports.
    #[error("snapshot version {found} is newer than this build supports ({supported})")]
    SnapshotVersionRefused {
        /// The version found on disk.
        found: u32,
        /// The version this build supports.
        supported: u32,
    },
    /// Two loaded sources share one namespace.
    #[error("namespace {namespace} appears in more than one source")]
    NamespaceConflict {
        /// The duplicated namespace.
        namespace: Namespace,
    },
}

/// Read-only content access for the engine.
///
/// Implementations are immutable after construction (snapshots are pinned), so
/// caching is trivial: load once. `Ok(None)` from [`ReferenceSource::get`]
/// means "not in any loaded source" and is what the rules layer turns into an
/// unresolved-reference warning; a failure means the source itself is broken.
pub trait ReferenceSource: Send + Sync {
    /// Resolves one entry, following a one-hop alias.
    fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault>;

    /// Name-and-description search, deterministic order.
    fn search(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault>;

    /// Ranked name autocomplete, deterministic order.
    fn autocomplete(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault>;

    /// Sources (namespaces) currently loaded, in load order.
    fn sources(&self) -> Vec<Provenance>;
}

#[cfg(test)]
mod tests {
    use super::ContentKind;

    /// The exhaustive match is the guard: adding a variant breaks this test's
    /// compilation, forcing a deliberate decision about the variant's `ALL` slot
    /// instead of silently dropping it from unfiltered ranking.
    fn rank(kind: ContentKind) -> usize {
        match kind {
            ContentKind::Spell => 0,
            ContentKind::Item => 1,
            ContentKind::Monster => 2,
            ContentKind::Condition => 3,
            ContentKind::Class => 4,
            ContentKind::Subclass => 5,
            ContentKind::Race => 6,
            ContentKind::Background => 7,
            ContentKind::Feat => 8,
            ContentKind::Feature => 9,
            ContentKind::Rule => 10,
        }
    }

    #[test]
    fn all_lists_every_kind_in_rank_order() {
        let ranks: Vec<usize> = ContentKind::ALL.iter().copied().map(rank).collect();

        assert_eq!(ranks, (0..ContentKind::ALL.len()).collect::<Vec<_>>());
    }
}
