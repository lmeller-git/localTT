//! Pure document migrations, keyed by `(major, minor)`: `serde_json::Value ->
//! Value` functions applied after the version gate and before parsing (RFC
//! section 5.6; STORE-2, STORE-10).
//!
//! The registry is empty at v1.0: no older document version exists yet. It
//! exists so the first breaking document change has a home that is not a
//! redesign.

use localtt_core::types::SchemaVersion;
use localtt_engine::ports::store::StoreFault;
use serde_json::Value;

use crate::load::value_version;

/// A pure JSON document migration: it moves one document version forward.
pub(crate) type DocMigration = fn(Value) -> Value;

/// The document migration registry: `(major, minor)` -> migration.
///
/// Empty at v1.0: no older document version exists yet.
pub(crate) const DOC_MIGRATIONS: &[((u16, u16), DocMigration)] = &[];

/// Runs the registered migrations for a document, one version step at a time,
/// until no step is registered for the document's current version.
///
/// Each step must strictly advance `schema_version`; a step that leaves it
/// unchanged or drops it is a [`StoreFault::DocumentMigration`]. The caller
/// re-checks the version gate on the result.
///
/// The registry is a parameter (not always [`DOC_MIGRATIONS`]) so the whole
/// gate -> migrate -> re-gate -> parse sequence is testable without a public
/// registration seam (RFC section 6.3, S3).
pub(crate) fn apply_with(
    registry: &[((u16, u16), DocMigration)],
    mut value: Value,
) -> Result<Value, StoreFault> {
    loop {
        let Some(version) = version_of(&value) else {
            return Ok(value);
        };
        let Some(migration) = find_step(registry, version.major, version.minor) else {
            return Ok(value);
        };
        let migrated = migration(value);
        match version_of(&migrated) {
            Some(next) if next > version => value = migrated,
            Some(next) => {
                return Err(StoreFault::DocumentMigration {
                    from: version,
                    to: next,
                    message: "the migration did not advance the document version".to_owned(),
                });
            }
            None => {
                return Err(StoreFault::DocumentMigration {
                    from: version,
                    to: version,
                    message: "the migration dropped schema_version".to_owned(),
                });
            }
        }
    }
}

/// Finds a registered step for one version.
pub(crate) fn find_step(
    registry: &[((u16, u16), DocMigration)],
    major: u16,
    minor: u16,
) -> Option<DocMigration> {
    registry
        .iter()
        .find_map(|&((m, n), migration)| (m == major && n == minor).then_some(migration))
}

/// Reads a document's version, treating a missing or malformed field as
/// "nothing to migrate": the gate and the parse report it.
fn version_of(value: &Value) -> Option<SchemaVersion> {
    value_version(value).ok().flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Test-only step: renames `legacy_body` and moves (0, 9) to (1, 0).
    fn to_1_0(mut value: Value) -> Value {
        value["body"] = value["legacy_body"].take();
        if let Some(object) = value.as_object_mut() {
            object.remove("legacy_body");
        }
        value["schema_version"] = json!({ "major": 1, "minor": 0 });
        value
    }

    /// Test-only step: moves (1, 0) to (1, 1).
    fn to_1_1(mut value: Value) -> Value {
        value["schema_version"] = json!({ "major": 1, "minor": 1 });
        value
    }

    /// Test-only step that violates its contract: it keeps the version.
    fn stuck(value: Value) -> Value {
        value
    }

    #[test]
    fn an_empty_registry_leaves_the_document_untouched() {
        let value = json!({ "schema_version": { "major": 1, "minor": 0 }, "body": "note" });

        let applied = apply_with(DOC_MIGRATIONS, value.clone())
            .expect("no migrations are registered at v1.0");

        assert_eq!(applied, value);
    }

    #[test]
    fn registered_steps_run_in_sequence_from_the_document_version() {
        let registry: &[((u16, u16), DocMigration)] = &[((0, 9), to_1_0), ((1, 0), to_1_1)];
        let legacy = json!({
            "schema_version": { "major": 0, "minor": 9 },
            "legacy_body": "session 4"
        });

        let migrated = apply_with(registry, legacy).expect("both steps advance the version");

        assert_eq!(migrated["body"], "session 4");
        assert!(migrated.get("legacy_body").is_none());
        assert_eq!(
            migrated["schema_version"],
            json!({ "major": 1, "minor": 1 })
        );
    }

    #[test]
    fn a_document_below_the_registry_stops_where_no_step_exists() {
        let registry: &[((u16, u16), DocMigration)] = &[((1, 0), to_1_1)];
        let value = json!({ "schema_version": { "major": 0, "minor": 9 } });

        let applied = apply_with(registry, value.clone()).expect("no step for (0, 9)");

        assert_eq!(applied, value);
    }

    #[test]
    fn a_document_without_a_version_is_left_alone() {
        let registry: &[((u16, u16), DocMigration)] = &[((0, 9), to_1_0)];
        let value = json!({ "text": "a feed entry has no schema_version" });

        let applied = apply_with(registry, value.clone()).expect("nothing to dispatch on");

        assert_eq!(applied, value);
    }

    #[test]
    fn a_step_that_does_not_advance_the_version_is_refused() {
        let value = json!({ "schema_version": { "major": 0, "minor": 9 } });

        match apply_with(&[((0, 9), stuck)], value) {
            Err(StoreFault::DocumentMigration { from, to, .. }) => {
                assert_eq!(from, SchemaVersion { major: 0, minor: 9 });
                assert_eq!(to, from);
            }
            other => panic!("a stuck migration must be a DocumentMigration fault, got {other:?}"),
        }
    }

    #[test]
    fn a_step_that_drops_the_version_is_refused() {
        fn drops(mut value: Value) -> Value {
            if let Some(object) = value.as_object_mut() {
                object.remove("schema_version");
            }
            value
        }

        match apply_with(
            &[((0, 9), drops)],
            json!({ "schema_version": { "major": 0, "minor": 9 } }),
        ) {
            Err(StoreFault::DocumentMigration { message, .. }) => {
                assert!(message.contains("dropped"), "diagnostic names the failure");
            }
            other => panic!("a version-dropping migration must be refused, got {other:?}"),
        }
    }
}
