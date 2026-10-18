//! U4 target test: derived sheets and roll parts, as a client reads them.
//!
//! This file is the fixed point for `docs/rfcs/rules.md`: client code written
//! against the pinned API before any library code exists. It intentionally does
//! not compile until the U4 skeleton lands; that is the spec-phase contract,
//! not an oversight.

use std::collections::BTreeMap;

use localtt_core::{
    builders::{StatblockBuilder, content_ref, ref_id, stat},
    types::{
        Abilities,
        ArmorProperties,
        CampaignTime,
        Condition,
        DiceExpr,
        EntityId,
        EntityKind,
        Ext,
        FeatureCategory,
        FeatureEntry,
        Formula,
        FormulaExpr,
        InlineItem,
        InventoryEntry,
        InventoryEntryId,
        InventoryItem,
        ItemKind,
        ItemProperties,
        ItemState,
        Modifier,
        ModifierOp,
        Namespace,
        OverrideValue,
        PlayerId,
        Recharge,
        RefId,
        Resource,
        ResourceId,
        ResourceKind,
        ShieldProperties,
        Size,
        StatblockEntity,
        Status,
        StatusDuration,
        StatusId,
        StatusKind,
    },
};
use localtt_engine::{
    content::{ContentData, ContentEntry, ContentKind, Provenance},
    error::{ElementRef, WarningCode},
    rules::{
        ContributionOp,
        ContributionSource,
        DerivedResource,
        DerivedSheet,
        DerivedValue,
        DiceTerm,
        PactSlots,
        RollHint,
        RollParts,
        RulesContext,
        derive,
        roll_parts,
    },
};
use localtt_integration::fixtures;

/// A context over a materialized content map, at campaign second 0.
fn context(content: &BTreeMap<RefId, ContentEntry>) -> RulesContext<'_> {
    RulesContext {
        now: CampaignTime::from_seconds(0),
        content,
    }
}

/// A context at a given campaign instant.
fn context_at(content: &BTreeMap<RefId, ContentEntry>, now: i64) -> RulesContext<'_> {
    RulesContext {
        now: CampaignTime::from_seconds(now),
        content,
    }
}

/// One derived value, which the test only reads after `derive` produced it.
fn value<'a>(sheet: &'a DerivedSheet, key: &str) -> &'a DerivedValue {
    sheet
        .values
        .get(&stat(key).expect("a literal stat key"))
        .unwrap_or_else(|| panic!("{key} is derived"))
}

/// The effective integer of a derived value; text and boolean values panic.
fn effective_int(sheet: &DerivedSheet, key: &str) -> i32 {
    match value(sheet, key).effective {
        Some(OverrideValue::Int(value)) => value,
        other => panic!("{key} has no integer effective value, got {other:?}"),
    }
}

/// One resource's derived maximum, which the test only reads after `derive`.
fn derived_resource<'a>(sheet: &'a DerivedSheet, id: ResourceId) -> &'a DerivedResource {
    sheet
        .resources
        .get(&id)
        .unwrap_or_else(|| panic!("resource {id} is derived"))
}

/// A test hero: a fresh character with the given abilities and an empty sheet.
fn hero(id: EntityId, owner: PlayerId, abilities: Abilities) -> StatblockEntity {
    let mut builder = StatblockBuilder::character();
    builder.abilities(abilities).hp(20, 20).size(Size::Medium);
    builder
        .entity_builder(EntityKind::Pc, id, "Tester", owner)
        .expect("abilities and hp are set")
        .build()
        .expect("a character kind pairs with a statblock body")
}

/// A full inline item with no resource and no attunement requirement.
fn inline_item(
    name: &str,
    kind: ItemKind,
    properties: Option<ItemProperties>,
    modifiers: Vec<Modifier>,
) -> InlineItem {
    InlineItem {
        name: name.to_owned(),
        description: String::new(),
        kind,
        requires_attunement: false,
        weight_oz: None,
        value: None,
        properties,
        modifiers,
        resource: None,
        ext: Ext::default(),
    }
}

