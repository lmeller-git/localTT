//! Access grants and the closed set of maskable sheet sections.

use std::{collections::BTreeSet, hash::Hash};

use serde::{Deserialize, Serialize};

use crate::types::PlayerId;

/// Access level of an ACL entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AclLevel {
    /// Read access.
    Read,
    /// Read and write access.
    Write,
}

/// One access grant on an entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEntry {
    /// The grantee.
    pub player: PlayerId,
    /// The granted access level.
    pub level: AclLevel,
    /// Present and default-deny only for read-shares: only the listed sections
    /// are exposed. `None` means not field-masked. Masks apply to any entity,
    /// not only DM-owned shares; a `Write` grant ignores the mask.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_mask: Option<FieldMask>,
}

/// A union of sheet sections; `Default` is empty, which means nothing visible.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldMask(BTreeSet<ViewField>);

impl FieldMask {
    /// True when the section is contained in the mask.
    #[must_use]
    #[inline]
    pub fn contains(&self, f: ViewField) -> bool {
        self.0.contains(&f)
    }

    /// Adds a section to the mask.
    #[inline]
    pub fn insert(&mut self, f: ViewField) {
        self.0.insert(f);
    }

    /// True when no section is visible.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates the contained sections.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = ViewField> + '_ {
        self.0.iter().copied()
    }
}

/// Redactable sheet sections; `Ext` is a single bucket for all plugin data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewField {
    /// Entity name.
    Name,
    /// Identity box.
    Identity,
    /// Ability scores.
    Abilities,
    /// Saving throw proficiencies.
    Saves,
    /// Skill proficiencies.
    Skills,
    /// Tools and languages.
    Proficiencies,
    /// Hit points.
    Hp,
    /// Armor class.
    Ac,
    /// Movement speeds.
    Speed,
    /// Death saves.
    DeathSaves,
    /// Coins.
    Coins,
    /// Derived-value overrides.
    Overrides,
    /// Resources.
    Resources,
    /// Spells.
    Spells,
    /// Inventory.
    Inventory,
    /// Attack lines.
    Attacks,
    /// Features.
    Features,
    /// Statuses.
    Statuses,
    /// Notes.
    Notes,
    /// Plugin data.
    Ext,
}
