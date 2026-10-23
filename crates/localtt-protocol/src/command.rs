//! The client-to-server command DTOs (`docs/rfcs/server.md` 4.3).
//!
//! The engine's [`Command`] stays Rust-only. This module owns the tagged wire
//! form, the unknown fallback for a newer engine, and the conversion into the
//! engine command. Every known variant is accepted; an unknown tag is a
//! client error, never a silently ignored command.

use localtt_core::types::{
    AnyEntity,
    Audience,
    CampaignTime,
    DeathSaves,
    DiceExpr,
    EntityId,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    Modifier,
    RefId,
    ResourceId,
    SpellEntryId,
    StatKey,
    StatusDuration,
    StatusId,
    StatusKind,
    StatusSource,
};
use localtt_engine::commands::{Command, RollSpec, StatusSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A tagged command sent to `POST /api/v1/commands`.
///
/// The tag is `type`; variant spelling is `snake_case`. Optional fields are
/// omitted when absent, so a command written by hand can leave them out.
/// [`Self::Unknown`] is the forward-compatible input fallback; it is rejected
/// by [`Command::try_from`] with [`UnsupportedCommand`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CommandDto {
    /// Moves the clock by `by_seconds`; a DM command.
    AdvanceClock {
        /// The signed move in campaign seconds.
        by_seconds: i64,
    },
    /// Sets the clock to an absolute instant; a DM command.
    SetClock {
        /// The new campaign instant; pre-origin times are legal.
        to: CampaignTime,
    },
    /// Creates one entity from a client-supplied document.
    CreateEntity {
        /// The draft document.
        entity: Box<AnyEntity>,
    },
    /// Adds one inventory entry to a statblock.
    AddInventoryItem {
        /// The target statblock.
        entity: EntityId,
        /// The item definition or reference.
        item: Box<InventoryItem>,
        /// Quantity; the engine rejects a negative value.
        qty: i32,
        /// Whether the entry starts carried, equipped or stowed.
        state: ItemState,
        /// Whether the entry starts attuned.
        attuned: bool,
        /// Free-text notes.
        notes: String,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Edits one inventory entry; an absent field leaves it unchanged.
    EditInventoryItem {
        /// The target statblock.
        entity: EntityId,
        /// The entry to edit.
        entry: InventoryEntryId,
        /// New quantity, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        qty: Option<i32>,
        /// New carried/equipped/stowed state, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<ItemState>,
        /// New attunement flag, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attuned: Option<bool>,
        /// New notes, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Removes one inventory entry.
    RemoveInventoryItem {
        /// The target statblock.
        entity: EntityId,
        /// The entry to remove.
        entry: InventoryEntryId,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Copies a handout's item into a recipient's inventory; a DM command.
    GiveHandout {
        /// The handout to give.
        handout: EntityId,
        /// The statblock receiving the copy.
        recipient: EntityId,
        /// Optional compare-and-swap on the recipient's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Adds a status; the engine mints its id and stamps its start.
    AddStatus {
        /// The target entity.
        entity: EntityId,
        /// The client-supplied part of the new status.
        status: StatusSpecDto,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Removes a status from an entity.
    RemoveStatus {
        /// The target entity.
        entity: EntityId,
        /// The status to remove.
        status: StatusId,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Replaces a status's duration.
    EditStatusDuration {
        /// The target entity.
        entity: EntityId,
        /// The status to edit.
        status: StatusId,
        /// The new duration.
        duration: StatusDuration,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Applies damage: temp HP absorbs first, then current.
    ApplyDamage {
        /// The target entity.
        entity: EntityId,
        /// The damage amount; negative amounts are malformed.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Heals: current HP rises, never above max.
    Heal {
        /// The target entity.
        entity: EntityId,
        /// The healing amount; negative amounts are malformed.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Sets temp HP, non-stacking.
    SetTempHp {
        /// The target entity.
        entity: EntityId,
        /// The temp HP amount; negative amounts are malformed.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Replaces an entity's death-save tally.
    SetDeathSaves {
        /// The target entity.
        entity: EntityId,
        /// The new tally; fields above 3 are malformed.
        value: DeathSaves,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Spends a tracked resource; overspending is allowed and warned.
    SpendResource {
        /// The target entity.
        entity: EntityId,
        /// The resource to spend.
        resource: ResourceId,
        /// The amount; negative amounts are malformed.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Restores a tracked resource; clamped when the maximum is constant.
    RestoreResource {
        /// The target entity.
        entity: EntityId,
        /// The resource to restore.
        resource: ResourceId,
        /// The amount; negative amounts are malformed.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// A short rest.
    ShortRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are malformed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// A long rest.
    LongRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are malformed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Rolls dice; the actor comes from the authenticated request.
    Roll {
        /// The entity the roll is for; absent for a free expression.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<EntityId>,
        /// What to roll.
        spec: RollSpecDto,
        /// Audience override; absent means the campaign default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        audience: Option<Audience>,
    },
    /// Casts a known spell from a tracked slot.
    CastSpell {
        /// The caster.
        entity: EntityId,
        /// The known spell entry.
        spell: SpellEntryId,
        /// The slot resource to spend.
        slot: ResourceId,
        /// The declared upcast in slot levels above the spell's level.
        upcast_slots: u32,
        /// The client's concentration request.
        concentrate: bool,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Starts (or replaces) concentration from a client-supplied status.
    BeginConcentration {
        /// The concentrating entity.
        entity: EntityId,
        /// The new status; its kind must be `Concentration`.
        status: StatusSpecDto,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Ends every concentration status on the entity.
    EndConcentration {
        /// The entity.
        entity: EntityId,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Spends one hit die and rolls its healing.
    SpendHitDie {
        /// The creature spending the die.
        entity: EntityId,
        /// The `ResourceKind::HitDie` resource to spend.
        resource: ResourceId,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// A command tag this build does not know.
    ///
    /// This is only produced by deserialization. It is rejected by
    /// [`Command::try_from`] with [`UnsupportedCommand`].
    #[serde(other)]
    Unknown,
}

/// What a `roll` command rolls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RollSpecDto {
    /// Rolls a client-supplied expression.
    Expression {
        /// The expression; validated before rolling.
        expr: DiceExpr,
        /// The stat the roll is for, when any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<StatKey>,
    },
    /// Rolls a check for one stat.
    Check {
        /// The stat, e.g. `"save.wis"`.
        stat: StatKey,
        /// The DC, when the client compares one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_dc: Option<i32>,
    },
}

/// The client-supplied part of a new status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSpecDto {
    /// The status kind.
    pub kind: StatusKind,
    /// Display name.
    pub name: String,
    /// Optional reference into the content catalogs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Optional free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How the status ends.
    pub duration: StatusDuration,
    /// Optional provenance of the status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<StatusSource>,
    /// Stat modifiers the status applies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

impl From<StatusSpecDto> for StatusSpec {
    #[inline]
    fn from(dto: StatusSpecDto) -> Self {
        Self {
            kind: dto.kind,
            name: dto.name,
            reference: dto.reference,
            text: dto.text,
            duration: dto.duration,
            source: dto.source,
            modifiers: dto.modifiers,
        }
    }
}

impl From<RollSpecDto> for RollSpec {
    #[inline]
    fn from(dto: RollSpecDto) -> Self {
        match dto {
            RollSpecDto::Expression { expr, target } => Self::Expression { expr, target },
            RollSpecDto::Check { stat, target_dc } => Self::Check { stat, target_dc },
        }
    }
}

/// A command tag this build does not support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("the command type is not supported by this build")]
pub struct UnsupportedCommand;

impl TryFrom<CommandDto> for Command {
    type Error = UnsupportedCommand;

    #[inline]
    fn try_from(dto: CommandDto) -> Result<Self, Self::Error> {
        match dto {
            CommandDto::AdvanceClock { by_seconds } => Ok(Self::AdvanceClock { by_seconds }),
            CommandDto::SetClock { to } => Ok(Self::SetClock { to }),
            CommandDto::CreateEntity { entity } => Ok(Self::CreateEntity { entity }),
            CommandDto::AddInventoryItem {
                entity,
                item,
                qty,
                state,
                attuned,
                notes,
                expected_rev,
            } => Ok(Self::AddInventoryItem {
                entity,
                item,
                qty,
                state,
                attuned,
                notes,
                expected_rev,
            }),
            CommandDto::EditInventoryItem {
                entity,
                entry,
                qty,
                state,
                attuned,
                notes,
                expected_rev,
            } => Ok(Self::EditInventoryItem {
                entity,
                entry,
                qty,
                state,
                attuned,
                notes,
                expected_rev,
            }),
            CommandDto::RemoveInventoryItem {
                entity,
                entry,
                expected_rev,
            } => Ok(Self::RemoveInventoryItem {
                entity,
                entry,
                expected_rev,
            }),
            CommandDto::GiveHandout {
                handout,
                recipient,
                expected_rev,
            } => Ok(Self::GiveHandout {
                handout,
                recipient,
                expected_rev,
            }),
            CommandDto::AddStatus {
                entity,
                status,
                expected_rev,
            } => Ok(Self::AddStatus {
                entity,
                status: status.into(),
                expected_rev,
            }),
            CommandDto::RemoveStatus {
                entity,
                status,
                expected_rev,
            } => Ok(Self::RemoveStatus {
                entity,
                status,
                expected_rev,
            }),
            CommandDto::EditStatusDuration {
                entity,
                status,
                duration,
                expected_rev,
            } => Ok(Self::EditStatusDuration {
                entity,
                status,
                duration,
                expected_rev,
            }),
            CommandDto::ApplyDamage {
                entity,
                amount,
                expected_rev,
            } => Ok(Self::ApplyDamage {
                entity,
                amount,
                expected_rev,
            }),
            CommandDto::Heal {
                entity,
                amount,
                expected_rev,
            } => Ok(Self::Heal {
                entity,
                amount,
                expected_rev,
            }),
            CommandDto::SetTempHp {
                entity,
                amount,
                expected_rev,
            } => Ok(Self::SetTempHp {
                entity,
                amount,
                expected_rev,
            }),
            CommandDto::SetDeathSaves {
                entity,
                value,
                expected_rev,
            } => Ok(Self::SetDeathSaves {
                entity,
                value,
                expected_rev,
            }),
            CommandDto::SpendResource {
                entity,
                resource,
                amount,
                expected_rev,
            } => Ok(Self::SpendResource {
                entity,
                resource,
                amount,
                expected_rev,
            }),
            CommandDto::RestoreResource {
                entity,
                resource,
                amount,
                expected_rev,
            } => Ok(Self::RestoreResource {
                entity,
                resource,
                amount,
                expected_rev,
            }),
            CommandDto::ShortRest {
                entity,
                advance_seconds,
                expected_rev,
            } => Ok(Self::ShortRest {
                entity,
                advance_seconds,
                expected_rev,
            }),
            CommandDto::LongRest {
                entity,
                advance_seconds,
                expected_rev,
            } => Ok(Self::LongRest {
                entity,
                advance_seconds,
                expected_rev,
            }),
            CommandDto::Roll {
                entity,
                spec,
                audience,
            } => Ok(Self::Roll {
                entity,
                spec: spec.into(),
                audience,
            }),
            CommandDto::CastSpell {
                entity,
                spell,
                slot,
                upcast_slots,
                concentrate,
                expected_rev,
            } => Ok(Self::CastSpell {
                entity,
                spell,
                slot,
                upcast_slots,
                concentrate,
                expected_rev,
            }),
            CommandDto::BeginConcentration {
                entity,
                status,
                expected_rev,
            } => Ok(Self::BeginConcentration {
                entity,
                status: status.into(),
                expected_rev,
            }),
            CommandDto::EndConcentration {
                entity,
                expected_rev,
            } => Ok(Self::EndConcentration {
                entity,
                expected_rev,
            }),
            CommandDto::SpendHitDie {
                entity,
                resource,
                expected_rev,
            } => Ok(Self::SpendHitDie {
                entity,
                resource,
                expected_rev,
            }),
            CommandDto::Unknown => Err(UnsupportedCommand),
        }
    }
}
