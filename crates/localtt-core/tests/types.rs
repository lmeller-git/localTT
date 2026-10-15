//! Target test for RFC-0002: `crates/localtt-core/RFC-0002-core-types.md`.
//!
//! Spec phase. This file intentionally does not compile until the skeleton
//! phase lands the signatures sketched in section 3 of the RFC. Do not add
//! scaffolding here to make it build; adjust the RFC sketch first if the API
//! is wrong.
//!
//! Coverage: a realistic PC (round trip + pinned JSON shape), the three body
//! kinds through `AnyEntity`, failure cases (kind/body mismatch at build and
//! at deserialization, invalid `RefId`, invalid custom `StatKey`, refused
//! newer schema major), forward-compatible unknown fields, default-deny
//! `FieldMask`, and `Send + Sync` assertions. Core types are plain data, so
//! the concurrency contract is the static `Send + Sync` bound; there is no
//! contention case to test in this subsystem.

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::types::{
    Ability, Abilities, AclEntry, AclLevel, AnyEntity, AttackEntry, AttackEntryId, Audience, Body,
    BodyKind, CampaignTime, ClassLevel, ClassLevelId, Coins, Condition, ContentRef, DamageType,
    DeathSaves, DiceExpr, DieResult, Entity, EntityError, EntityId, EntityKind, Ext,
    FeatureCategory, FeatureEntry, FeatureEntryId, FeedEntry, FeedEntryId, FeedPayload, FieldMask,
    Formula, FormulaExpr, HandoutBody, HandoutContent, HandoutEntity, Hp, Identity, InlineItem,
    InventoryEntry, InventoryEntryId, InventoryItem, ItemKind, ItemProperties, ItemState, KnownStat,
    Modifier, ModifierOp, Namespace, NoteBody, NoteEntity, OverrideValue, PlayerId, Profile,
    Proficiencies, RefId, RefOrText, Recharge, Resource, ResourceId, ResourceKind, RestKind, Roll,
    RollContext, RollId, RollKind, RulesetId, SchemaVersion, Size, Skill, SkillProficiency, Speed,
    SpellEntry, SpellEntryId, SpellcastingBlock, SpellcastingBlockId, StatKey, StatblockBody,
    StatblockEntity, Status, StatusDuration, StatusId, StatusKind, StatusSource, TermResult,
    UnixTime, ViewField, WeaponProperties, WeaponTag,
};
use serde_json::json;

fn name_and_ac_mask() -> FieldMask {
    let mut mask = FieldMask::default();
    mask.insert(ViewField::Name);
    mask.insert(ViewField::Ac);
    mask
}

