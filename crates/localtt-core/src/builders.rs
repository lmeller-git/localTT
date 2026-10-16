//! Ergonomic constructors and builders over the core data model (`DESIGN.md`).
//!
//! Everything here is pure data assembly: no rules, no I/O. The builders live
//! in their own module (not in [`crate::types`]) so the versioned wire contract
//! stays one file and this convenience layer can grow independently
//! (`DESIGN.md` section 8).
//!
//! # Two builder idioms, deliberately
//!
//! [`crate::types::EntityBuilder`] is *consuming* (`fn ruleset(self) -> Self`):
//! it is fully fluent and never yields anything but `Self` until `build`.
//! [`CampaignBuilder`] and [`StatblockBuilder`] take `&mut self` because some
//! methods must also hand back a product mid-chain ([`CampaignBuilder::dm`]
//! returns a `PlayerId`, [`StatblockBuilder::item`] returns an
//! `InventoryEntryId`). A consuming chain cannot both stay fluent and yield
//! that product without tuple gymnastics, so the split is principled rather
//! than accidental.

use std::collections::BTreeMap;

use crate::types::{
    Abilities,
    Ability,
    AttackEntry,
    AttackEntryId,
    Audience,
    Campaign,
    CampaignId,
    CampaignSettings,
    CampaignTime,
    ChallengeRating,
    ClassLevel,
    ClassLevelId,
    Clock,
    Coins,
    Condition,
    ContentRef,
    DeathSaves,
    DiceExpr,
    Entity,
    EntityBuilder,
    EntityError,
    EntityId,
    EntityKind,
    Ext,
    FeatureEntry,
    Formula,
    FormulaExpr,
    Hp,
    Identity,
    InlineItem,
    InventoryEntry,
    InventoryEntryId,
    InventoryItem,
    ItemKind,
    ItemState,
    Modifier,
    ModifierOp,
    OverrideValue,
    Player,
    PlayerId,
    Proficiencies,
    Profile,
    RefId,
    RefIdError,
    RefOrText,
    Resource,
    Role,
    RulesetId,
    SchemaVersion,
    Size,
    Skill,
    SkillProficiency,
    Speed,
    SpellEntry,
    SpellcastingBlock,
    SpellcastingBlockId,
    StatKey,
    StatKeyError,
    StatblockBody,
    StatblockEntity,
    Status,
    UnixTime,
};

// ---------------------------------------------------------------------------
// Constructor sugar on the data types
// ---------------------------------------------------------------------------

impl Abilities {
    /// Every ability set to the same score.
    ///
    /// The explicit alternative to a made-up `Default` (docs/decisions.md
    /// DATA-10): a silent
    /// 10 in every ability is exactly the kind of "helpful" default that
    /// corrupts a sheet.
    #[must_use]
    #[inline]
    pub fn all(score: i32) -> Self {
        Self {
            strength: score,
            dexterity: score,
            constitution: score,
            intelligence: score,
            wisdom: score,
            charisma: score,
        }
    }
}

impl RulesetId {
    /// The default 5e (2014) ruleset, already parsed.
    #[must_use]
    #[inline]
    pub fn dnd5e_2014() -> Self {
        Self::parse(Self::DND5E_2014).expect("the default ruleset id is a valid constant")
    }
}

impl Formula {
    /// The proficiency bonus leaf, `proficiency_bonus`.
    #[must_use]
    #[inline]
    pub fn proficiency_bonus() -> Self {
        Self::Expr(FormulaExpr::ProficiencyBonus)
    }

    /// The ability modifier leaf, `ability_mod`.
    #[must_use]
    #[inline]
    pub fn ability_mod(ability: Ability) -> Self {
        Self::Expr(FormulaExpr::AbilityMod { ability })
    }

    /// A dice leaf, e.g. Bless's `1d4`.
    #[must_use]
    #[inline]
    pub fn dice(expr: DiceExpr) -> Self {
        Self::Expr(FormulaExpr::Dice { expr })
    }
}

impl Modifier {
    /// An unconditional `Add` modifier on `target`.
    #[must_use]
    #[inline]
    pub fn add(target: StatKey, value: Formula) -> Self {
        Self {
            target,
            op: ModifierOp::Add { value },
            when: None,
            note: None,
        }
    }

