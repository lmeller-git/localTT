//! Abilities, skills and the stat keys that modifiers and overrides address.

use std::{fmt, hash::Hash, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{AbilityError, StatKeyError, refs::is_segment};

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

impl CustomStatKey {
    /// The custom stat path.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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
pub(super) fn custom_stat_key(raw: &str) -> Result<CustomStatKey, StatKeyError> {
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
