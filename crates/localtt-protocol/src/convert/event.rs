//! Conversions between the engine's `event` types and their DTOs.

use localtt_engine::{
    commands::Event,
    error::{ElementRef, Warning, WarningCode},
};

use crate::{ElementRefDto, EventDto, WarningCodeDto, WarningDto};

impl From<ElementRef> for ElementRefDto {
    #[inline]
    fn from(element: ElementRef) -> Self {
        match element {
            ElementRef::Status(id) => Self::Status { id },
            ElementRef::Resource(id) => Self::Resource { id },
            ElementRef::Item(id) => Self::Item { id },
            ElementRef::Feature(id) => Self::Feature { id },
            ElementRef::Spell(id) => Self::Spell { id },
            ElementRef::SpellcastingBlock(id) => Self::SpellcastingBlock { id },
            _ => Self::Unknown,
        }
    }
}

impl From<WarningCode> for WarningCodeDto {
    #[inline]
    fn from(code: WarningCode) -> Self {
        match code {
            WarningCode::ResourceOverspent => Self::ResourceOverspent,
            WarningCode::NegativeHp => Self::NegativeHp,
            WarningCode::MultipleConcentration => Self::MultipleConcentration,
            WarningCode::DawnRewind => Self::DawnRewind,
            WarningCode::FormulaUnevaluated => Self::FormulaUnevaluated,
            WarningCode::DiceInStaticContext => Self::DiceInStaticContext,
            WarningCode::UnresolvedReference => Self::UnresolvedReference,
            WarningCode::OverArmored => Self::OverArmored,
            WarningCode::OverAttunement => Self::OverAttunement,
            WarningCode::StrengthRequirement => Self::StrengthRequirement,
            WarningCode::ZeroQuantity => Self::ZeroQuantity,
            WarningCode::UnmodeledCondition => Self::UnmodeledCondition,
            WarningCode::UnmodeledClass => Self::UnmodeledClass,
            WarningCode::ConcentrationUnsupported => Self::ConcentrationUnsupported,
            _ => Self::Unknown,
        }
    }
}

impl From<Warning> for WarningDto {
    #[inline]
    fn from(warning: Warning) -> Self {
        Self {
            code: warning.code.into(),
            message: warning.message,
            entity: warning.entity,
            element: warning.element.map(ElementRefDto::from),
        }
    }
}

impl From<Event> for EventDto {
    #[inline]
    fn from(event: Event) -> Self {
        match event {
            Event::EntityChanged { id, rev, kind } => Self::EntityChanged { id, rev, kind },
            Event::EntityRemoved { id } => Self::EntityRemoved { id },
            Event::CampaignChanged => Self::CampaignChanged,
            Event::FeedAppended { ids } => Self::FeedAppended { ids },
            _ => Self::Unknown,
        }
    }
}
