//! Store implementations behind the engine's `CampaignStore` port
//! (`docs/rfcs/store.md` section 4.3).
//!
//! Layout per RFC section 6.5: `memory` (engine tests and the contract suite),
//! `sqlite` (the shipped store), `export` (bundle decode and import), and the
//! crate-internal `schema`, `doc_migrations` and `backup` modules.
//!
//! The document pipeline and the patch-shape preconditions live here, so both
//! stores behave identically where the RFC demands it: `parse_document` and
//! `parse_value` run the schema gate, then registered document migrations,
//! then the full parse; `to_document` is the single serialization point.

use std::collections::BTreeSet;

use localtt_core::types::SchemaVersion;
use localtt_engine::ports::store::{StatePatch, StoreFault};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::load::{LoadError, check_version, value_version};

pub mod export;
pub mod memory;
pub mod sqlite;

pub(crate) mod backup;
pub(crate) mod doc_migrations;
pub(crate) mod schema;

pub use export::{decode_bundle, import_into};
pub use memory::MemoryStore;
pub use sqlite::{SqliteOptions, SqliteStore};

/// Parses one stored document text: schema gate, then registered document
/// migrations, then the full parse (RFC sections 5.5 and 5.6).
pub(crate) fn parse_document<T: DeserializeOwned>(text: &str) -> Result<T, StoreFault> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| store_fault(LoadError::Json(error)))?;
    parse_value(&value)
}

/// Parses one document already decoded into a JSON value.
///
/// The gate runs on the document's own `schema_version`, before anything else,
/// so a newer major is refused without migrating or parsing. A registered
/// migration runs next; the gate is re-checked after it, so a migration can
/// never carry a document past what this build accepts.
pub(crate) fn parse_value<T: DeserializeOwned>(value: &Value) -> Result<T, StoreFault> {
    parse_value_with(doc_migrations::DOC_MIGRATIONS, value)
}

/// [`parse_value`] against an explicit migration registry.
///
/// The registry is a parameter so the whole gate -> migrate -> re-gate -> parse
/// sequence runs in a unit test without a public registration seam (RFC section
/// 6.3, S3).
fn parse_value_with<T: DeserializeOwned>(
    registry: &[((u16, u16), doc_migrations::DocMigration)],
    value: &Value,
) -> Result<T, StoreFault> {
    let version = value_version(value).map_err(store_fault)?;
    check_version(version).map_err(store_fault)?;

    let migrated;
    let source = match version {
        Some(version)
            if doc_migrations::find_step(registry, version.major, version.minor).is_some() =>
        {
            migrated = doc_migrations::apply_with(registry, value.clone())?;
            let produced = value_version(&migrated).map_err(store_fault)?;
            check_version(produced).map_err(store_fault)?;
            &migrated
        }
        _ => value,
    };

    T::deserialize(source).map_err(|error| StoreFault::Corrupt {
        message: error.to_string(),
    })
}

/// Serializes a typed document to its stored form.
///
/// Serialization is total for the current document types (no floats,
/// string-keyed maps): a failure can only mean a future document type that
/// cannot be stored, and it is a `Backend` fault, not a corrupt file.
pub(crate) fn to_document<T: Serialize>(value: &T) -> Result<String, StoreFault> {
    serde_json::to_string(value).map_err(|error| StoreFault::Backend {
        message: format!("serializing a document failed: {error}"),
    })
}

/// Enforces the patch-only `apply` preconditions (RFC section 5.3 items 1, 2,
/// 4 and 6): duplicate or overlapping entity lists, feed append order, and
/// appends below the trim cutoff.
///
/// The store-state preconditions (items 3, 5, 7 and 8) stay with each
/// implementation, which knows how to ask its own state.
pub(crate) fn validate_shape(patch: &StatePatch<'_>) -> Result<(), StoreFault> {
    let mut upserted = BTreeSet::new();
    for entity in &patch.upserts {
        if !upserted.insert(entity.id()) {
            return Err(invalid_patch(format!(
                "entity {} appears twice in the upserts",
                entity.id()
            )));
        }
    }
    for id in &patch.removals {
        if upserted.contains(id) {
            return Err(invalid_patch(format!(
                "entity {id} is both upserted and removed"
            )));
        }
    }

    for pair in patch.feed_appends.windows(2) {
        if pair[0].id >= pair[1].id {
            return Err(invalid_patch("feed appends are not strictly ascending"));
        }
    }
    if let Some(keep_from) = patch.feed_keep_from {
        for entry in &patch.feed_appends {
            if entry.id < keep_from {
                return Err(invalid_patch(format!(
                    "feed entry {} is below the trim cutoff",
                    entry.id.get()
                )));
            }
        }
    }

    Ok(())
}

/// Builds an [`StoreFault::InvalidPatch`] with a diagnostic message.
pub(crate) fn invalid_patch(message: impl Into<String>) -> StoreFault {
    StoreFault::InvalidPatch {
        message: message.into(),
    }
}

/// Maps the schema-gate error type onto the store fault the port exposes.
fn store_fault(error: LoadError) -> StoreFault {
    match error {
        LoadError::Schema(found) => StoreFault::VersionRefused {
            found,
            supported: SchemaVersion::CURRENT,
        },
        LoadError::Json(error) => StoreFault::Corrupt {
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::types::SchemaVersion;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::store::doc_migrations::DocMigration;

    /// A minimal gated document, so the test does not depend on a core type.
    #[derive(Debug, Deserialize, PartialEq)]
    struct Doc {
        schema_version: SchemaVersion,
        body: String,
    }

    /// Test-only step: renames `legacy_body`, moving (0, 9) to (1, 0).
    fn to_1_0(mut value: Value) -> Value {
        value["body"] = value["legacy_body"].take();
        if let Some(object) = value.as_object_mut() {
            object.remove("legacy_body");
        }
        value["schema_version"] = json!({ "major": 1, "minor": 0 });
        value
    }

    /// Test-only step that overshoots the supported version.
    fn to_2_0(mut value: Value) -> Value {
        value["schema_version"] = json!({ "major": 2, "minor": 0 });
        value
    }

    #[test]
    fn a_registered_migration_runs_before_the_parse() {
        let registry: &[((u16, u16), DocMigration)] = &[((0, 9), to_1_0)];
        let legacy = json!({
            "schema_version": { "major": 0, "minor": 9 },
            "legacy_body": "session 4"
        });

        let parsed: Doc =
            parse_value_with(registry, &legacy).expect("the migration renames the field");

        assert_eq!(
            parsed,
            Doc {
                schema_version: SchemaVersion { major: 1, minor: 0 },
                body: "session 4".to_owned(),
            }
        );
    }

    #[test]
    fn a_migration_that_overshoots_the_gate_is_refused() {
        let registry: &[((u16, u16), DocMigration)] = &[((0, 9), to_2_0)];
        let legacy = json!({
            "schema_version": { "major": 0, "minor": 9 },
            "legacy_body": "session 4"
        });

        match parse_value_with::<Doc>(registry, &legacy) {
            Err(StoreFault::VersionRefused { found, .. }) => {
                assert_eq!(found, SchemaVersion { major: 2, minor: 0 });
            }
            other => panic!("a migration past the gate must be refused, got {other:?}"),
        }
    }
}
