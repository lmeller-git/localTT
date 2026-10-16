//! Data model of RFC-0002: serialized documents plus their validation.
//!
//! The module follows the section order of RFC-0002 section 3 so a later split
//! into `types/` submodules can keep every public path unchanged.
#![expect(
    clippy::todo,
    unused_variables,
    dead_code,
    reason = "skeleton phase: every body is `todo!()`, so parameters and builder fields are not read yet; this gate is removed in the implement phase"
)]

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 1. Ids
// ---------------------------------------------------------------------------

/// Opaque, time-sortable (UUID v7) identifier with a type-level domain tag.
///
/// The tag never appears on the wire: an id serializes as the plain UUID
/// string of its domain type. The marker keeps `Id<T>` `Copy`, `Send` and
/// `Sync` regardless of `T`, so plugins may mint `Id<TheirType>`.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct Id<T> {
    uuid: Uuid,
    #[serde(skip)]
    marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// Generates a fresh v7 id.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::from_uuid(Uuid::now_v7())
    }

    /// Wraps an existing UUID (import, migration, tests).
    #[must_use]
    #[inline]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            marker: PhantomData,
        }
    }

    /// Returns the underlying UUID.
    #[must_use]
    #[inline]
    pub fn as_uuid(&self) -> Uuid {
        self.uuid
    }
}

impl<T> Default for Id<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for Id<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> fmt::Debug for Id<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.uuid, f)
    }
}

impl<T> PartialEq for Id<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl<T> PartialOrd for Id<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.uuid.cmp(&other.uuid)
    }
}

impl<T> fmt::Display for Id<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.uuid, f)
    }
}

/// Campaign document id.
pub type CampaignId = Id<Campaign>;
/// Player id.
pub type PlayerId = Id<Player>;
/// Id of an entity of any body kind.
pub type EntityId = Id<AnyEntity>;
/// Status id.
pub type StatusId = Id<Status>;
/// Resource id.
pub type ResourceId = Id<Resource>;
/// Spellcasting block id.
pub type SpellcastingBlockId = Id<SpellcastingBlock>;
/// Spell entry id.
pub type SpellEntryId = Id<SpellEntry>;
/// Class level id.
pub type ClassLevelId = Id<ClassLevel>;
/// Inventory entry id.
pub type InventoryEntryId = Id<InventoryEntry>;
/// Attack entry id.
pub type AttackEntryId = Id<AttackEntry>;
/// Feature entry id.
pub type FeatureEntryId = Id<FeatureEntry>;
/// Roll id.
pub type RollId = Id<Roll>;

/// Monotonic per-campaign feed position; not a UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeedEntryId(u64);

