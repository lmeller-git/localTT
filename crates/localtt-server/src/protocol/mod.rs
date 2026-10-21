//! Transport DTOs for the HTTP and WebSocket adapters (`docs/rfcs/server.md`
//! 4.3 and 7).
//!
//! The engine and actor stay transport-free. This module owns the tagged
//! command input, the stable error projection, content/sheet/health DTOs and the
//! response and stream shapes. Diagnostic strings never cross this boundary; unknown
//! engine variants map to explicit `Unknown` values.

mod command;
mod content;
mod error;
mod event;
mod health;
mod outcome;
mod sheet;

pub use self::{
    command::{CommandDto, RollSpecDto, StatusSpecDto, UnsupportedCommand},
    content::{ContentEntryDto, ContentSummaryDto, SourceInfoDto},
    error::{ErrorBody, ErrorCode, ErrorResponse, HttpStatus},
    event::{ElementRefDto, EventDto, WarningCodeDto, WarningDto},
    health::HealthDto,
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

#[cfg(test)]
mod tests;