/// One inventory entry at a known element id.
fn entry(
    id: InventoryEntryId,
    item: InventoryItem,
    qty: i32,
    state: ItemState,
    attuned: bool,
) -> InventoryEntry {
    InventoryEntry {
        id,
        item,
        qty,
        state,
        attuned,
        notes: String::new(),
    }
}

/// One modifier with the usual attribution fields.
fn modifier(target: &str, op: ModifierOp, when: Option<Condition>, note: Option<&str>) -> Modifier {
    Modifier {
        target: stat(target).expect("a literal stat key"),
        op,
        when,
        note: note.map(ToOwned::to_owned),
    }
}

/// A fixed provenance for the test-local content entries.
fn provenance() -> Provenance {
    Provenance {
        namespace: Namespace::parse("fixture").expect("a literal namespace"),
        source: "rules target".to_owned(),
        source_revision: "v1".to_owned(),
        license: "CC0".to_owned(),
        attribution: String::new(),
        url: None,
    }
}

/// A content entry wrapping one inline item.
fn item_content(id: &str, name: &str, item: InlineItem) -> ContentEntry {
    ContentEntry {
        id: ref_id(id).expect("a literal reference"),
        kind: ContentKind::Item,
        name: name.to_owned(),
        description: String::new(),
        data: ContentData::Item(item),
        provenance: provenance(),
        needs_review: false,
        review_notes: Vec::new(),
        ext: Ext::default(),
    }
}

/// A status with the given name, duration and modifiers, started at zero.
fn status(name: &str, duration: StatusDuration, modifiers: Vec<Modifier>) -> Status {
    Status {
        id: StatusId::new(),
        kind: StatusKind::Effect,
        name: name.to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration,
        source: None,
        modifiers,
    }
}

#[test]
fn the_wizard_derives_the_printed_sheet_values() {
    // Arrange: the golden wizard and an empty content map; her sheet needs no
    // content lookup to derive.
    let party = fixtures::party();
    let aria = fixtures::golden_wizard(party.player);
    let content = BTreeMap::new();

    // Act: derive the sheet.
    let sheet = derive(&aria, &context(&content));

    // Assert: the golden values (catalog DESIGN section 3).
    assert_eq!(effective_int(&sheet, "proficiency_bonus"), 3);
    assert_eq!(effective_int(&sheet, "ability.int"), 18);
    assert_eq!(effective_int(&sheet, "ability_mod.int"), 4);
    assert_eq!(effective_int(&sheet, "save.int"), 7);
    assert_eq!(effective_int(&sheet, "save.wis"), 4);
    assert_eq!(
        effective_int(&sheet, "skill.arcana"),
        10,
        "expertise doubles"
    );
    assert_eq!(effective_int(&sheet, "skill.investigation"), 7);
    assert_eq!(effective_int(&sheet, "skill.athletics"), -1);
    assert_eq!(effective_int(&sheet, "passive.perception"), 11);
    assert_eq!(effective_int(&sheet, "passive.arcana"), 20);
    assert_eq!(effective_int(&sheet, "spell.dc"), 15);
    assert_eq!(effective_int(&sheet, "attack.spell"), 7);
    assert_eq!(effective_int(&sheet, "ac"), 12);
    assert_eq!(effective_int(&sheet, "initiative"), 2);
    assert_eq!(sheet.warnings, Vec::new());
}

#[test]
fn an_override_wins_and_the_computed_value_stays_visible() {
    // Arrange: Mira overrides AC to 15; her unarmored computed AC is 12.
    let party = fixtures::party();
    let mira = fixtures::golden_multiclass(party.player);
    let content = BTreeMap::new();

    // Act: derive the sheet.
    let sheet = derive(&mira, &context(&content));

    // Assert: both values are visible and the override is effective.
    let ac = value(&sheet, "ac");
    assert_eq!(ac.computed, Some(12));
    assert_eq!(ac.override_value, Some(OverrideValue::Int(15)));
    assert_eq!(ac.effective, Some(OverrideValue::Int(15)));
    assert!(
        ac.breakdown
            .iter()
            .any(|contribution| contribution.op == ContributionOp::Add),
        "the computed breakdown survives the override"
    );

    // A stat without an override exposes no override value.
    let int = value(&sheet, "ability_mod.int");
    assert_eq!(int.override_value, None);
    assert_eq!(int.effective, Some(OverrideValue::Int(3)));
}