impl FeedEntryId {
    /// Wraps a feed position.
    #[must_use]
    #[inline]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the feed position.
    #[must_use]
    #[inline]
    pub fn get(self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// 2. Primitives: time, versions, namespaces, refs, stat keys
// ---------------------------------------------------------------------------

/// Seconds on the DM-controlled campaign clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CampaignTime(i64);

impl CampaignTime {
    /// Builds a campaign time from seconds.
    #[must_use]
    #[inline]
    pub fn from_seconds(s: i64) -> Self {
        Self(s)
    }

    /// Returns the campaign time in seconds.
    #[must_use]
    #[inline]
    pub fn seconds(self) -> i64 {
        self.0
    }
}

impl fmt::Display for CampaignTime {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Unix seconds; wall clock, for audit and display only, never for durations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTime(i64);

impl UnixTime {
    /// Builds a wall-clock time from seconds.
    #[must_use]
    #[inline]
    pub fn from_seconds(s: i64) -> Self {
        Self(s)
    }

    /// Returns the wall-clock time in seconds.
    #[must_use]
    #[inline]
    pub fn seconds(self) -> i64 {
        self.0
    }
}

impl fmt::Display for UnixTime {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Document schema version, `major.minor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// Breaking-change counter.
    pub major: u16,
    /// Additive-change counter.
    pub minor: u16,
}

impl SchemaVersion {
    /// The version this build writes.
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    /// True when this build may load `file`: same or older major.
    #[must_use]
    #[inline]
    pub const fn accepts(self, file: Self) -> bool {
        file.major <= self.major
    }
}

/// Ruleset identifier, e.g. `"5e-2014"`: lowercase `[a-z0-9-]`, at most 32
/// characters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RulesetId(String);

impl RulesetId {
    /// The default 5e (2014) ruleset id string.
    pub const DND5E_2014: &'static str = "5e-2014";

    /// Parses and validates a ruleset id.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, RulesetIdError> {
        todo!()
    }
}

impl fmt::Display for RulesetId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Serialize for RulesetId {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}

impl<'de> Deserialize<'de> for RulesetId {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        todo!()
    }
}

/// Plugin data namespace, e.g. `"com.example.foo"`: lowercase dotted segments.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(String);

impl Namespace {
    /// Parses and validates a namespace.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, NamespaceError> {
        todo!()
    }
}

impl fmt::Display for Namespace {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Serialize for Namespace {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}

impl<'de> Deserialize<'de> for Namespace {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        todo!()
    }
}

/// Namespaced content reference: `<source>:<kind>/<slug>`, e.g.
/// `"srd2014:spell/fireball"` or `"pack.mytome:item/foo"`.
///
/// Internals are the canonical string plus two segment boundaries: one
/// allocation per reference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefId {
    raw: String,
    namespace_end: u32,
    kind_end: u32,
}

impl RefId {
    /// Parses and validates a reference.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, RefIdError> {
        let (namespace, rest) = raw.split_once(':').ok_or(RefIdError::Invalid)?;
        let (kind, slug) = rest.split_once('/').ok_or(RefIdError::Invalid)?;
        if !is_namespace(namespace) || !is_segment(kind) || !is_segment(slug) {
            return Err(RefIdError::Invalid);
        }
        Ok(Self {
            raw: raw.to_owned(),
            namespace_end: namespace.len() as u32,
            kind_end: (namespace.len() + 1 + kind.len()) as u32,
        })
    }

    /// The source namespace, e.g. `"srd2014"` or `"pack.mytome"`.
    #[must_use]
    #[inline]
    pub fn namespace(&self) -> &str {
        &self.raw[..self.namespace_end as usize]
    }

    /// The content kind, e.g. `"spell"`.
    #[must_use]
    #[inline]
    pub fn kind(&self) -> &str {
        &self.raw[self.namespace_end as usize + 1..self.kind_end as usize]
    }

    /// The content slug, e.g. `"fireball"`.
    #[must_use]
    #[inline]
    pub fn slug(&self) -> &str {
        &self.raw[self.kind_end as usize + 1..]
    }

    /// The canonical `<source>:<kind>/<slug>` string.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for RefId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl Serialize for RefId {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for RefId {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// True when every dotted segment of `s` is a non-empty lower-kebab segment.
fn is_namespace(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(is_segment)
}

/// True when `s` is a non-empty run of `[a-z0-9_-]`.
fn is_segment(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// A reference to shipped/pack content plus its name snapshot, so a missing
/// pack degrades to text instead of a broken sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRef {
    /// The content reference.
    pub id: RefId,
    /// The display name captured at reference time.
    pub name: String,
}

/// Either a reference or inline text (identity boxes, subclasses, spell
/// sources).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RefOrText {
    /// A reference to shipped/pack content.
    Ref(ContentRef),
    /// Inline free text.
    Text(String),
}

/// Plugin payload slot: namespace to arbitrary JSON.
pub type Ext = BTreeMap<Namespace, Json>;
pub use serde_json::Value as Json;

/// The six 5e abilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Ability {
    /// Strength.
    #[serde(rename = "str")]
    Strength,
    /// Dexterity.
    #[serde(rename = "dex")]
    Dexterity,
    /// Constitution.
    #[serde(rename = "con")]
    Constitution,
    /// Intelligence.
    #[serde(rename = "int")]
    Intelligence,
    /// Wisdom.
    #[serde(rename = "wis")]
    Wisdom,
    /// Charisma.
    #[serde(rename = "cha")]
    Charisma,
}

impl fmt::Display for Ability {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Strength => "str",
            Self::Dexterity => "dex",
            Self::Constitution => "con",
            Self::Intelligence => "int",
            Self::Wisdom => "wis",
            Self::Charisma => "cha",
        })
    }
}

impl FromStr for Ability {
    type Err = AbilityError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "str" => Ok(Self::Strength),
            "dex" => Ok(Self::Dexterity),
            "con" => Ok(Self::Constitution),
            "int" => Ok(Self::Intelligence),
            "wis" => Ok(Self::Wisdom),
            "cha" => Ok(Self::Charisma),
            _ => Err(AbilityError),
        }
    }
}

/// The eighteen 5e skills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    /// Acrobatics.
    Acrobatics,
    /// Animal handling.
    AnimalHandling,
    /// Arcana.
    Arcana,
    /// Athletics.
    Athletics,
    /// Deception.
    Deception,
    /// History.
    History,
    /// Insight.
    Insight,
    /// Intimidation.
    Intimidation,
    /// Investigation.
    Investigation,
    /// Medicine.
    Medicine,
    /// Nature.
    Nature,
    /// Perception.
    Perception,
    /// Performance.
    Performance,
    /// Persuasion.
    Persuasion,
    /// Religion.
    Religion,
    /// Sleight of hand.
    SleightOfHand,
    /// Stealth.
    Stealth,
    /// Survival.
    Survival,
}

/// Attack categories known to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttackKind {
    /// Melee attack.
    Melee,
    /// Ranged attack.
    Ranged,
    /// Spell attack.
    Spell,
}

/// Group targets: a modifier on a group applies to every member when the
/// engine composes a matching roll (Bless: `+1d4` to saves and attacks).
///
/// Which concrete rolls belong to a group is engine policy; core only names
/// the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatGroup {
    /// All ability and skill checks.
    Check,
    /// All saving throws.
    Save,
    /// All attacks.
    Attack,
}

/// Stats the engine knows how to compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KnownStat {
    /// Armor class.
    Ac,
    /// Initiative.
    Initiative,
    /// Walking speed.
    Speed,
    /// Proficiency bonus.
    ProficiencyBonus,
    /// Ability score (e.g. `"ability.str"`).
    AbilityScore(Ability),
    /// Ability modifier (e.g. `"ability_mod.str"`).
    AbilityModifier(Ability),
    /// Saving throw (e.g. `"save.wis"`).
    Save(Ability),
    /// Skill check (e.g. `"skill.stealth"`).
    Skill(Skill),
    /// Passive score (e.g. `"passive.perception"`).
    PassiveScore(Skill),
    /// Attack (e.g. `"attack.melee"`).
    Attack(AttackKind),
    /// Spell save DC.
    SpellDc,
    /// Group target (e.g. `"save"`).
    Group(StatGroup),
}

/// Validated custom stat path: lowercase `[a-z0-9_.-]`, 1 to 64 characters,
/// non-empty dot segments, e.g. `"house.luck"`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomStatKey(String);

impl fmt::Display for CustomStatKey {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for CustomStatKey {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CustomStatKey {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        custom_stat_key(&raw).map_err(serde::de::Error::custom)
    }
}

/// Validates a custom stat path: 1..=64 bytes, non-empty `[a-z0-9_-]` dot segments.
fn custom_stat_key(raw: &str) -> Result<CustomStatKey, StatKeyError> {
    if raw.len() > 64 || !raw.split('.').all(is_segment) {
        return Err(StatKeyError::Invalid);
    }
    Ok(CustomStatKey(raw.to_owned()))
}

/// A stat path: known to the engine, or a validated extension key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatKey {
    /// A stat the engine understands.
    Known(KnownStat),
    /// A validated extension key.
    Custom(CustomStatKey),
}

impl StatKey {
    /// Parses canonical known strings first, then validates as custom.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, StatKeyError> {
        match known_stat(raw) {
            Some(known) => Ok(Self::Known(known)),
            None => custom_stat_key(raw).map(Self::Custom),
        }
    }
}

impl fmt::Display for StatKey {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(known) => f.write_str(&known_key(known)),
            Self::Custom(custom) => custom.fmt(f),
        }
    }
}

impl FromStr for StatKey {
    type Err = StatKeyError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for StatKey {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for StatKey {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Parses a canonical known-stat key, exact and case-sensitive.
fn known_stat(raw: &str) -> Option<KnownStat> {
    let known = match raw {
        "ac" => KnownStat::Ac,
        "initiative" => KnownStat::Initiative,
        "speed" => KnownStat::Speed,
        "proficiency_bonus" => KnownStat::ProficiencyBonus,
        "spell.dc" => KnownStat::SpellDc,
        "check" => KnownStat::Group(StatGroup::Check),
        "save" => KnownStat::Group(StatGroup::Save),
        "attack" => KnownStat::Group(StatGroup::Attack),
        _ => {
            if let Some(rest) = raw.strip_prefix("ability.") {
                KnownStat::AbilityScore(Ability::from_str(rest).ok()?)
            } else if let Some(rest) = raw.strip_prefix("ability_mod.") {
                KnownStat::AbilityModifier(Ability::from_str(rest).ok()?)
            } else if let Some(rest) = raw.strip_prefix("save.") {
                KnownStat::Save(Ability::from_str(rest).ok()?)
            } else if let Some(rest) = raw.strip_prefix("skill.") {
                KnownStat::Skill(skill_from_slug(rest)?)
            } else if let Some(rest) = raw.strip_prefix("passive.") {
                KnownStat::PassiveScore(skill_from_slug(rest)?)
            } else {
                let rest = raw.strip_prefix("attack.")?;
                KnownStat::Attack(attack_kind_from_key(rest)?)
            }
        }
    };
    Some(known)
}

/// The canonical key for a known stat, e.g. `"skill.stealth"`.
fn known_key(known: &KnownStat) -> String {
    match known {
        KnownStat::Ac => "ac".to_owned(),
        KnownStat::Initiative => "initiative".to_owned(),
        KnownStat::Speed => "speed".to_owned(),
        KnownStat::ProficiencyBonus => "proficiency_bonus".to_owned(),
        KnownStat::AbilityScore(ability) => format!("ability.{ability}"),
        KnownStat::AbilityModifier(ability) => format!("ability_mod.{ability}"),
        KnownStat::Save(ability) => format!("save.{ability}"),
        KnownStat::Skill(skill) => format!("skill.{}", skill_slug(*skill)),
        KnownStat::PassiveScore(skill) => format!("passive.{}", skill_slug(*skill)),
        KnownStat::Attack(kind) => format!("attack.{}", attack_kind_key(*kind)),
        KnownStat::SpellDc => "spell.dc".to_owned(),
        KnownStat::Group(group) => group_key(*group).to_owned(),
    }
}

/// The serde slug for a skill (mirrors `Skill`'s `rename_all = "snake_case"`).
fn skill_slug(skill: Skill) -> &'static str {
    match skill {
        Skill::Acrobatics => "acrobatics",
        Skill::AnimalHandling => "animal_handling",
        Skill::Arcana => "arcana",
        Skill::Athletics => "athletics",
        Skill::Deception => "deception",
        Skill::History => "history",
        Skill::Insight => "insight",
        Skill::Intimidation => "intimidation",
        Skill::Investigation => "investigation",
        Skill::Medicine => "medicine",
        Skill::Nature => "nature",
        Skill::Perception => "perception",
        Skill::Performance => "performance",
        Skill::Persuasion => "persuasion",
        Skill::Religion => "religion",
        Skill::SleightOfHand => "sleight_of_hand",
        Skill::Stealth => "stealth",
        Skill::Survival => "survival",
    }
}

/// The skill for a serde slug.
fn skill_from_slug(slug: &str) -> Option<Skill> {
    Some(match slug {
        "acrobatics" => Skill::Acrobatics,
        "animal_handling" => Skill::AnimalHandling,
        "arcana" => Skill::Arcana,
        "athletics" => Skill::Athletics,
        "deception" => Skill::Deception,
        "history" => Skill::History,
        "insight" => Skill::Insight,
        "intimidation" => Skill::Intimidation,
        "investigation" => Skill::Investigation,
        "medicine" => Skill::Medicine,
        "nature" => Skill::Nature,
        "perception" => Skill::Perception,
        "performance" => Skill::Performance,
        "persuasion" => Skill::Persuasion,
        "religion" => Skill::Religion,
        "sleight_of_hand" => Skill::SleightOfHand,
        "stealth" => Skill::Stealth,
        "survival" => Skill::Survival,
        _ => return None,
    })
}

/// The key suffix for an attack kind, e.g. `"melee"`.
fn attack_kind_key(kind: AttackKind) -> &'static str {
    match kind {
        AttackKind::Melee => "melee",
        AttackKind::Ranged => "ranged",
        AttackKind::Spell => "spell",
    }
}

/// The attack kind for a key suffix.
fn attack_kind_from_key(key: &str) -> Option<AttackKind> {
    Some(match key {
        "melee" => AttackKind::Melee,
        "ranged" => AttackKind::Ranged,
        "spell" => AttackKind::Spell,
        _ => return None,
    })
}

/// The key for a stat group, e.g. `"save"`.
fn group_key(group: StatGroup) -> &'static str {
    match group {
        StatGroup::Check => "check",
        StatGroup::Save => "save",
        StatGroup::Attack => "attack",
    }
}

// ---------------------------------------------------------------------------
// 3. Campaign, players, clock
// ---------------------------------------------------------------------------

/// Singleton campaign document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Campaign {
    /// Campaign id.
    pub id: CampaignId,
    /// Display name.
    pub name: String,
    /// Document schema version.
    pub schema_version: SchemaVersion,
    /// Default ruleset for new entities.
    pub default_ruleset: RulesetId,
    /// The campaign clock.
    pub clock: Clock,
    /// Campaign-wide settings.
    pub settings: CampaignSettings,
    /// The participants of the campaign.
    pub players: Vec<Player>,
    /// Plugin payload slot.
    pub ext: Ext,
}

/// The DM-controlled campaign clock; rounds are derived (6 s), not stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    /// Current campaign time in seconds.
    pub now_s: CampaignTime,
}

/// Campaign-wide settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignSettings {
    /// Default audience for player rolls; default `RollerAndDm`.
    pub player_roll_audience_default: Audience,
    /// Ring buffer capacity of the feed.
    pub feed_capacity: u32,
    /// Hour of day (0 to 23) at which Dawn recharges happen.
    pub dawn_hour: u8,
}

/// A campaign participant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    /// Player id.
    pub id: PlayerId,
    /// Display name.
    pub display_name: String,
    /// DM or player role.
    pub role: Role,
    /// Hex-encoded digest of the join token; never the token itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_hash: Option<TokenHash>,
    /// When the player joined.
    pub created_at: UnixTime,
    /// Last time the player was seen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<UnixTime>,
}

/// Lowercase-hex digest of a join token.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenHash(String);

/// Campaign role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Dungeon master.
    Dm,
    /// Player.
    Player,
}

// ---------------------------------------------------------------------------
// 4. ACL and view fields
// ---------------------------------------------------------------------------

/// Access level of an ACL entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AclLevel {
    /// Read access.
    Read,
    /// Read and write access.
    Write,
}

/// One access grant on an entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEntry {
    /// The grantee.
    pub player: PlayerId,
    /// The granted access level.
    pub level: AclLevel,
    /// Present and default-deny only for read-shares of DM-owned entities:
    /// only the listed sections are exposed. `None` means not field-masked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_mask: Option<FieldMask>,
}

/// Skeleton helper: pins the hidden type of an `impl Trait` return while the
/// body is still `todo!()`.
fn pending<T>() -> T {
    todo!()
}

/// A union of sheet sections; `Default` is empty, which means nothing visible.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldMask(BTreeSet<ViewField>);

impl FieldMask {
    /// True when the section is contained in the mask.
    #[must_use]
    #[inline]
    pub fn contains(&self, f: ViewField) -> bool {
        todo!()
    }

    /// Adds a section to the mask.
    #[inline]
    pub fn insert(&mut self, f: ViewField) {
        todo!()
    }

    /// True when no section is visible.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        todo!()
    }

    /// Iterates the contained sections.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = ViewField> + '_ {
        pending::<std::iter::Empty<ViewField>>()
    }
}

/// Redactable sheet sections; `Ext` is a single bucket for all plugin data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewField {
    /// Entity name.
    Name,
    /// Identity box.
    Identity,
    /// Ability scores.
    Abilities,
    /// Saving throw proficiencies.
    Saves,
    /// Skill proficiencies.
    Skills,
    /// Tools and languages.
    Proficiencies,
    /// Hit points.
    Hp,
    /// Armor class.
    Ac,
    /// Movement speeds.
    Speed,
    /// Death saves.
    DeathSaves,
    /// Coins.
    Coins,
    /// Derived-value overrides.
    Overrides,
    /// Resources.
    Resources,
    /// Spells.
    Spells,
    /// Inventory.
    Inventory,
    /// Attack lines.
    Attacks,
    /// Features.
    Features,
    /// Statuses.
    Statuses,
    /// Notes.
    Notes,
    /// Plugin data.
    Ext,
}

// ---------------------------------------------------------------------------
// 5. Entities and bodies
// ---------------------------------------------------------------------------

/// The eight kinds of entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// Player character.
    Pc,
    /// Companion creature.
    Companion,
    /// Summoned creature.
    Summon,
    /// Familiar.
    Familiar,
    /// Non-player character.
    Npc,
    /// Monster from a content pack.
    Monster,
    /// Session note.
    Note,
    /// Handout.
    Handout,
}

/// The three body families; six entity kinds share the statblock family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// Character/creature sheet.
    Statblock,
    /// Session note.
    Note,
    /// Handout.
    Handout,
}

impl EntityKind {
    /// The body family this kind carries.
    #[must_use]
    #[inline]
    pub const fn body_kind(self) -> BodyKind {
        todo!()
    }
}

mod sealed {
    /// Prevents downstream implementations of [`super::Body`].
    #[expect(
        unnameable_types,
        reason = "deliberate sealed-trait pattern (RFC-0002 5): reachable to satisfy the public Body bound, nameable by nobody"
    )]
    pub trait Sealed {}
}

/// Sealed marker trait implemented only by the three body types.
pub trait Body:
    sealed::Sealed + Clone + fmt::Debug + PartialEq + Eq + Send + Sync + Serialize + DeserializeOwned
{
    /// The body family this type belongs to.
    const BODY_KIND: BodyKind;
}

/// Common header plus a statically known body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity<B: Body> {
    /// Entity kind; validated against the body family.
    pub kind: EntityKind,
    /// Entity id.
    pub id: EntityId,
    /// Ruleset the sheet follows.
    pub ruleset: RulesetId,
    /// Display name.
    pub name: String,
    /// Owning player.
    pub owner: PlayerId,
    /// Parent entity, e.g. a summon's caster.
    pub parent: Option<EntityId>,
    /// Access grants.
    pub acl: Vec<AclEntry>,
    /// Revision counter for optimistic concurrency.
    pub rev: u64,
    /// Document schema version.
    pub schema_version: SchemaVersion,
    /// Plugin payload slot.
    pub ext: Ext,
    /// When the entity was created (wall clock).
    pub created_at: UnixTime,
    /// When the entity was last updated (wall clock).
    pub updated_at: UnixTime,
    /// The body payload.
    pub body: B,
}

/// A character or creature entity.
pub type StatblockEntity = Entity<StatblockBody>;
/// A session note entity.
pub type NoteEntity = Entity<NoteBody>;
/// A handout entity.
pub type HandoutEntity = Entity<HandoutBody>;

impl<B: Body> Entity<B> {
    /// Creates a builder with defaults (rev 0, current schema version, empty
    /// ACL/ext); the kind/body pairing is checked in `build`.
    #[inline]
    pub fn builder(
        kind: EntityKind,
        id: EntityId,
        name: impl Into<String>,
        owner: PlayerId,
        body: B,
    ) -> EntityBuilder<B> {
        todo!()
    }
}

/// Builder for [`Entity`] documents.
pub struct EntityBuilder<B: Body> {
    kind: EntityKind,
    id: EntityId,
    name: String,
    owner: PlayerId,
    body: B,
    ruleset: Option<RulesetId>,
    parent: Option<EntityId>,
    acl: Vec<AclEntry>,
    rev: u64,
    schema_version: SchemaVersion,
    ext: Ext,
    created_at: Option<UnixTime>,
    updated_at: Option<UnixTime>,
}

impl<B: Body> EntityBuilder<B> {
    /// Sets the ruleset.
    #[inline]
    pub fn ruleset(self, r: RulesetId) -> Self {
        todo!()
    }

    /// Sets the parent entity.
    #[inline]
    pub fn parent(self, p: EntityId) -> Self {
        todo!()
    }

    /// Sets the ACL.
    #[inline]
    pub fn acl(self, acl: Vec<AclEntry>) -> Self {
        todo!()
    }

    /// Sets the revision counter.
    #[inline]
    pub fn rev(self, rev: u64) -> Self {
        todo!()
    }

    /// Sets the schema version.
    #[inline]
    pub fn schema_version(self, v: SchemaVersion) -> Self {
        todo!()
    }

    /// Sets the plugin payload slot.
    #[inline]
    pub fn ext(self, ext: Ext) -> Self {
        todo!()
    }

    /// Sets the creation time.
    #[inline]
    pub fn created_at(self, t: UnixTime) -> Self {
        todo!()
    }

    /// Sets the last-update time.
    #[inline]
    pub fn updated_at(self, t: UnixTime) -> Self {
        todo!()
    }

    /// Fails with [`EntityError::KindBodyMismatch`] unless
    /// `kind.body_kind() == B::BODY_KIND`.
    #[inline]
    pub fn build(self) -> Result<Entity<B>, EntityError> {
        todo!()
    }
}

impl<B: Body> Serialize for Entity<B> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}

impl<'de, B: Body> Deserialize<'de> for Entity<B> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        todo!()
    }
}

/// Runtime-tagged entity for storage, iteration and `view_of`, where the body
/// kind is only known at runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (RFC-0002 3.5); boxing the statblock variant changes the public shape, deferred to the implement phase"
)]
pub enum AnyEntity {
    /// A statblock entity.
    Statblock(Entity<StatblockBody>),
    /// A note entity.
    Note(Entity<NoteBody>),
    /// A handout entity.
    Handout(Entity<HandoutBody>),
}

impl AnyEntity {
    /// The entity id.
    #[must_use]
    #[inline]
    pub fn id(&self) -> EntityId {
        todo!()
    }

    /// The entity kind.
    #[must_use]
    #[inline]
    pub fn kind(&self) -> EntityKind {
        todo!()
    }

    /// The display name.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        todo!()
    }

    /// The owning player.
    #[must_use]
    #[inline]
    pub fn owner(&self) -> PlayerId {
        todo!()
    }

    /// The parent entity, if any.
    #[must_use]
    #[inline]
    pub fn parent(&self) -> Option<EntityId> {
        todo!()
    }

    /// The access grants.
    #[must_use]
    #[inline]
    pub fn acl(&self) -> &[AclEntry] {
        todo!()
    }

    /// The revision counter.
    #[must_use]
    #[inline]
    pub fn rev(&self) -> u64 {
        todo!()
    }

    /// The document schema version.
    #[must_use]
    #[inline]
    pub fn schema_version(&self) -> SchemaVersion {
        todo!()
    }

    /// The ruleset.
    #[must_use]
    #[inline]
    pub fn ruleset(&self) -> &RulesetId {
        todo!()
    }

    /// The plugin payload slot.
    #[must_use]
    #[inline]
    pub fn ext(&self) -> &Ext {
        todo!()
    }

    /// When the entity was created.
    #[must_use]
    #[inline]
    pub fn created_at(&self) -> UnixTime {
        todo!()
    }

    /// When the entity was last updated.
    #[must_use]
    #[inline]
    pub fn updated_at(&self) -> UnixTime {
        todo!()
    }

    /// The statblock entity, when the body is a statblock.
    #[must_use]
    #[inline]
    pub fn as_statblock(&self) -> Option<&Entity<StatblockBody>> {
        todo!()
    }

    /// The note entity, when the body is a note.
    #[must_use]
    #[inline]
    pub fn as_note(&self) -> Option<&Entity<NoteBody>> {
        todo!()
    }

    /// The handout entity, when the body is a handout.
    #[must_use]
    #[inline]
    pub fn as_handout(&self) -> Option<&Entity<HandoutBody>> {
        todo!()
    }
}

impl From<Entity<StatblockBody>> for AnyEntity {
    #[inline]
    fn from(entity: Entity<StatblockBody>) -> Self {
        todo!()
    }
}

impl From<Entity<NoteBody>> for AnyEntity {
    #[inline]
    fn from(entity: Entity<NoteBody>) -> Self {
        todo!()
    }
}

impl From<Entity<HandoutBody>> for AnyEntity {
    #[inline]
    fn from(entity: Entity<HandoutBody>) -> Self {
        todo!()
    }
}

impl Serialize for AnyEntity {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}

impl<'de> Deserialize<'de> for AnyEntity {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// 6. Statblock sections
// ---------------------------------------------------------------------------

/// Character or creature sheet; one body shape serves both.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatblockBody {
    /// Character or creature profile.
    pub profile: Profile,
    /// Identity box.
    pub identity: Identity,
    /// Ability scores.
    pub abilities: Abilities,
    /// Class levels; empty for creatures.
    pub levels: Vec<ClassLevel>,
    /// Challenge rating; creatures only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cr: Option<ChallengeRating>,
    /// Creature type, free text, e.g. `"fiend (demon)"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creature_type: Option<String>,
    /// Size category.
    pub size: Size,
    /// Senses, free text, display only.
    pub senses: String,
    /// Save and skill proficiencies, tools and languages.
    pub proficiencies: Proficiencies,
    /// Hit points.
    pub hp: Hp,
    /// Movement speeds.
    pub speed: Speed,
    /// Death save state.
    pub death_saves: DeathSaves,
    /// Exhaustion level (0 to 6 in 2014).
    pub exhaustion: u8,
    /// Inspiration flag.
    pub inspiration: bool,
    /// Coins.
    pub coins: Coins,
    /// Sparse overrides of derived values.
    pub overrides: BTreeMap<StatKey, OverrideValue>,
    /// Tracked resources.
    pub resources: Vec<Resource>,
    /// Spellcasting blocks.
    pub spellcasting: Vec<SpellcastingBlock>,
    /// Inventory entries.
    pub inventory: Vec<InventoryEntry>,
    /// Manual attack lines.
    pub attacks: Vec<AttackEntry>,
    /// Features.
    pub features: Vec<FeatureEntry>,
    /// Active statuses (conditions, effects, concentration).
    pub statuses: Vec<Status>,
    /// Free-text session notes; markdown, opaque to core.
    pub notes: String,
}

impl sealed::Sealed for StatblockBody {}

impl Body for StatblockBody {
    const BODY_KIND: BodyKind = BodyKind::Statblock;
}

/// Markdown note, opaque to core, size-capped by the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteBody {
    /// The note text.
    pub text: String,
}

impl sealed::Sealed for NoteBody {}

impl Body for NoteBody {
    const BODY_KIND: BodyKind = BodyKind::Note;
}

/// Handout payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoutBody {
    /// The handout content.
    pub content: HandoutContent,
}

impl sealed::Sealed for HandoutBody {}

impl Body for HandoutBody {
    const BODY_KIND: BodyKind = BodyKind::Handout;
}

/// A handout is either free text or an item definition ("give").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (RFC-0002 3.6); boxing the item variant is an API decision deferred to the implement phase"
)]
pub enum HandoutContent {
    /// Free text shown to the recipient.
    Text {
        /// The text.
        text: String,
    },
    /// The item lands in the recipient's inventory as their own copy.
    Item {
        /// The item definition.
        item: InlineItem,
    },
}

