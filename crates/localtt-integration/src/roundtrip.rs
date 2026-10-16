//! JSON round trips for the scenario suite (`DESIGN.md` section 2).
//!
//! The schema gate that once lived here moved to `localtt_adapter::load`, where
//! store logic belongs (docs/decisions.md ARCH-6); what remains is the thin
//! serde serialization the suite needs.

use serde::{Serialize, de::DeserializeOwned};

/// Serializes any core document to a JSON string.
#[inline]
pub fn to_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

/// Deserializes a core document from JSON.
#[inline]
pub fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(value)
}
