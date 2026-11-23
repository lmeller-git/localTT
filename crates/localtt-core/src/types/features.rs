//! Attack lines, features and spellcasting blocks.

use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::types::{
    Ability,
    AttackEntryId,
    Ext,
    FeatureEntryId,
    InventoryEntryId,
    Modifier,
    RefId,
    RefOrText,
    ResourceId,
    SpellEntryId,
    SpellcastingBlockId,
};

/// Manual attack line on the sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackEntry {
    /// Element id.
    pub id: AttackEntryId,
    /// Attack name.
    pub name: String,
    /// Displayed base bonus; computed suggestions are UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonus: Option<i32>,
    /// Free-text damage line, e.g. `"1d8+3 slashing"`.
    pub damage_text: String,
    /// Linked inventory entry for equip state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_inventory: Option<InventoryEntryId>,
}

/// Feature entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureEntry {
    /// Element id.
    pub id: FeatureEntryId,
    /// Feature category.
    pub category: FeatureCategory,
    /// Feature name.
    pub name: String,
    /// Content reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Inline description for homebrew features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Resource on this statblock that the feature spends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<ResourceId>,
    /// Modifiers while the feature is active.
    pub modifiers: Vec<Modifier>,
    /// Plugin payload slot.
    pub ext: Ext,
}

/// Feature category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureCategory {
    /// Class feature.
    Class,
    /// Racial trait.
    Race,
    /// Feat.
    Feat,
    /// Creature trait.
    Trait,
    /// Action.
    Action,
    /// Reaction.
    Reaction,
    /// Legendary action.
    Legendary,
    /// Anything else.
    Other,
}

/// One spellcasting block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellcastingBlock {
    /// Element id.
    pub id: SpellcastingBlockId,
    /// Class or other source, referenced or inline.
    pub source: RefOrText,
    /// Spellcasting ability.
    pub ability: Ability,
    /// Known and prepared spells.
    pub spells: Vec<SpellEntry>,
}

/// One spell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellEntry {
    /// Element id.
    pub id: SpellEntryId,
    /// Name snapshot for homebrew and referred spells.
    pub name: String,
    /// Content reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Spell level snapshot: 0 for cantrips, 1 to 9 for leveled spells, and
    /// `None` when the level is unknown or the entry predates schema `1.2`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// Whether the spell is prepared.
    pub prepared: bool,
}