/// Identity box.
///
/// `Default` is the blank box (RFC-0002 9.9: the empty-able leaf sections get a
/// `Default` so an importer or builder can fill a statblock field by field).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Race, referenced or inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub race: Option<RefOrText>,
    /// Background, referenced or inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<RefOrText>,
    /// Alignment, free text.
    pub alignment: String,
}

/// The six ability scores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Abilities {
    /// Strength score.
    #[serde(rename = "str")]
    pub strength: i32,
    /// Dexterity score.
    #[serde(rename = "dex")]
    pub dexterity: i32,
    /// Constitution score.
    #[serde(rename = "con")]
    pub constitution: i32,
    /// Intelligence score.
    #[serde(rename = "int")]
    pub intelligence: i32,
    /// Wisdom score.
    #[serde(rename = "wis")]
    pub wisdom: i32,
    /// Charisma score.
    #[serde(rename = "cha")]
    pub charisma: i32,
}

/// One class level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassLevel {
    /// Element id.
    pub id: ClassLevelId,
    /// Class, referenced or inline.
    pub class: RefOrText,
    /// Level in this class.
    pub level: i32,
    /// Subclass, referenced or inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subclass: Option<RefOrText>,
}

/// Character or creature profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Character.
    Character,
    /// Creature.
    Creature,
}

