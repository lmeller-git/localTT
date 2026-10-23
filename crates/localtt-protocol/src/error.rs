//! Stable error projections for the HTTP adapter (`docs/rfcs/server.md` 4.3
//! and findings RFC F6).
//!
//! An [`ErrorResponse`] carries the HTTP status separately from the serializable
//! [`ErrorBody`]. The body exposes a stable machine code and the structured
//! fields a client can act on; diagnostic messages from stores, content sources
//! and malformed-command reasons are never copied into it.

use localtt_core::types::EntityId;
use localtt_engine::{content::ContentFault, error::EngineError, ports::store::StoreFault};
use serde::{Deserialize, Serialize};

use super::{command::UnsupportedCommand, event::ElementRefDto};

/// The HTTP status classes the server maps errors to.
///
/// The protocol layer stays independent of the HTTP crate; M4 translates
/// [`Self::as_u16`] into its framework's status type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpStatus {
    /// The request body or command is invalid.
    BadRequest,
    /// Authentication is missing or wrong.
    Unauthorized,
    /// The actor may not perform the operation.
    Forbidden,
    /// The addressed entity or element is missing.
    NotFound,
    /// The request conflicts with current state.
    Conflict,
    /// A request body exceeded the transport limit.
    PayloadTooLarge,
    /// Too many failed authentication attempts.
    TooManyRequests,
    /// The server could not complete the request.
    InternalServerError,
    /// A dependency is temporarily unavailable.
    ServiceUnavailable,
}

impl HttpStatus {
    /// The numeric HTTP status code.
    #[must_use]
    #[inline]
    pub const fn as_u16(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::Conflict => 409,
            Self::PayloadTooLarge => 413,
            Self::TooManyRequests => 429,
            Self::InternalServerError => 500,
            Self::ServiceUnavailable => 503,
        }
    }
}

/// Stable machine-readable error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// The command or request body is invalid.
    Malformed,
    /// Authentication is missing, or the actor may not perform the operation.
    Unauthorized,
    /// The addressed entity or element is missing.
    NotFound,
    /// A create used an existing entity id.
    AlreadyExists,
    /// An optimistic-concurrency revision did not match.
    Conflict,
    /// A logical input limit was exceeded.
    TooLarge,
    /// The transport request-body cap was exceeded.
    PayloadTooLarge,
    /// Too many failed authentication attempts.
    AuthRateLimited,
    /// The request origin is not allowed.
    OriginDenied,
    /// The command tag is newer than this build.
    UnsupportedCommand,
    /// The store has no campaign.
    StoreNoCampaign,
    /// The store already holds a campaign.
    StoreNotEmpty,
    /// A stored document needs a newer build.
    StoreVersionRefused,
    /// The store is corrupt.
    StoreCorrupt,
    /// The store DDL needs a newer build.
    StoreDdlVersionRefused,
    /// A document or DDL migration failed.
    StoreMigrationFailed,
    /// A bundle format needs a newer build.
    StoreFormatRefused,
    /// The store is temporarily unavailable.
    StoreUnavailable,
    /// The store backend failed.
    StoreBackend,
    /// The engine built an invalid patch (an engine bug).
    StoreInvalidPatch,
    /// The command environment is unavailable.
    EnvironmentUnavailable,
    /// A content source is temporarily unavailable.
    ContentUnavailable,
    /// A content snapshot is corrupt.
    ContentCorrupt,
    /// A content snapshot version is unsupported.
    ContentVersionRefused,
    /// Two content sources share a namespace.
    ContentNamespaceConflict,
    /// A blocking content read failed to join.
    ContentTaskFailed,
    /// The actor thread is gone.
    ActorGone,
    /// An unexpected server failure.
    Internal,
}

/// The safe, structured body of an error response.
///
/// Diagnostic strings are deliberately absent. The fields are present only
/// when the code makes them meaningful.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// The stable machine code.
    pub code: ErrorCode,
    /// The entity the error is about, when one was addressed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// The element inside the entity, when one was addressed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementRefDto>,
    /// The revision the client expected, for a conflict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_rev: Option<u64>,
    /// The revision the entity actually has, for a conflict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_rev: Option<u64>,
    /// The capped quantity's name, for a logical limit error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub what: Option<String>,
    /// The configured limit, for a logical limit error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One error response: the status line plus the serializable body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorResponse {
    /// The HTTP status the adapter must use.
    pub status: HttpStatus,
    /// The body to serialize as JSON.
    pub body: ErrorBody,
}

impl ErrorResponse {
    /// A response with no client-useful fields, for transport-level errors that
    /// did not come from an engine, store or content failure.
    #[must_use]
    #[inline]
    pub fn new(status: HttpStatus, code: ErrorCode) -> Self {
        Self {
            status,
            body: ErrorBody {
                code,
                entity: None,
                element: None,
                expected_rev: None,
                actual_rev: None,
                what: None,
                limit: None,
            },
        }
    }

    /// A malformed request body with no diagnostic detail.
    #[must_use]
    #[inline]
    pub fn malformed() -> Self {
        Self::new(HttpStatus::BadRequest, ErrorCode::Malformed)
    }

    /// The transport body cap was exceeded.
    #[must_use]
    #[inline]
    pub fn payload_too_large() -> Self {
        Self::new(HttpStatus::PayloadTooLarge, ErrorCode::PayloadTooLarge)
    }

    /// An unknown command tag.
    #[must_use]
    #[inline]
    pub fn unsupported_command() -> Self {
        Self::new(HttpStatus::BadRequest, ErrorCode::UnsupportedCommand)
    }

    /// A missing or wrong bearer token.
    #[must_use]
    #[inline]
    pub fn unauthorized() -> Self {
        Self::new(HttpStatus::Unauthorized, ErrorCode::Unauthorized)
    }

    /// Too many failed authentication attempts in the current window.
    #[must_use]
    #[inline]
    pub fn auth_rate_limited() -> Self {
        Self::new(HttpStatus::TooManyRequests, ErrorCode::AuthRateLimited)
    }

    /// A browser origin outside the allowlist.
    #[must_use]
    #[inline]
    pub fn origin_denied() -> Self {
        Self::new(HttpStatus::Forbidden, ErrorCode::OriginDenied)
    }

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

impl From<UnsupportedCommand> for ErrorResponse {
    #[inline]
    fn from(_: UnsupportedCommand) -> Self {
        Self::unsupported_command()
    }
}
