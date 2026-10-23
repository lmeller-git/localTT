//! Transport DTOs and projected values shared by local-tt clients
//! (`docs/rfcs/server.md` 4.3 and 7).
//!
//! The engine and actor stay transport-free. This crate owns the tagged command
//! input, the stable error projection, content/sheet/health DTOs, response and
//! stream shapes, the projected values the actor returns, and the dice parser
//! shared with clients. Diagnostic strings are never serialized into error DTOs;
//! unknown engine variants map to explicit `Unknown` values.

#![deny(missing_docs)]

mod command;
mod content;
mod error;
mod event;
mod health;
mod model;
mod outcome;
mod sheet;

pub use localtt_engine::authz::{
    CampaignSettingsView,
    CampaignView,
    EntityView,
    EntityViewBody,
    FeedEntryView,
    HandoutView,
    NoteView,
    RosterEntryView,
    StatblockView,
};

pub use self::{
    command::{CommandDto, RollSpecDto, StatusSpecDto, UnsupportedCommand},
    content::{ContentEntryDto, ContentSummaryDto, SourceInfoDto},
    error::{ErrorBody, ErrorCode, ErrorResponse, HttpStatus},
    event::{ElementRefDto, EventDto, WarningCodeDto, WarningDto},
    health::HealthDto,
    model::{CommandOutcome, FeedPage, StreamEvent},
    outcome::{CommandOutcomeDto, ExecutedDto, FeedPageDto, StreamEventDto},
    sheet::{
        ContributionDto,
        ContributionOpDto,
        ContributionSourceDto,
        DerivedResourceDto,
        DerivedSheetDto,
        DerivedValueDto,
        PactSlotsDto,
        RollHintDto,
        SheetViewDto,
        SlotMaximaDto,
    },
};

/// Parses dice notation with the engine's validated grammar.
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
    localtt_engine::dice::parse(notation).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