/// Size category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    /// Tiny.
    Tiny,
    /// Small.
    Small,
    /// Medium.
    Medium,
    /// Large.
    Large,
    /// Huge.
    Huge,
    /// Gargantuan.
    Gargantuan,
}

impl Default for Size {
    /// Medium: the size of a typical player character (RFC-0002 9.9); a
    /// builder starts here and callers override it for other creatures.
    #[inline]
    fn default() -> Self {
        Self::Medium
    }
}

/// Challenge rating stored in eighths: 0, 1/8, 1/4, 1/2, 1 to 30 all fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChallengeRating(i32);

impl ChallengeRating {
    /// Builds a challenge rating from eighths.
    #[must_use]
    #[inline]
    pub fn from_eighths(e: i32) -> Self {
        todo!()
    }

    /// Returns the rating in eighths.
    #[must_use]
    #[inline]
    pub fn as_eighths(self) -> i32 {
        todo!()
    }

    /// Parses `"0"`, `"1/8"`, `"1/4"`, `"1/2"`, `"5"`, `"30"`.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, ChallengeRatingError> {
        todo!()
    }
}

impl fmt::Display for ChallengeRating {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Serialize for ChallengeRating {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}

impl<'de> Deserialize<'de> for ChallengeRating {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        todo!()
    }
}