#[test]
fn spell_slot_maxima_and_hit_dice_follow_the_class_tables() {
    let party = fixtures::party();
    let empty = BTreeMap::new();

    // Aria: a level-5 full caster, d6 hit dice, and a tracked 3rd-level slot
    // whose constant maximum derives as-is.
    let aria = fixtures::golden_wizard(party.player);
    let slot3 = aria.body.resources[0].id;
    let sheet = derive(&aria, &context(&empty));
    assert_eq!(sheet.slots.spell, BTreeMap::from([(1, 4), (2, 3), (3, 2)]));
    assert_eq!(sheet.slots.pact, None);
    assert_eq!(sheet.hit_dice, BTreeMap::from([(6, 5)]));
    assert_eq!(derived_resource(&sheet, slot3).max, Some(2));

    // Borin: a non-caster with one Second Wind use.
    let borin = fixtures::golden_fighter(party.player);
    let second_wind = borin.body.resources[0].id;
    let sheet = derive(&borin, &context(&empty));
    assert_eq!(sheet.slots.spell, BTreeMap::new());
    assert_eq!(sheet.slots.pact, None);
    assert_eq!(sheet.hit_dice, BTreeMap::from([(10, 3)]));
    assert_eq!(derived_resource(&sheet, second_wind).max, Some(1));

    // Lyra: Pact Magic is separate from the multiclass spell table.
    let lyra = fixtures::golden_warlock(party.player);
    let sheet = derive(&lyra, &context(&empty));
    assert_eq!(sheet.slots.spell, BTreeMap::new());
    assert_eq!(sheet.slots.pact, Some(PactSlots { level: 1, count: 2 }));
    assert_eq!(sheet.hit_dice, BTreeMap::from([(8, 2)]));

    // Mira: wizard 3 / cleric 2 combines to caster level 5.
    let mira = fixtures::golden_multiclass(party.player);
    let sheet = derive(&mira, &context(&empty));
    assert_eq!(sheet.slots.spell, BTreeMap::from([(1, 4), (2, 3), (3, 2)]));
    assert_eq!(sheet.hit_dice, BTreeMap::from([(6, 3), (8, 2)]));

    // A creature gets its proficiency bonus from the challenge rating and has
    // no class-derived slots or hit dice.
    let goblin = fixtures::goblin(party.player);
    let sheet = derive(&goblin, &context(&empty));
    assert_eq!(effective_int(&sheet, "proficiency_bonus"), 2);
    assert_eq!(sheet.slots.spell, BTreeMap::new());
    assert_eq!(sheet.slots.pact, None);
    assert_eq!(sheet.hit_dice, BTreeMap::new());
}

