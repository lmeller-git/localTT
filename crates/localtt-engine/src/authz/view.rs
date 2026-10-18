//! `EntityView` and the `ViewField` section map (RFC `authz.md` 5.3, 5.4 and
//! section 6).

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::types::{
    Abilities,
    Ability,
    AttackEntry,
    CampaignTime,
    ChallengeRating,
    ClassLevel,
    Coins,
    DeathSaves,
    EntityId,
    EntityKind,
    Ext,
    FeatureEntry,
    FeedEntryId,
    FeedPayload,
    HandoutContent,
    Hp,
    Identity,
    InventoryEntry,
    OverrideValue,
    PlayerId,
    Profile,
    Resource,
    Size,
    Skill,
    SkillProficiency,
    Speed,
    SpellcastingBlock,
    StatKey,
    Status,
    UnixTime,
    ViewField,
};
use serde::Serialize;

/// An engine-owned mirror of one entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntityView {
    /// Mirrored entity id.
    pub id: EntityId,
    /// Mirrored entity kind.
    pub kind: EntityKind,
    /// Mirrored owner.
    pub owner: PlayerId,
    /// Mirrored handout/companion parent.
    pub parent: Option<EntityId>,
    /// Mirrored document revision.
    pub rev: u64,
    /// Whether this viewer may write; `can_write` for the same arguments.
    pub writable: bool,
    /// Every section this viewer may not see; `ALL \ visible`.
    pub hidden: BTreeSet<ViewField>,
    /// Visible name; absent when `Name` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Visible plugin bucket; absent when `Ext` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<Ext>,
    /// The visible body family; absent when no carrier section is visible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<EntityViewBody>,
}

/// The three visible body families (core `BodyKind`), tagged like the core
/// document bodies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "the unboxed `Statblock(StatblockView)` shape is the pinned public contract (authz.md 4.1); one view is built per request, so the size is irrelevant"
)]
pub enum EntityViewBody {
    /// A statblock family entity.
    Statblock(StatblockView),
    /// A session note.
    Note(NoteView),
    /// A handout.
    Handout(HandoutView),
}

/// One statblock as a viewer sees it: one `Option` per carrier field.
///
/// Where the core field is already an `Option` (`cr`, `creature_type`), the
/// view keeps the core type; the visibility signal lives in `hidden`, so an
/// absent value and a hidden section stay distinguishable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StatblockView {
    /// Profile; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Identity box; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
    /// Class levels; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<ClassLevel>>,
    /// Challenge rating; gated by `ViewField::Identity`, mirrors the core
    /// `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cr: Option<ChallengeRating>,
    /// Creature type; gated by `ViewField::Identity`, mirrors the core
    /// `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creature_type: Option<String>,
    /// Size; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Senses; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub senses: Option<String>,
    /// Ability scores; gated by `ViewField::Abilities`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abilities: Option<Abilities>,
    /// Saves, skills, tools and languages; one gate each (section 5.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proficiencies: Option<ProficienciesView>,
    /// Hit points; gated by `ViewField::Hp`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp: Option<Hp>,
    /// Movement speeds; gated by `ViewField::Speed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<Speed>,
    /// Death saves; gated by `ViewField::DeathSaves`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_saves: Option<DeathSaves>,
    /// Coins; gated by `ViewField::Coins`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coins: Option<Coins>,
    /// Derived-value overrides; gated by `ViewField::Overrides`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<BTreeMap<StatKey, OverrideValue>>,
    /// Resources; gated by `ViewField::Resources`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<Resource>>,
    /// Spellcasting blocks; gated by `ViewField::Spells`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spellcasting: Option<Vec<SpellcastingBlock>>,
    /// Inventory; gated by `ViewField::Inventory`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Vec<InventoryEntry>>,
    /// Attack lines; gated by `ViewField::Attacks`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attacks: Option<Vec<AttackEntry>>,
    /// Features; gated by `ViewField::Features`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<FeatureEntry>>,
    /// Statuses; gated by `ViewField::Statuses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<Status>>,
    /// Notes; gated by `ViewField::Notes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Exhaustion level; an always-visible rider (section 9, OQ-1).
    pub exhaustion: u8,
    /// Inspiration flag; an always-visible rider (section 9, OQ-1).
    pub inspiration: bool,
}

/// The four maskable parts of a statblock's proficiencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProficienciesView {
    /// Save proficiencies; gated by `ViewField::Saves`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saves: Option<BTreeSet<Ability>>,
    /// Skill proficiencies; gated by `ViewField::Skills`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<BTreeMap<Skill, SkillProficiency>>,
    /// Tool lines; gated by `ViewField::Proficiencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<String>,
    /// Language lines; gated by `ViewField::Proficiencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<String>,
}

/// A session note as a viewer sees it; present iff `Notes` is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteView {
    /// The note text.
    pub text: String,
}

/// A handout as a viewer sees it; present iff `Notes` is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HandoutView {
    /// The handout payload.
    pub content: HandoutContent,
}

/// One feed entry as a viewer sees it: the entry minus its audience.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FeedEntryView {
    /// Feed position.
    pub id: FeedEntryId,
    /// Wall-clock stamp.
    pub ts: UnixTime,
    /// Campaign-clock stamp.
    pub clock_s: CampaignTime,
    /// Acting player, when the entry has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<PlayerId>,
    /// Related entity, when the entry has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// What happened.
    pub payload: FeedPayload,
}
