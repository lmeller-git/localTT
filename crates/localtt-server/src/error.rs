//! The server's failure type (`docs/rfcs/server.md` 4.3).

use localtt_engine::{content::ContentFault, error::EngineError, ports::store::StoreFault};
use thiserror::Error;

use crate::protocol::{ErrorCode, ErrorResponse, HttpStatus};

/// Failure of one `AppHandle` call.
///
/// Diagnostic store/content messages are deliberately not user-facing; the
/// HTTP adapter maps variants to stable codes and statuses (F6).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppError {
    /// The engine refused the command or read.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The store failed; its diagnostic message must not be forwarded.
    #[error(transparent)]
    Store(#[from] StoreFault),
    /// A reference source failed.
    #[error(transparent)]
    Content(#[from] ContentFault),
    /// A blocking content read failed to join or panicked.
    #[error("the content reader stopped unexpectedly")]
    ContentTask,
    /// The actor thread is gone; the session is over.
    #[error("the server actor is gone")]
    ActorGone,
}

impl From<AppError> for ErrorResponse {
    #[inline]
    fn from(error: AppError) -> Self {
        Self::from(&error)
    }
}

impl From<&AppError> for ErrorResponse {
    #[inline]
    fn from(error: &AppError) -> Self {
        match error {
            AppError::Engine(error) => Self::from_engine(error),
            AppError::Store(error) => Self::from_store(error),
            AppError::Content(error) => Self::from_content(error),
            AppError::ContentTask => {
                Self::new(HttpStatus::ServiceUnavailable, ErrorCode::ContentTaskFailed)
            }
            AppError::ActorGone => Self::new(HttpStatus::ServiceUnavailable, ErrorCode::ActorGone),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;
    use crate::protocol::{ErrorCode, ErrorResponse, HttpStatus};

    #[test]
    fn app_only_failures_map_to_stable_codes() {
        let task = ErrorResponse::from(&AppError::ContentTask);
        assert_eq!(task.status, HttpStatus::ServiceUnavailable);
        assert_eq!(task.body.code, ErrorCode::ContentTaskFailed);

        let gone = ErrorResponse::from(&AppError::ActorGone);
        assert_eq!(gone.status, HttpStatus::ServiceUnavailable);
        assert_eq!(gone.body.code, ErrorCode::ActorGone);
    }
}
