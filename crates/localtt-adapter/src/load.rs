//! Loading persisted core documents behind the schema gate (RFC-0001 section
//! 5.9; RFC-0003 section 5.5).
//!
//! This is store logic: the sqlite adapter, and any import path, must refuse a
//! document whose major version is newer than this build before partially
//! loading it.
#![expect(
    clippy::todo,
    unused_variables,
    reason = "skeleton phase: every body is `todo!()`; removed in the implement phase"
)]

use localtt_core::types::SchemaVersion;
use serde::de::DeserializeOwned;

/// Loads a document after checking `SchemaVersion::CURRENT.accepts` on the
/// document's own version (RFC-0001 5.9): a newer major is refused rather than
/// partially loaded.
///
/// The implementation peeks `schema_version` out of the raw JSON with a small
/// `SchemaProbe` before the full parse, so a too-new file is rejected without
/// materializing it (RFC-0003 P2).
#[inline]
pub fn load_checked<T: DeserializeOwned>(value: &str) -> Result<T, LoadError> {
    todo!()
}

/// Failure loading a persisted document.
///
/// `#[non_exhaustive]`: more kinds are expected and adding one must not break
/// matchers. A typed "parsed but invalid" (`EntityError`) variant is deliberately
/// absent: the kind/body family check lives in `Entity`'s `Deserialize`, so
/// serde reports it through the deserializer's own error type, which is not
/// recoverable here (RFC-0003 section 5.5).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// The file's major version is newer than this build supports.
    #[error("file schema {0:?} is newer than this build")]
    Schema(SchemaVersion),
    /// The document did not parse, or failed a semantic check inside a
    /// `Deserialize` impl.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
