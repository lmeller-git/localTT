//! Golden-character fixtures (RFC-0003 section 3).
//!
//! Every fixture is data only: it types in the base data the engine will later
//! derive from. Fixtures use deterministic ids and times (see the internal
//! `ids` module), so their serialized bytes are reproducible: every element id
//! is passed to the builders explicitly rather than minted, and the header is
//! stamped with a fixed time.

use localtt_core::{
    builders::{StatblockBuilder, content_ref, ref_id, stat},
    types::{
        Abilities,
        Ability,
        AnyEntity,
        Audience,
        Campaign,
        CampaignSettings,
        CampaignTime,
        ChallengeRating,
        Clock,
        Entity,
        EntityId,
        EntityKind,
        Ext,
        FeatureCategory,
        FeatureEntry,
        Formula,
        HandoutBody,
        HandoutContent,
        HandoutEntity,
        Id,
        Identity,
        InlineItem,
        InventoryItem,
        ItemKind,
        ItemState,
        NoteBody,
        NoteEntity,
        OverrideValue,
        Player,
        PlayerId,
        Recharge,
        RefOrText,
        Resource,
        ResourceId,
        ResourceKind,
        Role,
        RulesetId,
        SchemaVersion,
        Size,
        Skill,
        SkillProficiency,
        SpellEntry,
        StatblockEntity,
        Status,
        StatusDuration,
        StatusKind,
        UnixTime,
    },
};

use crate::ids::fixture_id;

const CAMPAIGN_SEED: u64 = 1;
const DM_SEED: u64 = 2;
const PLAYER_SEED: u64 = 3;
const WIZARD_SEED: u64 = 10;
const FIGHTER_SEED: u64 = 11;
const WARLOCK_SEED: u64 = 12;
const MULTICLASS_SEED: u64 = 13;
const GOBLIN_SEED: u64 = 14;
const COMPANION_SEED: u64 = 20;
const FAMILIAR_SEED: u64 = 21;
const NOTE_SEED: u64 = 22;
const HANDOUT_SEED: u64 = 23;

/// A fixed timestamp every fixture shares, so snapshots never depend on a
/// clock.
const FIXED_TIME: i64 = 1_750_000_000;

/// An element id unique within the fixture owned by `seed`.
///
/// Each fixture has its own seed space, so element ids never collide across the
/// entities of a party.
fn element_id<T>(seed: u64, index: u64) -> Id<T> {
    fixture_id(seed * 1_000 + index)
}

/// A whole campaign with its cast; the unit this suite round-trips.
pub struct Party {
    /// The campaign document, including the player roster.
    pub campaign: Campaign,
    /// The DM slot id.
    pub dm: PlayerId,
    /// The player slot id who owns the character entities.
    pub player: PlayerId,
    /// Aria, the level-5 high-elf evoker.
    pub pc: StatblockEntity,
    /// Aria's wolf companion (`parent` = the PC).
    pub companion: StatblockEntity,
    /// Aria's owl familiar (`parent` = the PC).
    pub familiar: StatblockEntity,
    /// A DM-only session note.
    pub note: NoteEntity,
    /// An item handout.
    pub handout: HandoutEntity,
}

impl Party {
    /// Every entity as the runtime-tagged type storage speaks.
    #[must_use]
    #[inline]
    pub fn entities(&self) -> Vec<AnyEntity> {
        vec![
            AnyEntity::from(self.pc.clone()),
            AnyEntity::from(self.companion.clone()),
            AnyEntity::from(self.familiar.clone()),
            AnyEntity::from(self.note.clone()),
            AnyEntity::from(self.handout.clone()),
        ]
    }
}

/// Finishes a statblock, stamping the fixed time on the header.
fn finish(
    builder: StatblockBuilder,
    kind: EntityKind,
    seed: u64,
    name: &str,
    owner: PlayerId,
) -> StatblockEntity {
    builder
        .entity_builder(kind, fixture_id(seed), name, owner)
        .expect("every fixture body sets its required abilities and hp")
        .created_at(UnixTime::from_seconds(FIXED_TIME))
        .updated_at(UnixTime::from_seconds(FIXED_TIME))
        .build()
        .expect("a statblock kind pairs with a statblock body")
}

