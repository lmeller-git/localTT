//! Derived weapon attacks (see `docs/engine.md` section 3).
//!
//! One line per equipped, usable, damage-dealing weapon, staged through the
//! same contribution pipeline as every other derived value.

use std::collections::BTreeMap;

use localtt_core::types::{
    Ability,
    AttackKind,
    DamageType,
    DiceExpr,
    InventoryEntryId,
    ItemProperties,
    ItemState,
    KnownStat,
    StatKey,
    WeaponProperties,
    WeaponTag,
};

use super::{
    Contribution,
    ContributionOp,
    ContributionSource,
    RollHint,
    collect,
    formula,
    stage_key,
};

/// The two kinds of weapon attack; spell attacks are not derived here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponAttackKind {
    /// A melee attack.
    Melee,
    /// A ranged attack.
    Ranged,
}

/// One equipped weapon's derived attack line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedAttack {
    /// The inventory entry that produced this line.
    pub entry: InventoryEntryId,
    /// The resolved item's display name.
    pub name: String,
    /// Melee or ranged; selects the contribution key.
    pub kind: WeaponAttackKind,
    /// The ability that adds its modifier to attack and damage.
    pub ability: Ability,
    /// Ability modifier plus proficiency plus applicable attack modifiers.
    pub attack_bonus: i32,
    /// The weapon's damage dice, without the ability modifier.
    pub damage: DiceExpr,
    /// The ability modifier added to each damage line.
    pub damage_bonus: i32,
    /// The versatile damage dice, when the weapon has them.
    pub versatile_damage: Option<DiceExpr>,
    /// The weapon's damage type, when known.
    pub damage_type: Option<DamageType>,
    /// Normal and long range in feet, when known.
    pub range_ft: Option<(i32, i32)>,
    /// Net advantage or disadvantage from attack contributions.
    pub hint: RollHint,
    /// The attack-bonus contributions in application order.
    pub breakdown: Vec<Contribution>,
}

/// Derives one attack line per equipped, usable, damage-dealing weapon, in
/// inventory order (see `docs/engine.md` section 3).
pub(super) fn derive(
    items: &[collect::ItemSource<'_>],
    pendings: &[collect::Pending],
    scores: &BTreeMap<Ability, i32>,
    proficiency_bonus: i32,
    resolved: &[Option<Contribution>],
) -> Vec<DerivedAttack> {
    let mut attacks = Vec::new();
    for source in items {
        if source.entry.state != ItemState::Equipped || !source.usable {
            continue;
        }
        let Some(item) = source.item else {
            continue;
        };
        let Some(ItemProperties::Weapon(weapon)) = &item.properties else {
            continue;
        };
        let Some(damage) = &weapon.damage else {
            continue;
        };
        let kind = kind_of(weapon);
        let ability = ability_for(weapon, scores);
        let modifier = formula::ability_modifier(scores[&ability]);
        let base = vec![
            labeled_base(ContributionOp::Set, modifier, ability_label(ability)),
            labeled_base(ContributionOp::Add, proficiency_bonus, "Proficiency"),
        ];
        let staged = stage_key(&attack_key(kind), base, pendings, resolved);
        attacks.push(DerivedAttack {
            entry: source.entry.id,
            name: item.name.clone(),
            kind,
            ability,
            attack_bonus: staged.computed.expect("the attack base stages a `Set`"),
            damage: damage.clone(),
            damage_bonus: modifier,
            versatile_damage: weapon.versatile_damage.clone(),
            damage_type: weapon.damage_type,
            range_ft: weapon.range_ft,
            hint: staged.hint.unwrap_or(RollHint::Normal),
            breakdown: staged.breakdown,
        });
    }
    attacks
}

/// Selects the attack kind: an ammunition weapon is ranged, everything else is
/// melee (see `docs/engine.md` section 3).
fn kind_of(weapon: &WeaponProperties) -> WeaponAttackKind {
    if weapon.tags.contains(&WeaponTag::Ammunition) {
        WeaponAttackKind::Ranged
    } else {
        WeaponAttackKind::Melee
    }
}

/// Selects the ability: ammunition uses Dexterity, finesse the higher of
/// Strength and Dexterity with a tie keeping Strength, everything else
/// Strength (see `docs/engine.md` section 3).
fn ability_for(weapon: &WeaponProperties, scores: &BTreeMap<Ability, i32>) -> Ability {
    if weapon.tags.contains(&WeaponTag::Ammunition) {
        return Ability::Dexterity;
    }
    if weapon.tags.contains(&WeaponTag::Finesse) {
        let strength = formula::ability_modifier(scores[&Ability::Strength]);
        let dexterity = formula::ability_modifier(scores[&Ability::Dexterity]);
        return if dexterity > strength {
            Ability::Dexterity
        } else {
            Ability::Strength
        };
    }
    Ability::Strength
}

/// The contribution key an attack kind stages against.
fn attack_key(kind: WeaponAttackKind) -> StatKey {
    StatKey::Known(KnownStat::Attack(match kind {
        WeaponAttackKind::Melee => AttackKind::Melee,
        WeaponAttackKind::Ranged => AttackKind::Ranged,
    }))
}

/// The display label of one ability.
fn ability_label(ability: Ability) -> &'static str {
    match ability {
        Ability::Strength => "Strength",
        Ability::Dexterity => "Dexterity",
        Ability::Constitution => "Constitution",
        Ability::Intelligence => "Intelligence",
        Ability::Wisdom => "Wisdom",
        Ability::Charisma => "Charisma",
    }
}