/// Save and skill proficiencies, tools and languages.
///
/// `Default` is the empty set (RFC-0002 9.9).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proficiencies {
    /// Proficient save abilities.
    pub saves: BTreeSet<Ability>,
    /// Sparse skill proficiency map; absent means none.
    pub skills: BTreeMap<Skill, SkillProficiency>,
    /// Tool proficiencies, free-text lines.
    pub tools: String,
    /// Languages, free-text lines.
    pub languages: String,
}

/// Proficiency level in a skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProficiency {
    /// Normal proficiency.
    Proficient,
    /// Double proficiency bonus.
    Expertise,
}

/// Hit points; plain inputs, rules behavior lives in the engine.
///
/// `Default` is all-zero (RFC-0002 9.9). A zero maximum is a placeholder, not a
/// usable sheet: builders must set a real `max` before an entity is stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hp {
    /// Maximum hit points.
    pub max: i32,
    /// Current hit points; may go negative.
    pub current: i32,
    /// Temporary hit points.
    pub temp: i32,
}

/// Movement speeds in feet.
///
/// `Default` is all-zero with no special movement (RFC-0002 9.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speed {
    /// Walking speed.
    pub walk_ft: i32,
    /// Flying speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fly_ft: Option<i32>,
    /// Swimming speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swim_ft: Option<i32>,
    /// Climbing speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub climb_ft: Option<i32>,
    /// Burrowing speed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burrow_ft: Option<i32>,
    /// Whether the fly speed hovers.
    pub hover: bool,
}

/// Death save state, 0 to 3 each.
///
/// `Default` is zero successes and zero failures (RFC-0002 9.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathSaves {
    /// Successful death saves.
    pub successes: u8,
    /// Failed death saves.
    pub failures: u8,
}

/// Coin amounts.
///
/// `Default` is an empty purse (RFC-0002 9.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coins {
    /// Copper pieces.
    pub cp: i32,
    /// Silver pieces.
    pub sp: i32,
    /// Electrum pieces.
    pub ep: i32,
    /// Gold pieces.
    pub gp: i32,
    /// Platinum pieces.
    pub pp: i32,
}

/// Sparse override of a derived value; `override ?? computed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OverrideValue {
    /// Integer override.
    Int(i32),
    /// Boolean override.
    Bool(bool),
    /// Text override.
    Text(String),
}

// ---------------------------------------------------------------------------
// 7. Items and inventory
// ---------------------------------------------------------------------------

/// One inventory entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryEntry {
    /// Element id.
    pub id: InventoryEntryId,
    /// Referenced or inline item.
    pub item: InventoryItem,
    /// Quantity; signed, 0 is allowed and warned about by the engine.
    pub qty: i32,
    /// Carried, equipped or stowed.
    pub state: ItemState,
    /// Whether the entry is attuned.
    pub attuned: bool,
    /// Free-text notes.
    pub notes: String,
}