    /// An `Add` modifier that applies only while `when` holds.
    #[must_use]
    #[inline]
    pub fn add_when(target: StatKey, value: Formula, when: Condition) -> Self {
        Self {
            target,
            op: ModifierOp::Add { value },
            when: Some(when),
            note: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Campaign builder
// ---------------------------------------------------------------------------

/// Ergonomic assembly of a [`Campaign`]; fills the current schema version and
/// the model defaults, and owns the player roster.
///
/// `&mut self` methods per the module note; [`Self::dm`] and [`Self::player`]
/// return the id of the slot they add, so the chain cannot be consuming.
#[must_use]
pub struct CampaignBuilder {
    campaign: Campaign,
}

impl CampaignBuilder {
    /// Starts a campaign with a fresh id, [`crate::types::SchemaVersion::CURRENT`],
    /// the 5e (2014) ruleset, a clock at zero and default settings.
    #[inline]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            campaign: Campaign {
                id: CampaignId::new(),
                name: name.into(),
                schema_version: SchemaVersion::CURRENT,
                default_ruleset: RulesetId::dnd5e_2014(),
                clock: Clock {
                    now_s: CampaignTime::from_seconds(0),
                },
                settings: CampaignSettings {
                    player_roll_audience_default: Audience::RollerAndDm,
                    feed_capacity: 1000,
                    dawn_hour: 6,
                },
                players: Vec::new(),
                ext: Ext::new(),
            },
        }
    }

    /// Sets the default ruleset for new entities.
    #[inline]
    pub fn ruleset(&mut self, r: RulesetId) -> &mut Self {
        self.campaign.default_ruleset = r;
        self
    }

    /// Sets the hour of day (0 to 23) at which Dawn recharges happen.
    #[inline]
    pub fn dawn_hour(&mut self, hour: u8) -> &mut Self {
        self.campaign.settings.dawn_hour = hour;
        self
    }

    /// Sets the feed ring-buffer capacity.
    #[inline]
    pub fn feed_capacity(&mut self, capacity: u32) -> &mut Self {
        self.campaign.settings.feed_capacity = capacity;
        self
    }

    /// Adds a participant with an explicit [`Role`] and returns its id.
    ///
    /// The general form; [`Self::dm`] and [`Self::player`] are the two
    /// shorthands the common flows use.
    #[inline]
    pub fn participant(&mut self, display_name: impl Into<String>, role: Role) -> PlayerId {
        let id = PlayerId::new();
        self.campaign.players.push(Player {
            id,
            display_name: display_name.into(),
            role,
            token_hash: None,
            created_at: UnixTime::from_seconds(0),
            last_seen: None,
        });
        id
    }

    /// Adds a DM slot and returns its id.
    #[inline]
    pub fn dm(&mut self, display_name: impl Into<String>) -> PlayerId {
        self.participant(display_name, Role::Dm)
    }

    /// Adds a player slot and returns its id.
    #[inline]
    pub fn player(&mut self, display_name: impl Into<String>) -> PlayerId {
        self.participant(display_name, Role::Player)
    }

    /// Finishes the campaign.
    #[must_use]
    #[inline]
    pub fn build(self) -> Campaign {
        self.campaign
    }
}

// ---------------------------------------------------------------------------
// Statblock builder
// ---------------------------------------------------------------------------

/// Ergonomic assembly of a [`StatblockBody`].
///
/// # Why the building is explicit
///
/// [`StatblockBody`] has no `Default` (docs/decisions.md DATA-10) and holds two sections a
/// silent default would corrupt: `abilities` and `hp`. The builder therefore
/// tracks whether they were set and [`Self::build`] returns
/// [`BuildError::MissingAbilities`]/[`BuildError::MissingHp`] rather than
/// fabricating a usable-looking sheet.
///
/// # Shape and drift
///
/// The builder *holds* a [`StatblockBody`], so it does not re-declare the
/// section fields; what it adds is one optional convenience setter per section.
/// That is inherent to a builder for a struct this wide, and it does not
/// duplicate [`crate::types::EntityBuilder`] (only the body lives here; the
/// header is composed in [`Self::build_entity`]). If the setter list grows
/// unwieldy, a declarative setter macro is the implement-phase remedy.
#[must_use]
pub struct StatblockBuilder {
    body: StatblockBody,
    abilities_set: bool,
    hp_set: bool,
}

