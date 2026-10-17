//! Pure document migrations, keyed by `(major, minor)`: `serde_json::Value ->
//! Value` functions applied after the version gate and before parsing (RFC
//! section 5.6; STORE-2, STORE-10).
//!
//! The registry is empty at v1.0: no older document version exists yet. It
//! exists so the first breaking document change has a home that is not a
//! redesign.

/// A pure JSON document migration: it moves one document version forward.
pub(crate) type DocMigration = fn(serde_json::Value) -> serde_json::Value;

/// The document migration registry: `(major, minor)` -> migration.
///
/// Empty at v1.0: no older document version exists yet.
pub(crate) const DOC_MIGRATIONS: &[((u16, u16), DocMigration)] = &[];

/// Returns the migration registered for document version `(major, minor)`.
pub(crate) fn lookup(major: u16, minor: u16) -> Option<DocMigration> {
    todo!()
}

#[cfg(test)]
mod tests {
    // The migration-hook test lives here (RFC section 6.3): it registers a
    // test-only migration and asserts the registry runs after the gate and
    // before parsing.
}
