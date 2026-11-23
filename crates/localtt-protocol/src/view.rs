//! Per-viewer view DTOs (`docs/architecture.md` section 6).
//!
//! These are the wire shapes of the engine's `authz` views: one `Option` per
//! maskable section, absent keys (never `null`) for hidden sections, and no
//! join-token material. The engine's view types stay serde-free; every shape
//! here is built from one by a `From` impl. Core value types (`Hp`, `Resource`,
//! `InventoryEntry`, ...) appear unchanged inside these DTOs, so their serde
//! shape is part of the transport contract as well (`crates/localtt-core/DESIGN.md`
//! section 4).

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::types::{
    Abilities,
    Ability,
    AttackEntry,
    Audience,
    CampaignId,
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
    Role,
    RulesetId,
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
use localtt_engine::authz::{
    CampaignSettingsView,
    CampaignView,
    EntityView,
    EntityViewBody,
    FeedEntryView,
    HandoutView,
    InitiativeEntryView,
    InitiativeView,
    NoteView,
    ProficienciesView,
    RosterAdminEntry,
    RosterEntryView,
    StatblockView,
};
use serde::{Deserialize, Serialize};

/// One entity as a viewer sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityViewDto {
    /// Entity id.
    pub id: EntityId,
    /// Entity kind.
    pub kind: EntityKind,
    /// Owner.
    pub owner: PlayerId,
    /// Handout/companion parent.
    pub parent: Option<EntityId>,
    /// Document revision.
    pub rev: u64,
    /// Whether this viewer may write.
    pub writable: bool,
    /// Every section this viewer may not see.
    pub hidden: BTreeSet<ViewField>,
    /// Visible name; absent when `Name` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Visible plugin bucket; absent when `Ext` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<Ext>,
    /// The visible body family; absent when no carrier section is visible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<EntityViewBodyDto>,
}

/// The three visible body families, tagged like the core document bodies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "the unboxed `Statblock(StatblockViewDto)` shape is the pinned wire contract; one view is built per request, so the size is irrelevant"
)]
pub enum EntityViewBodyDto {
    /// A statblock family entity.
    Statblock(StatblockViewDto),
    /// A session note.
    Note(NoteViewDto),
    /// A handout.
    Handout(HandoutViewDto),
}

/// One statblock as a viewer sees it: one `Option` per carrier field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatblockViewDto {
    /// Profile; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Identity box; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
    /// Class levels; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<ClassLevel>>,
    /// Challenge rating; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cr: Option<ChallengeRating>,
    /// Creature type; gated by `ViewField::Identity`.
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
    /// Saves, skills, tools and languages; one gate each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proficiencies: Option<ProficienciesViewDto>,
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
    /// Exhaustion level; an always-visible rider.
    pub exhaustion: u8,
    /// Inspiration flag; an always-visible rider.
    pub inspiration: bool,
}

/// The four maskable parts of a statblock's proficiencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProficienciesViewDto {
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

/// A session note as a viewer sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteViewDto {
    /// The note text.
    pub text: String,
}

/// A handout as a viewer sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoutViewDto {
    /// The handout payload.
    pub content: HandoutContent,
}

/// One feed entry as a viewer sees it: the entry minus its audience.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedEntryViewDto {
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

/// One campaign participant as the roster exposes them.
///
/// Never carries the join-token hash or the last-seen stamp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntryViewDto {
    /// Player id.
    pub id: PlayerId,
    /// Display name.
    pub display_name: String,
    /// Campaign role.
    pub role: Role,
}

/// One campaign participant as the DM's roster admin surface exposes them.
///
/// Reports whether an invite hash is present, never the hash itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterAdminEntryDto {
    /// Player id.
    pub id: PlayerId,
    /// Display name.
    pub display_name: String,
    /// Campaign role.
    pub role: Role,
    /// Whether the seat currently has an invite hash.
    pub has_invite: bool,
}

/// Campaign-wide settings every roster member may read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignSettingsViewDto {
    /// Default audience for player rolls.
    pub player_roll_audience_default: Audience,
    /// Feed ring-buffer capacity.
    pub feed_capacity: u32,
    /// Hour of day (0 to 23) at which Dawn recharges happen.
    pub dawn_hour: u8,
}

/// One participant in the shared initiative order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitiativeEntryViewDto {
    /// The participating entity.
    pub entity: EntityId,
    /// Name snapshot.
    pub name: String,
    /// Owner snapshot.
    pub owner: PlayerId,
    /// The rolled or entered initiative value.
    pub value: i32,
}

/// The shared initiative order, current turn and round.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitiativeViewDto {
    /// Participants in turn order.
    pub entries: Vec<InitiativeEntryViewDto>,
    /// The participant whose turn it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<EntityId>,
    /// The current round; `0` while the encounter has not started.
    pub round: u32,
}

