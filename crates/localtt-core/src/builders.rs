//! Ergonomic constructors and builders over the RFC-0002 data model.
//!
//! Everything here is pure data assembly: no rules, no I/O. The builders live
//! in their own module (not in [`crate::types`]) so the versioned wire contract
//! stays one file and this convenience layer can grow independently
//! (RFC-0003 sections 4 and 5.2).
#![expect(
    clippy::todo,
    unused_variables,
    dead_code,
    reason = "skeleton phase: every body is `todo!()`, so parameters and private builder fields are not read yet; removed in the implement phase"
)]

use crate::types::{
    Abilities,
    Ability,
    Campaign,
    ChallengeRating,
    Coins,
    Condition,
    ContentRef,
    DiceExpr,
    FeatureEntry,
    Formula,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    Modifier,
    OverrideValue,
    PlayerId,
    RefId,
    RefOrText,
    Resource,
    RulesetId,
    Size,
    Skill,
    SkillProficiency,
    Speed,
    SpellEntry,
    StatKey,
    StatblockBody,
    Status,
};

// ---------------------------------------------------------------------------
// Constructor sugar on the data types
// ---------------------------------------------------------------------------

impl Abilities {
    /// Every ability set to the same score.
    ///
    /// The explicit alternative to a made-up `Default` (RFC-0002 9.9): a silent
    /// 10 in every ability is exactly the kind of "helpful" default that
    /// corrupts a sheet.
    #[must_use]
    #[inline]
    pub fn all(score: i32) -> Self {
        todo!()
    }
}

impl RulesetId {
    /// The default 5e (2014) ruleset, already parsed.
    #[must_use]
    #[inline]
    pub fn dnd5e_2014() -> Self {
        todo!()
    }
}

impl Formula {
    /// The proficiency bonus leaf, `proficiency_bonus`.
    #[must_use]
    #[inline]
    pub fn proficiency_bonus() -> Self {
        todo!()
    }

    /// The ability modifier leaf, `ability_mod`.
    #[must_use]
    #[inline]
    pub fn ability_mod(ability: Ability) -> Self {
        todo!()
    }

    /// A dice leaf, e.g. Bless's `1d4`.
    #[must_use]
    #[inline]
    pub fn dice(expr: DiceExpr) -> Self {
        todo!()
    }
}

impl Modifier {
    /// An unconditional `Add` modifier on `target`.
    #[must_use]
    #[inline]
    pub fn add(target: StatKey, value: Formula) -> Self {
        todo!()
    }

