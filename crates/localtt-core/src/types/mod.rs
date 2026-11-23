//! The domain data model: serialized documents plus their validation. See
//! `DESIGN.md` in the crate root.
//!
//! The module tree follows the section order of `DESIGN.md` section 3. Every
//! public item is re-exported here, so the public path is always
//! `localtt_core::types::Name`; the submodules themselves are private.

mod acl;
mod campaign;
mod dice;
mod entity;
mod error;
mod features;
mod feed;
mod ids;
mod items;
mod modifiers;
mod refs;
mod resources;
mod statblock;
mod stats;
mod time;
mod version;

pub use self::{
    acl::{AclEntry, AclLevel, FieldMask, ViewField},
    campaign::{
        Campaign,
        CampaignSettings,
        Clock,
        Initiative,
        InitiativeEntry,
        Player,
        Role,
        TokenHash,
    },
    dice::{DiceExpr, DiceResult, DieResult, Keep, Roll, RollContext, RollKind, TermResult},
    entity::{
        AnyEntity,
        Body,
        BodyKind,
        Entity,
        EntityBuilder,
        EntityHeader,
        EntityKind,
        HandoutBody,
        HandoutContent,
        HandoutEntity,
        NoteBody,
        NoteEntity,
        StatblockEntity,
    },
    error::{
        AbilityError,
        ChallengeRatingError,
        EntityError,
        NamespaceError,
        RefIdError,
        RulesetIdError,
        StatKeyError,
        TokenHashError,
    },
    features::{AttackEntry, FeatureCategory, FeatureEntry, SpellEntry, SpellcastingBlock},
    feed::{Audience, FeedEntry, FeedPayload},
    ids::{
        AttackEntryId,
        CampaignId,
        ClassLevelId,
        EntityId,
        FeatureEntryId,
        FeedEntryId,
        Id,
        InventoryEntryId,
        PlayerId,
        ResourceId,
        RollId,
        SpellEntryId,
        SpellcastingBlockId,
        StatusId,
        Uuid,
    },
    items::{
        ArmorProperties,
        DamageType,
        InlineItem,
        InventoryEntry,
        InventoryItem,
        ItemKind,
        ItemProperties,
        ItemState,
        ShieldProperties,
        WeaponProperties,
        WeaponTag,
    },
    modifiers::{Condition, Formula, FormulaExpr, Modifier, ModifierOp},
    refs::{ContentRef, Ext, Json, Namespace, RefId, RefOrText, RulesetId},
    resources::{
        Recharge,
        Resource,
        ResourceKind,
        RestKind,
        Status,
        StatusDuration,
        StatusKind,
        StatusSource,
    },
    statblock::{
        Abilities,
        ChallengeRating,
        ClassLevel,
        Coins,
        DeathSaves,
        Hp,
        Identity,
        OverrideValue,
        Proficiencies,
        Profile,
        Size,
        SkillProficiency,
        Speed,
        StatblockBody,
    },
    stats::{Ability, AttackKind, CustomStatKey, KnownStat, Skill, StatGroup, StatKey},
    time::{CampaignTime, DAYS_PER_WEEK, SECONDS_PER_DAY, SECONDS_PER_ROUND, TimeOfDay, UnixTime},
    version::SchemaVersion,
};

#[cfg(test)]
mod tests;
