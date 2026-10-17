//! The SQLite store (RFC sections 4.3 and 6.1).

use std::{fs, path::Path, time::Duration};

use localtt_core::types::{AnyEntity, Campaign, FeedEntry};
use localtt_engine::ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreFault};
use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::store::{backup, invalid_patch, parse_document, schema, to_document, validate_shape};

/// `PRAGMA application_id` magic: the ASCII bytes `LTT1`. A present file with
/// any other value is not a localtt store (S10).
const APPLICATION_ID: i32 = 0x4C54_5431;

/// How long SQLite waits on a contended lock before failing; deployment is one
/// process per campaign file, so this only rides out short-lived readers.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

/// The shipped file-backed store: one `rusqlite` connection, documents in
/// `doc` columns next to the extracted projection columns (VER-3, STORE-1).
#[derive(Debug)]
pub struct SqliteStore {
    connection: Connection,
}

/// Opening options; `Default` is what [`SqliteStore::open`] uses.
///
/// `#[non_exhaustive]` keeps later options additive.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct SqliteOptions {
    /// Online startup backup before DDL migrations (design section 5.7);
    /// skipped for a fresh file. `--no-backup` maps to `false` (U7).
    pub startup_backup: bool,
}

impl SqliteStore {
    /// Opens the campaign file, creating it when missing or zero-length.
    ///
    /// Sets the connection pragmas outside any transaction, verifies file
    /// identity (`application_id` magic; a foreign database is `Corrupt`), the
    /// DDL version (refusing a newer one) and that the version's tables are
    /// present (a magic-carrying file missing them is `Corrupt`, not
    /// `Unavailable`), takes an atomic startup backup when
    /// [`SqliteOptions::startup_backup`] asks for one, then applies pending DDL
    /// migrations in one transaction.
    ///
    /// # Errors
    ///
    /// `Corrupt` when the file is not a localtt store, `DdlVersionRefused`
    /// when its DDL version is newer than this build knows, `Unavailable` on
    /// IO or lock failures.
    #[inline]
    pub fn open(path: &Path) -> Result<Self, StoreFault> {
        Self::open_with(path, SqliteOptions::default())
    }

    /// Like [`SqliteStore::open`], with explicit opening options.
    #[inline]
    pub fn open_with(path: &Path, options: SqliteOptions) -> Result<Self, StoreFault> {
        let fresh = fs::metadata(path).map_or(true, |metadata| metadata.len() == 0);
        let connection =
            Connection::open(path).map_err(|error| fault("opening the store file", error))?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| fault("setting busy_timeout", error))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;\n\
                 PRAGMA synchronous = NORMAL;\n\
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|error| fault("setting connection pragmas", error))?;

        if fresh {
            connection
                .pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(|error| fault("stamping the store file", error))?;
        } else {
            let application_id: i32 = connection
                .pragma_query_value(None, "application_id", |row| row.get(0))
                .map_err(|error| fault("reading application_id", error))?;
            if application_id != APPLICATION_ID {
                return Err(StoreFault::Corrupt {
                    message: format!(
                        "{} is not a localtt store (application_id {application_id:#x})",
                        path.display()
                    ),
                });
            }
            let version = schema::user_version(&connection)?;
            if version < 0 {
                return Err(StoreFault::Corrupt {
                    message: format!("{} has a negative DDL version {version}", path.display()),
                });
            }
            if version > schema::SUPPORTED_DDL_VERSION {
                return Err(StoreFault::DdlVersionRefused {
                    found: version,
                    supported: schema::SUPPORTED_DDL_VERSION,
                });
            }
            schema::verify(&connection, version)?;
            if options.startup_backup {
                backup::startup_backup(&connection, path)?;
            }
        }

        schema::migrate(&connection)?;
        Ok(Self { connection })
    }

    /// The live connection, for the backup capability.
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Reads every `doc` of the query, parsing each through the document
    /// pipeline.
    fn load_documents<T: DeserializeOwned>(
        connection: &Connection,
        sql: &str,
    ) -> Result<Vec<T>, StoreFault> {
        let mut statement = connection
            .prepare_cached(sql)
            .map_err(|error| fault("preparing a load query", error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| fault("reading documents", error))?;
        let mut documents = Vec::new();
        for row in rows {
            let doc = row.map_err(|error| fault("reading a document", error))?;
            documents.push(parse_document(&doc)?);
        }
        Ok(documents)
    }
}

