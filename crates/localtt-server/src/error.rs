//! The server's failure type (`docs/rfcs/server.md` 4.3).

use localtt_engine::{content::ContentFault, error::EngineError, ports::store::StoreFault};
use thiserror::Error;

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
    /// The actor thread is gone; the session is over.
    #[error("the server actor is gone")]
    ActorGone,
}
