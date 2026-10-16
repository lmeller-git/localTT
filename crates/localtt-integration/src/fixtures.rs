//! Golden-character fixtures (RFC-0003 section 3).
//!
//! Every fixture is data only: it types in the base data the engine will later
//! derive from. Fixtures use deterministic ids and times (see the internal
//! `ids` module), so their serialized bytes are reproducible.

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
        EntityKind,
        Ext,
        FeatureCategory,
        FeatureEntry,
        Formula,
        HandoutBody,
        HandoutContent,
        HandoutEntity,
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
        ResourceKind,
        Role,
        RulesetId,
        SchemaVersion,
        Size,
        Skill,
        SkillProficiency,
        SpellEntry,
        StatblockBody,
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

const LEVEL_SEED: u64 = 1_000;
const RESOURCE_SEED: u64 = 2_000;
const INVENTORY_SEED: u64 = 3_000;
const ATTACK_SEED: u64 = 4_000;
const FEATURE_SEED: u64 = 5_000;
const STATUS_SEED: u64 = 6_000;
const BLOCK_SEED: u64 = 7_000;
const SPELL_SEED: u64 = 8_000;

/// A fixed timestamp every fixture shares, so snapshots never depend on a
/// clock.
const FIXED_TIME: i64 = 1_750_000_000;

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

/// Rewrites every element id in `body` to a deterministic value, using
/// disjoint per-section seed ranges so no two elements collide.
fn reseed(body: &mut StatblockBody) {
    for (i, level) in body.levels.iter_mut().enumerate() {
        level.id = fixture_id(LEVEL_SEED + i as u64);
    }
    for (i, resource) in body.resources.iter_mut().enumerate() {
        resource.id = fixture_id(RESOURCE_SEED + i as u64);
    }
    for (i, entry) in body.inventory.iter_mut().enumerate() {
        entry.id = fixture_id(INVENTORY_SEED + i as u64);
    }
    for (i, attack) in body.attacks.iter_mut().enumerate() {
        attack.id = fixture_id(ATTACK_SEED + i as u64);
    }
    for (i, feature) in body.features.iter_mut().enumerate() {
        feature.id = fixture_id(FEATURE_SEED + i as u64);
    }
    for (i, status) in body.statuses.iter_mut().enumerate() {
        status.id = fixture_id(STATUS_SEED + i as u64);
    }
    for (i, block) in body.spellcasting.iter_mut().enumerate() {
        block.id = fixture_id(BLOCK_SEED + i as u64);
        for (j, spell) in block.spells.iter_mut().enumerate() {
            spell.id = fixture_id(SPELL_SEED + i as u64 * 100 + j as u64);
        }
    }
}

/// Finishes a statblock: builds the entity, resets its element ids to the
/// deterministic seeds, and stamps the fixed time.
fn finish(
    builder: StatblockBuilder,
    kind: EntityKind,
    seed: u64,
    name: &str,
    owner: PlayerId,
) -> StatblockEntity {
    let mut entity = builder
        .build_entity(kind, fixture_id(seed), name, owner)
        .expect("every fixture body sets its required abilities and hp");
    reseed(&mut entity.body);
    entity.created_at = UnixTime::from_seconds(FIXED_TIME);
    entity.updated_at = UnixTime::from_seconds(FIXED_TIME);
    entity
}

