//! Conversions between the engine's `content` types and their DTOs.

use localtt_engine::content::{
    ContentData,
    ContentEntry,
    ContentKind,
    ContentSummary,
    FeatureData,
    MonsterData,
    Provenance,
    SpellData,
};

use crate::{
    ContentDataDto,
    ContentEntryDto,
    ContentKindDto,
    ContentSummaryDto,
    FeatureDataDto,
    MonsterDataDto,
    SourceInfoDto,
    SpellDataDto,
};

impl From<ContentKind> for ContentKindDto {
    #[inline]
    fn from(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Spell => Self::Spell,
            ContentKind::Item => Self::Item,
            ContentKind::Monster => Self::Monster,
            ContentKind::Condition => Self::Condition,
            ContentKind::Class => Self::Class,
            ContentKind::Subclass => Self::Subclass,
            ContentKind::Race => Self::Race,
            ContentKind::Background => Self::Background,
            ContentKind::Feat => Self::Feat,
            ContentKind::Feature => Self::Feature,
            ContentKind::Rule => Self::Rule,
        }
    }
}

impl From<ContentKindDto> for ContentKind {
    #[inline]
    fn from(kind: ContentKindDto) -> Self {
        match kind {
            ContentKindDto::Spell => Self::Spell,
            ContentKindDto::Item => Self::Item,
            ContentKindDto::Monster => Self::Monster,
            ContentKindDto::Condition => Self::Condition,
            ContentKindDto::Class => Self::Class,
            ContentKindDto::Subclass => Self::Subclass,
            ContentKindDto::Race => Self::Race,
            ContentKindDto::Background => Self::Background,
            ContentKindDto::Feat => Self::Feat,
            ContentKindDto::Feature => Self::Feature,
            ContentKindDto::Rule => Self::Rule,
        }
    }
}

impl From<ContentSummary> for ContentSummaryDto {
    #[inline]
    fn from(summary: ContentSummary) -> Self {
        Self {
            id: summary.id,
            kind: summary.kind.into(),
            name: summary.name,
        }
    }
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

impl From<ContentData> for ContentDataDto {
    #[inline]
    fn from(data: ContentData) -> Self {
        match data {
            ContentData::Spell(data) => Self::Spell(data.into()),
            ContentData::Item(item) => Self::Item(item),
            ContentData::Monster(data) => Self::Monster(data.into()),
            ContentData::Feature(data) => Self::Feature(data.into()),
            ContentData::Text => Self::Text,
        }
    }
}

impl From<SpellData> for SpellDataDto {
    #[inline]
    fn from(data: SpellData) -> Self {
        Self {
            level: data.level,
            concentration: data.concentration,
            school: data.school,
            ritual: data.ritual,
        }
    }
}

impl From<MonsterData> for MonsterDataDto {
    #[inline]
    fn from(data: MonsterData) -> Self {
        Self {
            abilities: data.abilities,
            challenge_rating: data.challenge_rating,
            armor_class: data.armor_class,
            hit_points: data.hit_points,
        }
    }
}

impl From<FeatureData> for FeatureDataDto {
    #[inline]
    fn from(data: FeatureData) -> Self {
        Self {
            category: data.category,
            text: data.text,
        }
    }
}

impl From<ContentEntry> for ContentEntryDto {
    #[inline]
    fn from(entry: ContentEntry) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind.into(),
            name: entry.name,
            description: entry.description,
            data: entry.data.into(),
            provenance: entry.provenance.into(),
            needs_review: entry.needs_review,
            review_notes: entry.review_notes,
            ext: entry.ext,
        }
    }
}