impl CampaignStore for SqliteStore {
    #[inline]
    fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        // One read transaction covers all three queries, so a concurrent
        // commit cannot tear the snapshot (RFC section 5.4).
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| fault("starting the load transaction", error))?;
        let campaign_doc: Option<String> = transaction
            .query_row("SELECT doc FROM campaign LIMIT 1", [], |row| row.get(0))
            .optional()
            .map_err(|error| fault("reading the campaign document", error))?;
        let Some(campaign_doc) = campaign_doc else {
            return Err(StoreFault::NoCampaign);
        };

        Ok(CampaignSnapshot {
            campaign: parse_document(&campaign_doc)?,
            entities: Self::load_documents(&transaction, "SELECT doc FROM entities ORDER BY id")?,
            feed: Self::load_documents(&transaction, "SELECT doc FROM feed ORDER BY id")?,
        })
    }

    #[inline]
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        validate_shape(patch)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| fault("starting the patch transaction", error))?;
        // Validated inside the write lock, so the preconditions and the write
        // are atomic: a second handle cannot slip a conflicting patch between
        // them. An `InvalidPatch` drops the transaction, rolling it back.
        validate_state(&transaction, patch)?;

        if let Some(campaign) = patch.campaign {
            write_campaign(&transaction, campaign)?;
        }
        for entity in &patch.upserts {
            write_entity(&transaction, entity)?;
        }
        for id in &patch.removals {
            transaction
                .execute(
                    "DELETE FROM entities WHERE id = ?1",
                    params![id.to_string()],
                )
                .map_err(|error| fault("removing an entity", error))?;
        }
        if let Some(keep_from) = patch.feed_keep_from {
            transaction
                .execute(
                    "DELETE FROM feed WHERE id < ?1",
                    params![sql_integer(keep_from.get(), "feed cutoff")?],
                )
                .map_err(|error| fault("trimming the feed", error))?;
        }
        for entry in &patch.feed_appends {
            write_feed(&transaction, entry)?;
        }

        transaction
            .commit()
            .map_err(|error| fault("committing the patch", error))
    }
}

/// Maps a `rusqlite` failure to its store fault class: a file that is not a
/// database is `Corrupt`, everything else is `Unavailable` (IO, locks).
pub(crate) fn fault(context: &str, error: rusqlite::Error) -> StoreFault {
    match error.sqlite_error_code() {
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => StoreFault::Corrupt {
            message: format!("{context}: {error}"),
        },
        _ => StoreFault::Unavailable {
            message: format!("{context}: {error}"),
        },
    }
}

/// Enforces the store-state `apply` preconditions (RFC section 5.3 items 3, 5,
/// 7 and 8) inside the patch's `BEGIN IMMEDIATE` transaction, so the state it
/// reads is the state the write sees.
fn validate_state(connection: &Connection, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
    let stored_campaign: Option<String> = connection
        .query_row("SELECT id FROM campaign LIMIT 1", [], |row| row.get(0))
        .optional()
        .map_err(|error| fault("reading the stored campaign id", error))?;

    let carries_documents =
        !patch.upserts.is_empty() || !patch.removals.is_empty() || !patch.feed_appends.is_empty();
    if stored_campaign.is_none() && patch.campaign.is_none() && carries_documents {
        return Err(invalid_patch(
            "entities and feed entries require a stored campaign",
        ));
    }
    if let (Some(stored), Some(campaign)) = (&stored_campaign, patch.campaign)
        && *stored != campaign.id.to_string()
    {
        return Err(invalid_patch(format!(
            "campaign {} does not match the stored campaign {stored}",
            campaign.id
        )));
    }

    for id in &patch.removals {
        if !exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM entities WHERE id = ?1)",
            params![id.to_string()],
        )? {
            return Err(invalid_patch(format!("entity {id} is not stored")));
        }
    }
    for entry in &patch.feed_appends {
        if exists(
            connection,
            "SELECT EXISTS(SELECT 1 FROM feed WHERE id = ?1)",
            params![sql_integer(entry.id.get(), "feed id")?],
        )? {
            return Err(invalid_patch(format!(
                "feed entry {} is already stored",
                entry.id.get()
            )));
        }
    }

    Ok(())
}

/// Runs one boolean `EXISTS` query.
fn exists(
    connection: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<bool, StoreFault> {
    connection
        .query_row(sql, parameters, |row| row.get(0))
        .map_err(|error| fault("checking stored ids", error))
}

/// Writes the campaign row: document plus the extracted projection columns.
fn write_campaign(connection: &Connection, campaign: &Campaign) -> Result<(), StoreFault> {
    let doc = to_document(campaign)?;
    let version = campaign.schema_version;
    connection
        .execute(
            "INSERT OR REPLACE INTO campaign (id, schema_version_major, schema_version_minor, doc)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                campaign.id.to_string(),
                i64::from(version.major),
                i64::from(version.minor),
                doc
            ],
        )
        .map_err(|error| fault("writing the campaign", error))?;
    Ok(())
}