/// Either a reference to pack/SRD content or a full inline definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (RFC-0002 3.7); the pinned target test constructs Inline(InlineItem { .. }) inline, so boxing is deferred to the implement phase"
)]
pub enum InventoryItem {
    /// Name snapshot plus reference; description resolves via the source.
    Ref(ContentRef),
    /// Full inline definition.
    Inline(InlineItem),
}

/// Inventory state of an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    /// Carried but not worn or wielded.
    Carried,
    /// Worn or wielded.
    Equipped,
    /// Packed away.
    Stowed,
}

/// Full inline item definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineItem {
    /// Item name.
    pub name: String,
    /// Markdown description, opaque to core.
    pub description: String,
    /// Item category.
    pub kind: ItemKind,
    /// Whether the item requires attunement.
    pub requires_attunement: bool,
    /// Weight in ounces; 1 lb = 16 oz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_oz: Option<i32>,
    /// Value in coins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Coins>,
    /// Weapon/armor/shield properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<ItemProperties>,
    /// Modifiers while carried, equipped or attuned.
    pub modifiers: Vec<Modifier>,
    /// Charge definition; `current` is ignored until instantiated as a
    /// tracked copy in the statblock's resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
    /// Plugin payload slot.
    pub ext: Ext,
}

/// Item category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Weapon.
    Weapon,
    /// Armor.
    Armor,
    /// Shield.
    Shield,
    /// Wondrous item.
    Wondrous,
    /// Adventuring gear.
    Gear,
    /// Anything else.
    Other,
}

/// Category-specific item properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemProperties {
    /// Weapon properties.
    Weapon(WeaponProperties),
    /// Armor properties.
    Armor(ArmorProperties),
    /// Shield properties.
    Shield(ShieldProperties),
}

/// Weapon properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponProperties {
    /// Damage dice, e.g. `1d8`; `None` for utility weapons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<DiceExpr>,
    /// Damage type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<DamageType>,
    /// `(normal, long)` range in feet; reach for melee weapons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_ft: Option<(i32, i32)>,
    /// Versatile damage dice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versatile_damage: Option<DiceExpr>,
    /// Weapon tags.
    pub tags: Vec<WeaponTag>,
}

/// Armor properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmorProperties {
    /// Base armor class.
    pub base_ac: i32,
    /// Maximum Dexterity bonus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_dex_bonus: Option<i32>,
    /// Whether wearing it imposes Stealth disadvantage.
    pub stealth_disadvantage: bool,
    /// Strength requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength_requirement: Option<i32>,
}

/// Shield properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShieldProperties {
    /// Armor class bonus.
    pub ac_bonus: i32,
}

/// Weapon tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponTag {
    /// Finesse.
    Finesse,
    /// Light.
    Light,
    /// Heavy.
    Heavy,
    /// Two-handed.
    TwoHanded,
    /// Versatile.
    Versatile,
    /// Thrown.
    Thrown,
    /// Reach.
    Reach,
    /// Loading.
    Loading,
    /// Ammunition.
    Ammunition,
    /// Special.
    Special,
}

/// Damage types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    /// Acid.
    Acid,
    /// Bludgeoning.
    Bludgeoning,
    /// Cold.
    Cold,
    /// Fire.
    Fire,
    /// Force.
    Force,
    /// Lightning.
    Lightning,
    /// Necrotic.
    Necrotic,
    /// Piercing.
    Piercing,
    /// Poison.
    Poison,
    /// Psychic.
    Psychic,
    /// Radiant.
    Radiant,
    /// Slashing.
    Slashing,
    /// Thunder.
    Thunder,
}

// ---------------------------------------------------------------------------
// 8. Attacks, features, spellcasting
// ---------------------------------------------------------------------------

/// Manual attack line on the sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackEntry {
    /// Element id.
    pub id: AttackEntryId,
    /// Attack name.
    pub name: String,
    /// Displayed base bonus; computed suggestions are UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonus: Option<i32>,
    /// Free-text damage line, e.g. `"1d8+3 slashing"`.
    pub damage_text: String,
    /// Linked inventory entry for equip state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_inventory: Option<InventoryEntryId>,
}

/// Feature entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureEntry {
    /// Element id.
    pub id: FeatureEntryId,
    /// Feature category.
    pub category: FeatureCategory,
    /// Feature name.
    pub name: String,
    /// Content reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Inline description for homebrew features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Resource on this statblock that the feature spends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<ResourceId>,
    /// Modifiers while the feature is active.
    pub modifiers: Vec<Modifier>,
    /// Plugin payload slot.
    pub ext: Ext,
}

/// Feature category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureCategory {
    /// Class feature.
    Class,
    /// Racial trait.
    Race,
    /// Feat.
    Feat,
    /// Creature trait.
    Trait,
    /// Action.
    Action,
    /// Reaction.
    Reaction,
    /// Legendary action.
    Legendary,
    /// Anything else.
    Other,
}

/// One spellcasting block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellcastingBlock {
    /// Element id.
    pub id: SpellcastingBlockId,
    /// Class or other source, referenced or inline.
    pub source: RefOrText,
    /// Spellcasting ability.
    pub ability: Ability,
    /// Known and prepared spells.
    pub spells: Vec<SpellEntry>,
}

/// One spell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellEntry {
    /// Element id.
    pub id: SpellEntryId,
    /// Name snapshot for homebrew and referred spells.
    pub name: String,
    /// Content reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Whether the spell is prepared.
    pub prepared: bool,
}

// ---------------------------------------------------------------------------
// 9. Modifiers, formulas, conditions
// ---------------------------------------------------------------------------

/// A modifier contributed by an item, feature or status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifier {
    /// The stat (or group) the modifier applies to.
    pub target: StatKey,
    /// The operation, carrying its operand.
    #[serde(flatten)]
    pub op: ModifierOp,
    /// Condition that must hold for the modifier to apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<Condition>,
    /// Human-readable attribution, e.g. `"Bless"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The operation carries its operand, so "advantage with a value" is not
/// representable; flattened into [`Modifier`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ModifierOp {
    /// Add the value.
    Add {
        /// The addend.
        value: Formula,
    },
    /// Set the value.
    Set {
        /// The value to set.
        value: Formula,
    },
    /// Clamp the value to at least this.
    Min {
        /// The lower bound.
        value: Formula,
    },
    /// Clamp the value to at most this.
    Max {
        /// The upper bound.
        value: Formula,
    },
    /// Grant advantage.
    Advantage,
    /// Grant disadvantage.
    Disadvantage,
}

/// Small closed expression language, not a scripting language.
///
/// Untagged so the common case is a bare number: `2` or
/// `{"type":"proficiency_bonus"}`. Dice leaves only evaluate in roll
/// contexts; static contexts use the flat part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Formula {
    /// Constant shorthand.
    Const(i32),
    /// A tagged expression.
    Expr(FormulaExpr),
}

