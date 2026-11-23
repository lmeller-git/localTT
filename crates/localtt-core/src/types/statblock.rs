//! Statblock body sections: identity, abilities, hit points, coins and overrides.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    hash::Hash,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{
    Ability,
    AttackEntry,
    Body,
    BodyKind,
    ChallengeRatingError,
    ClassLevelId,
    FeatureEntry,
    InventoryEntry,
    RefOrText,
    Resource,
    Skill,
    SpellcastingBlock,
    StatKey,
    Status,
    entity::sealed,
};

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

/// Identity box.
///
/// `Default` is the blank box (docs/decisions.md DATA-10: the empty-able leaf sections get a
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
    /// Medium: the size of a typical player character (docs/decisions.md DATA-10); a
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
    /// Builds a challenge rating from eighths, rejecting fractions 5e does not
    /// define.
    ///
    /// The valid inputs are 0, 1 (1/8), 2 (1/4), 4 (1/2) and the multiples of
    /// 8 from 8 (1) through 240 (30) - the same set [`Self::parse`] accepts, so
    /// every constructed rating round-trips through its string form.
    #[inline]
    pub fn from_eighths(e: i32) -> Result<Self, ChallengeRatingError> {
        if matches!(e, 0 | 1 | 2 | 4) || (e % 8 == 0 && (8..=240).contains(&e)) {
            Ok(Self(e))
        } else {
            Err(ChallengeRatingError::Invalid)
        }
    }

    /// Returns the rating in eighths.
    #[must_use]
    #[inline]
    pub fn as_eighths(self) -> i32 {
        self.0
    }

    /// Parses `"0"`, `"1/8"`, `"1/4"`, `"1/2"`, `"5"`, `"30"`.
    ///
    /// Anything else, including a fraction 5e does not define, is
    /// [`ChallengeRatingError::Invalid`].
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, ChallengeRatingError> {
        match raw {
            "0" => Self::from_eighths(0),
            "1/8" => Self::from_eighths(1),
            "1/4" => Self::from_eighths(2),
            "1/2" => Self::from_eighths(4),
            _ => {
                let whole: i32 = raw.parse().map_err(|_| ChallengeRatingError::Invalid)?;
                // `str::parse` also accepts a leading `+` and leading zeros;
                // reject them so `parse` stays the exact inverse of `Display`.
                let canonical = !raw.starts_with('0') && raw.bytes().all(|b| b.is_ascii_digit());
                if whole <= 0 || whole > 30 || !canonical {
                    return Err(ChallengeRatingError::Invalid);
                }
                Self::from_eighths(whole * 8)
            }
        }
    }
}

impl fmt::Display for ChallengeRating {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let eighths = self.0;
        if eighths % 8 == 0 {
            return write!(f, "{}", eighths / 8);
        }
        let divisor = gcd(eighths.unsigned_abs(), 8);
        write!(f, "{}/{}", eighths / divisor as i32, 8 / divisor)
    }
}

impl Serialize for ChallengeRating {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ChallengeRating {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Greatest common divisor, for reducing an eighth fraction.
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Save and skill proficiencies, tools and languages.
///
/// `Default` is the empty set (docs/decisions.md DATA-10).
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
/// `Default` is all-zero (docs/decisions.md DATA-10). A zero maximum is a placeholder, not a
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
/// `Default` is all-zero with no special movement (docs/decisions.md DATA-10).
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
/// `Default` is zero successes and zero failures (docs/decisions.md DATA-10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathSaves {
    /// Successful death saves.
    pub successes: u8,
    /// Failed death saves.
    pub failures: u8,
}

/// Coin amounts.
///
/// `Default` is an empty purse (docs/decisions.md DATA-10).
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