impl StatblockBuilder {
    /// Starts a character statblock (`Profile::Character`), size Medium.
    #[inline]
    pub fn character() -> Self {
        Self {
            body: Self::blank(Profile::Character),
            abilities_set: false,
            hp_set: false,
        }
    }

    /// Starts a creature statblock (`Profile::Creature`) at a challenge rating.
    #[inline]
    pub fn creature(cr: ChallengeRating) -> Self {
        let mut body = Self::blank(Profile::Creature);
        body.cr = Some(cr);
        Self {
            body,
            abilities_set: false,
            hp_set: false,
        }
    }

    /// A sheet with every section empty and the required ones unset.
    fn blank(profile: Profile) -> StatblockBody {
        StatblockBody {
            profile,
            identity: Identity::default(),
            abilities: Abilities::all(0),
            levels: Vec::new(),
            cr: None,
            creature_type: None,
            size: Size::Medium,
            senses: String::new(),
            proficiencies: Proficiencies::default(),
            hp: Hp {
                max: 0,
                current: 0,
                temp: 0,
            },
            speed: Speed {
                walk_ft: 30,
                fly_ft: None,
                swim_ft: None,
                climb_ft: None,
                burrow_ft: None,
                hover: false,
            },
            death_saves: DeathSaves {
                successes: 0,
                failures: 0,
            },
            exhaustion: 0,
            inspiration: false,
            coins: Coins {
                cp: 0,
                sp: 0,
                ep: 0,
                gp: 0,
                pp: 0,
            },
            overrides: BTreeMap::new(),
            resources: Vec::new(),
            spellcasting: Vec::new(),
            inventory: Vec::new(),
            attacks: Vec::new(),
            features: Vec::new(),
            statuses: Vec::new(),
            notes: String::new(),
        }
    }

    /// Sets the size category.
    #[inline]
    pub fn size(&mut self, size: Size) -> &mut Self {
        self.body.size = size;
        self
    }

    /// Sets the ability scores. Required before `build`.
    #[inline]
    pub fn abilities(&mut self, abilities: Abilities) -> &mut Self {
        self.body.abilities = abilities;
        self.abilities_set = true;
        self
    }

    /// Sets the hit-point maximum and current value. Required before `build`.
    #[inline]
    pub fn hp(&mut self, max: i32, current: i32) -> &mut Self {
        self.body.hp.max = max;
        self.body.hp.current = current;
        self.hp_set = true;
        self
    }

    /// Sets the whole identity box (race, background, alignment).
    #[inline]
    pub fn identity(&mut self, identity: Identity) -> &mut Self {
        self.body.identity = identity;
        self
    }

    /// Sets the tool proficiencies box.
    #[inline]
    pub fn tools(&mut self, text: impl Into<String>) -> &mut Self {
        self.body.proficiencies.tools = text.into();
        self
    }

    /// Sets the languages box.
    #[inline]
    pub fn languages(&mut self, text: impl Into<String>) -> &mut Self {
        self.body.proficiencies.languages = text.into();
        self
    }

    /// Sets the creature type, e.g. `"fiend (demon)"`; creatures only.
    #[inline]
    pub fn creature_type(&mut self, text: impl Into<String>) -> &mut Self {
        self.body.creature_type = Some(text.into());
        self
    }

    /// Sets the death save state.
    #[inline]
    pub fn death_saves(&mut self, successes: u8, failures: u8) -> &mut Self {
        self.body.death_saves = DeathSaves {
            successes,
            failures,
        };
        self
    }

    /// Sets the exhaustion level (0 to 6 in 2014).
    #[inline]
    pub fn exhaustion(&mut self, level: u8) -> &mut Self {
        self.body.exhaustion = level;
        self
    }

    /// Sets the inspiration flag.
    #[inline]
    pub fn inspiration(&mut self, inspired: bool) -> &mut Self {
        self.body.inspiration = inspired;
        self
    }

    /// Sets the challenge rating (also set by [`Self::creature`]).
    #[inline]
    pub fn cr(&mut self, cr: ChallengeRating) -> &mut Self {
        self.body.cr = Some(cr);
        self
    }

