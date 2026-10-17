//! DDL and DDL migrations: the ordered step list and `PRAGMA user_version`
//! (RFC sections 5.6 and 6.1; STORE-2, STORE-12).

use localtt_engine::ports::store::StoreFault;
use rusqlite::Connection;

/// One DDL migration: bumps `PRAGMA user_version` by one.
pub(crate) type DdlStep = fn(&Connection) -> Result<(), StoreFault>;

/// The ordered DDL migrations; entry `n` migrates DDL version `n` to `n + 1`.
///
/// [`migrate`] wraps a step's failure in [`StoreFault::DdlMigration`] with its
/// version range. One entry today: `create_v1`.
pub(crate) const DDL_MIGRATIONS: &[DdlStep] = &[create_v1];

/// The DDL version this build knows how to open.
pub(crate) const SUPPORTED_DDL_VERSION: i32 = DDL_MIGRATIONS.len() as i32;

/// The v1 schema (design section 5.2).
const CREATE_V1: &str = "\
CREATE TABLE campaign (
    id                   TEXT PRIMARY KEY,
    schema_version_major INTEGER NOT NULL,
    schema_version_minor INTEGER NOT NULL,
    doc                  TEXT NOT NULL
);
CREATE TABLE entities (
    id                   TEXT PRIMARY KEY,
    kind                 TEXT NOT NULL,
    body_kind            TEXT NOT NULL,
    owner                TEXT NOT NULL,
    parent               TEXT,
    name                 TEXT NOT NULL,
    rev                  INTEGER NOT NULL,
    schema_version_major INTEGER NOT NULL,
    schema_version_minor INTEGER NOT NULL,
    doc                  TEXT NOT NULL
);
CREATE INDEX entities_owner ON entities(owner);
CREATE INDEX entities_kind  ON entities(kind);
CREATE TABLE feed (
    id       INTEGER PRIMARY KEY,
    ts       INTEGER NOT NULL,
    clock_s  INTEGER NOT NULL,
    audience TEXT NOT NULL,
    doc      TEXT NOT NULL
);";

/// Creates the v1 schema (design section 5.2): `campaign`, `entities` with its
/// indexes, and `feed`.
fn create_v1(connection: &Connection) -> Result<(), StoreFault> {
    connection
        .execute_batch(CREATE_V1)
        .map_err(|error| StoreFault::Backend {
            message: format!("creating the v1 schema failed: {error}"),
        })
}

/// Reads `PRAGMA user_version`; signed, like SQLite itself (STORE-8).
pub(crate) fn user_version(connection: &Connection) -> Result<i32, StoreFault> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| StoreFault::Backend {
            message: format!("reading user_version failed: {error}"),
        })
}

/// Writes `PRAGMA user_version`.
pub(crate) fn set_user_version(connection: &Connection, version: i32) -> Result<(), StoreFault> {
    connection
        .pragma_update(None, "user_version", version)
        .map_err(|error| StoreFault::Backend {
            message: format!("writing user_version failed: {error}"),
        })
}

/// Applies every pending DDL migration and stamps `user_version` in one
/// transaction (S8).
///
/// The caller refuses a `user_version` newer than [`SUPPORTED_DDL_VERSION`]
/// before calling.
pub(crate) fn migrate(connection: &Connection) -> Result<(), StoreFault> {
    let start = user_version(connection)?;
    if start < 0 {
        return Err(StoreFault::Corrupt {
            message: format!("negative DDL version {start}"),
        });
    }
    if start >= SUPPORTED_DDL_VERSION {
        return Ok(());
    }

    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| StoreFault::Backend {
            message: format!("starting the DDL migration transaction failed: {error}"),
        })?;
    let mut version = start;
    while let Some(step) = DDL_MIGRATIONS.get(version as usize) {
        step(&transaction).map_err(|error| StoreFault::DdlMigration {
            from: version,
            to: version + 1,
            message: error.to_string(),
        })?;
        version += 1;
    }
    set_user_version(&transaction, version)?;
    transaction.commit().map_err(|error| StoreFault::Backend {
        message: format!("committing the DDL migration failed: {error}"),
    })
}

/// Verifies the tables a DDL version promises are actually present.
///
/// A file carrying the localtt magic but missing its tables is damaged or
/// tampered, not merely unreadable: `Corrupt`, so the server never retries it as
/// an advisory failure (S10). Version 0 must hold none of them: the v1 migration
/// is one transaction, so a half-created schema cannot be a crash artifact. Only
/// v0 and v1 exist today; a later DDL version must extend this expectation.
pub(crate) fn verify(connection: &Connection, version: i32) -> Result<(), StoreFault> {
    let present: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('campaign', 'entities', 'feed')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| StoreFault::Backend {
            message: format!("checking the schema tables failed: {error}"),
        })?;
    let matches = (version == 0 && present == 0) || (version > 0 && present == 3);
    if !matches {
        return Err(StoreFault::Corrupt {
            message: format!(
                "DDL version {version} does not match the schema: {present} of the expected tables are present"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway database; schema work never touches a file.
    fn connection() -> Connection {
        Connection::open_in_memory().expect("an in-memory database opens")
    }

    #[test]
    fn migrate_creates_the_v1_schema_and_stamps_the_version() {
        let connection = connection();

        migrate(&connection).expect("a fresh database migrates");

        assert_eq!(
            user_version(&connection).expect("user_version is readable"),
            SUPPORTED_DDL_VERSION
        );
        connection
            .execute_batch(
                "INSERT INTO campaign (id, schema_version_major, schema_version_minor, doc)
                     VALUES ('campaign', 1, 0, '{}');
                 INSERT INTO entities
                     (id, kind, body_kind, owner, parent, name, rev,
                      schema_version_major, schema_version_minor, doc)
                     VALUES ('entity', 'note', 'note', 'player', NULL, 'Session 4', 1, 1, 0, '{}');
                 INSERT INTO feed (id, ts, clock_s, audience, doc)
                     VALUES (1, 1750000000, 0, 'public', '{}');",
            )
            .expect("the v1 schema accepts the documented columns");
    }

    #[test]
    fn migrate_is_idempotent() {
        let connection = connection();
        migrate(&connection).expect("a fresh database migrates");

        migrate(&connection).expect("an up-to-date database is left alone");

        assert_eq!(
            user_version(&connection).expect("user_version is readable"),
            SUPPORTED_DDL_VERSION
        );
    }

    #[test]
    fn user_version_round_trips() {
        let connection = connection();

        set_user_version(&connection, 7).expect("a version is set");

        assert_eq!(user_version(&connection).expect("readable"), 7);
    }
}
