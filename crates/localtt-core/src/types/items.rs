//! Inventory entries, inline items and their weapon, armor and shield properties.

use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::types::{Coins, ContentRef, DiceExpr, Ext, InventoryEntryId, Modifier, Resource};

/// One inventory entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryEntry {
    /// Element id.
    pub id: InventoryEntryId,
    /// Referenced or inline item.
    pub item: InventoryItem,
    /// Quantity; signed, 0 is allowed and warned about by the engine.
    pub qty: i32,
    /// Carried, equipped or stowed.
    pub state: ItemState,
    /// Whether the entry is attuned.
    pub attuned: bool,
    /// Free-text notes.
    pub notes: String,
}

/// Either a reference to pack/SRD content or a full inline definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (core DESIGN.md section 3.6); the core test constructs Inline(InlineItem { .. }) inline, so boxing is deferred to a later pass"
)]
pub enum InventoryItem {
    /// Name snapshot plus reference; description resolves via the source.
    Ref(ContentRef),
    /// Full inline definition.
    Inline(InlineItem),
}

/// Inventory state of an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    /// Carried but not worn or wielded.
    Carried,
    /// Worn or wielded.
    Equipped,
    /// Packed away.
    Stowed,
}

/// Full inline item definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineItem {
    /// Item name.
    pub name: String,
    /// Markdown description, opaque to core.
    pub description: String,
    /// Item category.
    pub kind: ItemKind,
    /// Whether the item requires attunement.
    pub requires_attunement: bool,
    /// Weight in ounces; 1 lb = 16 oz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_oz: Option<i32>,
    /// Value in coins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Coins>,
    /// Weapon/armor/shield properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<ItemProperties>,
    /// Modifiers while carried, equipped or attuned.
    pub modifiers: Vec<Modifier>,
    /// Charge definition; `current` is ignored until instantiated as a
    /// tracked copy in the statblock's resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
    /// Plugin payload slot.
    pub ext: Ext,
}

/// Item category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Weapon.
    Weapon,
    /// Armor.
    Armor,
    /// Shield.
    Shield,
    /// Wondrous item.
    Wondrous,
    /// Adventuring gear.
    Gear,
    /// Anything else.
    Other,
}

/// Category-specific item properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemProperties {
    /// Weapon properties.
    Weapon(WeaponProperties),
    /// Armor properties.
    Armor(ArmorProperties),
    /// Shield properties.
    Shield(ShieldProperties),
}

/// Weapon properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponProperties {
    /// Damage dice, e.g. `1d8`; `None` for utility weapons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<DiceExpr>,
    /// Damage type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<DamageType>,
    /// `(normal, long)` range in feet; reach for melee weapons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_ft: Option<(i32, i32)>,
    /// Versatile damage dice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versatile_damage: Option<DiceExpr>,
    /// Weapon tags.
    pub tags: Vec<WeaponTag>,
}

/// Armor properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmorProperties {
    /// Base armor class.
    pub base_ac: i32,
    /// Maximum Dexterity bonus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_dex_bonus: Option<i32>,
    /// Whether wearing it imposes Stealth disadvantage.
    pub stealth_disadvantage: bool,
    /// Strength requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength_requirement: Option<i32>,
}

/// Shield properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShieldProperties {
    /// Armor class bonus.
    pub ac_bonus: i32,
}

/// Weapon tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponTag {
    /// Finesse.
    Finesse,
    /// Light.
    Light,
    /// Heavy.
    Heavy,
    /// Two-handed.
    TwoHanded,
    /// Versatile.
    Versatile,
    /// Thrown.
    Thrown,
    /// Reach.
    Reach,
    /// Loading.
    Loading,
    /// Ammunition.
    Ammunition,
    /// Special.
    Special,
}

/// Damage types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    /// Acid.
    Acid,
    /// Bludgeoning.
    Bludgeoning,
    /// Cold.
    Cold,
    /// Fire.
    Fire,
    /// Force.
    Force,
    /// Lightning.
    Lightning,
    /// Necrotic.
    Necrotic,
    /// Piercing.
    Piercing,
    /// Poison.
    Poison,
    /// Psychic.
    Psychic,
    /// Radiant.
    Radiant,
    /// Slashing.
    Slashing,
    /// Thunder.
    Thunder,
}