/// Writes one entity row: document plus the extracted projection columns.
fn write_entity(connection: &Connection, entity: &AnyEntity) -> Result<(), StoreFault> {
    let doc = to_document(entity)?;
    let kind = sql_text(&entity.kind())?;
    let body_kind = sql_text(&entity.kind().body_kind())?;
    let revision = sql_integer(entity.rev(), &format!("entity {} revision", entity.id()))?;
    let version = entity.schema_version();
    let parent = entity.parent().map(|id| id.to_string());
    connection
        .execute(
            "INSERT OR REPLACE INTO entities
                 (id, kind, body_kind, owner, parent, name, rev,
                  schema_version_major, schema_version_minor, doc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entity.id().to_string(),
                kind,
                body_kind,
                entity.owner().to_string(),
                parent,
                entity.name(),
                revision,
                i64::from(version.major),
                i64::from(version.minor),
                doc
            ],
        )
        .map_err(|error| fault("writing an entity", error))?;
    Ok(())
}

/// Writes one feed row: document plus the extracted projection columns.
fn write_feed(connection: &Connection, entry: &FeedEntry) -> Result<(), StoreFault> {
    let doc = to_document(entry)?;
    connection
        .execute(
            "INSERT OR REPLACE INTO feed (id, ts, clock_s, audience, doc)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                sql_integer(entry.id.get(), "feed id")?,
                entry.ts.seconds(),
                entry.clock_s.seconds(),
                sql_text(&entry.audience)?,
                doc
            ],
        )
        .map_err(|error| fault("writing a feed entry", error))?;
    Ok(())
}

/// Renders a unit enum as the snake_case text the wire format uses.
fn sql_text<T: Serialize>(value: &T) -> Result<String, StoreFault> {
    match serde_json::to_value(value).map_err(|error| StoreFault::Backend {
        message: format!("serializing a projection column failed: {error}"),
    })? {
        Value::String(text) => Ok(text),
        other => Err(StoreFault::Backend {
            message: format!("projection column is not a string: {other}"),
        }),
    }
}