/// A spell known to a caster, referenced from the SRD.
fn spell(name: &str, reference: &str) -> SpellEntry {
    SpellEntry {
        id: localtt_core::types::SpellEntryId::new(),
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
        .class_level(
            RefOrText::Ref(content_ref("srd2014:class/wizard", "Wizard").expect("a literal ref")),
            5,
            Some(RefOrText::Text("Evocation".to_owned())),
        )
        .tools("Dice set")
        .languages("Common, Elvish")
        .notes("Aria, the party's evoker.");
    builder.resource(Resource {
        id: localtt_core::types::ResourceId::new(),
        name: "Spell Slots (3rd)".to_owned(),
        kind: ResourceKind::SpellSlot { level: 3 },
        max: Formula::Const(2),
        current: 2,
        recharge: Recharge::Long,
    });
    builder.spellcasting_block(
        RefOrText::Text("Wizard".to_owned()),
        Ability::Intelligence,
        vec![
            spell("Fireball", "srd2014:spell/fireball"),
            spell("Shield", "srd2014:spell/shield"),
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
        .class_level(
            RefOrText::Ref(content_ref("srd2014:class/fighter", "Fighter").expect("a literal ref")),
            3,
            Some(RefOrText::Text("Champion".to_owned())),
        )
        .languages("Common");
    builder.resource(Resource {
        id: localtt_core::types::ResourceId::new(),
        name: "Second Wind".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Const(1),
        current: 1,
        recharge: Recharge::Short,
    });

    let mut entity = finish(builder, EntityKind::Pc, FIGHTER_SEED, "Borin", owner);
    let resource_id = entity.body.resources[0].id;
    entity.body.features.push(FeatureEntry {
        id: localtt_core::types::FeatureEntryId::new(),
        category: FeatureCategory::Class,
        name: "Second Wind".to_owned(),
        reference: None,
        text: Some("Regain 1d10 + fighter level hit points as a bonus action.".to_owned()),
        resource_id: Some(resource_id),
        modifiers: Vec::new(),
        ext: Ext::default(),
    });
    reseed(&mut entity.body);
    entity.body.features[0].resource_id = Some(entity.body.resources[0].id);
    entity
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
        .class_level(
            RefOrText::Ref(content_ref("srd2014:class/warlock", "Warlock").expect("a literal ref")),
            2,
            Some(RefOrText::Text("Fiend".to_owned())),
        )
        .languages("Common, Infernal");
    builder.resource(Resource {
        id: localtt_core::types::ResourceId::new(),
        name: "Pact Magic".to_owned(),
        kind: ResourceKind::PactSlot,
        max: Formula::Const(2),
        current: 2,
        recharge: Recharge::Short,
    });
    builder.status(Status {
        id: localtt_core::types::StatusId::new(),
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
        .class_level(RefOrText::Text("Wizard".to_owned()), 3, None)
        .class_level(RefOrText::Text("Cleric".to_owned()), 2, None)
        .override_stat(
            stat("ac").expect("a literal stat key"),
            OverrideValue::Int(15),
        )
        .languages("Common");
    builder.spellcasting_block(
        RefOrText::Text("Wizard".to_owned()),
        Ability::Intelligence,
        vec![spell("Mage Armor", "srd2014:spell/mage-armor")],
    );
    builder.spellcasting_block(
        RefOrText::Text("Cleric".to_owned()),
        Ability::Wisdom,
        vec![spell("Bless", "srd2014:spell/bless")],
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
        .attack("Scimitar", Some(4), "1d6 + 2 slashing", None);
    builder.item(
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
        id: localtt_core::types::FeatureEntryId::new(),
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
fn wolf(pc_id: localtt_core::types::EntityId, owner: PlayerId) -> StatblockEntity {
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
            id: localtt_core::types::FeatureEntryId::new(),
            category: FeatureCategory::Trait,
            name: "Pack Tactics".to_owned(),
            reference: None,
            text: Some("Advantage when an ally is within 5 feet of the target.".to_owned()),
            resource_id: None,
            modifiers: Vec::new(),
            ext: Ext::default(),
        });

    let mut entity = finish(
        builder,
        EntityKind::Companion,
        COMPANION_SEED,
        "Fang",
        owner,
    );
    entity.parent = Some(pc_id);
    entity
}

/// An owl: Aria's CR 0 familiar.
fn owl(pc_id: localtt_core::types::EntityId, owner: PlayerId) -> StatblockEntity {
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
            id: localtt_core::types::FeatureEntryId::new(),
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

    let mut entity = finish(builder, EntityKind::Familiar, FAMILIAR_SEED, "Talon", owner);
    entity.parent = Some(pc_id);
    entity
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