/// The campaign as one roster member sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignViewDto {
    /// The roster member this view was projected for.
    pub viewer: PlayerId,
    /// Campaign id.
    pub id: CampaignId,
    /// Display name.
    pub name: String,
    /// Default ruleset for new entities.
    pub default_ruleset: RulesetId,
    /// The campaign clock.
    pub clock: CampaignTime,
    /// Member-visible campaign settings.
    pub settings: CampaignSettingsViewDto,
    /// The shared initiative order.
    pub initiative: InitiativeViewDto,
    /// The roster in stored order; no join-token material.
    pub roster: Vec<RosterEntryViewDto>,
}

impl From<EntityView> for EntityViewDto {
    #[inline]
    fn from(view: EntityView) -> Self {
        Self {
            id: view.id,
            kind: view.kind,
            owner: view.owner,
            parent: view.parent,
            rev: view.rev,
            writable: view.writable,
            hidden: view.hidden,
            name: view.name,
            ext: view.ext,
            body: view.body.map(EntityViewBodyDto::from),
        }
    }
}

impl From<EntityViewBody> for EntityViewBodyDto {
    #[inline]
    fn from(body: EntityViewBody) -> Self {
        match body {
            EntityViewBody::Statblock(view) => Self::Statblock(view.into()),
            EntityViewBody::Note(view) => Self::Note(view.into()),
            EntityViewBody::Handout(view) => Self::Handout(view.into()),
        }
    }
}

impl From<StatblockView> for StatblockViewDto {
    #[inline]
    fn from(view: StatblockView) -> Self {
        Self {
            profile: view.profile,
            identity: view.identity,
            levels: view.levels,
            cr: view.cr,
            creature_type: view.creature_type,
            size: view.size,
            senses: view.senses,
            abilities: view.abilities,
            proficiencies: view.proficiencies.map(ProficienciesViewDto::from),
            hp: view.hp,
            speed: view.speed,
            death_saves: view.death_saves,
            coins: view.coins,
            overrides: view.overrides,
            resources: view.resources,
            spellcasting: view.spellcasting,
            inventory: view.inventory,
            attacks: view.attacks,
            features: view.features,
            statuses: view.statuses,
            notes: view.notes,
            exhaustion: view.exhaustion,
            inspiration: view.inspiration,
        }
    }
}

impl From<ProficienciesView> for ProficienciesViewDto {
    #[inline]
    fn from(view: ProficienciesView) -> Self {
        Self {
            saves: view.saves,
            skills: view.skills,
            tools: view.tools,
            languages: view.languages,
        }
    }
}

impl From<NoteView> for NoteViewDto {
    #[inline]
    fn from(view: NoteView) -> Self {
        Self { text: view.text }
    }
}

impl From<HandoutView> for HandoutViewDto {
    #[inline]
    fn from(view: HandoutView) -> Self {
        Self {
            content: view.content,
        }
    }
}

impl From<FeedEntryView> for FeedEntryViewDto {
    #[inline]
    fn from(view: FeedEntryView) -> Self {
        Self {
            id: view.id,
            ts: view.ts,
            clock_s: view.clock_s,
            actor: view.actor,
            entity: view.entity,
            payload: view.payload,
        }
    }
}

impl From<RosterEntryView> for RosterEntryViewDto {
    #[inline]
    fn from(view: RosterEntryView) -> Self {
        Self {
            id: view.id,
            display_name: view.display_name,
            role: view.role,
        }
    }
}

impl From<RosterAdminEntry> for RosterAdminEntryDto {
    #[inline]
    fn from(entry: RosterAdminEntry) -> Self {
        Self {
            id: entry.id,
            display_name: entry.display_name,
            role: entry.role,
            has_invite: entry.has_invite,
        }
    }
}

impl From<CampaignSettingsView> for CampaignSettingsViewDto {
    #[inline]
    fn from(view: CampaignSettingsView) -> Self {
        Self {
            player_roll_audience_default: view.player_roll_audience_default,
            feed_capacity: view.feed_capacity,
            dawn_hour: view.dawn_hour,
        }
    }
}

impl From<InitiativeEntryView> for InitiativeEntryViewDto {
    #[inline]
    fn from(view: InitiativeEntryView) -> Self {
        Self {
            entity: view.entity,
            name: view.name,
            owner: view.owner,
            value: view.value,
        }
    }
}

impl From<InitiativeView> for InitiativeViewDto {
    #[inline]
    fn from(view: InitiativeView) -> Self {
        Self {
            entries: view
                .entries
                .into_iter()
                .map(InitiativeEntryViewDto::from)
                .collect(),
            current: view.current,
            round: view.round,
        }
    }
}

impl From<CampaignView> for CampaignViewDto {
    #[inline]
    fn from(view: CampaignView) -> Self {
        Self {
            viewer: view.viewer,
            id: view.id,
            name: view.name,
            default_ruleset: view.default_ruleset,
            clock: view.clock,
            settings: view.settings.into(),
            initiative: view.initiative.into(),
            roster: view
                .roster
                .into_iter()
                .map(RosterEntryViewDto::from)
                .collect(),
        }
    }
}