#[test]
fn equipped_body_armor_and_shields_compose_ac_with_warnings() {
    // Arrange: a dexterous hero in chain mail (dex cap 0) and a shield, plus a
    // lighter armor that must lose the "best base" comparison.
    let owner = PlayerId::new();
    let mut hero = hero(
        EntityId::new(),
        owner,
        Abilities {
            strength: 8,
            dexterity: 14,
            constitution: 12,
            intelligence: 10,
            wisdom: 10,
            charisma: 10,
        },
    );
    let chain = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        chain,
        InventoryItem::Inline(inline_item(
            "Chain Mail",
            ItemKind::Armor,
            Some(ItemProperties::Armor(ArmorProperties {
                base_ac: 16,
                max_dex_bonus: Some(0),
                stealth_disadvantage: true,
                strength_requirement: Some(13),
            })),
            Vec::new(),
        )),
        1,
        ItemState::Equipped,
        false,
    ));
    let leather = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        leather,
        InventoryItem::Inline(inline_item(
            "Leather Armor",
            ItemKind::Armor,
            Some(ItemProperties::Armor(ArmorProperties {
                base_ac: 11,
                max_dex_bonus: None,
                stealth_disadvantage: false,
                strength_requirement: None,
            })),
            Vec::new(),
        )),
        1,
        ItemState::Equipped,
        false,
    ));
    let shield = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        shield,
        InventoryItem::Inline(inline_item(
            "Shield",
            ItemKind::Shield,
            Some(ItemProperties::Shield(ShieldProperties { ac_bonus: 2 })),
            Vec::new(),
        )),
        1,
        ItemState::Equipped,
        false,
    ));
    let content = BTreeMap::new();

    // Act: derive the sheet.
    let sheet = derive(&hero, &context(&content));

    // Assert: the best applicable base (16, dex capped to 0) plus the best
    // shield (2) wins; both body armors are reported and the unmet strength
    // requirement warns without blocking.
    assert_eq!(effective_int(&sheet, "ac"), 18);
    let ac = value(&sheet, "ac");
    let base = ac
        .breakdown
        .iter()
        .find(|contribution| contribution.source == ContributionSource::Item(chain))
        .expect("the chain mail sets the base");
    assert_eq!(base.op, ContributionOp::Set);
    assert_eq!(base.value, Some(16));
    assert!(
        ac.breakdown.iter().any(|contribution| {
            contribution.source == ContributionSource::Item(shield)
                && contribution.op == ContributionOp::Add
                && contribution.value == Some(2)
        }),
        "the shield adds its bonus"
    );
    assert_eq!(
        sheet
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::OverArmored)
            .count(),
        1,
        "two body armors warn once"
    );
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::StrengthRequirement
            && warning.element == Some(ElementRef::Item(chain))
    }));

    // Act: add a second shield; the best bonus still wins and the clash warns.
    let shield2 = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        shield2,
        InventoryItem::Inline(inline_item(
            "Buckler",
            ItemKind::Shield,
            Some(ItemProperties::Shield(ShieldProperties { ac_bonus: 1 })),
            Vec::new(),
        )),
        1,
        ItemState::Equipped,
        false,
    ));
    let sheet = derive(&hero, &context(&content));

    assert_eq!(effective_int(&sheet, "ac"), 18);
    assert_eq!(
        sheet
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::OverArmored)
            .count(),
        2,
        "the second shield adds its own over-armored warning"
    );
}