/// Converts an unsigned counter (feed position, revision) to SQLite's signed
/// `INTEGER`.
fn sql_integer(value: u64, what: &str) -> Result<i64, StoreFault> {
    i64::try_from(value).map_err(|_| StoreFault::Backend {
        message: format!("{what} {value} does not fit SQLite INTEGER"),
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use localtt_core::{
        builders::CampaignBuilder,
        types::{
            AnyEntity,
            Audience,
            Campaign,
            CampaignTime,
            Entity,
            EntityId,
            EntityKind,
            FeedEntry,
            FeedEntryId,
            FeedPayload,
            NoteBody,
            PlayerId,
            UnixTime,
        },
    };
    use localtt_engine::ports::store::StoreBackup;
    use serde::Serialize;
    use tempfile::TempDir;

    use super::*;

    const FIXED_TIME: i64 = 1_750_000_000;

    fn store_path(dir: &TempDir) -> PathBuf {
        dir.path().join("campaign.sqlite")
    }

    fn open(dir: &TempDir) -> SqliteStore {
        SqliteStore::open(&store_path(dir)).expect("a fresh store opens")
    }

    fn campaign() -> Campaign {
        CampaignBuilder::new("Curse of the Vault").build()
    }

    fn note(owner: PlayerId) -> AnyEntity {
        AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                EntityId::new(),
                "Session 4",
                owner,
                NoteBody {
                    text: "The party opened the vault.".to_owned(),
                },
            )
            .created_at(UnixTime::from_seconds(FIXED_TIME))
            .updated_at(UnixTime::from_seconds(FIXED_TIME))
            .build()
            .expect("a note kind pairs with a note body"),
        )
    }

    fn feed_entry(id: u64) -> FeedEntry {
        FeedEntry {
            id: FeedEntryId::new(id),
            ts: UnixTime::from_seconds(FIXED_TIME),
            clock_s: CampaignTime::from_seconds(0),
            actor: None,
            entity: None,
            audience: Audience::Public,
            payload: FeedPayload::Hint {
                text: format!("hint {id}"),
            },
        }
    }

    fn creation_patch<'a>(
        campaign: &'a Campaign,
        entities: &'a [AnyEntity],
        feed: &'a [FeedEntry],
    ) -> StatePatch<'a> {
        StatePatch {
            campaign: Some(campaign),
            upserts: entities.iter().collect(),
            removals: Vec::new(),
            feed_appends: feed.iter().collect(),
            feed_keep_from: None,
        }
    }

    fn bytes<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("documents serialize")
    }

    fn backup_names(dir: &TempDir) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .expect("the directory is listable")
            .map(|entry| {
                entry
                    .expect("entries are readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.contains(".backup-"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_fresh_file_opens_empty_and_reopens() {
        let dir = TempDir::new().expect("a temporary directory");
        let store = open(&dir);
        assert!(matches!(store.load(), Err(StoreFault::NoCampaign)));
        drop(store);

        let reopened = SqliteStore::open(&store_path(&dir)).expect("the created file reopens");

        assert!(matches!(reopened.load(), Err(StoreFault::NoCampaign)));
    }

    #[test]
    fn apply_then_load_round_trips_documents_in_order() {
        let dir = TempDir::new().expect("a temporary directory");
        let mut store = open(&dir);
        let campaign = campaign();
        let owner = PlayerId::new();
        let entities = [note(owner), note(owner)];
        let feed = [feed_entry(1), feed_entry(2)];

        store
            .apply(&creation_patch(&campaign, &entities, &feed))
            .expect("a fresh store accepts the creation patch");
        let loaded = store.load().expect("the patch was just committed");

        assert_eq!(bytes(&loaded.campaign), bytes(&campaign));
        assert_eq!(loaded.entities.len(), entities.len());
        for (loaded, original) in loaded.entities.iter().zip(&entities) {
            assert_eq!(bytes(loaded), bytes(original));
        }
        assert_eq!(loaded.feed.len(), feed.len());
        for (loaded, original) in loaded.feed.iter().zip(&feed) {
            assert_eq!(bytes(loaded), bytes(original));
        }
    }

    #[test]
    fn a_reopen_sees_the_committed_state() {
        let dir = TempDir::new().expect("a temporary directory");
        let campaign = campaign();
        let mut store = open(&dir);
        store
            .apply(&creation_patch(&campaign, &[], &[]))
            .expect("a campaign-only patch is accepted");
        drop(store);

        let reopened = SqliteStore::open(&store_path(&dir)).expect("the file reopens");

        let loaded = reopened.load().expect("the committed campaign is there");
        assert_eq!(bytes(&loaded.campaign), bytes(&campaign));
    }

    #[test]
    fn invalid_patch_is_refused_without_writing() {
        let dir = TempDir::new().expect("a temporary directory");
        let mut store = open(&dir);
        let campaign = campaign();
        let owner = PlayerId::new();
        let entities = [note(owner)];
        store
            .apply(&creation_patch(&campaign, &entities, &[]))
            .expect("a fresh store accepts the creation patch");
        let other = CampaignBuilder::new("A different campaign").build();

        let refused = store
            .apply(&StatePatch {
                campaign: Some(&other),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect_err("a campaign id that does not match is refused");

        assert!(matches!(refused, StoreFault::InvalidPatch { .. }));
        let loaded = store.load().expect("the refused patch wrote nothing");
        assert_eq!(bytes(&loaded.campaign), bytes(&campaign));
        assert_eq!(loaded.entities.len(), 1);
    }

    #[test]
    fn a_foreign_database_is_refused() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        let connection = Connection::open(&path).expect("a plain database opens");
        connection
            .execute_batch("CREATE TABLE unrelated (x TEXT);")
            .expect("a plain database takes DDL");
        drop(connection);

        match SqliteStore::open(&path) {
            Err(fault) => assert!(matches!(fault, StoreFault::Corrupt { .. }), "got {fault:?}"),
            Ok(_) => panic!("a foreign database must be refused"),
        }
    }

    #[test]
    fn a_garbage_file_is_refused() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        fs::write(&path, b"this is not a database").expect("the file is writable");

        match SqliteStore::open(&path) {
            Err(fault) => assert!(matches!(fault, StoreFault::Corrupt { .. }), "got {fault:?}"),
            Ok(_) => panic!("a garbage file must be refused"),
        }
    }

    #[test]
    fn a_newer_ddl_version_is_refused() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        drop(open(&dir));
        let connection = Connection::open(&path).expect("the store file opens");
        connection
            .pragma_update(None, "user_version", 99)
            .expect("user_version is writable");
        drop(connection);

        match SqliteStore::open(&path) {
            Err(StoreFault::DdlVersionRefused { found, supported }) => {
                assert_eq!(found, 99);
                assert_eq!(supported, schema::SUPPORTED_DDL_VERSION);
            }
            other => panic!("a newer DDL version must be refused, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_table_is_corrupt() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        drop(open(&dir));
        let connection = Connection::open(&path).expect("the store file opens");
        connection
            .execute_batch("DROP TABLE entities;")
            .expect("the table is droppable");
        drop(connection);

        match SqliteStore::open(&path) {
            Err(StoreFault::Corrupt { .. }) => {}
            other => panic!("a missing table must be Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_version_zero_file_with_tables_is_corrupt() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        drop(open(&dir));
        let connection = Connection::open(&path).expect("the store file opens");
        connection
            .pragma_update(None, "user_version", 0)
            .expect("user_version is writable");
        drop(connection);

        match SqliteStore::open(&path) {
            Err(StoreFault::Corrupt { .. }) => {}
            other => panic!("tables at DDL version 0 must be Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_negative_ddl_version_is_corrupt() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        drop(open(&dir));
        let connection = Connection::open(&path).expect("the store file opens");
        connection
            .pragma_update(None, "user_version", -1)
            .expect("user_version is writable");
        drop(connection);

        match SqliteStore::open(&path) {
            Err(StoreFault::Corrupt { .. }) => {}
            other => panic!("a negative DDL version must be Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_store_in_a_missing_directory_is_unavailable() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = dir.path().join("absent").join("campaign.sqlite");

        match SqliteStore::open(&path) {
            Err(StoreFault::Unavailable { .. }) => {}
            other => panic!("a missing parent directory must be Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_revision_beyond_sqlite_integer_is_a_backend_fault() {
        let dir = TempDir::new().expect("a temporary directory");
        let mut store = open(&dir);
        let campaign = campaign();
        let oversized = AnyEntity::from(
            Entity::builder(
                EntityKind::Note,
                EntityId::new(),
                "Huge",
                PlayerId::new(),
                NoteBody {
                    text: "revision beyond i64".to_owned(),
                },
            )
            .rev(u64::MAX)
            .build()
            .expect("a note kind pairs with a note body"),
        );

        let refused = store
            .apply(&creation_patch(&campaign, &[oversized], &[]))
            .expect_err("a revision beyond i64::MAX cannot be stored");

        assert!(
            matches!(refused, StoreFault::Backend { .. }),
            "an unrepresentable revision is a Backend fault, got {refused:?}"
        );
        assert!(
            matches!(store.load(), Err(StoreFault::NoCampaign)),
            "the failed write rolled back the campaign too"
        );
    }

    #[test]
    fn open_with_startup_backup_copies_the_previous_state() {
        let dir = TempDir::new().expect("a temporary directory");
        let path = store_path(&dir);
        let campaign = campaign();
        let mut store = open(&dir);
        store
            .apply(&creation_patch(&campaign, &[], &[]))
            .expect("a campaign-only patch is accepted");
        drop(store);

        let store = SqliteStore::open_with(
            &path,
            SqliteOptions {
                startup_backup: true,
            },
        )
        .expect("the store reopens");

        let backups = backup_names(&dir);
        assert_eq!(backups.len(), 1, "one startup backup: {backups:?}");
        let copy = SqliteStore::open(&dir.path().join(&backups[0])).expect("the copy opens");
        let loaded = copy.load().expect("the startup backup holds a campaign");
        assert_eq!(bytes(&loaded.campaign), bytes(&campaign));
        drop(store);
    }

    #[test]
    fn the_trait_backup_writes_an_openable_copy() {
        let dir = TempDir::new().expect("a temporary directory");
        let campaign = campaign();
        let mut store = open(&dir);
        store
            .apply(&creation_patch(&campaign, &[], &[]))
            .expect("a campaign-only patch is accepted");
        let target = dir.path().join("copy.sqlite");

        StoreBackup::backup(&mut store, &target).expect("a backup is written");

        let copy = SqliteStore::open(&target).expect("the copy opens as a store");
        let loaded = copy.load().expect("the copy holds the campaign");
        assert_eq!(bytes(&loaded.campaign), bytes(&campaign));
        let leftovers = backup_names(&dir);
        assert!(
            leftovers.is_empty(),
            "backups of explicit paths do not rotate: {leftovers:?}"
        );
        assert!(!target.with_file_name("copy.sqlite.tmp").exists());
    }
}
