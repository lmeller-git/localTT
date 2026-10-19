//! AC bases: body armor, shields, strength requirements and over-armor
//! warnings (RFC `rules.md` 5.6 and section 6).

use localtt_core::types::{ArmorProperties, EntityId, InlineItem, ItemProperties, ItemState};

use super::{Contribution, ContributionOp, ContributionSource, base_contribution, collect};
use crate::error::{ElementRef, Warning, WarningCode};

/// The AC contributions armor and shields provide, plus their warnings.
pub(super) struct ArmorBase {
    /// The structural base contributions, staged before the item modifiers.
    pub(super) contributions: Vec<Contribution>,
    /// The armor warnings.
    pub(super) warnings: Vec<Warning>,
}

/// Derives the AC base from the entity's equipped body armor and shields
/// (`rules.md` 5.6).
///
/// `dex_modifier` and `strength_score` are read from the final computed
/// ability scores. Body armor: the highest `base_ac + min(dex, cap)` wins,
/// ties keep the earliest entry; without one, AC starts from the unarmored
/// base. Shields: the highest `ac_bonus` wins, ties earliest.
pub(super) fn ac_base(
    items: &[collect::ItemSource<'_>],
    dex_modifier: i32,
    strength_score: i32,
    entity: EntityId,
) -> ArmorBase {
    let mut contributions = Vec::new();
    let mut warnings = Vec::new();

    let armors = candidates(items, |item| match &item.properties {
        Some(ItemProperties::Armor(armor)) => Some(*armor),
        _ => None,
    });
    if armors.len() > 1 {
        warnings.push(over_armored(entity, "more than one body armor is equipped"));
    }
    let mut best_armor: Option<(&collect::ItemSource<'_>, ArmorProperties, i32)> = None;
    for (source, armor) in armors {
        let dex = match armor.max_dex_bonus {
            Some(cap) => dex_modifier.min(cap),
            None => dex_modifier,
        };
        let value = armor.base_ac.saturating_add(dex);
        if best_armor.is_none_or(|(_, _, best)| value > best) {
            best_armor = Some((source, armor, value));
        }
    }
    match best_armor {
        Some((source, armor, value)) => {
            let item = source.item.expect("a candidate has a resolved item");
            if let Some(required) = armor.strength_requirement
                && required > strength_score
            {
                warnings.push(Warning {
                    code: WarningCode::StrengthRequirement,
                    message: format!(
                        "`{}` requires a Strength score of {required} or more",
                        item.name
                    ),
                    entity: Some(entity),
                    element: Some(ElementRef::Item(source.entry.id)),
                });
            }
            contributions.push(Contribution {
                source: ContributionSource::Item(source.entry.id),
                label: item.name.clone(),
                op: ContributionOp::Set,
                value: Some(value),
            });
        }
        None => {
            contributions.push(base_contribution(ContributionOp::Set, 10));
            contributions.push(base_contribution(ContributionOp::Add, dex_modifier));
        }
    }

    let shields = candidates(items, |item| match &item.properties {
        Some(ItemProperties::Shield(shield)) => Some(*shield),
        _ => None,
    });
    if shields.len() > 1 {
        warnings.push(over_armored(entity, "more than one shield is equipped"));
    }
    let mut best_shield: Option<(&collect::ItemSource<'_>, i32)> = None;
    for (source, shield) in shields {
        if best_shield.is_none_or(|(_, best)| shield.ac_bonus > best) {
            best_shield = Some((source, shield.ac_bonus));
        }
    }
    if let Some((source, bonus)) = best_shield {
        let item = source.item.expect("a candidate has a resolved item");
        contributions.push(Contribution {
            source: ContributionSource::Item(source.entry.id),
            label: item.name.clone(),
            op: ContributionOp::Add,
            value: Some(bonus),
        });
    }

    ArmorBase {
        contributions,
        warnings,
    }
}

/// The equipped, usable, resolved entries whose item carries a properties
/// payload of the requested kind, in document order.
fn candidates<'i, 'a, T>(
    items: &'i [collect::ItemSource<'a>],
    properties: impl Fn(&InlineItem) -> Option<T>,
) -> Vec<(&'i collect::ItemSource<'a>, T)> {
    items
        .iter()
        .filter(|source| source.usable && source.entry.state == ItemState::Equipped)
        .filter_map(|source| {
            let item = source.item?;
            Some((source, properties(item)?))
        })
        .collect()
}

/// The element-less over-armor warning.
fn over_armored(entity: EntityId, message: &str) -> Warning {
    Warning {
        code: WarningCode::OverArmored,
        message: message.to_owned(),
        entity: Some(entity),
        element: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use localtt_core::{
        builders::{StatblockBuilder, stat},
        types::{
            Abilities,
            CampaignTime,
            Condition,
            EntityId,
            EntityKind,
            Ext,
            Formula,
            InventoryEntry,
            InventoryEntryId,
            InventoryItem,
            ItemKind,
            ItemState,
            Modifier,
            ModifierOp,
            OverrideValue,
            PlayerId,
            RefId,
            ShieldProperties,
            StatblockEntity,
        },
    };

    use super::*;
    use crate::{
        content::ContentEntry,
        rules::{DerivedSheet, RulesContext, derive},
    };

    fn hero() -> StatblockEntity {
        let mut builder = StatblockBuilder::character();
        builder
            .abilities(Abilities {
                strength: 8,
                dexterity: 14,
                constitution: 12,
                intelligence: 10,
                wisdom: 10,
                charisma: 10,
            })
            .hp(20, 20);
        builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Tester", PlayerId::new())
            .expect("abilities and hp are set")
    }

    fn inline(
        name: &str,
        properties: Option<ItemProperties>,
        modifiers: Vec<Modifier>,
    ) -> InlineItem {
        InlineItem {
            name: name.to_owned(),
            description: String::new(),
            kind: ItemKind::Armor,
            requires_attunement: false,
            weight_oz: None,
            value: None,
            properties,
            modifiers,
            resource: None,
            ext: Ext::default(),
        }
    }

    fn entry(id: InventoryEntryId, item: InlineItem, qty: i32, state: ItemState) -> InventoryEntry {
        InventoryEntry {
            id,
            item: InventoryItem::Inline(item),
            qty,
            state,
            attuned: false,
            notes: String::new(),
        }
    }

    fn armor(base_ac: i32, max_dex_bonus: Option<i32>) -> ArmorProperties {
        ArmorProperties {
            base_ac,
            max_dex_bonus,
            stealth_disadvantage: false,
            strength_requirement: None,
        }
    }

    fn derive_ac(
        entity: &StatblockEntity,
        content: &BTreeMap<RefId, ContentEntry>,
    ) -> DerivedSheet {
        derive(
            entity,
            &RulesContext {
                now: CampaignTime::from_seconds(0),
                content,
            },
        )
    }

    fn effective_ac(sheet: &DerivedSheet) -> i32 {
        match &sheet
            .values
            .get(&stat("ac").expect("a literal stat"))
            .expect("ac always derives")
            .effective
        {
            Some(OverrideValue::Int(value)) => *value,
            other => panic!("ac has no integer effective value, got {other:?}"),
        }
    }

    #[test]
    fn the_highest_armor_value_wins_and_ties_keep_the_earliest() {
        let tied = InventoryEntryId::new();
        let mut entity = hero();
        entity.body.inventory.push(entry(
            tied,
            inline(
                "Leather A",
                Some(ItemProperties::Armor(armor(12, None))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Leather B",
                Some(ItemProperties::Armor(armor(12, None))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        let content = BTreeMap::new();

        let sheet = derive_ac(&entity, &content);

        assert_eq!(effective_ac(&sheet), 14, "12 base plus the dex modifier 2");
        let base = sheet
            .values
            .get(&stat("ac").expect("a literal stat"))
            .expect("ac derives")
            .breakdown
            .iter()
            .find(|contribution| contribution.op == ContributionOp::Set)
            .expect("a set base applies");
        assert_eq!(
            base.source,
            ContributionSource::Item(tied),
            "the tie keeps the earliest entry"
        );

        // A better armor takes over.
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Half Plate",
                Some(ItemProperties::Armor(armor(15, Some(0)))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        let sheet = derive_ac(&entity, &content);
        assert_eq!(effective_ac(&sheet), 15);
    }

    #[test]
    fn the_dexterity_cap_clamps_the_armor_bonus() {
        let mut entity = hero();
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Chain Mail",
                Some(ItemProperties::Armor(armor(16, Some(0)))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        let content = BTreeMap::new();

        assert_eq!(effective_ac(&derive_ac(&entity, &content)), 16);

        // No armor: the unarmored base keeps the full dexterity modifier.
        entity.body.inventory.clear();
        assert_eq!(effective_ac(&derive_ac(&entity, &content)), 12);
    }

    #[test]
    fn carried_armor_and_stowed_shields_contribute_nothing() {
        let mut entity = hero();
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Chain Mail",
                Some(ItemProperties::Armor(armor(16, Some(0)))),
                Vec::new(),
            ),
            1,
            ItemState::Carried,
        ));
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Shield",
                Some(ItemProperties::Shield(ShieldProperties { ac_bonus: 2 })),
                Vec::new(),
            ),
            1,
            ItemState::Stowed,
        ));
        let content = BTreeMap::new();

        assert_eq!(effective_ac(&derive_ac(&entity, &content)), 12);
    }

    #[test]
    fn the_dex_cap_does_not_neutralize_a_negative_modifier() {
        let mut entity = hero();
        entity.body.abilities.dexterity = 6;
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Leather",
                Some(ItemProperties::Armor(armor(11, Some(4)))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        let content = BTreeMap::new();

        assert_eq!(
            effective_ac(&derive_ac(&entity, &content)),
            9,
            "11 plus the -2 dex modifier"
        );
    }

    #[test]
    fn over_armor_warnings_are_per_category_and_element_less() {
        let mut entity = hero();
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Leather",
                Some(ItemProperties::Armor(armor(11, None))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Chain Mail",
                Some(ItemProperties::Armor(armor(16, Some(0)))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Shield",
                Some(ItemProperties::Shield(ShieldProperties { ac_bonus: 2 })),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Buckler",
                Some(ItemProperties::Shield(ShieldProperties { ac_bonus: 1 })),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        let entity_id = entity.id;
        let content = BTreeMap::new();

        let sheet = derive_ac(&entity, &content);

        assert_eq!(effective_ac(&sheet), 18, "chain 16 plus the best shield 2");
        let over_armored: Vec<_> = sheet
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::OverArmored)
            .collect();
        assert_eq!(over_armored.len(), 2, "one per category");
        assert!(over_armored.iter().all(|warning| warning.element.is_none()));
        assert!(
            sheet
                .warnings
                .iter()
                .all(|warning| warning.entity == Some(entity_id))
        );
    }

    #[test]
    fn a_strength_requirement_warns_on_the_winner_only() {
        let wearing = InventoryEntryId::new();
        let mut entity = hero();
        let mut heavy = armor(16, Some(0));
        heavy.strength_requirement = Some(13);
        entity.body.inventory.push(entry(
            wearing,
            inline("Chain Mail", Some(ItemProperties::Armor(heavy)), Vec::new()),
            1,
            ItemState::Equipped,
        ));
        // A second armor that also demands Strength loses the comparison and
        // stays silent.
        let mut loser = armor(15, Some(0));
        loser.strength_requirement = Some(15);
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline("Ring Mail", Some(ItemProperties::Armor(loser)), Vec::new()),
            1,
            ItemState::Equipped,
        ));
        let content = BTreeMap::new();

        let sheet = derive_ac(&entity, &content);

        let strength_warnings: Vec<_> = sheet
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::StrengthRequirement)
            .collect();
        assert_eq!(strength_warnings.len(), 1);
        assert_eq!(
            strength_warnings[0].element,
            Some(ElementRef::Item(wearing))
        );
    }

    #[test]
    fn equipped_item_modifiers_add_on_top_of_the_armor_base() {
        let mut entity = hero();
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Leather",
                Some(ItemProperties::Armor(armor(11, None))),
                Vec::new(),
            ),
            1,
            ItemState::Equipped,
        ));
        entity.body.inventory.push(entry(
            InventoryEntryId::new(),
            inline(
                "Ring of Protection",
                None,
                vec![Modifier {
                    target: stat("ac").expect("a literal stat"),
                    op: ModifierOp::Add {
                        value: Formula::Const(1),
                    },
                    when: Some(Condition::Attuned),
                    note: None,
                }],
            ),
            1,
            ItemState::Equipped,
        ));
        entity.body.inventory[1].attuned = true;
        let content = BTreeMap::new();

        assert_eq!(
            effective_ac(&derive_ac(&entity, &content)),
            14,
            "11 + 2 dex + 1 ring"
        );
    }
}