    /// Appends a class level, minting a fresh element id.
    #[inline]
    pub fn class_level(
        &mut self,
        class: RefOrText,
        level: i32,
        subclass: Option<RefOrText>,
    ) -> &mut Self {
        self.class_level_with_id(ClassLevelId::new(), class, level, subclass)
    }

    /// Appends a class level with a caller-supplied element id, so a fixture
    /// can build the same body byte-for-byte on every run (docs/decisions.md
    /// BLD-3).
    #[inline]
    pub fn class_level_with_id(
        &mut self,
        id: ClassLevelId,
        class: RefOrText,
        level: i32,
        subclass: Option<RefOrText>,
    ) -> &mut Self {
        self.body.levels.push(ClassLevel {
            id,
            class,
            level,
            subclass,
        });
        self
    }

    /// Marks a saving throw as proficient.
    #[inline]
    pub fn save_proficiency(&mut self, ability: Ability) -> &mut Self {
        self.body.proficiencies.saves.insert(ability);
        self
    }

    /// Sets the proficiency level of a skill.
    #[inline]
    pub fn skill(&mut self, skill: Skill, level: SkillProficiency) -> &mut Self {
        self.body.proficiencies.skills.insert(skill, level);
        self
    }

    /// Sets the movement speeds.
    #[inline]
    pub fn speed(&mut self, speed: Speed) -> &mut Self {
        self.body.speed = speed;
        self
    }

    /// Sets the free-text senses box.
    #[inline]
    pub fn senses(&mut self, text: impl Into<String>) -> &mut Self {
        self.body.senses = text.into();
        self
    }

    /// Sets the coin purse.
    #[inline]
    pub fn coins(&mut self, coins: Coins) -> &mut Self {
        self.body.coins = coins;
        self
    }

    /// Adds a sparse override of a derived value; `override ?? computed`
    /// (RFC-0001 2.3).
    #[inline]
    pub fn override_stat(&mut self, key: StatKey, value: OverrideValue) -> &mut Self {
        self.body.overrides.insert(key, value);
        self
    }

    /// Appends a tracked resource.
    #[inline]
    pub fn resource(&mut self, resource: Resource) -> &mut Self {
        self.body.resources.push(resource);
        self
    }

    /// Appends a spellcasting block, minting a fresh element id.
    #[inline]
    pub fn spellcasting_block(
        &mut self,
        source: RefOrText,
        ability: Ability,
        spells: Vec<SpellEntry>,
    ) -> &mut Self {
        self.spellcasting_block_with_id(SpellcastingBlockId::new(), source, ability, spells)
    }

    /// Appends a spellcasting block with a caller-supplied element id.
    #[inline]
    pub fn spellcasting_block_with_id(
        &mut self,
        id: SpellcastingBlockId,
        source: RefOrText,
        ability: Ability,
        spells: Vec<SpellEntry>,
    ) -> &mut Self {
        self.body.spellcasting.push(SpellcastingBlock {
            id,
            source,
            ability,
            spells,
        });
        self
    }

    /// Appends an inventory entry with a fresh element id and returns it.
    #[inline]
    pub fn item(
        &mut self,
        item: InventoryItem,
        qty: i32,
        state: ItemState,
        attuned: bool,
    ) -> InventoryEntryId {
        self.item_with_id(InventoryEntryId::new(), item, qty, state, attuned)
    }

    /// Appends an inventory entry with a caller-supplied element id and
    /// returns it.
    #[inline]
    pub fn item_with_id(
        &mut self,
        id: InventoryEntryId,
        item: InventoryItem,
        qty: i32,
        state: ItemState,
        attuned: bool,
    ) -> InventoryEntryId {
        self.body.inventory.push(InventoryEntry {
            id,
            item,
            qty,
            state,
            attuned,
            notes: String::new(),
        });
        id
    }

    /// Appends a bare inline item (empty description, no properties) and
    /// returns its id; the common "just carried it" case.
    ///
    /// `attuned` is whether the entry is currently attuned; `requires_attunement`
    /// is a property of the item itself. They are separate so a caller can hand
    /// over an item the sheet could attune to later.
    #[inline]
    pub fn item_inline(
        &mut self,
        name: impl Into<String>,
        kind: ItemKind,
        state: ItemState,
        attuned: bool,
        requires_attunement: bool,
    ) -> InventoryEntryId {
        self.item(
            InventoryItem::Inline(InlineItem {
                name: name.into(),
                description: String::new(),
                kind,
                requires_attunement,
                weight_oz: None,
                value: None,
                properties: None,
                modifiers: Vec::new(),
                resource: None,
                ext: Ext::new(),
            }),
            1,
            state,
            attuned,
        )
    }

