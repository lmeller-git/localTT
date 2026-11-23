//! Conversions between the engine's `command` types and their DTOs.

use localtt_engine::commands::{Command, ItemMove, NewSpell, RollSpec, SpellPlacement, StatusSpec};

use crate::{
    CommandDto,
    ItemMoveDto,
    NewSpellDto,
    RollSpecDto,
    SpellPlacementDto,
    StatusSpecDto,
    UnsupportedCommand,
};

impl From<ItemMoveDto> for ItemMove {
    #[inline]
    fn from(dto: ItemMoveDto) -> Self {
        Self {
            entry: dto.entry,
            qty: dto.qty,
        }
    }
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