#[test]
fn conditions_gate_item_feature_and_status_contributions() {
    let owner = PlayerId::new();
    let mut hero = hero(EntityId::new(), owner, Abilities::all(10));
    let content = BTreeMap::new();

    // An item modifier applies while equipped, never while carried.
    let blade = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        blade,
        InventoryItem::Inline(inline_item(
            "Lucky Blade",
            ItemKind::Weapon,
            None,
            vec![modifier(
                "ability_mod.str",
                ModifierOp::Add {
                    value: Formula::Const(1),
                },
                Some(Condition::Equipped),
                None,
            )],
        )),
        1,
        ItemState::Carried,
        false,
    ));
    // An attuned item needs the attuned flag.
    let ring = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        ring,
        InventoryItem::Inline(inline_item(
            "Ring of War",
            ItemKind::Wondrous,
            None,
            vec![modifier(
                "ability_mod.str",
                ModifierOp::Add {
                    value: Formula::Const(2),
                },
                Some(Condition::Attuned),
                None,
            )],
        )),
        1,
        ItemState::Equipped,
        false,
    ));
    // A feature modifier applies on its own.
    hero.body.features.push(FeatureEntry {
        id: localtt_core::types::FeatureEntryId::new(),
        category: FeatureCategory::Class,
        name: "Brave".to_owned(),
        reference: None,
        text: None,
        resource_id: None,
        modifiers: vec![modifier(
            "ability_mod.str",
            ModifierOp::Add {
                value: Formula::Const(3),
            },
            None,
            Some("Brave"),
        )],
        ext: Ext::default(),
    });
    // A status-gated modifier matches case-insensitively and ignores expiry.
    hero.body.statuses.push(status(
        "Bless",
        StatusDuration::Timed {
            expires_at: CampaignTime::from_seconds(100),
        },
        vec![modifier(
            "ability_mod.str",
            ModifierOp::Add {
                value: Formula::Const(4),
            },
            Some(Condition::StatusActive {
                name: "BLESS".to_owned(),
            }),
            Some("Bless"),
        )],
    ));
    // A custom condition is not modeled and warns.
    let rage = StatusId::new();
    hero.body.statuses.push(Status {
        id: rage,
        kind: StatusKind::Effect,
        name: "Raging".to_owned(),
        reference: None,
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: vec![modifier(
            "ability_mod.str",
            ModifierOp::Add {
                value: Formula::Const(1),
            },
            Some(Condition::Custom {
                name: "while raging".to_owned(),
            }),
            None,
        )],
    });

    // Act: derive with the status active (now < expires_at).
    let sheet = derive(&hero, &context(&content));

    // Assert: carried item out, attuned item out (not attuned), feature in,
    // status in; the custom condition warns and contributes nothing.
    assert_eq!(
        effective_int(&sheet, "ability_mod.str"),
        3 + 4,
        "feature (+3) and Bless (+4) only"
    );
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::UnmodeledCondition
            && warning.element == Some(ElementRef::Status(rage))
    }));

    // Act: equip the blade and attune the ring.
    for candidate in &mut hero.body.inventory {
        if candidate.id == blade {
            candidate.state = ItemState::Equipped;
        }
        if candidate.id == ring {
            candidate.attuned = true;
        }
    }
    let sheet = derive(&hero, &context(&content));
    assert_eq!(effective_int(&sheet, "ability_mod.str"), 1 + 2 + 3 + 4);

    // Act: a rewind past the status expiry drops its contribution; the status
    // stays on the sheet (soft expiry).
    let sheet = derive(&hero, &context_at(&content, 100));
    assert_eq!(effective_int(&sheet, "ability_mod.str"), 1 + 2 + 3);
}

#[test]
fn zero_quantity_and_over_attunement_warn_and_do_not_contribute() {
    let owner = PlayerId::new();
    let mut hero = hero(EntityId::new(), owner, Abilities::all(10));
    let content = BTreeMap::new();

    // A zero-quantity item contributes nothing and warns.
    let ghost_ring = InventoryEntryId::new();
    hero.body.inventory.push(entry(
        ghost_ring,
        InventoryItem::Inline(inline_item(
            "Phantom Ring",
            ItemKind::Wondrous,
            None,
            vec![modifier(
                "ac",
                ModifierOp::Add {
                    value: Formula::Const(1),
                },
                None,
                None,
            )],
        )),
        0,
        ItemState::Equipped,
        false,
    ));
    // Four attuned items exceed the 5e limit of three.
    for index in 0..4 {
        hero.body.inventory.push(entry(
            InventoryEntryId::new(),
            InventoryItem::Inline(inline_item(
                &format!("Charm {index}"),
                ItemKind::Wondrous,
                None,
                Vec::new(),
            )),
            1,
            ItemState::Carried,
            true,
        ));
    }

    // Act: derive the sheet.
    let sheet = derive(&hero, &context(&content));

    // Assert: the phantom contributes nothing; both warnings are scoped.
    assert_eq!(effective_int(&sheet, "ac"), 10);
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::ZeroQuantity
            && warning.element == Some(ElementRef::Item(ghost_ring))
    }));
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::OverAttunement && warning.element.is_none()
    }));
}