    /// Appends a manual attack line with a fresh element id.
    ///
    /// `bonus` is optional to mirror [`crate::types::AttackEntry::bonus`].
    #[inline]
    pub fn attack(
        &mut self,
        name: impl Into<String>,
        bonus: Option<i32>,
        damage_text: impl Into<String>,
        from_inventory: Option<InventoryEntryId>,
    ) -> &mut Self {
        self.attack_with_id(
            AttackEntryId::new(),
            name,
            bonus,
            damage_text,
            from_inventory,
        )
    }

    /// Appends a manual attack line with a caller-supplied element id.
    #[inline]
    pub fn attack_with_id(
        &mut self,
        id: AttackEntryId,
        name: impl Into<String>,
        bonus: Option<i32>,
        damage_text: impl Into<String>,
        from_inventory: Option<InventoryEntryId>,
    ) -> &mut Self {
        self.body.attacks.push(AttackEntry {
            id,
            name: name.into(),
            bonus,
            damage_text: damage_text.into(),
            from_inventory,
        });
        self
    }

    /// Appends a feature.
    #[inline]
    pub fn feature(&mut self, feature: FeatureEntry) -> &mut Self {
        self.body.features.push(feature);
        self
    }

    /// Appends a status (condition, effect or concentration).
    #[inline]
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.body.statuses.push(status);
        self
    }

    /// Sets the free-text notes box.
    #[inline]
    pub fn notes(&mut self, text: impl Into<String>) -> &mut Self {
        self.body.notes = text.into();
        self
    }

    /// Finishes the statblock body, refusing a body whose `abilities` or `hp`
    /// were never set.
    #[inline]
    pub fn build(self) -> Result<StatblockBody, BuildError> {
        if !self.abilities_set {
            return Err(BuildError::MissingAbilities);
        }
        if !self.hp_set {
            return Err(BuildError::MissingHp);
        }
        Ok(self.body)
    }

    /// Finishes the body and hands back the core
    /// [`crate::types::EntityBuilder`], already seeded with `kind`/`id`/`name`/
    /// `owner`, so the caller can set header fields (`parent`, `acl`, `rev`,
    /// timestamps, ...) before [`crate::types::EntityBuilder::build`].
    ///
    /// Fails with the same [`BuildError`] as [`Self::build`] when `abilities`
    /// or `hp` were never set.
    ///
    /// The two-step shape (`entity_builder(..)?.build()`) is deliberate: the
    /// body gate here and the kind/body gate in `EntityBuilder` are separate
    /// invariants, each surfaced as it is crossed.
    #[inline]
    pub fn entity_builder(
        self,
        kind: EntityKind,
        id: EntityId,
        name: impl Into<String>,
        owner: PlayerId,
    ) -> Result<EntityBuilder<StatblockBody>, BuildError> {
        let body = self.build()?;
        Ok(Entity::builder(kind, id, name, owner, body))
    }

    /// Finishes the body and wraps it in an entity header in one call, so the
    /// common client path is a single expression (the header itself is still
    /// built and validated by `crate::types::EntityBuilder`).
    #[inline]
    pub fn build_entity(
        self,
        kind: EntityKind,
        id: EntityId,
        name: impl Into<String>,
        owner: PlayerId,
    ) -> Result<StatblockEntity, BuildError> {
        self.entity_builder(kind, id, name, owner)?
            .build()
            .map_err(BuildError::from)
    }
}

/// Failure assembling a statblock body or entity.
///
/// Non-exhaustive so new required sections can be added without breaking
/// matchers.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// `abilities` was never set. Refusing here is what keeps the builder from
    /// injecting the silent-10 default docs/decisions.md DATA-10 forbids.
    #[error("abilities were not set on the statblock builder")]
    MissingAbilities,
    /// `hp` was never set.
    #[error("hit points were not set on the statblock builder")]
    MissingHp,
    /// The header and the body belong to different families.
    #[error(transparent)]
    Entity(#[from] EntityError),
}

