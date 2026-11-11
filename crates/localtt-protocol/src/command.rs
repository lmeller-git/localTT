//! The client-to-server command DTOs (`docs/architecture.md` section 6).
//!
//! The engine's [`Command`] stays Rust-only. This module owns the tagged wire
//! form, the unknown fallback for a newer engine, and the conversion into the
//! engine command. Every known variant is accepted; an unknown tag is a
//! client error, never a silently ignored command.

use localtt_core::types::{
    Ability,
    AnyEntity,
    Audience,
    CampaignTime,
    Coins,
    DeathSaves,
    DiceExpr,
    EntityId,
    Formula,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    Modifier,
    PlayerId,
    Recharge,
    RefId,
    RefOrText,
    ResourceId,
    ResourceKind,
    SpellEntryId,
    SpellcastingBlockId,
    StatKey,
    StatusDuration,
    StatusId,
    StatusKind,
    StatusSource,
};
use localtt_engine::commands::{Command, ItemMove, NewSpell, RollSpec, SpellPlacement, StatusSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One line of a `transfer_items` command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemMoveDto {
    /// The source entry.
    pub entry: InventoryEntryId,
    /// How many units to move.
    pub qty: i32,
}

impl From<ItemMoveDto> for ItemMove {
    #[inline]
    fn from(dto: ItemMoveDto) -> Self {
        Self {
            entry: dto.entry,
            qty: dto.qty,
        }
    }
}

/// Where an `add_spell` command puts the new entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpellPlacementDto {
    /// Append to an existing spellcasting block.
    Existing {
        /// The block to append to.
        block: SpellcastingBlockId,
    },
    /// Create a block and append the spell to it.
    New {
        /// Class or other source, referenced or inline.
        source: RefOrText,
        /// Spellcasting ability.
        ability: Ability,
    },
}

impl From<SpellPlacementDto> for SpellPlacement {
    #[inline]
    fn from(dto: SpellPlacementDto) -> Self {
        match dto {
            SpellPlacementDto::Existing { block } => Self::Existing(block),
            SpellPlacementDto::New { source, ability } => Self::New { source, ability },
        }
    }
}

/// The client-supplied part of a new spell entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSpellDto {
    /// Name snapshot; the engine trims it and rejects an empty one.
    pub name: String,
    /// Content reference, when the spell comes from a source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// Level snapshot: 0 for a cantrip, 1 to 9, or absent for unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// Whether the spell starts prepared.
    pub prepared: bool,
}