#[test]
fn dice_modifiers_warn_in_static_contexts_and_compose_for_rolls() {
    // Arrange: Aria under a Bless-like status: +1d4 to the save group.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let bless = StatusId::new();
    aria.body.statuses.push(Status {
        id: bless,
        kind: StatusKind::Effect,
        name: "Bless".to_owned(),
        reference: Some(ref_id("srd2014:spell/bless").expect("a literal ref")),
        text: None,
        started_at: CampaignTime::from_seconds(0),
        duration: StatusDuration::Indefinite,
        source: None,
        modifiers: vec![modifier(
            "save",
            ModifierOp::Add {
                value: Formula::Expr(FormulaExpr::Dice {
                    expr: DiceExpr::Roll {
                        count: 1,
                        sides: 4,
                        keep: None,
                    },
                }),
            },
            None,
            Some("Bless"),
        )],
    });
    let content = BTreeMap::new();

    // Act: derive the sheet.
    let sheet = derive(&aria, &context(&content));

    // Assert: the group applies to every save with the flat part (0), one
    // warning names the flattened dice leaf, and the static value is unchanged.
    assert_eq!(effective_int(&sheet, "save.wis"), 4);
    assert_eq!(effective_int(&sheet, "save.dex"), 2);
    assert_eq!(effective_int(&sheet, "save.cha"), 0);
    assert_eq!(
        sheet
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::DiceInStaticContext)
            .count(),
        1,
        "one warning per flattened contribution"
    );
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::DiceInStaticContext
            && warning.element == Some(ElementRef::Status(bless))
    }));
    let save = value(&sheet, "save.wis");
    assert!(save.breakdown.iter().any(|contribution| {
        contribution.source == ContributionSource::Status(bless)
            && contribution.op == ContributionOp::Add
            && contribution.value == Some(0)
    }));

    // Act: compose the roll parts for the same save.
    let parts = roll_parts(
        &aria,
        &stat("save.wis").expect("a literal stat"),
        &context(&content),
    );

    // Assert: the flat part is the static value and the dice leaf becomes an
    // attributed term for the roller.
    assert_eq!(
        parts,
        RollParts {
            flat: 4,
            dice: vec![DiceTerm {
                source: Some("Bless".to_owned()),
                expr: DiceExpr::Roll {
                    count: 1,
                    sides: 4,
                    keep: None,
                },
            }],
            hint: RollHint::Normal,
        }
    );
}

#[test]
fn advantage_contributions_cancel_pairwise_into_roll_hints() {
    let owner = PlayerId::new();
    let mut hero = hero(EntityId::new(), owner, Abilities::all(10));
    let content = BTreeMap::new();

    // Two advantage sources and one disadvantage source: net advantage.
    hero.body.statuses.push(status(
        "Focused",
        StatusDuration::Indefinite,
        vec![modifier(
            "skill.athletics",
            ModifierOp::Advantage,
            None,
            Some("Focused"),
        )],
    ));
    hero.body.statuses.push(status(
        "Inspired",
        StatusDuration::Indefinite,
        vec![modifier(
            "check",
            ModifierOp::Advantage,
            None,
            Some("Inspired"),
        )],
    ));
    hero.body.statuses.push(status(
        "Shaken",
        StatusDuration::Indefinite,
        vec![modifier(
            "skill.athletics",
            ModifierOp::Disadvantage,
            None,
            Some("Shaken"),
        )],
    ));

    // Act: derive the sheet.
    let sheet = derive(&hero, &context(&content));

    // Assert: the group's advantage reaches the skill; the pair cancels and
    // leaves a net advantage hint.
    assert_eq!(
        value(&sheet, "skill.athletics").hint,
        Some(RollHint::Advantage)
    );
    assert_eq!(
        value(&sheet, "skill.stealth").hint,
        Some(RollHint::Advantage)
    );
    assert_eq!(value(&sheet, "skill.athletics").computed, Some(0));

    // Act: roll parts carry the same hint.
    let parts = roll_parts(
        &hero,
        &stat("skill.athletics").expect("a literal stat"),
        &context(&content),
    );

    // Assert: a check has no dice of its own and the hint survives.
    assert_eq!(parts.flat, 0);
    assert!(parts.dice.is_empty());
    assert_eq!(parts.hint, RollHint::Advantage);

    // Act: an unhinted stat stays normal.
    let parts = roll_parts(
        &hero,
        &stat("save.con").expect("a literal stat"),
        &context(&content),
    );
    assert_eq!(parts.hint, RollHint::Normal);
}