/// A rules-base contribution with an explicit display label.
fn labeled_base(op: ContributionOp, value: i32, label: &str) -> Contribution {
    Contribution {
        source: ContributionSource::Base,
        label: label.to_owned(),
        op,
        value: Some(value),
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::{StatblockBuilder, dice_pool, stat},
        types::{
            Abilities,
            Ability,
            CampaignTime,
            ContentRef,
            DamageType,
            EntityId,
            EntityKind,
            Ext,
            FeatureCategory,
            FeatureEntry,
            FeatureEntryId,
            Formula,
            InlineItem,
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
            RefId,
            RefOrText,
            StatblockEntity,
            WeaponProperties,
            WeaponTag,
        },
    };

    use super::*;
    use crate::{
        content::{ContentData, ContentEntry, ContentKind, Provenance},
        error::{ElementRef, WarningCode},
        rules::{DerivedSheet, RulesContext, derive},
    };

    fn context(content: &BTreeMap<RefId, ContentEntry>) -> RulesContext<'_> {
        RulesContext {
            now: CampaignTime::from_seconds(0),
            content,
        }
    }

    fn character(abilities: Abilities, level: i32) -> StatblockBuilder {
        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities).hp(20, 20);
        if level > 0 {
            builder.class_level(RefOrText::Text("Fighter".to_owned()), level, None);
        }
        builder
    }

    fn abilities(strength: i32, dexterity: i32) -> Abilities {
        Abilities {
            strength,
            dexterity,
            ..Abilities::all(10)
        }
    }

    fn weapon(
        damage: Option<DiceExpr>,
        damage_type: Option<DamageType>,
        range_ft: Option<(i32, i32)>,
        versatile_damage: Option<DiceExpr>,
        tags: Vec<WeaponTag>,
    ) -> ItemProperties {
        ItemProperties::Weapon(WeaponProperties {
            damage,
            damage_type,
            range_ft,
            versatile_damage,
            tags,
        })
    }

    fn inline(name: &str, kind: ItemKind, properties: Option<ItemProperties>) -> InventoryItem {
        InventoryItem::Inline(InlineItem {
            name: name.to_owned(),
            description: String::new(),
            kind,
            requires_attunement: false,
            weight_oz: None,
            value: None,
            properties,
            modifiers: Vec::new(),
            resource: None,
            ext: Ext::default(),
        })
    }

    fn hero(builder: StatblockBuilder) -> StatblockEntity {
        builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Tester", PlayerId::new())
            .expect("abilities and hp are set")
    }

    fn derive_sheet(entity: &StatblockEntity) -> DerivedSheet {
        derive(entity, &context(&BTreeMap::new()))
    }

    fn modifier(target: &str, op: ModifierOp) -> Modifier {
        Modifier {
            target: stat(target).expect("a literal stat key"),
            op,
            when: None,
            note: None,
        }
    }

    fn add(amount: i32) -> ModifierOp {
        ModifierOp::Add {
            value: Formula::Const(amount),
        }
    }

    fn feature(name: &str, modifiers: Vec<Modifier>) -> FeatureEntry {
        FeatureEntry {
            id: FeatureEntryId::new(),
            category: FeatureCategory::Feat,
            name: name.to_owned(),
            reference: None,
            text: None,
            resource_id: None,
            modifiers,
            ext: Ext::default(),
        }
    }

    fn attack_of(sheet: &DerivedSheet, entry: InventoryEntryId) -> &DerivedAttack {
        sheet
            .attacks
            .iter()
            .find(|attack| attack.entry == entry)
            .unwrap_or_else(|| panic!("{entry} has an attack line"))
    }

    fn srd_longsword() -> InlineItem {
        let ItemProperties::Weapon(properties) = weapon(
            Some(dice_pool(1, 8)),
            Some(DamageType::Slashing),
            None,
            Some(dice_pool(1, 10)),
            vec![WeaponTag::Versatile],
        ) else {
            unreachable!("the helper builds weapon properties");
        };
        InlineItem {
            name: "Longsword".to_owned(),
            description: String::new(),
            kind: ItemKind::Weapon,
            requires_attunement: false,
            weight_oz: Some(48),
            value: None,
            properties: Some(ItemProperties::Weapon(properties)),
            modifiers: Vec::new(),
            resource: None,
            ext: Ext::default(),
        }
    }

    fn content_entry(item: InlineItem) -> ContentEntry {
        let id = RefId::parse("srd2014:item/longsword").expect("a literal reference");
        ContentEntry {
            id,
            kind: ContentKind::Item,
            name: item.name.clone(),
            description: String::new(),
            data: ContentData::Item(item),
            provenance: Provenance {
                namespace: Namespace::parse("srd2014").expect("a literal namespace"),
                source: "test".to_owned(),
                source_revision: "rev".to_owned(),
                license: "CC0".to_owned(),
                attribution: "test".to_owned(),
                url: None,
            },
            needs_review: false,
            review_notes: Vec::new(),
            ext: Ext::default(),
        }
    }

    #[test]
    fn a_level_five_strength_fighter_gets_the_longsword_line() {
        let mut builder = character(abilities(16, 12), 5);
        let longsword = builder.item(
            inline(
                "Longsword",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 8)),
                    Some(DamageType::Slashing),
                    None,
                    Some(dice_pool(1, 10)),
                    vec![WeaponTag::Versatile],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(builder);

        let sheet = derive_sheet(&entity);

        assert_eq!(sheet.attacks.len(), 1);
        let attack = &sheet.attacks[0];
        assert_eq!(attack.entry, longsword);
        assert_eq!(attack.name, "Longsword");
        assert_eq!(attack.kind, WeaponAttackKind::Melee);
        assert_eq!(attack.ability, Ability::Strength);
        assert_eq!(attack.attack_bonus, 6);
        assert_eq!(attack.damage, dice_pool(1, 8));
        assert_eq!(attack.damage_bonus, 3);
        assert_eq!(attack.versatile_damage, Some(dice_pool(1, 10)));
        assert_eq!(attack.damage_type, Some(DamageType::Slashing));
        assert_eq!(attack.range_ft, None);
        assert_eq!(attack.hint, RollHint::Normal);
        assert_eq!(
            attack.breakdown,
            vec![
                Contribution {
                    source: ContributionSource::Base,
                    label: "Strength".to_owned(),
                    op: ContributionOp::Set,
                    value: Some(3),
                },
                Contribution {
                    source: ContributionSource::Base,
                    label: "Proficiency".to_owned(),
                    op: ContributionOp::Add,
                    value: Some(3),
                },
            ]
        );
    }

    #[test]
    fn finesse_uses_the_higher_modifier_and_strength_wins_a_tie() {
        let mut dexterous = character(abilities(10, 18), 1);
        dexterous.item(
            inline(
                "Dagger",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 4)),
                    Some(DamageType::Piercing),
                    Some((20, 60)),
                    None,
                    vec![WeaponTag::Finesse, WeaponTag::Light, WeaponTag::Thrown],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(dexterous);

        let sheet = derive_sheet(&entity);

        let attack = &sheet.attacks[0];
        assert_eq!(attack.ability, Ability::Dexterity);
        assert_eq!(attack.attack_bonus, 6);
        assert_eq!(attack.damage_bonus, 4);

        let mut tied = character(abilities(14, 14), 1);
        tied.item(
            inline(
                "Dagger",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 4)),
                    Some(DamageType::Piercing),
                    Some((20, 60)),
                    None,
                    vec![WeaponTag::Finesse, WeaponTag::Light, WeaponTag::Thrown],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(tied);

        let sheet = derive_sheet(&entity);

        assert_eq!(
            sheet.attacks[0].ability,
            Ability::Strength,
            "a finesse tie keeps Strength"
        );
    }

    #[test]
    fn ammunition_uses_dexterity_and_a_thrown_weapon_keeps_the_melee_rules() {
        let mut archer = character(abilities(8, 16), 5);
        archer.item(
            inline(
                "Longbow",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 8)),
                    Some(DamageType::Piercing),
                    Some((150, 600)),
                    None,
                    vec![
                        WeaponTag::Ammunition,
                        WeaponTag::Heavy,
                        WeaponTag::TwoHanded,
                    ],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(archer);

        let sheet = derive_sheet(&entity);

        let bow = &sheet.attacks[0];
        assert_eq!(bow.kind, WeaponAttackKind::Ranged);
        assert_eq!(bow.ability, Ability::Dexterity);
        assert_eq!(bow.attack_bonus, 6);
        assert_eq!(bow.damage_bonus, 3);
        assert_eq!(bow.range_ft, Some((150, 600)));

        let mut thrower = character(abilities(16, 12), 5);
        thrower.item(
            inline(
                "Handaxe",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 6)),
                    Some(DamageType::Slashing),
                    Some((20, 60)),
                    None,
                    vec![WeaponTag::Light, WeaponTag::Thrown],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(thrower);

        let sheet = derive_sheet(&entity);

        let axe = &sheet.attacks[0];
        assert_eq!(axe.kind, WeaponAttackKind::Melee);
        assert_eq!(axe.ability, Ability::Strength);
        assert_eq!(axe.range_ft, Some((20, 60)));
    }

    #[test]
    fn only_equipped_usable_damage_dealing_weapons_produce_lines() {
        let properties = || {
            Some(weapon(
                Some(dice_pool(1, 8)),
                Some(DamageType::Slashing),
                None,
                None,
                vec![WeaponTag::Versatile],
            ))
        };
        let mut builder = character(abilities(16, 12), 5);
        let carried = builder.item(
            inline("Carried Sword", ItemKind::Weapon, properties()),
            1,
            ItemState::Carried,
            false,
        );
        let stowed = builder.item(
            inline("Stowed Sword", ItemKind::Weapon, properties()),
            1,
            ItemState::Stowed,
            false,
        );
        let empty = builder.item(
            inline("Empty Sword", ItemKind::Weapon, properties()),
            0,
            ItemState::Equipped,
            false,
        );
        let net = builder.item(
            inline(
                "Net",
                ItemKind::Weapon,
                Some(weapon(
                    None,
                    None,
                    Some((5, 15)),
                    None,
                    vec![WeaponTag::Special, WeaponTag::Thrown],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let gear = builder.item(
            inline("Rope", ItemKind::Gear, None),
            1,
            ItemState::Equipped,
            false,
        );
        let unresolved = builder.item(
            InventoryItem::Ref(ContentRef {
                id: RefId::parse("srd2014:item/ghost-blade").expect("a literal reference"),
                name: "Ghost Blade".to_owned(),
            }),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(builder);

        let sheet = derive_sheet(&entity);

        assert!(sheet.attacks.is_empty());
        let warning = |code: WarningCode, element: ElementRef| {
            sheet
                .warnings
                .iter()
                .any(|warning| warning.code == code && warning.element == Some(element))
        };
        assert!(warning(WarningCode::ZeroQuantity, ElementRef::Item(empty)));
        assert!(warning(
            WarningCode::UnresolvedReference,
            ElementRef::Item(unresolved)
        ));
        for silent in [carried, stowed] {
            assert!(!warning(
                WarningCode::ZeroQuantity,
                ElementRef::Item(silent)
            ));
        }
        assert!(!warning(
            WarningCode::UnresolvedReference,
            ElementRef::Item(net)
        ));
        assert!(!warning(WarningCode::ZeroQuantity, ElementRef::Item(gear)));
    }

    #[test]
    fn an_inline_weapon_and_a_referenced_weapon_behave_identically() {
        let item = srd_longsword();
        let reference = RefId::parse("srd2014:item/longsword").expect("a literal reference");
        let mut builder = character(abilities(16, 12), 5);
        let inline_entry = builder.item(
            InventoryItem::Inline(item.clone()),
            1,
            ItemState::Equipped,
            false,
        );
        let referenced_entry = builder.item(
            InventoryItem::Ref(ContentRef {
                id: reference.clone(),
                name: item.name.clone(),
            }),
            1,
            ItemState::Equipped,
            false,
        );
        let entity = hero(builder);
        let content = BTreeMap::from([(reference, content_entry(item))]);

        let sheet = derive(&entity, &context(&content));

        assert_eq!(sheet.attacks.len(), 2);
        let inline_attack = attack_of(&sheet, inline_entry);
        let referenced_attack = attack_of(&sheet, referenced_entry);
        let mut expected = inline_attack.clone();
        expected.entry = referenced_attack.entry;
        assert_eq!(&expected, referenced_attack);
    }

    #[test]
    fn attack_modifiers_apply_to_the_matching_lines_and_hints_do_not_change_the_bonus() {
        let mut builder = character(abilities(16, 14), 5);
        let longsword = builder.item(
            inline(
                "Longsword",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 8)),
                    Some(DamageType::Slashing),
                    None,
                    None,
                    vec![WeaponTag::Versatile],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let longbow = builder.item(
            inline(
                "Longbow",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 8)),
                    Some(DamageType::Piercing),
                    Some((150, 600)),
                    None,
                    vec![WeaponTag::Ammunition, WeaponTag::TwoHanded],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let mut entity = hero(builder);
        entity
            .body
            .features
            .push(feature("Duelist", vec![modifier("attack.melee", add(2))]));
        entity.body.features.push(feature(
            "Focused",
            vec![modifier("attack", ModifierOp::Advantage)],
        ));

        let sheet = derive_sheet(&entity);

        let melee = attack_of(&sheet, longsword);
        let ranged = attack_of(&sheet, longbow);
        assert_eq!(melee.attack_bonus, 8);
        assert_eq!(ranged.attack_bonus, 5);
        assert_eq!(melee.hint, RollHint::Advantage);
        assert_eq!(ranged.hint, RollHint::Advantage);
        assert!(
            melee
                .breakdown
                .iter()
                .any(|contribution| contribution.label == "Duelist")
        );
        assert!(
            !ranged
                .breakdown
                .iter()
                .any(|contribution| contribution.label == "Duelist")
        );
    }

    #[test]
    fn a_generic_attack_override_does_not_feed_a_weapon_line() {
        let mut builder = character(abilities(16, 12), 5);
        let longsword = builder.item(
            inline(
                "Longsword",
                ItemKind::Weapon,
                Some(weapon(
                    Some(dice_pool(1, 8)),
                    Some(DamageType::Slashing),
                    None,
                    None,
                    vec![WeaponTag::Versatile],
                )),
            ),
            1,
            ItemState::Equipped,
            false,
        );
        let mut entity = hero(builder);
        for key in ["attack", "attack.melee"] {
            entity
                .body
                .overrides
                .insert(stat(key).expect("a literal stat"), OverrideValue::Int(99));
        }

        let sheet = derive_sheet(&entity);

        let attack = attack_of(&sheet, longsword);
        assert_eq!(attack.attack_bonus, 6, "the weapon line keeps its own base");
        assert_eq!(
            sheet
                .values
                .get(&stat("attack.melee").expect("a literal stat"))
                .and_then(|value| value.effective.clone()),
            Some(OverrideValue::Int(99)),
            "the generic key still materializes its override"
        );
    }

    #[test]
    fn a_dice_attack_modifier_warns_once_for_all_weapons() {
        let mut builder = character(abilities(16, 14), 5);
        for name in ["Longsword", "Longbow"] {
            builder.item(
                inline(
                    name,
                    ItemKind::Weapon,
                    Some(weapon(
                        Some(dice_pool(1, 8)),
                        Some(DamageType::Slashing),
                        None,
                        None,
                        Vec::new(),
                    )),
                ),
                1,
                ItemState::Equipped,
                false,
            );
        }
        let mut entity = hero(builder);
        let bless = feature(
            "Bless",
            vec![modifier(
                "attack",
                ModifierOp::Add {
                    value: Formula::dice(dice_pool(1, 4)),
                },
            )],
        );
        let bless_id = bless.id;
        entity.body.features.push(bless);

        let sheet = derive_sheet(&entity);

        assert_eq!(sheet.attacks.len(), 2);
        assert_eq!(
            sheet
                .warnings
                .iter()
                .filter(|warning| warning.code == WarningCode::DiceInStaticContext
                    && warning.element == Some(ElementRef::Feature(bless_id)))
                .count(),
            1,
            "the contribution warns once, not once per weapon"
        );
        for attack in &sheet.attacks {
            assert!(
                attack
                    .breakdown
                    .iter()
                    .any(|contribution| contribution.label == "Bless"
                        && contribution.op == ContributionOp::Add
                        && contribution.value == Some(0)),
                "the dice modifier flattens to 0 on every line"
            );
        }
    }

    #[test]
    fn multiple_equipped_weapons_keep_inventory_order() {
        let mut builder = character(abilities(16, 14), 5);
        for name in ["First", "Second", "Third"] {
            builder.item(
                inline(
                    name,
                    ItemKind::Weapon,
                    Some(weapon(
                        Some(dice_pool(1, 6)),
                        Some(DamageType::Slashing),
                        None,
                        None,
                        Vec::new(),
                    )),
                ),
                1,
                ItemState::Equipped,
                false,
            );
        }
        let entity = hero(builder);

        let sheet = derive_sheet(&entity);

        assert_eq!(
            sheet
                .attacks
                .iter()
                .map(|attack| attack.name.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second", "Third"]
        );
    }
}