/// A spell known to a caster, referenced from the SRD.
fn spell(seed: u64, index: u64, name: &str, reference: &str) -> SpellEntry {
    SpellEntry {
        id: element_id(seed, index),
        name: name.to_owned(),
        reference: Some(ref_id(reference).expect("a literal spell reference is valid")),
        prepared: true,
    }
}

/// Aria: a level-5 high-elf evoker.
#[must_use]
#[inline]
pub fn golden_wizard(owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::character();
    builder
        .abilities(Abilities {
            strength: 8,
            dexterity: 14,
            constitution: 14,
            intelligence: 18,
            wisdom: 12,
            charisma: 10,
        })
        .save_proficiency(Ability::Intelligence)
        .save_proficiency(Ability::Wisdom)
        .skill(Skill::Arcana, SkillProficiency::Expertise)
        .skill(Skill::Investigation, SkillProficiency::Proficient)
        .size(Size::Medium)
        .hp(32, 32)
        .identity(Identity {
            race: Some(RefOrText::Ref(
                content_ref("srd2014:race/high-elf", "High Elf").expect("a literal race ref"),
            )),
            background: Some(RefOrText::Ref(
                content_ref("srd2014:background/sage", "Sage").expect("a literal background ref"),
            )),
            alignment: "Neutral Good".to_owned(),
        })
        .class_level_with_id(
            element_id(WIZARD_SEED, 0),
            RefOrText::Ref(content_ref("srd2014:class/wizard", "Wizard").expect("a literal ref")),
            5,
            Some(RefOrText::Text("Evocation".to_owned())),
        )
        .tools("Dice set")
        .languages("Common, Elvish")
        .notes("Aria, the party's evoker.");
    builder.resource(Resource {
        id: element_id(WIZARD_SEED, 1),
        name: "Spell Slots (3rd)".to_owned(),
        kind: ResourceKind::SpellSlot { level: 3 },
        max: Formula::Const(2),
        current: 2,
        recharge: Recharge::Long,
    });
    builder.spellcasting_block_with_id(
        element_id(WIZARD_SEED, 2),
        RefOrText::Text("Wizard".to_owned()),
        Ability::Intelligence,
        vec![
            spell(WIZARD_SEED, 3, "Fireball", "srd2014:spell/fireball"),
            spell(WIZARD_SEED, 4, "Shield", "srd2014:spell/shield"),
        ],
    );

    finish(builder, EntityKind::Pc, WIZARD_SEED, "Aria", owner)
}

/// Borin: a level-3 champion fighter.
#[must_use]
#[inline]
pub fn golden_fighter(owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::character();
    builder
        .abilities(Abilities {
            strength: 16,
            dexterity: 14,
            constitution: 16,
            intelligence: 10,
            wisdom: 12,
            charisma: 8,
        })
        .save_proficiency(Ability::Strength)
        .save_proficiency(Ability::Constitution)
        .skill(Skill::Athletics, SkillProficiency::Proficient)
        .skill(Skill::Perception, SkillProficiency::Proficient)
        .size(Size::Medium)
        .hp(28, 28)
        .identity(Identity {
            race: Some(RefOrText::Text("Human".to_owned())),
            background: Some(RefOrText::Text("Soldier".to_owned())),
            alignment: "Lawful Good".to_owned(),
        })
        .class_level_with_id(
            element_id(FIGHTER_SEED, 0),
            RefOrText::Ref(content_ref("srd2014:class/fighter", "Fighter").expect("a literal ref")),
            3,
            Some(RefOrText::Text("Champion".to_owned())),
        )
        .languages("Common");
    let second_wind: ResourceId = element_id(FIGHTER_SEED, 1);
    builder.resource(Resource {
        id: second_wind,
        name: "Second Wind".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(1),
        current: 1,
        recharge: Recharge::Short,
    });
    builder.feature(FeatureEntry {
        id: element_id(FIGHTER_SEED, 2),
        category: FeatureCategory::Class,
        name: "Second Wind".to_owned(),
        reference: None,
        text: Some("Regain 1d10 + fighter level hit points as a bonus action.".to_owned()),
        resource_id: Some(second_wind),
        modifiers: Vec::new(),
        ext: Ext::default(),
    });

    finish(builder, EntityKind::Pc, FIGHTER_SEED, "Borin", owner)
}