    /// An `Add` modifier that applies only while `when` holds.
    #[must_use]
    #[inline]
    pub fn add_when(target: StatKey, value: Formula, when: Condition) -> Self {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Campaign builder
// ---------------------------------------------------------------------------

/// Ergonomic assembly of a [`Campaign`]; fills the current schema version and
/// the model defaults, and owns the player roster.
#[must_use]
pub struct CampaignBuilder {
    campaign: Campaign,
}

impl CampaignBuilder {
    /// Starts a campaign with a fresh id, [`crate::types::SchemaVersion::CURRENT`],
    /// the 5e (2014) ruleset, a clock at zero and default settings.
    #[inline]
    pub fn new(name: impl Into<String>) -> Self {
        todo!()
    }

    /// Sets the default ruleset for new entities.
    #[inline]
    pub fn ruleset(&mut self, r: RulesetId) -> &mut Self {
        todo!()
    }

    /// Sets the hour of day (0 to 23) at which Dawn recharges happen.
    #[inline]
    pub fn dawn_hour(&mut self, hour: u8) -> &mut Self {
        todo!()
    }

    /// Sets the feed ring-buffer capacity.
    #[inline]
    pub fn feed_capacity(&mut self, capacity: u32) -> &mut Self {
        todo!()
    }

    /// Adds a DM slot and returns its id.
    #[inline]
    pub fn dm(&mut self, display_name: impl Into<String>) -> PlayerId {
        todo!()
    }

    /// Adds a player slot and returns its id.
    #[inline]
    pub fn player(&mut self, display_name: impl Into<String>) -> PlayerId {
        todo!()
    }

    /// Finishes the campaign.
    #[must_use]
    #[inline]
    pub fn build(self) -> Campaign {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Statblock builder
// ---------------------------------------------------------------------------

/// Ergonomic assembly of a [`StatblockBody`].
///
/// Methods take `&mut self` so an entry id can be handed back mid-chain
/// ([`Self::item`]) and reused by a later [`Self::attack`]; the consuming style
/// of [`crate::types::Entity::builder`] cannot do that.
///
/// The `character`/`creature` constructors take a display name for convenience,
/// but [`StatblockBody`] has no name field: the name is only stored on the
/// entity (`Entity::builder`). See the friction note in RFC-0003.
#[must_use]
pub struct StatblockBuilder {
    body: StatblockBody,
}

impl StatblockBuilder {
    /// Starts a character statblock (`Profile::Character`).
    #[inline]
    pub fn character(name: impl Into<String>) -> Self {
        todo!()
    }

    /// Starts a creature statblock (`Profile::Creature`) with a challenge rating.
    #[inline]
    pub fn creature(name: impl Into<String>, cr: ChallengeRating) -> Self {
        todo!()
    }

    /// Sets the size category.
    #[inline]
    pub fn size(&mut self, size: Size) -> &mut Self {
        todo!()
    }

    /// Sets the ability scores.
    #[inline]
    pub fn abilities(&mut self, abilities: Abilities) -> &mut Self {
        todo!()
    }

    /// Appends a class level.
    #[inline]
    pub fn class_level(
        &mut self,
        class: RefOrText,
        level: i32,
        subclass: Option<RefOrText>,
    ) -> &mut Self {
        todo!()
    }

    /// Marks a saving throw as proficient.
    #[inline]
    pub fn save_proficiency(&mut self, ability: Ability) -> &mut Self {
        todo!()
    }

    /// Sets the proficiency level of a skill.
    #[inline]
    pub fn skill(&mut self, skill: Skill, level: SkillProficiency) -> &mut Self {
        todo!()
    }

    /// Sets the hit-point maximum and current value.
    #[inline]
    pub fn hp(&mut self, max: i32, current: i32) -> &mut Self {
        todo!()
    }

    /// Sets the movement speeds.
    #[inline]
    pub fn speed(&mut self, speed: Speed) -> &mut Self {
        todo!()
    }

    /// Sets the free-text senses box.
    #[inline]
    pub fn senses(&mut self, text: impl Into<String>) -> &mut Self {
        todo!()
    }

    /// Sets the coin purse.
    #[inline]
    pub fn coins(&mut self, coins: Coins) -> &mut Self {
        todo!()
    }

    /// Adds a sparse override of a derived value; `override ?? computed`
    /// (RFC-0001 2.3).
    #[inline]
    pub fn override_stat(&mut self, key: StatKey, value: OverrideValue) -> &mut Self {
        todo!()
    }

    /// Appends a tracked resource.
    #[inline]
    pub fn resource(&mut self, resource: Resource) -> &mut Self {
        todo!()
    }

    /// Appends a spellcasting block.
    #[inline]
    pub fn spellcasting_block(
        &mut self,
        source: RefOrText,
        ability: Ability,
        spells: Vec<SpellEntry>,
    ) -> &mut Self {
        todo!()
    }

    /// Appends an inventory entry and returns its id.
    #[inline]
    pub fn item(
        &mut self,
        item: InventoryItem,
        qty: i32,
        state: ItemState,
        attuned: bool,
    ) -> InventoryEntryId {
        todo!()
    }

    /// Appends a manual attack line, optionally linked to an inventory entry.
    #[inline]
    pub fn attack(
        &mut self,
        name: impl Into<String>,
        bonus: i32,
        damage_text: impl Into<String>,
        from_inventory: Option<InventoryEntryId>,
    ) -> &mut Self {
        todo!()
    }

    /// Appends a feature.
    #[inline]
    pub fn feature(&mut self, feature: FeatureEntry) -> &mut Self {
        todo!()
    }

    /// Appends a status (condition, effect or concentration).
    #[inline]
    pub fn status(&mut self, status: Status) -> &mut Self {
        todo!()
    }

    /// Sets the free-text notes box.
    #[inline]
    pub fn notes(&mut self, text: impl Into<String>) -> &mut Self {
        todo!()
    }

    /// Finishes the statblock body.
    #[must_use]
    #[inline]
    pub fn build(self) -> StatblockBody {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Literal helpers
// ---------------------------------------------------------------------------

/// Builds a [`ContentRef`] from a literal id and name.
///
/// Panics on a bad literal: test scaffolding, not library code.
#[must_use]
#[inline]
pub fn content_ref(id: &str, name: &str) -> ContentRef {
    todo!()
}

/// Parses a [`StatKey`] from a literal, panicking on a bad key.
#[must_use]
#[inline]
pub fn stat(raw: &str) -> StatKey {
    todo!()
}

/// Parses a [`RefId`] from a literal, panicking on a bad reference.
#[must_use]
#[inline]
pub fn ref_id(raw: &str) -> RefId {
    todo!()
}

/// Builds a plain dice pool, e.g. `dice(1, 4)` for `1d4`.
#[must_use]
#[inline]
pub fn dice(count: i32, sides: i32) -> DiceExpr {
    todo!()
}
