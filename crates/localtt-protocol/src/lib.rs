//! Transport DTOs and projected values shared by local-tt clients
//! (`docs/rfcs/server.md` 4.3 and 7).
//!
//! The engine and actor stay transport-free. This crate owns the tagged command
//! input, the stable error projection, content/sheet/health DTOs, response and
//! stream shapes, and the projected values the actor returns. Diagnostic strings
//! never cross this boundary; unknown engine variants map to explicit `Unknown`
//! values.

#![deny(missing_docs)]

mod command;
mod content;
mod error;
mod event;
mod health;
mod model;
mod outcome;
mod sheet;

pub use localtt_engine::authz::{CampaignView, EntityView, FeedEntryView};

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

#[cfg(test)]
mod tests;