// ---------------------------------------------------------------------------
// Literal helpers
// ---------------------------------------------------------------------------

/// Builds a [`ContentRef`] from a literal id and name.
///
/// Returns an error instead of panicking so it is safe on any call path.
#[inline]
pub fn content_ref(id: &str, name: &str) -> Result<ContentRef, RefIdError> {
    Ok(ContentRef {
        id: RefId::parse(id)?,
        name: name.to_owned(),
    })
}

/// Parses a [`StatKey`] from a literal.
#[inline]
pub fn stat(raw: &str) -> Result<StatKey, StatKeyError> {
    StatKey::parse(raw)
}

/// Parses a [`RefId`] from a literal.
#[inline]
pub fn ref_id(raw: &str) -> Result<RefId, RefIdError> {
    RefId::parse(raw)
}

/// Builds a plain dice pool, e.g. `dice_pool(1, 4)` for `1d4`.
#[must_use]
#[inline]
pub fn dice_pool(count: i32, sides: i32) -> DiceExpr {
    DiceExpr::Roll {
        count,
        sides,
        keep: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abilities() -> Abilities {
        Abilities {
            strength: 10,
            dexterity: 10,
            constitution: 10,
            intelligence: 10,
            wisdom: 10,
            charisma: 10,
        }
    }

    fn inline(name: &str, kind: ItemKind, requires_attunement: bool) -> InlineItem {
        InlineItem {
            name: name.to_owned(),
            description: String::new(),
            kind,
            requires_attunement,
            weight_oz: None,
            value: None,
            properties: None,
            modifiers: Vec::new(),
            resource: None,
            ext: Ext::new(),
        }
    }

    #[test]
    fn with_id_variants_keep_the_caller_id() {
        // Arrange.
        let level_id = ClassLevelId::new();
        let item_id = InventoryEntryId::new();
        let block_id = SpellcastingBlockId::new();
        let attack_id = AttackEntryId::new();
        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities()).hp(10, 10);

        // Act.
        builder
            .class_level_with_id(level_id, RefOrText::Text("Fighter".to_owned()), 1, None)
            .spellcasting_block_with_id(
                block_id,
                RefOrText::Text("Fighter".to_owned()),
                Ability::Intelligence,
                Vec::new(),
            )
            .attack_with_id(attack_id, "Sword", Some(3), "1d8 + 1", None);
        let pushed = builder.item_with_id(
            item_id,
            InventoryItem::Inline(inline("Sword", ItemKind::Weapon, false)),
            1,
            ItemState::Equipped,
            false,
        );
        let body = builder.build().expect("abilities and hp are set");

        // Assert.
        assert_eq!(pushed, item_id);
        assert_eq!(body.levels[0].id, level_id);
        assert_eq!(body.spellcasting[0].id, block_id);
        assert_eq!(body.attacks[0].id, attack_id);
        assert_eq!(body.inventory[0].id, item_id);
    }

    #[test]
    fn item_inline_separates_attunement_from_the_requirement() {
        // Arrange: the sheet could attune to the item, but has not.
        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities()).hp(10, 10);

        // Act.
        let id = builder.item_inline(
            "Cloak of Elvenkind",
            ItemKind::Wondrous,
            ItemState::Carried,
            false,
            true,
        );
        let body = builder.build().expect("abilities and hp are set");

        // Assert.
        let entry = body
            .inventory
            .iter()
            .find(|e| e.id == id)
            .expect("the entry was added");
        assert!(!entry.attuned, "the entry is not attuned");
        assert!(matches!(
            &entry.item,
            InventoryItem::Inline(item) if item.requires_attunement
        ));
    }

    #[test]
    fn entity_builder_exposes_the_header_to_the_caller() {
        // Arrange.
        let owner = PlayerId::new();
        let parent = EntityId::new();
        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities()).hp(10, 10);

        // Act.
        let entity = builder
            .entity_builder(EntityKind::Companion, EntityId::new(), "Fang", owner)
            .expect("abilities and hp are set")
            .parent(parent)
            .build()
            .expect("a statblock kind pairs with a statblock body");

        // Assert.
        assert_eq!(entity.kind, EntityKind::Companion);
        assert_eq!(entity.owner, owner);
        assert_eq!(entity.parent, Some(parent));
    }
}