/// Tagged formula expressions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FormulaExpr {
    /// The proficiency bonus.
    ProficiencyBonus,
    /// The modifier of an ability.
    AbilityMod {
        /// The ability.
        ability: Ability,
    },
    /// Total character level.
    Level,
    /// Level in a class, matched by name snapshot.
    ClassLevel {
        /// The class name.
        class: String,
    },
    /// Addition.
    Add {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Subtraction.
    Sub {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Multiplication.
    Mul {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Minimum.
    Min {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Maximum.
    Max {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Floor division.
    FloorDiv {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Dice term, e.g. Bless's `1d4`.
    Dice {
        /// The dice expression.
        expr: DiceExpr,
    },
}

/// Closed condition set; engine matching is case-insensitive for names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// The item is equipped.
    Equipped,
    /// The item is attuned.
    Attuned,
    /// A status with this name snapshot is active.
    StatusActive {
        /// The status name.
        name: String,
    },
    /// A table-provided condition matched by name, e.g. "while raging"
    /// (RFC-0001 5.10): core stores the name, the engine decides the effects.
    Custom {
        /// The condition name.
        name: String,
    },
}

// ---------------------------------------------------------------------------
// 10. Resources and statuses
// ---------------------------------------------------------------------------

/// A tracked resource on a statblock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    /// Element id.
    pub id: ResourceId,
    /// Display name.
    pub name: String,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Maximum; a bare integer is allowed by the formula shorthand.
    pub max: Formula,
    /// Current value; signed, overspending is warned about by the engine.
    pub current: i32,
    /// Recharge trigger.
    pub recharge: Recharge,
}

/// Resource kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceKind {
    /// Spell slot of a level.
    SpellSlot {
        /// Spell level, 1 to 9.
        level: i32,
    },
    /// Warlock pact slot.
    PactSlot,
    /// Hit die.
    HitDie {
        /// Die sides, 4 to 12.
        sides: i32,
    },
    /// Generic feature resource.
    Feature,
    /// Item charge resource.
    Item,
    /// Anything else.
    Other,
}

/// Recharge trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recharge {
    /// On a short rest.
    Short,
    /// On a long rest.
    Long,
    /// At dawn.
    Dawn,
    /// Manually.
    Manual,
    /// Never.
    Never,
}

/// A status effect on an entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Element id.
    pub id: StatusId,
    /// Status kind.
    pub kind: StatusKind,
    /// Display name.
    pub name: String,
    /// Content reference, e.g. `srd2014:condition/prone`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Inline description for custom statuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// When the status started, on the campaign clock.
    pub started_at: CampaignTime,
    /// How the status ends.
    pub duration: StatusDuration,
    /// Who applied it and/or the concentration status it hangs off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<StatusSource>,
    /// Modifiers applied when the engine composes a value for this entity:
    /// static derived values use the flat part, rolls add dice terms
    /// attributed to this status.
    pub modifiers: Vec<Modifier>,
}

/// Status kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// A named condition such as prone.
    Condition,
    /// A spell or ability effect such as Bless.
    Effect,
    /// The one concentration effect an entity can hold.
    Concentration,
    /// Table-provided status.
    Custom,
}

/// How a status ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusDuration {
    /// Absolute expiry on the campaign clock: expired when `now >= expires_at`.
    Timed {
        /// Expiry time.
        expires_at: CampaignTime,
    },
    /// Cleared by a rest.
    UntilRest {
        /// The rest kind that clears it.
        rest: RestKind,
    },
    /// Never auto-expired.
    Indefinite,
    /// Cleared manually, with a note like "until the vault is opened".
    Manual {
        /// The reminder text.
        text: String,
    },
}

/// Rest kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestKind {
    /// Short rest.
    Short,
    /// Long rest.
    Long,
}

/// Who applied a status and/or the concentration status it hangs off; both
/// optional, absent means unknown or table-provided.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSource {
    /// The caster entity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// The concentration status this rider hangs off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusId>,
}

// ---------------------------------------------------------------------------
// 11. Dice, rolls, feed
// ---------------------------------------------------------------------------

/// Structured dice AST; part of the serialized [`Roll`] contract. Parser and
/// roller live in the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiceExpr {
    /// A constant term.
    Const {
        /// The value.
        value: i32,
    },
    /// A dice pool, e.g. `4d6kh3`.
    Roll {
        /// Number of dice.
        count: i32,
        /// Die sides.
        sides: i32,
        /// Keep rule.
        #[serde(skip_serializing_if = "Option::is_none")]
        keep: Option<Keep>,
    },
    /// Roll twice, keep the better result (usually the d20 term).
    Advantage {
        /// The inner expression.
        inner: Box<DiceExpr>,
    },
    /// Roll twice, keep the worse result.
    Disadvantage {
        /// The inner expression.
        inner: Box<DiceExpr>,
    },
    /// Additional term.
    Add {
        /// Left operand.
        lhs: Box<DiceExpr>,
        /// Right operand.
        rhs: Box<DiceExpr>,
    },
    /// Subtracted term.
    Sub {
        /// Left operand.
        lhs: Box<DiceExpr>,
        /// Right operand.
        rhs: Box<DiceExpr>,
    },
    /// Multiplied term.
    Mul {
        /// Left operand.
        lhs: Box<DiceExpr>,
        /// Right operand.
        rhs: Box<DiceExpr>,
    },
}

/// Keep rule for a dice pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Keep {
    /// Keep the highest dice.
    Highest {
        /// How many dice to keep.
        count: i32,
    },
    /// Keep the lowest dice.
    Lowest {
        /// How many dice to keep.
        count: i32,
    },
}

/// A persisted, replayable roll.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roll {
    /// Roll id.
    pub id: RollId,
    /// Who rolled.
    pub roller: PlayerId,
    /// The entity the roll was made for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// Wall-clock time.
    pub ts: UnixTime,
    /// The composed expression that was actually rolled.
    pub expr: DiceExpr,
    /// The result breakdown.
    pub result: DiceResult,
    /// What the roll was for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<RollContext>,
    /// Who sees the roll.
    pub audience: Audience,
}

/// Per-term and per-die breakdown, so frontends can render dice, detect nat 1
/// and nat 20 under advantage, and attribute composed terms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiceResult {
    /// The terms of the expression.
    pub terms: Vec<TermResult>,
    /// The summed total.
    pub total: i32,
}

/// Result of one term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermResult {
    /// The dice rolled for this term.
    pub dice: Vec<DieResult>,
    /// The summed term total.
    pub total: i32,
    /// Attribution label for a composed term, e.g. `"Bless"`; `None` for the
    /// base expression. A label snapshot, not an id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Result of one die.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DieResult {
    /// The rolled value.
    pub value: i32,
    /// Whether the die was kept by its keep rule.
    pub kept: bool,
}

/// What a roll was for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollContext {
    /// The roll category.
    pub kind: RollKind,
    /// The composed stat, e.g. `"skill.stealth"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stat: Option<StatKey>,
    /// The DC the roll was against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dc: Option<i32>,
}

/// Roll category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollKind {
    /// Ability or skill check.
    Check,
    /// Saving throw.
    Save,
    /// Attack roll.
    Attack,
    /// Damage roll.
    Damage,
    /// Anything else.
    Other,
}

/// Role-agnostic audience of a roll or feed entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// Visible to everyone.
    Public,
    /// Visible to the roller and the DM.
    RollerAndDm,
    /// Visible to the roller only.
    RollerOnly,
}

/// One entry of the persisted feed ring buffer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedEntry {
    /// Feed position.
    pub id: FeedEntryId,
    /// Wall-clock time.
    pub ts: UnixTime,
    /// Campaign-clock time.
    pub clock_s: CampaignTime,
    /// Acting player.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<PlayerId>,
    /// Related entity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// Who sees the entry.
    pub audience: Audience,
    /// What happened.
    pub payload: FeedPayload,
}

