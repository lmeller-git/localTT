//! Typed identifiers: the generic [`Id`] and its per-domain aliases, plus the feed position.

use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{
    AnyEntity,
    AttackEntry,
    Campaign,
    ClassLevel,
    FeatureEntry,
    InventoryEntry,
    Player,
    Resource,
    Roll,
    SpellEntry,
    SpellcastingBlock,
    Status,
};

/// Opaque, time-sortable (UUID v7) identifier with a type-level domain tag.
///
/// The tag never appears on the wire: an id serializes as the plain UUID
/// string of its domain type. The marker keeps `Id<T>` `Copy`, `Send` and
/// `Sync` regardless of `T`, so plugins may mint `Id<TheirType>`.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct Id<T> {
    uuid: Uuid,
    #[serde(skip)]
    marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// Generates a fresh v7 id.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::from_uuid(Uuid::now_v7())
    }

    /// Wraps an existing UUID (import, migration, tests).
    #[must_use]
    #[inline]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            marker: PhantomData,
        }
    }

    /// Returns the underlying UUID.
    #[must_use]
    #[inline]
    pub fn as_uuid(&self) -> Uuid {
        self.uuid
    }
}

impl<T> Default for Id<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for Id<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> fmt::Debug for Id<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.uuid, f)
    }
}

impl<T> PartialEq for Id<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl<T> PartialOrd for Id<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.uuid.cmp(&other.uuid)
    }
}

impl<T> fmt::Display for Id<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.uuid, f)
    }
}

/// Campaign document id.
pub type CampaignId = Id<Campaign>;
/// Player id.
pub type PlayerId = Id<Player>;
/// Id of an entity of any body kind.
pub type EntityId = Id<AnyEntity>;
/// Status id.
pub type StatusId = Id<Status>;
/// Resource id.
pub type ResourceId = Id<Resource>;
/// Spellcasting block id.
pub type SpellcastingBlockId = Id<SpellcastingBlock>;
/// Spell entry id.
pub type SpellEntryId = Id<SpellEntry>;
/// Class level id.
pub type ClassLevelId = Id<ClassLevel>;
/// Inventory entry id.
pub type InventoryEntryId = Id<InventoryEntry>;
/// Attack entry id.
pub type AttackEntryId = Id<AttackEntry>;
/// Feature entry id.
pub type FeatureEntryId = Id<FeatureEntry>;
/// Roll id.
pub type RollId = Id<Roll>;

/// Monotonic per-campaign feed position; not a UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeedEntryId(u64);

impl FeedEntryId {
    /// Wraps a feed position.
    #[must_use]
    #[inline]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the feed position.
    #[must_use]
    #[inline]
    pub fn get(self) -> u64 {
        self.0
    }
}