fn sample_statblock() -> StatblockBody {
    let dagger_id = InventoryEntryId::new();
    StatblockBody {
        profile: Profile::Character,
        identity: Identity {
            race: Some(RefOrText::Ref(ContentRef {
                id: RefId::parse("srd2014:race/high-elf").expect("valid race ref"),
                name: "High Elf".to_string(),
            })),
            background: Some(RefOrText::Text("Sage".to_string())),
            alignment: "Neutral Good".to_string(),
        },
        abilities: Abilities {
            strength: 8,
            dexterity: 16,
            constitution: 14,
            intelligence: 18,
            wisdom: 12,
            charisma: 10,
        },
        levels: vec![ClassLevel {
            id: ClassLevelId::new(),
            class: RefOrText::Ref(ContentRef {
                id: RefId::parse("srd2014:class/wizard").expect("valid class ref"),
                name: "Wizard".to_string(),
            }),
            level: 5,
            subclass: Some(RefOrText::Text("School of Evocation".to_string())),
        }],
        cr: None,
        creature_type: None,
        size: Size::Medium,
        senses: "darkvision 60 ft.".to_string(),
        proficiencies: Proficiencies {
            saves: BTreeSet::from([Ability::Intelligence, Ability::Wisdom]),
            skills: BTreeMap::from([
                (Skill::Arcana, SkillProficiency::Proficient),
                (Skill::Investigation, SkillProficiency::Expertise),
                (Skill::SleightOfHand, SkillProficiency::Proficient),
            ]),
            tools: "Calligrapher's supplies".to_string(),
            languages: "Common, Elvish, Dwarvish".to_string(),
        },
        hp: Hp {
            max: 32,
            current: 27,
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
        inspiration: true,
        coins: Coins {
            cp: 0,
            sp: 12,
            ep: 0,
            gp: 25,
            pp: 0,
        },
        overrides: BTreeMap::from([(StatKey::parse("ac").expect("known stat"), OverrideValue::Int(18))]),
        resources: vec![Resource {
            id: ResourceId::new(),
            name: "Spell Slots (3rd level)".to_string(),
            kind: ResourceKind::SpellSlot { level: 3 },
            max: Formula::Const(2),
            current: 1,
            recharge: Recharge::Long,
        }],
        spellcasting: vec![SpellcastingBlock {
            id: SpellcastingBlockId::new(),
            source: RefOrText::Text("Wizard".to_string()),
            ability: Ability::Intelligence,
            spells: vec![
                SpellEntry {
                    id: SpellEntryId::new(),
                    name: "Fireball".to_string(),
                    reference: Some(RefId::parse("srd2014:spell/fireball").expect("valid spell ref")),
                    prepared: true,
                },
                SpellEntry {
                    id: SpellEntryId::new(),
                    name: "Melf's Minute Meteors".to_string(),
                    reference: None,
                    prepared: true,
                },
            ],
        }],
        inventory: vec![
            InventoryEntry {
                id: InventoryEntryId::new(),
                item: InventoryItem::Ref(ContentRef {
                    id: RefId::parse("srd2014:item/longsword").expect("valid item ref"),
                    name: "Longsword".to_string(),
                }),
                qty: 1,
                state: ItemState::Equipped,
                attuned: false,
                notes: String::new(),
            },
            InventoryEntry {
                id: InventoryEntryId::new(),
                item: InventoryItem::Inline(InlineItem {
                    name: "Ring of Warmth".to_string(),
                    description: "You have resistance to cold damage while wearing this ring."
                        .to_string(),
                    kind: ItemKind::Wondrous,
                    requires_attunement: true,
                    weight_oz: None,
                    value: Some(Coins {
                        cp: 0,
                        sp: 0,
                        ep: 0,
                        gp: 500,
                        pp: 0,
                    }),
                    properties: None,
                    modifiers: vec![Modifier {
                        target: StatKey::parse("ac").expect("known stat"),
                        op: ModifierOp::Add {
                            value: Formula::Const(1),
                        },
                        when: Some(Condition::Attuned),
                        note: Some("Ring of Warmth".to_string()),
                    }],
                    resource: None,
                    ext: Ext::new(),
                }),
                qty: 1,
                state: ItemState::Equipped,
                attuned: true,
                notes: String::new(),
            },
            InventoryEntry {
                id: dagger_id,
                item: InventoryItem::Inline(InlineItem {
                    name: "Dagger".to_string(),
                    description: "A simple blade.".to_string(),
                    kind: ItemKind::Weapon,
                    requires_attunement: false,
                    weight_oz: Some(16),
                    value: Some(Coins {
                        cp: 0,
                        sp: 2,
                        ep: 0,
                        gp: 0,
                        pp: 0,
                    }),
                    properties: Some(ItemProperties::Weapon(WeaponProperties {
                        damage: Some(DiceExpr::Roll {
                            count: 1,
                            sides: 4,
                            keep: None,
                        }),
                        damage_type: Some(DamageType::Piercing),
                        range_ft: Some((20, 60)),
                        versatile_damage: None,
                        tags: vec![WeaponTag::Finesse, WeaponTag::Light, WeaponTag::Thrown],
                    })),
                    modifiers: Vec::new(),
                    resource: None,
                    ext: Ext::new(),
                }),
                qty: 1,
                state: ItemState::Carried,
                attuned: false,
                notes: String::new(),
            },
        ],
        attacks: vec![AttackEntry {
            id: AttackEntryId::new(),
            name: "Dagger".to_string(),
            bonus: Some(6),
            damage_text: "1d4+3 piercing".to_string(),
            from_inventory: Some(dagger_id),
        }],
        features: vec![FeatureEntry {
            id: FeatureEntryId::new(),
            category: FeatureCategory::Class,
            name: "Arcane Recovery".to_string(),
            reference: None,
            text: Some("Once per day when you finish a short rest...".to_string()),
            resource_id: None,
            modifiers: vec![Modifier {
                target: StatKey::parse("save.wis").expect("known stat"),
                op: ModifierOp::Add {
                    value: Formula::Expr(FormulaExpr::ProficiencyBonus),
                },
                when: None,
                note: None,
            }],
            ext: Ext::new(),
        }],
        statuses: vec![
            Status {
                id: StatusId::new(),
                kind: StatusKind::Concentration,
                name: "Melf's Minute Meteors".to_string(),
                reference: None,
                text: None,
                started_at: CampaignTime::from_seconds(3600),
                duration: StatusDuration::Timed {
                    expires_at: CampaignTime::from_seconds(3660),
                },
                source: None,
                modifiers: Vec::new(),
            },
            Status {
                id: StatusId::new(),
                kind: StatusKind::Custom,
                name: "Blessed by the Dawnfather".to_string(),
                reference: None,
                text: Some("The PC glows faintly in the dark.".to_string()),
                started_at: CampaignTime::from_seconds(3500),
                duration: StatusDuration::UntilRest {
                    rest: RestKind::Long,
                },
                source: Some(StatusSource {
                    entity: None,
                    status: None,
                }),
                modifiers: Vec::new(),
            },
        ],
        notes: "Session 12: the vault was opened.".to_string(),
    }
}

fn sample_pc(owner: PlayerId) -> StatblockEntity {
    Entity::builder(EntityKind::Pc, EntityId::new(), "Aria", owner, sample_statblock())
        .ruleset(RulesetId::parse("5e-2014").expect("valid ruleset id"))
        .rev(3)
        .acl(vec![AclEntry {
            player: PlayerId::new(),
            level: AclLevel::Read,
            field_mask: Some(name_and_ac_mask()),
        }])
        .ext(BTreeMap::from([(
            Namespace::parse("com.example.tracker").expect("valid namespace"),
            json!({"quest": "open the vault"}),
        )]))
        .created_at(UnixTime::from_seconds(1_750_000_000))
        .updated_at(UnixTime::from_seconds(1_750_000_000))
        .build()
        .expect("a pc kind pairs with a statblock body")
}

#[test]
fn entity_builder_rejects_kind_body_mismatch() {
    let err = Entity::builder(
        EntityKind::Note,
        EntityId::new(),
        "Session 12",
        PlayerId::new(),
        sample_statblock(),
    )
    .build()
    .expect_err("a note cannot carry a statblock body");
    assert!(matches!(err, EntityError::KindBodyMismatch { .. }));
}

#[test]
fn kind_and_body_families_agree() {
    assert_eq!(EntityKind::Pc.body_kind(), BodyKind::Statblock);
    assert_eq!(EntityKind::Familiar.body_kind(), BodyKind::Statblock);
    assert_eq!(EntityKind::Note.body_kind(), BodyKind::Note);
    assert_eq!(EntityKind::Handout.body_kind(), BodyKind::Handout);
    assert_eq!(StatblockBody::BODY_KIND, BodyKind::Statblock);
    assert_eq!(NoteBody::BODY_KIND, BodyKind::Note);
    assert_eq!(HandoutBody::BODY_KIND, BodyKind::Handout);
}

#[test]
fn pc_round_trips_and_pins_the_json_shape() {
    let pc = sample_pc(PlayerId::new());
    let value = serde_json::to_value(&pc).expect("entity serializes");

    // Header.
    assert_eq!(value["kind"], json!("pc"));
    assert_eq!(value["rev"], json!(3));
    assert_eq!(value["schema_version"], json!({"major": 1, "minor": 0}));
    assert_eq!(value["ext"]["com.example.tracker"]["quest"], json!("open the vault"));
    assert_eq!(value["acl"][0]["level"], json!("read"));
    assert_eq!(value["acl"][0]["field_mask"], json!(["name", "ac"]));

    // Statblock base data.
    assert_eq!(value["body"]["profile"], json!("character"));
    assert_eq!(value["body"]["abilities"]["int"], json!(18));
    assert_eq!(value["body"]["hp"]["current"], json!(27));
    assert_eq!(value["body"]["proficiencies"]["saves"], json!(["int", "wis"]));
    assert_eq!(
        value["body"]["proficiencies"]["skills"]["sleight_of_hand"],
        json!("proficient")
    );
    assert_eq!(value["body"]["overrides"]["ac"], json!(18));
    assert_eq!(value["body"]["size"], json!("medium"));

    // Resources: Formula shorthand plus tagged kind.
    assert_eq!(value["body"]["resources"][0]["max"], json!(2));
    assert_eq!(value["body"]["resources"][0]["kind"]["type"], json!("spell_slot"));
    assert_eq!(value["body"]["resources"][0]["recharge"], json!("long"));

    // Spells: reference or name-only.
    assert_eq!(
        value["body"]["spellcasting"][0]["spells"][0]["reference"],
        json!("srd2014:spell/fireball")
    );
    assert_eq!(value["body"]["spellcasting"][0]["spells"][1]["name"], json!("Melf's Minute Meteors"));

    // Inventory: ref vs inline, flattened modifier, item properties.
    assert_eq!(value["body"]["inventory"][0]["item"]["type"], json!("ref"));
    assert_eq!(value["body"]["inventory"][1]["item"]["type"], json!("inline"));
    assert_eq!(value["body"]["inventory"][1]["item"]["requires_attunement"], json!(true));
    assert_eq!(value["body"]["inventory"][2]["item"]["properties"]["type"], json!("weapon"));
    assert_eq!(
        value["body"]["inventory"][2]["item"]["properties"]["damage"],
        json!({"type": "roll", "count": 1, "sides": 4})
    );
    assert_eq!(value["body"]["inventory"][1]["item"]["modifiers"][0]["target"], json!("ac"));
    assert_eq!(value["body"]["inventory"][1]["item"]["modifiers"][0]["op"], json!("add"));
    assert_eq!(value["body"]["inventory"][1]["item"]["modifiers"][0]["value"], json!(1));
    assert_eq!(
        value["body"]["inventory"][1]["item"]["modifiers"][0]["when"],
        json!({"type": "attuned"})
    );

    // Features: formula expressions are tagged objects.
    assert_eq!(
        value["body"]["features"][0]["modifiers"][0]["value"],
        json!({"type": "proficiency_bonus"})
    );

    // Statuses: absolute expiry on the campaign clock.
    assert_eq!(value["body"]["statuses"][0]["duration"]["type"], json!("timed"));
    assert_eq!(value["body"]["statuses"][0]["started_at"], json!(3600));
    assert_eq!(
        value["body"]["statuses"][1]["duration"],
        json!({"type": "until_rest", "rest": "long"})
    );

    let back: StatblockEntity = serde_json::from_value(value).expect("entity deserializes");
    assert_eq!(pc, back);
}

#[test]
fn deserializing_a_mismatched_kind_is_rejected() {
    let mut value = serde_json::to_value(sample_pc(PlayerId::new())).expect("entity serializes");
    value["kind"] = json!("note");
    // serde error text is an implementation detail; assert the failure only.
    serde_json::from_value::<StatblockEntity>(value).expect_err("kind/body mismatch must fail");
}

#[test]
fn unknown_fields_are_ignored_for_forward_compat() {
    let pc = sample_pc(PlayerId::new());
    let mut value = serde_json::to_value(&pc).expect("entity serializes");
    value["future_header_field"] = json!({"anything": true});
    value["body"]["future_section"] = json!(1);
    let back: StatblockEntity = serde_json::from_value(value).expect("unknown fields are ignored");
    assert_eq!(pc, back);
}

#[test]
fn any_entity_dispatches_on_kind() {
    let pc = sample_pc(PlayerId::new());
    let pc_value = serde_json::to_value(&pc).expect("entity serializes");

    let any = AnyEntity::from(pc.clone());
    assert_eq!(any.kind(), EntityKind::Pc);
    assert!(any.as_statblock().is_some());
    assert!(any.as_note().is_none());
    assert_eq!(serde_json::to_value(&any).expect("serializes"), pc_value);
    let back: AnyEntity = serde_json::from_value(pc_value).expect("AnyEntity deserializes");
    assert!(back.as_statblock().is_some());

    let note = Entity::builder(
        EntityKind::Note,
        EntityId::new(),
        "Session 12",
        PlayerId::new(),
        NoteBody {
            text: "The party entered the vault.".to_string(),
        },
    )
    .build()
    .expect("a note kind pairs with a note body");
    let note_value = serde_json::to_value(&note).expect("note serializes");
    assert_eq!(note_value["kind"], json!("note"));
    assert_eq!(note_value["body"]["text"], json!("The party entered the vault."));
    let back: AnyEntity = serde_json::from_value(note_value).expect("note deserializes");
    assert!(back.as_note().is_some());

    let handout = Entity::builder(
        EntityKind::Handout,
        EntityId::new(),
        "Handout: The Vault",
        PlayerId::new(),
        HandoutBody {
            content: HandoutContent::Text {
                text: "Read this aloud.".to_string(),
            },
        },
    )
    .build()
    .expect("a handout kind pairs with a handout body");
    let handout_value = serde_json::to_value(&handout).expect("handout serializes");
    assert_eq!(handout_value["body"]["content"]["type"], json!("text"));
    let back: AnyEntity = serde_json::from_value(handout_value).expect("handout deserializes");
    assert!(back.as_handout().is_some());
}

#[test]
fn handout_give_carries_an_inline_item_copy() {
    let dagger = InlineItem {
        name: "Dagger".to_string(),
        description: "A simple blade.".to_string(),
        kind: ItemKind::Weapon,
        requires_attunement: false,
        weight_oz: Some(16),
        value: Some(Coins {
            cp: 0,
            sp: 2,
            ep: 0,
            gp: 0,
            pp: 0,
        }),
        properties: None,
        modifiers: Vec::new(),
        resource: None,
        ext: Ext::new(),
    };
    let handout = Entity::builder(
        EntityKind::Handout,
        EntityId::new(),
        "Handout: Dagger",
        PlayerId::new(),
        HandoutBody {
            content: HandoutContent::Item { item: dagger },
        },
    )
    .build()
    .expect("a handout kind pairs with a handout body");
    let value = serde_json::to_value(&handout).expect("handout serializes");
    assert_eq!(value["body"]["content"]["type"], json!("item"));
    assert_eq!(value["body"]["content"]["item"]["value"]["sp"], json!(2));
}

#[test]
fn stat_key_known_and_custom_round_trip() {
    let ac = StatKey::parse("ac").expect("ac is a known stat");
    assert_eq!(ac, StatKey::Known(KnownStat::Ac));

    let stealth = StatKey::parse("skill.stealth").expect("canonical skill key is known");
    assert_eq!(stealth, StatKey::Known(KnownStat::Skill(Skill::Stealth)));

    let custom = StatKey::parse("house.luck").expect("valid custom key");
    assert!(matches!(custom, StatKey::Custom(_)));
    assert_eq!(custom.to_string(), "house.luck");
    assert_eq!(serde_json::to_value(&custom).expect("serializes"), json!("house.luck"));
    let back: StatKey = serde_json::from_value(json!("house.luck")).expect("deserializes");
    assert_eq!(back, custom);

    assert!(StatKey::parse("Bad Key!").is_err());
}

#[test]
fn ref_id_parses_and_rejects() {
    let r = RefId::parse("srd2014:spell/fireball").expect("valid ref");
    assert_eq!(r.namespace(), "srd2014");
    assert_eq!(r.kind(), "spell");
    assert_eq!(r.slug(), "fireball");
    assert_eq!(r.to_string(), "srd2014:spell/fireball");
    assert_eq!(serde_json::to_value(&r).expect("serializes"), json!("srd2014:spell/fireball"));

    let pack = RefId::parse("pack.mytome:item/foo-bar").expect("pack namespaces may contain dots");
    assert_eq!(pack.namespace(), "pack.mytome");

    assert!(RefId::parse("not-a-ref").is_err());
    assert!(RefId::parse("srd2014:spell/Fire Ball").is_err());
    assert!(RefId::parse("srd2014/spell/fireball").is_err());
}

#[test]
fn schema_version_gate_is_major_only() {
    assert!(SchemaVersion::CURRENT.accepts(SchemaVersion { major: 1, minor: 7 }));
    assert!(SchemaVersion::CURRENT.accepts(SchemaVersion { major: 0, minor: 9 }));
    assert!(!SchemaVersion::CURRENT.accepts(SchemaVersion { major: 2, minor: 0 }));
    assert!(!SchemaVersion::CURRENT.accepts(SchemaVersion { major: 99, minor: 0 }));
}

#[test]
fn mask_is_default_deny() {
    let mut mask = FieldMask::default();
    assert!(mask.is_empty());
    mask.insert(ViewField::Name);
    mask.insert(ViewField::Ac);
    assert!(mask.contains(ViewField::Ac));
    assert!(!mask.contains(ViewField::Hp));
    assert_eq!(mask.iter().count(), 2);
}

#[test]
fn roll_round_trips() {
    let roll = Roll {
        id: RollId::new(),
        roller: PlayerId::new(),
        entity: Some(EntityId::new()),
        ts: UnixTime::from_seconds(1_750_000_042),
        expr: DiceExpr::Add {
            lhs: Box::new(DiceExpr::Advantage {
                inner: Box::new(DiceExpr::Roll {
                    count: 1,
                    sides: 20,
                    keep: None,
                }),
            }),
            rhs: Box::new(DiceExpr::Const { value: 5 }),
        },
        result: localtt_core::types::DiceResult {
            terms: vec![
                TermResult {
                    dice: vec![
                        DieResult { value: 17, kept: true },
                        DieResult { value: 4, kept: false },
                    ],
                    total: 17,
                },
                TermResult {
                    dice: Vec::new(),
                    total: 5,
                },
            ],
            total: 22,
        },
        context: Some(RollContext {
            kind: RollKind::Check,
            stat: Some(StatKey::parse("skill.stealth").expect("known stat")),
            target_dc: Some(15),
        }),
        audience: Audience::RollerAndDm,
    };

    let value = serde_json::to_value(&roll).expect("roll serializes");
    assert_eq!(value["expr"]["type"], json!("add"));
    assert_eq!(value["expr"]["lhs"]["type"], json!("advantage"));
    assert_eq!(value["expr"]["lhs"]["inner"]["type"], json!("roll"));
    assert_eq!(value["result"]["terms"][0]["dice"][1]["kept"], json!(false));
    assert_eq!(value["context"]["stat"], json!("skill.stealth"));
    assert_eq!(value["audience"], json!("roller_and_dm"));

    let back: Roll = serde_json::from_value(value).expect("roll deserializes");
    assert_eq!(roll, back);
}

#[test]
fn feed_entry_round_trips() {
    let entry = FeedEntry {
        id: FeedEntryId::new(41),
        ts: UnixTime::from_seconds(1_750_000_042),
        clock_s: CampaignTime::from_seconds(7_200),
        actor: Some(PlayerId::new()),
        entity: Some(EntityId::new()),
        audience: Audience::Public,
        payload: FeedPayload::Hint {
            text: "Concentration check: DC 12".to_string(),
        },
    };
    let value = serde_json::to_value(&entry).expect("feed entry serializes");
    assert_eq!(value["id"], json!(41));
    assert_eq!(value["payload"]["type"], json!("hint"));
    let back: FeedEntry = serde_json::from_value(value).expect("feed entry deserializes");
    assert_eq!(entry, back);
}

#[test]
fn public_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<StatblockEntity>();
    assert_send_sync::<NoteEntity>();
    assert_send_sync::<HandoutEntity>();
    assert_send_sync::<AnyEntity>();
    assert_send_sync::<Roll>();
    assert_send_sync::<FeedEntry>();
    assert_send_sync::<StatblockBody>();
    assert_send_sync::<CampaignTime>();
}