impl From<NewSpellDto> for NewSpell {
    #[inline]
    fn from(dto: NewSpellDto) -> Self {
        Self {
            name: dto.name,
            reference: dto.reference,
            level: dto.level,
            prepared: dto.prepared,
        }
    }
}

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
    /// Adds a participant to the initiative order or replaces its value.
    SetInitiative {
        /// The participating entity.
        entity: EntityId,
        /// The rolled or entered initiative value.
        value: i32,
    },
    /// Removes one participant; a DM command.
    RemoveInitiative {
        /// The participant to remove.
        entity: EntityId,
    },
    /// Empties the initiative order; a DM command.
    ClearInitiative,
    /// Advances to the next participant and one round of clock; a DM command.
    AdvanceInitiative,
    /// Makes one participant the current turn without moving the clock; a DM
    /// command.
    SetInitiativeTurn {
        /// The participant whose turn it becomes.
        entity: EntityId,
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
    /// Moves inventory lines to a recipient's statblock.
    TransferItems {
        /// The statblock the items leave.
        source: EntityId,
        /// The lines to move.
        moves: Vec<ItemMoveDto>,
        /// The player receiving the items.
        recipient: PlayerId,
        /// The recipient's statblock, when the DM pinned one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<EntityId>,
        /// Optional compare-and-swap on the source's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_source_rev: Option<u64>,
        /// Optional compare-and-swap on the destination's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_destination_rev: Option<u64>,
    },
    /// Moves coins to a recipient's statblock.
    TransferCoins {
        /// The statblock the coins leave.
        source: EntityId,
        /// The exact per-denomination amounts to move.
        amount: Coins,
        /// The player receiving the coins.
        recipient: PlayerId,
        /// The recipient's statblock, when the DM pinned one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<EntityId>,
        /// Optional compare-and-swap on the source's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_source_rev: Option<u64>,
        /// Optional compare-and-swap on the destination's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_destination_rev: Option<u64>,
    },
    /// Moves an entity's ownership to another player.
    TransferEntity {
        /// The entity whose owner changes.
        entity: EntityId,
        /// The player who becomes the owner.
        recipient: PlayerId,
        /// Optional compare-and-swap on the entity's revision.
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
    /// Adds one tracked resource to a statblock.
    AddResource {
        /// The target statblock.
        entity: EntityId,
        /// Display name; trimmed and non-empty.
        name: String,
        /// Resource kind.
        kind: ResourceKind,
        /// Maximum; a constant must be non-negative.
        max: Formula,
        /// Starting value; absent means the constant maximum, or zero for an
        /// expression maximum.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<i32>,
        /// Recharge trigger.
        recharge: Recharge,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Edits one tracked resource; the kind is immutable.
    EditResource {
        /// The target statblock.
        entity: EntityId,
        /// The resource to edit.
        resource: ResourceId,
        /// New display name, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// New maximum, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<Formula>,
        /// New current value, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<i32>,
        /// New recharge trigger, when set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recharge: Option<Recharge>,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Removes one tracked resource.
    RemoveResource {
        /// The target statblock.
        entity: EntityId,
        /// The resource to remove.
        resource: ResourceId,
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
    /// Adds one spell entry to a spellcasting block.
    AddSpell {
        /// The target statblock.
        entity: EntityId,
        /// Where the spell goes.
        placement: SpellPlacementDto,
        /// The client-supplied spell fields.
        spell: NewSpellDto,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Replaces one spell entry's client-editable fields.
    EditSpell {
        /// The target statblock.
        entity: EntityId,
        /// The spell entry to edit.
        spell: SpellEntryId,
        /// The new name snapshot.
        name: String,
        /// The new reference, or absent to clear it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<RefId>,
        /// The new level, or absent for unknown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<u8>,
        /// Whether the spell is prepared.
        prepared: bool,
        /// Optional compare-and-swap on the entity's revision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
    /// Removes one spell entry.
    RemoveSpell {
        /// The target statblock.
        entity: EntityId,
        /// The spell entry to remove.
        spell: SpellEntryId,
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
            CommandDto::SetInitiative { entity, value } => {
                Ok(Self::SetInitiative { entity, value })
            }
            CommandDto::RemoveInitiative { entity } => Ok(Self::RemoveInitiative { entity }),
            CommandDto::ClearInitiative => Ok(Self::ClearInitiative),
            CommandDto::AdvanceInitiative => Ok(Self::AdvanceInitiative),
            CommandDto::SetInitiativeTurn { entity } => Ok(Self::SetInitiativeTurn { entity }),
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
            CommandDto::TransferItems {
                source,
                moves,
                recipient,
                destination,
                expected_source_rev,
                expected_destination_rev,
            } => Ok(Self::TransferItems {
                source,
                moves: moves.into_iter().map(ItemMove::from).collect(),
                recipient,
                destination,
                expected_source_rev,
                expected_destination_rev,
            }),
            CommandDto::TransferCoins {
                source,
                amount,
                recipient,
                destination,
                expected_source_rev,
                expected_destination_rev,
            } => Ok(Self::TransferCoins {
                source,
                amount,
                recipient,
                destination,
                expected_source_rev,
                expected_destination_rev,
            }),
            CommandDto::TransferEntity {
                entity,
                recipient,
                expected_rev,
            } => Ok(Self::TransferEntity {
                entity,
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
            CommandDto::AddResource {
                entity,
                name,
                kind,
                max,
                current,
                recharge,
                expected_rev,
            } => Ok(Self::AddResource {
                entity,
                name,
                kind,
                max,
                current,
                recharge,
                expected_rev,
            }),
            CommandDto::EditResource {
                entity,
                resource,
                name,
                max,
                current,
                recharge,
                expected_rev,
            } => Ok(Self::EditResource {
                entity,
                resource,
                name,
                max,
                current,
                recharge,
                expected_rev,
            }),
            CommandDto::RemoveResource {
                entity,
                resource,
                expected_rev,
            } => Ok(Self::RemoveResource {
                entity,
                resource,
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
            CommandDto::AddSpell {
                entity,
                placement,
                spell,
                expected_rev,
            } => Ok(Self::AddSpell {
                entity,
                placement: placement.into(),
                spell: spell.into(),
                expected_rev,
            }),
            CommandDto::EditSpell {
                entity,
                spell,
                name,
                reference,
                level,
                prepared,
                expected_rev,
            } => Ok(Self::EditSpell {
                entity,
                spell,
                name,
                reference,
                level,
                prepared,
                expected_rev,
            }),
            CommandDto::RemoveSpell {
                entity,
                spell,
                expected_rev,
            } => Ok(Self::RemoveSpell {
                entity,
                spell,
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