/// Lyra: a level-2 warlock.
#[must_use]
#[inline]
pub fn golden_warlock(owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::character();
    builder
        .abilities(Abilities {
            strength: 8,
            dexterity: 14,
            constitution: 14,
            intelligence: 10,
            wisdom: 12,
            charisma: 17,
        })
        .save_proficiency(Ability::Wisdom)
        .save_proficiency(Ability::Charisma)
        .skill(Skill::Deception, SkillProficiency::Proficient)
        .size(Size::Medium)
        .hp(17, 17)
        .identity(Identity {
            race: Some(RefOrText::Text("Tiefling".to_owned())),
            background: Some(RefOrText::Text("Charlatan".to_owned())),
            alignment: "Chaotic Neutral".to_owned(),
        })
        .class_level_with_id(
            element_id(WARLOCK_SEED, 0),
            RefOrText::Ref(content_ref("srd2014:class/warlock", "Warlock").expect("a literal ref")),
            2,
            Some(RefOrText::Text("Fiend".to_owned())),
        )
        .languages("Common, Infernal");
    builder.resource(Resource {
        id: element_id(WARLOCK_SEED, 1),
        name: "Pact Magic".to_owned(),
        kind: ResourceKind::PactSlot,
        max: Formula::Const(2),
        current: 2,
        recharge: Recharge::Short,
    });
    builder.status(Status {
        id: element_id(WARLOCK_SEED, 2),
        kind: StatusKind::Concentration,
        name: "Hex".to_owned(),
        reference: Some(ref_id("srd2014:spell/hex").expect("a literal spell reference")),
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Timed {
            expires_at: CampaignTime::from_seconds(3_600),
        },
        source: None,
        modifiers: Vec::new(),
    });

    finish(builder, EntityKind::Pc, WARLOCK_SEED, "Lyra", owner)
}

/// Mira: a level-5 wizard 3 / cleric 2 multiclass character.
#[must_use]
#[inline]
pub fn golden_multiclass(owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::character();
    builder
        .abilities(Abilities {
            strength: 8,
            dexterity: 14,
            constitution: 14,
            intelligence: 16,
            wisdom: 16,
            charisma: 10,
        })
        .save_proficiency(Ability::Intelligence)
        .save_proficiency(Ability::Wisdom)
        .size(Size::Medium)
        .hp(27, 27)
        .identity(Identity {
            race: Some(RefOrText::Text("Human".to_owned())),
            background: Some(RefOrText::Text("Acolyte".to_owned())),
            alignment: "Neutral".to_owned(),
        })
        .class_level_with_id(
            element_id(MULTICLASS_SEED, 0),
            RefOrText::Text("Wizard".to_owned()),
            3,
            None,
        )
        .class_level_with_id(
            element_id(MULTICLASS_SEED, 1),
            RefOrText::Text("Cleric".to_owned()),
            2,
            None,
        )
        .override_stat(
            stat("ac").expect("a literal stat key"),
            OverrideValue::Int(15),
        )
        .languages("Common");
    builder.spellcasting_block_with_id(
        element_id(MULTICLASS_SEED, 2),
        RefOrText::Text("Wizard".to_owned()),
        Ability::Intelligence,
        vec![spell(
            MULTICLASS_SEED,
            4,
            "Mage Armor",
            "srd2014:spell/mage-armor",
        )],
    );
    builder.spellcasting_block_with_id(
        element_id(MULTICLASS_SEED, 3),
        RefOrText::Text("Cleric".to_owned()),
        Ability::Wisdom,
        vec![spell(MULTICLASS_SEED, 5, "Bless", "srd2014:spell/bless")],
    );

    finish(builder, EntityKind::Pc, MULTICLASS_SEED, "Mira", owner)
}

