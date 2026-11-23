//! Reference-content DTOs (`docs/architecture.md` section 6).
//!
//! The engine's content types are the adapter's snapshot vocabulary, not the
//! wire contract: the server maps them to these transport shapes, and the wire
//! spelling of kinds and payloads is pinned here independently of them.

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

/// The closed set of reference kinds, spelled as the `<kind>` segment of a
/// reference id.
///
/// Declaration order is the fixed priority order used when a caller ranks
/// unfiltered results; [`ContentKindDto::ALL`] exposes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKindDto {
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

impl ContentKindDto {
    /// Every kind, in the fixed priority order used for unfiltered ranking.
    pub const ALL: [ContentKindDto; 11] = [
        ContentKindDto::Spell,
        ContentKindDto::Item,
        ContentKindDto::Monster,
        ContentKindDto::Condition,
        ContentKindDto::Class,
        ContentKindDto::Subclass,
        ContentKindDto::Race,
        ContentKindDto::Background,
        ContentKindDto::Feat,
        ContentKindDto::Feature,
        ContentKindDto::Rule,
    ];

    /// The `<kind>` segment spelling, e.g. `"spell"`.
    #[must_use]
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            ContentKindDto::Spell => "spell",
            ContentKindDto::Item => "item",
            ContentKindDto::Monster => "monster",
            ContentKindDto::Condition => "condition",
            ContentKindDto::Class => "class",
            ContentKindDto::Subclass => "subclass",
            ContentKindDto::Race => "race",
            ContentKindDto::Background => "background",
            ContentKindDto::Feat => "feat",
            ContentKindDto::Feature => "feature",
            ContentKindDto::Rule => "rule",
        }
    }
}

/// One reference hit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSummaryDto {
    /// The reference id.
    pub id: RefId,
    /// The content kind.
    pub kind: ContentKindDto,
    /// Display name.
    pub name: String,
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

/// One full reference entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentEntryDto {
    /// The reference id.
    pub id: RefId,
    /// The content kind.
    pub kind: ContentKindDto,
    /// Display name.
    pub name: String,
    /// Markdown; frontends sanitize before rendering.
    pub description: String,
    /// Structured payload, typed per kind.
    pub data: ContentDataDto,
    /// Source and licensing.
    pub provenance: SourceInfoDto,
    /// Conversion fidelity: true when the importer had to guess.
    pub needs_review: bool,
    /// Why the entry needs review.
    pub review_notes: Vec<String>,
    /// Unmapped source fields, verbatim.
    pub ext: Ext,
}

/// Structured payload, typed per kind; tagged `type` like the engine payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (core's InventoryItem has the same shape); one entry is built per request"
)]
pub enum ContentDataDto {
    /// A spell.
    Spell(SpellDataDto),
    /// An item, reusing the core item shape including modifiers.
    Item(InlineItem),
    /// A creature stat block.
    Monster(MonsterDataDto),
    /// A class, race, feat or creature feature.
    Feature(FeatureDataDto),
    /// Text-only content: conditions, rules, classes, races, backgrounds,
    /// feats.
    Text,
}

/// The spell payload of a reference entry.
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

/// The monster payload of a reference entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterDataDto {
    /// The six ability scores.
    pub abilities: Abilities,
    /// Challenge rating.
    pub challenge_rating: ChallengeRating,
    /// Armor class.
    pub armor_class: i32,
    /// Hit points.
    pub hit_points: i32,
}

/// The feature payload of a reference entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDataDto {
    /// Feature category.
    pub category: FeatureCategory,
    /// The feature text, markdown.
    pub text: String,
}

impl ContentEntryDto {
    /// The structured spell payload, when this entry is a spell.
    ///
    /// Clients use this to prefill a spell form and to show concentration and
    /// ritual markers.
    #[must_use]
    #[inline]
    pub fn spell(&self) -> Option<SpellDataDto> {
        if self.kind != ContentKindDto::Spell {
            return None;
        }
        match &self.data {
            ContentDataDto::Spell(data) => Some(data.clone()),
            _ => None,
        }
    }
}
