//! The committed `srd2014` snapshot carries the converter's item properties.
//!
//! These assertions guard the generated payloads themselves: the importer
//! tests build their own records, so without this suite a stale snapshot would
//! still load and every other test would stay green.

use std::path::PathBuf;

use localtt_adapter::reference::{IndexedSources, SHIPPED_DATA_ROOT};
use localtt_core::types::{
    ArmorProperties,
    DamageType,
    DiceExpr,
    InlineItem,
    ItemKind,
    ItemProperties,
    RefId,
    ShieldProperties,
    WeaponProperties,
    WeaponTag,
};
use localtt_engine::content::{ContentData, ReferenceSource};

/// The shipped data root with the committed `srd2014` namespace.
fn shipped() -> IndexedSources {
    IndexedSources::from_dirs(&[PathBuf::from(SHIPPED_DATA_ROOT)])
        .expect("the shipped snapshot loads")
}

/// One item payload from the shipped snapshot.
fn item(source: &IndexedSources, raw: &str) -> InlineItem {
    let id = RefId::parse(raw).expect("a literal reference");
    match source
        .get(&id)
        .expect("a lookup")
        .unwrap_or_else(|| panic!("{raw} is in the shipped snapshot"))
        .data
    {
        ContentData::Item(item) => item,
        other => panic!("{raw} carries an item payload, got {other:?}"),
    }
}

#[test]
fn the_shipped_longsword_carries_weapon_properties() {
    let source = shipped();
    let sword = item(&source, "srd2014:item/longsword");

    assert_eq!(sword.kind, ItemKind::Weapon);
    assert_eq!(
        sword.properties,
        Some(ItemProperties::Weapon(WeaponProperties {
            damage: Some(DiceExpr::Roll {
                count: 1,
                sides: 8,
                keep: None,
            }),
            damage_type: Some(DamageType::Slashing),
            range_ft: None,
            versatile_damage: Some(DiceExpr::Roll {
                count: 1,
                sides: 10,
                keep: None,
            }),
            tags: vec![WeaponTag::Versatile],
        }))
    );
}

#[test]
fn the_shipped_armor_and_shield_carry_ac_properties() {
    let source = shipped();

    assert_eq!(
        item(&source, "srd2014:item/chain-mail").properties,
        Some(ItemProperties::Armor(ArmorProperties {
            base_ac: 16,
            max_dex_bonus: Some(0),
            stealth_disadvantage: true,
            strength_requirement: Some(13),
        }))
    );
    assert_eq!(
        item(&source, "srd2014:item/breastplate").properties,
        Some(ItemProperties::Armor(ArmorProperties {
            base_ac: 14,
            max_dex_bonus: Some(2),
            stealth_disadvantage: false,
            strength_requirement: None,
        }))
    );
    assert_eq!(
        item(&source, "srd2014:item/leather").properties,
        Some(ItemProperties::Armor(ArmorProperties {
            base_ac: 11,
            max_dex_bonus: None,
            stealth_disadvantage: false,
            strength_requirement: None,
        }))
    );
    assert_eq!(
        item(&source, "srd2014:item/shield").properties,
        Some(ItemProperties::Shield(ShieldProperties { ac_bonus: 2 }))
    );
}