/// A goblin: a CR 1/4 creature.
#[must_use]
#[inline]
pub fn goblin(owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::creature(
        ChallengeRating::parse("1/4").expect("1/4 is a valid challenge rating"),
    );
    builder
        .abilities(Abilities {
            strength: 8,
            dexterity: 14,
            constitution: 10,
            intelligence: 10,
            wisdom: 8,
            charisma: 8,
        })
        .size(Size::Small)
        .hp(7, 7)
        .creature_type("humanoid (goblinoid)")
        .senses("darkvision 60 ft., passive Perception 9")
        .languages("Common, Goblin")
        .attack_with_id(
            element_id(GOBLIN_SEED, 0),
            "Scimitar",
            Some(4),
            "1d6 + 2 slashing",
            None,
        );
    builder.item_with_id(
        element_id(GOBLIN_SEED, 1),
        InventoryItem::Inline(InlineItem {
            name: "Scimitar".to_owned(),
            description: String::new(),
            kind: ItemKind::Weapon,
            requires_attunement: false,
            weight_oz: Some(48),
            value: None,
            properties: None,
            modifiers: Vec::new(),
            resource: None,
            ext: Ext::default(),
        }),
        1,
        ItemState::Equipped,
        false,
    );
    builder.feature(FeatureEntry {
        id: element_id(GOBLIN_SEED, 2),
        category: FeatureCategory::Action,
        name: "Scimitar".to_owned(),
        reference: None,
        text: Some("Melee Weapon Attack: +4 to hit, one target. Hit: 1d6 + 2 slashing.".to_owned()),
        resource_id: None,
        modifiers: Vec::new(),
        ext: Ext::default(),
    });

    finish(builder, EntityKind::Monster, GOBLIN_SEED, "Goblin", owner)
}

/// A wolf: Aria's CR 1/4 companion.
fn wolf(pc_id: EntityId, owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::creature(
        ChallengeRating::parse("1/4").expect("1/4 is a valid challenge rating"),
    );
    builder
        .abilities(Abilities {
            strength: 12,
            dexterity: 15,
            constitution: 12,
            intelligence: 3,
            wisdom: 12,
            charisma: 6,
        })
        .size(Size::Medium)
        .hp(11, 11)
        .creature_type("beast")
        .senses("passive Perception 13")
        .feature(FeatureEntry {
            id: element_id(COMPANION_SEED, 0),
            category: FeatureCategory::Trait,
            name: "Pack Tactics".to_owned(),
            reference: None,
            text: Some("Advantage when an ally is within 5 feet of the target.".to_owned()),
            resource_id: None,
            modifiers: Vec::new(),
            ext: Ext::default(),
        });

    builder
        .entity_builder(
            EntityKind::Companion,
            fixture_id(COMPANION_SEED),
            "Fang",
            owner,
        )
        .expect("every fixture body sets its required abilities and hp")
        .parent(pc_id)
        .created_at(UnixTime::from_seconds(FIXED_TIME))
        .updated_at(UnixTime::from_seconds(FIXED_TIME))
        .build()
        .expect("a statblock kind pairs with a statblock body")
}

