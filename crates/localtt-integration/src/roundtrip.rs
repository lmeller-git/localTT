//! JSON round trips and the schema gate (RFC-0003 section 5.5).
#![expect(
    clippy::todo,
    unused_variables,
    reason = "skeleton phase: every body is `todo!()`; removed in the implement phase"
)]

use localtt_core::types::SchemaVersion;
use serde::{Serialize, de::DeserializeOwned};

/// Serializes any core document to a JSON string.
#[inline]
pub fn to_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    todo!()
}

/// Deserializes a core document from JSON.
#[inline]
pub fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    todo!()
}

/// Loads a document after checking `SchemaVersion::accepts` on the document's
/// own version (RFC-0001 5.9): a newer major is refused rather than partially
/// loaded.
#[inline]
pub fn load_checked<T: DeserializeOwned>(value: &str) -> Result<T, LoadError> {
    todo!()
}

/// Failure loading a persisted document.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The file's major version is newer than this build supports.
    #[error("file schema {0:?} is newer than this build")]
    Schema(SchemaVersion),
    /// The document did not parse.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
