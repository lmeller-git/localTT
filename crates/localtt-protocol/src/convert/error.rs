//! Conversions between the engine's `error` types and their DTOs.

use localtt_engine::{content::ContentFault, error::EngineError, ports::store::StoreFault};

use crate::{ElementRefDto, ErrorCode, ErrorResponse, HttpStatus};

impl ErrorResponse {
    /// Maps an engine failure to a safe response.
    #[must_use]
    #[inline]
    pub fn from_engine(error: &EngineError) -> Self {
        match error {
            EngineError::Unauthorized { entity, .. } => {
                let mut response = Self::new(HttpStatus::Forbidden, ErrorCode::Unauthorized);
                response.body.entity = *entity;
                response
            }
            EngineError::NotFound { entity, element } => {
                let mut response = Self::new(HttpStatus::NotFound, ErrorCode::NotFound);
                response.body.entity = Some(*entity);
                response.body.element = element.map(ElementRefDto::from);
                response
            }
            EngineError::AlreadyExists { entity } => {
                let mut response = Self::new(HttpStatus::Conflict, ErrorCode::AlreadyExists);
                response.body.entity = Some(*entity);
                response
            }
            EngineError::PlayerNotFound { player: _ } => {
                Self::new(HttpStatus::NotFound, ErrorCode::NotFound)
            }
            EngineError::PlayerExists { player: _ } => {
                Self::new(HttpStatus::Conflict, ErrorCode::AlreadyExists)
            }
            EngineError::PlayerProtected { player: _ } => {
                Self::new(HttpStatus::Forbidden, ErrorCode::Unauthorized)
            }
            EngineError::PlayerTokenInUse { player: _ } => {
                Self::new(HttpStatus::Conflict, ErrorCode::Conflict)
            }
            EngineError::Conflict {
                entity,
                expected,
                actual,
            } => {
                let mut response = Self::new(HttpStatus::Conflict, ErrorCode::Conflict);
                response.body.entity = Some(*entity);
                response.body.expected_rev = Some(*expected);
                response.body.actual_rev = Some(*actual);
                response
            }
            EngineError::Malformed { .. } => Self::malformed(),
            EngineError::TooLarge { what, limit } => {
                let mut response = Self::new(HttpStatus::BadRequest, ErrorCode::TooLarge);
                response.body.what = Some((*what).to_owned());
                response.body.limit = Some(*limit);
                response
            }
            EngineError::Store(error) => Self::from_store(error),
            EngineError::Environment { .. } => Self::new(
                HttpStatus::InternalServerError,
                ErrorCode::EnvironmentUnavailable,
            ),
            EngineError::Content(error) => Self::from_content(error),
            _ => Self::new(HttpStatus::InternalServerError, ErrorCode::Internal),
        }
    }

    /// Maps a store failure to a safe response.
    #[must_use]
    #[inline]
    pub fn from_store(error: &StoreFault) -> Self {
        let (status, code) = match error {
            StoreFault::NoCampaign => (HttpStatus::InternalServerError, ErrorCode::StoreNoCampaign),
            StoreFault::StoreNotEmpty => (HttpStatus::Conflict, ErrorCode::StoreNotEmpty),
            StoreFault::VersionRefused { .. } => {
                (HttpStatus::Conflict, ErrorCode::StoreVersionRefused)
            }
            StoreFault::Corrupt { .. } => {
                (HttpStatus::InternalServerError, ErrorCode::StoreCorrupt)
            }
            StoreFault::DdlVersionRefused { .. } => {
                (HttpStatus::Conflict, ErrorCode::StoreDdlVersionRefused)
            }
            StoreFault::DocumentMigration { .. } | StoreFault::DdlMigration { .. } => (
                HttpStatus::InternalServerError,
                ErrorCode::StoreMigrationFailed,
            ),
            StoreFault::FormatRefused { .. } => {
                (HttpStatus::Conflict, ErrorCode::StoreFormatRefused)
            }
            StoreFault::Unavailable { .. } => {
                (HttpStatus::ServiceUnavailable, ErrorCode::StoreUnavailable)
            }
            StoreFault::Backend { .. } => {
                (HttpStatus::InternalServerError, ErrorCode::StoreBackend)
            }
            StoreFault::InvalidPatch { .. } => (
                HttpStatus::InternalServerError,
                ErrorCode::StoreInvalidPatch,
            ),
            _ => (HttpStatus::InternalServerError, ErrorCode::Internal),
        };
        Self::new(status, code)
    }

    /// Maps a content failure to a safe response.
    #[must_use]
    #[inline]
    pub fn from_content(error: &ContentFault) -> Self {
        let (status, code) = match error {
            ContentFault::Unavailable { .. } => (
                HttpStatus::ServiceUnavailable,
                ErrorCode::ContentUnavailable,
            ),
            ContentFault::Corrupt { .. } => {
                (HttpStatus::InternalServerError, ErrorCode::ContentCorrupt)
            }
            ContentFault::SnapshotVersionRefused { .. } => (
                HttpStatus::InternalServerError,
                ErrorCode::ContentVersionRefused,
            ),
            ContentFault::NamespaceConflict { .. } => (
                HttpStatus::InternalServerError,
                ErrorCode::ContentNamespaceConflict,
            ),
            _ => (HttpStatus::InternalServerError, ErrorCode::Internal),
        };
        Self::new(status, code)
    }
}