/// An owl: Aria's CR 0 familiar.
fn owl(pc_id: EntityId, owner: PlayerId) -> StatblockEntity {
    let mut builder = StatblockBuilder::creature(
        ChallengeRating::parse("0").expect("0 is a valid challenge rating"),
    );
    builder
        .abilities(Abilities {
            strength: 3,
            dexterity: 13,
            constitution: 8,
            intelligence: 2,
            wisdom: 12,
            charisma: 7,
        })
        .size(Size::Tiny)
        .hp(1, 1)
        .creature_type("beast")
        .senses("darkvision 120 ft., passive Perception 13")
        .feature(FeatureEntry {
            id: element_id(FAMILIAR_SEED, 0),
            category: FeatureCategory::Trait,
            name: "Flyby".to_owned(),
            reference: None,
            text: Some(
                "Does not provoke opportunity attacks when it flies out of reach.".to_owned(),
            ),
            resource_id: None,
            modifiers: Vec::new(),
            ext: Ext::default(),
        });

    builder
        .entity_builder(
            EntityKind::Familiar,
            fixture_id(FAMILIAR_SEED),
            "Talon",
            owner,
        )
        .expect("every fixture body sets its required abilities and hp")
        .parent(pc_id)
        .created_at(UnixTime::from_seconds(FIXED_TIME))
        .updated_at(UnixTime::from_seconds(FIXED_TIME))
        .build()
        .expect("a statblock kind pairs with a statblock body")
}

/// The assembled party used by most tests.
#[must_use]
#[inline]
pub fn party() -> Party {
    let dm: PlayerId = fixture_id(DM_SEED);
    let player: PlayerId = fixture_id(PLAYER_SEED);

    let pc = golden_wizard(player);
    let companion = wolf(pc.id, player);
    let familiar = owl(pc.id, player);

    let note = Entity::builder(
        EntityKind::Note,
        fixture_id(NOTE_SEED),
        "Session 4",
        dm,
        NoteBody {
            text: "# Session 4\n\nThe party opened the vault.".to_owned(),
        },
    )
    .parent(pc.id)
    .created_at(UnixTime::from_seconds(FIXED_TIME))
    .updated_at(UnixTime::from_seconds(FIXED_TIME))
    .build()
    .expect("a note kind pairs with a note body");

    let handout = Entity::builder(
        EntityKind::Handout,
        fixture_id(HANDOUT_SEED),
        "Vault Key",
        dm,
        HandoutBody {
            content: HandoutContent::Item {
                item: InlineItem {
                    name: "Vault Key".to_owned(),
                    description: "Opens the vault beneath the chapel.".to_owned(),
                    kind: ItemKind::Other,
                    requires_attunement: false,
                    weight_oz: Some(1),
                    value: None,
                    properties: None,
                    modifiers: Vec::new(),
                    resource: None,
                    ext: Ext::default(),
                },
            },
        },
    )
    .created_at(UnixTime::from_seconds(FIXED_TIME))
    .updated_at(UnixTime::from_seconds(FIXED_TIME))
    .build()
    .expect("a handout kind pairs with a handout body");

    let campaign = Campaign {
        id: fixture_id(CAMPAIGN_SEED),
        name: "Curse of the Vault".to_owned(),
        schema_version: SchemaVersion::CURRENT,
        default_ruleset: RulesetId::dnd5e_2014(),
        clock: Clock {
            now_s: CampaignTime::from_seconds(0),
        },
        settings: CampaignSettings {
            player_roll_audience_default: Audience::RollerAndDm,
            feed_capacity: 200,
            dawn_hour: 6,
        },
        players: vec![
            Player {
                id: dm,
                display_name: "Dungeon Master".to_owned(),
                role: Role::Dm,
                token_hash: None,
                created_at: UnixTime::from_seconds(FIXED_TIME),
                last_seen: None,
            },
            Player {
                id: player,
                display_name: "Alex".to_owned(),
                role: Role::Player,
                token_hash: None,
                created_at: UnixTime::from_seconds(FIXED_TIME),
                last_seen: None,
            },
        ],
        ext: Ext::default(),
    };

    Party {
        campaign,
        dm,
        player,
        pc,
        companion,
        familiar,
        note,
        handout,
    }
}
