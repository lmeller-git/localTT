//! Loading persisted core documents behind the schema gate (RFC-0001 section
//! 5.9; `crates/localtt-integration/DESIGN.md` section 5).
//!
//! This is store logic: the sqlite adapter, and any import path, must refuse a
//! document whose major version is newer than this build before partially
//! loading it.

use localtt_core::types::SchemaVersion;
use serde::{Deserialize, de::DeserializeOwned};

/// Loads a document after checking `SchemaVersion::CURRENT.accepts` on the
/// document's own version (RFC-0001 5.9): a newer major is refused rather than
/// partially loaded.
///
/// The implementation peeks `schema_version` out of the raw JSON with a small
/// `SchemaProbe` before the full parse, so a too-new file is rejected without
/// materializing it (docs/decisions.md ARCH-6).
#[inline]
pub fn load_checked<T: DeserializeOwned>(value: &str) -> Result<T, LoadError> {
    let probe: SchemaProbe = serde_json::from_str(value)?;
    if let Some(file) = probe.schema_version
        && !SchemaVersion::CURRENT.accepts(file)
    {
        return Err(LoadError::Schema(file));
    }
    Ok(serde_json::from_str(value)?)
}

/// Minimal view of a document used to read the version gate before the full
/// parse.
#[derive(Deserialize)]
struct SchemaProbe {
    #[serde(default)]
    schema_version: Option<SchemaVersion>,
}

/// Failure loading a persisted document.
///
/// `#[non_exhaustive]`: more kinds are expected and adding one must not break
/// matchers. A typed "parsed but invalid" (`EntityError`) variant is deliberately
/// absent: the kind/body family check lives in `Entity`'s `Deserialize`, so
/// serde reports it through the deserializer's own error type, which is not
/// recoverable here (`crates/localtt-core/DESIGN.md` section 9).
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