#[test]
fn unresolved_item_references_warn_and_contribute_nothing() {
    // Arrange: Aria wearing a referenced ring the content map does not hold.
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let ring = InventoryEntryId::new();
    aria.body.inventory.push(entry(
        ring,
        InventoryItem::Ref(
            content_ref("fixture:item/ring-of-protection", "Ring of Protection")
                .expect("a literal reference"),
        ),
        1,
        ItemState::Equipped,
        true,
    ));
    let empty = BTreeMap::new();

    // Act: derive without the entry.
    let sheet = derive(&aria, &context(&empty));

    // Assert: the reference warns, scoped to the entry, and contributes nothing.
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::UnresolvedReference
            && warning.element == Some(ElementRef::Item(ring))
    }));
    assert_eq!(effective_int(&sheet, "ac"), 12);

    // Act: derive with the referenced entry materialized.
    let mut content = BTreeMap::new();
    let mut embedded = inline_item(
        "Ring of Protection",
        ItemKind::Wondrous,
        None,
        vec![modifier(
            "ac",
            ModifierOp::Add {
                value: Formula::Const(1),
            },
            Some(Condition::Attuned),
            None,
        )],
    );
    embedded.requires_attunement = true;
    content.insert(
        ref_id("fixture:item/ring-of-protection").expect("a literal reference"),
        item_content(
            "fixture:item/ring-of-protection",
            "Ring of Protection",
            embedded,
        ),
    );
    let sheet = derive(&aria, &context(&content));

    // Assert: the attuned ring contributes and the warning is gone.
    assert_eq!(effective_int(&sheet, "ac"), 13);
    assert!(
        !sheet
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::UnresolvedReference)
    );
}

#[test]
fn unmodeled_classes_warn_and_unevaluable_maxima_stay_none() {
    // Arrange: a bard (not in the static class tables) and a class-level max.
    let owner = PlayerId::new();
    let mut bard = hero(EntityId::new(), owner, Abilities::all(10));
    bard.body.levels.push(localtt_core::types::ClassLevel {
        id: localtt_core::types::ClassLevelId::new(),
        class: localtt_core::types::RefOrText::Text("Bard".to_owned()),
        level: 3,
        subclass: None,
    });
    let content = BTreeMap::new();

    // Act: derive the sheet.
    let sheet = derive(&bard, &context(&content));

    // Assert: the class is unknown; level-derived maxima stay empty.
    assert_eq!(effective_int(&sheet, "proficiency_bonus"), 2);
    assert!(
        sheet
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::UnmodeledClass)
    );
    assert_eq!(sheet.slots.spell, BTreeMap::new());
    assert_eq!(sheet.hit_dice, BTreeMap::new());

    // Arrange: Aria with a class-level resource maximum (evaluable) and a dice
    // maximum (unevaluable).
    let party = fixtures::party();
    let mut aria = fixtures::golden_wizard(party.player);
    let arcane: ResourceId = ResourceId::new();
    aria.body.resources.push(Resource {
        id: arcane,
        name: "Arcane Recovery".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Expr(FormulaExpr::ClassLevel {
            class: "Wizard".to_owned(),
        }),
        current: 0,
        recharge: Recharge::Long,
    });
    let wild: ResourceId = ResourceId::new();
    aria.body.resources.push(Resource {
        id: wild,
        name: "Wild Surge".to_owned(),
        kind: ResourceKind::Feature,
        max: Formula::Expr(FormulaExpr::Dice {
            expr: DiceExpr::Roll {
                count: 1,
                sides: 4,
                keep: None,
            },
        }),
        current: 0,
        recharge: Recharge::Long,
    });

    // Act: derive the sheet.
    let sheet = derive(&aria, &context(&content));

    // Assert: the class-level max evaluates against the class table; the dice
    // max is unevaluable in a static context and degrades loudly.
    assert_eq!(derived_resource(&sheet, arcane).max, Some(5));
    assert_eq!(derived_resource(&sheet, wild).max, None);
    assert!(sheet.warnings.iter().any(|warning| {
        warning.code == WarningCode::FormulaUnevaluated
            && warning.element == Some(ElementRef::Resource(wild))
    }));
}