/// Feed payload kinds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedPayload {
    /// A roll; the entry audience mirrors the roll audience.
    Roll {
        /// The roll.
        roll: Roll,
    },
    /// A timed status expired.
    StatusExpired {
        /// The status.
        status: StatusId,
        /// Status name snapshot.
        name: String,
    },
    /// Concentration ended.
    ConcentrationEnded {
        /// The concentration status name snapshot.
        name: String,
    },
    /// A system hint, e.g. a suggested concentration check.
    Hint {
        /// The hint text.
        text: String,
    },
}

// ---------------------------------------------------------------------------
// 12. Errors
// ---------------------------------------------------------------------------

/// Failure building or deserializing an entity.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EntityError {
    /// `kind` and body belong to different families.
    #[error("kind {kind:?} cannot carry a {body_kind:?} body")]
    KindBodyMismatch {
        /// The entity kind.
        kind: EntityKind,
        /// The body family of the payload.
        body_kind: BodyKind,
    },
}

/// Failure parsing a [`RefId`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum RefIdError {
    /// The string is not a `<source>:<kind>/<slug>` reference.
    #[error("invalid reference id")]
    Invalid,
}

/// Failure parsing a [`RulesetId`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum RulesetIdError {
    /// The string is not a valid ruleset id.
    #[error("invalid ruleset id")]
    Invalid,
}

/// Failure parsing a [`Namespace`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum NamespaceError {
    /// The string is not a valid namespace.
    #[error("invalid namespace")]
    Invalid,
}

/// Failure parsing a [`StatKey`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum StatKeyError {
    /// The string is neither a canonical known stat nor a valid custom key.
    #[error("invalid stat key")]
    Invalid,
}

/// Failure parsing a [`ChallengeRating`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ChallengeRatingError {
    /// The string is not a challenge rating.
    #[error("invalid challenge rating")]
    Invalid,
}

/// Failure parsing an [`Ability`] abbreviation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("unknown ability abbreviation")]
pub struct AbilityError;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn id_wraps_a_uuid_and_serializes_transparently() {
        let raw = uuid(0x018f_2b3c_4d5e_7f80_9abc_def0_1234_5678);
        let id: EntityId = Id::from_uuid(raw);

        assert_eq!(id.as_uuid(), raw);
        assert_eq!(id, Id::from_uuid(raw));
        assert_ne!(id, Id::from_uuid(uuid(1)));
        assert_eq!(id.to_string(), raw.to_string());
        assert_eq!(
            serde_json::to_value(id).expect("id serializes"),
            json!(raw.to_string())
        );
        let back: EntityId =
            serde_json::from_value(json!(raw.to_string())).expect("id deserializes");
        assert_eq!(back, id);
    }

    #[test]
    fn id_is_a_v7_uuid_and_orders_and_hashes_by_it() {
        let fresh = EntityId::new();
        assert_eq!(fresh.as_uuid().get_version_num(), 7);

        let small: EntityId = Id::from_uuid(uuid(1));
        let big: EntityId = Id::from_uuid(uuid(2));
        assert!(small < big);
        assert_eq!(
            HashSet::from([small, big]).len(),
            2,
            "distinct ids hash apart"
        );
    }

    #[test]
    fn feed_entry_id_round_trips() {
        let id = FeedEntryId::new(41);
        assert_eq!(id.get(), 41);
        assert_eq!(serde_json::to_value(id).expect("serializes"), json!(41));
        assert_eq!(
            serde_json::from_value::<FeedEntryId>(json!(41)).expect("deserializes"),
            id
        );
    }

    #[test]
    fn time_newtypes_round_trip_as_seconds() {
        let campaign = CampaignTime::from_seconds(3600);
        assert_eq!(campaign.seconds(), 3600);
        assert_eq!(campaign.to_string(), "3600");
        assert_eq!(
            serde_json::to_value(campaign).expect("serializes"),
            json!(3600)
        );

        let wall = UnixTime::from_seconds(1_750_000_000);
        assert_eq!(wall.seconds(), 1_750_000_000);
        assert_eq!(wall.to_string(), "1750000000");
        assert_eq!(
            serde_json::to_value(wall).expect("serializes"),
            json!(1_750_000_000)
        );
    }

    #[test]
    fn schema_version_accepts_same_or_older_major() {
        let current = SchemaVersion::CURRENT;
        assert!(current.accepts(SchemaVersion {
            major: current.major,
            minor: u16::MAX
        }));
        assert!(current.accepts(SchemaVersion { major: 0, minor: 0 }));
        assert!(!current.accepts(SchemaVersion {
            major: current.major + 1,
            minor: 0
        }));
    }

    #[test]
    fn ref_id_parses_rejects_and_round_trips() {
        let r = RefId::parse("srd2014:spell/fireball").expect("valid ref");
        assert_eq!(r.namespace(), "srd2014");
        assert_eq!(r.kind(), "spell");
        assert_eq!(r.slug(), "fireball");
        assert_eq!(r.as_str(), "srd2014:spell/fireball");
        assert_eq!(r.to_string(), "srd2014:spell/fireball");
        assert_eq!(
            serde_json::to_value(&r).expect("serializes"),
            json!("srd2014:spell/fireball")
        );

        let pack = RefId::parse("pack.mytome:item/foo-bar").expect("valid pack ref");
        assert_eq!(pack.namespace(), "pack.mytome");
        assert_eq!(pack.slug(), "foo-bar");
        assert_eq!(
            serde_json::from_value::<RefId>(json!("pack.mytome:item/foo-bar"))
                .expect("deserializes"),
            pack
        );

        for bad in [
            "",
            "not-a-ref",
            "srd2014/spell/fireball",       // no colon
            "srd2014:spell",                // no slash
            "srd2014:spell/Fire Ball",      // uppercase and space
            "srd2014:spell/fireball/extra", // extra segment
            ":spell/fireball",
            "srd2014:/fireball",
            "srd2014:spell/",
            "pack..mytome:item/foo",
        ] {
            assert!(RefId::parse(bad).is_err(), "{bad:?} must be rejected");
            assert!(
                serde_json::from_value::<RefId>(json!(bad)).is_err(),
                "{bad:?} must be rejected on deserialize too"
            );
        }
    }

    #[test]
    fn stat_key_round_trips_byte_identically() {
        for raw in [
            "ac",
            "initiative",
            "speed",
            "proficiency_bonus",
            "ability.str",
            "ability_mod.dex",
            "save.wis",
            "skill.sleight_of_hand",
            "passive.perception",
            "attack.melee",
            "spell.dc",
            "check",
            "save",
            "attack",
            "house.luck",
        ] {
            let key = StatKey::parse(raw).unwrap_or_else(|_| panic!("{raw:?} parses"));
            assert_eq!(key.to_string(), raw);
            assert_eq!(serde_json::to_value(&key).expect("serializes"), json!(raw));
            assert_eq!(
                serde_json::from_value::<StatKey>(json!(raw)).expect("deserializes"),
                key
            );
        }

        assert_eq!(
            StatKey::parse("skill.stealth").expect("known"),
            StatKey::Known(KnownStat::Skill(Skill::Stealth))
        );
        assert!(matches!(
            StatKey::parse("house.luck").expect("custom"),
            StatKey::Custom(_)
        ));
    }

    #[test]
    fn stat_key_rejects_malformed_custom_keys_at_the_length_boundary() {
        for bad in ["", "Bad Key!", "House.Luck", "house..luck", "lucky!"] {
            assert!(StatKey::parse(bad).is_err(), "{bad:?} must be rejected");
        }

        let mut longest = "a.".repeat(31);
        longest.push_str("aa");
        assert_eq!(longest.len(), 64);
        assert!(StatKey::parse(&longest).is_ok());
        assert!(StatKey::parse(&format!("{longest}a")).is_err());
    }
}
