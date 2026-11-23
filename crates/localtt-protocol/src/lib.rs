//! Transport DTOs shared by local-tt clients
//! (`docs/architecture.md` section 6).
//!
//! The engine and actor stay transport-free. This crate owns the tagged command
//! input, the stable error projection, and the view, content, sheet, health,
//! diagnostics, response and stream DTOs, plus `parse_dice`, the entry point to
//! core's dice notation parser that clients share. Diagnostic strings are never
//! serialized into error DTOs; unknown engine variants map to explicit
//! `Unknown` values.
//!
//! The DTOs depend on `localtt-core` only. The `engine` feature adds the
//! `From`/`TryFrom` conversions that name engine types (`convert`); the server
//! enables it, the browser client does not, so the WASM bundle never links the
//! engine.

#![deny(missing_docs)]

mod command;
mod content;
#[cfg(feature = "engine")]
mod convert;
mod diagnostics;
pub mod endpoint;
mod error;
mod event;
mod health;
mod outcome;
mod roster;
mod sheet;
mod view;

pub use self::{
    command::{
        CommandDto,
        ItemMoveDto,
        NewSpellDto,
        RollSpecDto,
        SpellPlacementDto,
        StatusSpecDto,
        UnsupportedCommand,
    },
    content::{
        ContentDataDto,
        ContentEntryDto,
        ContentKindDto,
        ContentSummaryDto,
        FeatureDataDto,
        MonsterDataDto,
        SourceInfoDto,
        SpellDataDto,
    },
    diagnostics::{ActorStatsDto, DiagnosticsDto},
    error::{ErrorBody, ErrorCode, ErrorResponse, HttpStatus},
    event::{ElementRefDto, EventDto, WarningCodeDto, WarningDto},
    health::{HealthDto, PROTOCOL_VERSION},
    outcome::{CommandOutcomeDto, ExecutedDto, FeedPageDto, StreamEventDto},
    roster::{CreateRosterEntryRequest, InviteDto},
    sheet::{
        ContributionDto,
        ContributionOpDto,
        ContributionSourceDto,
        DerivedAttackDto,
        DerivedResourceDto,
        DerivedSheetDto,
        DerivedValueDto,
        PactSlotsDto,
        RollHintDto,
        SheetViewDto,
        SlotMaximaDto,
        WeaponAttackKindDto,
    },
    view::{
        CampaignSettingsViewDto,
        CampaignViewDto,
        EntityViewBodyDto,
        EntityViewDto,
        FeedEntryViewDto,
        HandoutViewDto,
        InitiativeEntryViewDto,
        InitiativeViewDto,
        NoteViewDto,
        ProficienciesViewDto,
        RosterAdminEntryDto,
        RosterEntryViewDto,
        StatblockViewDto,
    },
};

/// Parses dice notation with the shared validated grammar (`localtt_core::dice`).
///
/// Client adapters that build [`CommandDto::Roll`] use this instead of carrying
/// a second parser. Errors are display strings; the server still validates the
/// expression after deserialization.
///
/// # Errors
///
/// Returns the engine's syntax or range message when `notation` is malformed.
#[inline]
pub fn parse_dice(notation: &str) -> Result<localtt_core::types::DiceExpr, String> {
    localtt_core::dice::parse(notation).map_err(|error| error.to_string())
}

#[cfg(all(test, feature = "engine"))]
mod tests;
