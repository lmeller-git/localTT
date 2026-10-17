//! The store contract suite: one shared set of checks, run against every
//! `CampaignStore` implementation (`docs/rfcs/store.md` section 6.3; design
//! section 5.9).
//!
//! Fixtures are adapter-local and built through `localtt_core::builders`; the
//! golden cast stays in `localtt-integration`. The SQLite-only block
//! (`open`/`backup`/`sync`, DDL checks, raw-SQL fabrication) drives the
//! concrete store through this crate's own `rusqlite` dependency.

use std::{collections::BTreeMap, fs, path::Path};

use localtt_adapter::store::{
    decode_bundle,
    import_into,
    memory::MemoryStore,
    sqlite::{SqliteOptions, SqliteStore},
};
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{
        Abilities,
        AnyEntity,
        Audience,
        BodyKind,
        Campaign,
        CampaignTime,
        Entity,
        EntityId,
        EntityKind,
        FeedEntry,
        FeedEntryId,
        FeedPayload,
        HandoutBody,
        HandoutContent,
        NoteBody,
        PlayerId,
        SchemaVersion,
        UnixTime,
    },
};
use localtt_engine::{
    export::ExportBundle,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreBackup, StoreFault},
};
use rusqlite::Connection;
use serde::Serialize;
use tempfile::TempDir;

/// Supplies the shared suite with the stores it drives.
///
/// The harness owns implementation details (temp dirs, file paths) so the
/// suite itself only ever sees the [`CampaignStore`] port.
trait StoreHarness {
    /// Returns a fresh, empty store; every call is independent of earlier
    /// ones.
    fn fresh(&mut self) -> Box<dyn CampaignStore>;
}

/// Runs the shared contract suite against `harness`'s implementation.
fn store_contract(harness: &mut dyn StoreHarness) {
    fresh_store_reports_no_campaign(harness);
    apply_then_load_round_trips_every_document_kind(harness);
    shuffled_upserts_load_sorted(harness);
    campaign_singleton_semantics(harness);
    upserts_and_removals_change_the_stored_state(harness);
    every_precondition_is_refused_without_writing(harness);
    trimming_runs_before_appends(harness);
    version_gate(harness);
    bundles_round_trip(harness);
    bundles_refuse_unloadable_envelopes(harness);
    import_refuses_an_occupied_store(harness);
    import_propagates_a_non_empty_fault(harness);
}

#[test]
fn memory_store_satisfies_the_contract() {
    store_contract(&mut MemoryHarness);
}

#[test]
fn sqlite_store_satisfies_the_contract() {
    let directory = TempDir::new().expect("a temporary directory");
    let mut harness = SqliteHarness {
        directory: directory.path(),
        next: 0,
    };

    store_contract(&mut harness);
    sqlite_only_checks(directory.path());
}

/// The in-memory store; nothing to own.
struct MemoryHarness;

impl StoreHarness for MemoryHarness {
    fn fresh(&mut self) -> Box<dyn CampaignStore> {
        Box::new(MemoryStore::new())
    }
}

/// The file-backed store; one fresh file per [`StoreHarness::fresh`] call.
struct SqliteHarness<'a> {
    directory: &'a Path,
    next: usize,
}

impl StoreHarness for SqliteHarness<'_> {
    fn fresh(&mut self) -> Box<dyn CampaignStore> {
        let path = self.directory.join(format!("store-{}.sqlite", self.next));
        self.next += 1;
        Box::new(SqliteStore::open(&path).expect("a fresh store opens"))
    }
}

// ---------------------------------------------------------------------------
// Shared checks
// ---------------------------------------------------------------------------

fn fresh_store_reports_no_campaign(harness: &mut dyn StoreHarness) {
    let store = harness.fresh();

    assert!(matches!(store.load(), Err(StoreFault::NoCampaign)));
}

fn apply_then_load_round_trips_every_document_kind(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();

    store
        .apply(&fixture.creation_patch())
        .expect("a fresh store accepts the creation patch");
    let loaded = store.load().expect("the patch was just committed");

    assert_eq!(bytes(&loaded.campaign), bytes(&fixture.campaign));
    assert_eq!(
        entity_docs(&loaded.entities),
        entity_docs(&fixture.entities)
    );
    assert_eq!(feed_docs(&loaded.feed), feed_docs(&fixture.feed));
    assert_sorted(&loaded);
}

fn shuffled_upserts_load_sorted(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&patch(
            Some(&fixture.campaign),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("the campaign lands first");
    let mut shuffled: Vec<&AnyEntity> = fixture.entities.iter().collect();
    shuffled.sort_by_key(|entity| std::cmp::Reverse(entity.id()));

    store
        .apply(&patch(
            None,
            shuffled,
            Vec::new(),
            fixture.feed.iter().collect(),
            None,
        ))
        .expect("a patch with shuffled upserts is applied");

    let loaded = store.load().expect("the patch was just committed");
    assert_sorted(&loaded);
    assert_eq!(
        entity_docs(&loaded.entities),
        entity_docs(&fixture.entities)
    );
}

fn campaign_singleton_semantics(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&patch(
            Some(&fixture.campaign),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("`Some` creates the campaign on an empty store");

    let mut renamed = fixture.campaign.clone();
    renamed.name = "Renamed mid-campaign".to_owned();
    store
        .apply(&patch(
            Some(&renamed),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("`Some` with the stored id updates it");

    let loaded = store.load().expect("the update was committed");
    assert_eq!(bytes(&loaded.campaign), bytes(&renamed));
}

fn upserts_and_removals_change_the_stored_state(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");

    let updated = note_with_id(
        fixture.entities[1].id(),
        fixture.entities[1].owner(),
        "The vault is open.",
    );
    let removed = fixture.entities[2].id();
    store
        .apply(&patch(
            None,
            vec![&updated],
            vec![removed],
            Vec::new(),
            None,
        ))
        .expect("an upsert of a stored entity plus a removal is accepted");

    let loaded = store.load().expect("the patch was just committed");
    let expected = BTreeMap::from([
        (fixture.entities[0].id(), bytes(&fixture.entities[0])),
        (updated.id(), bytes(&updated)),
    ]);
    assert_eq!(
        entity_docs(&loaded.entities),
        expected,
        "the removal is gone and the upsert replaced the stored revision"
    );
    assert_sorted(&loaded);
}

fn every_precondition_is_refused_without_writing(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let baseline = snapshot_docs(&store.load().expect("setup: the fixture loads"));

    let unknown = note(PlayerId::new());
    let stored_append = feed_entry(10);
    let below_cutoff = feed_entry(12);
    let other_campaign = CampaignBuilder::new("Another campaign").build();

    let refusals: Vec<(&str, StatePatch<'_>)> = vec![
        (
            "1: an entity is upserted twice",
            patch(
                None,
                vec![&fixture.entities[1], &fixture.entities[1]],
                Vec::new(),
                Vec::new(),
                None,
            ),
        ),
        (
            "2: an entity is upserted and removed",
            patch(
                None,
                vec![&fixture.entities[1]],
                vec![fixture.entities[1].id()],
                Vec::new(),
                None,
            ),
        ),
        (
            "3: a removal names an unstored entity",
            patch(None, Vec::new(), vec![unknown.id()], Vec::new(), None),
        ),
        (
            "4: feed appends are not ascending",
            patch(
                None,
                Vec::new(),
                Vec::new(),
                vec![&fixture.feed[1], &fixture.feed[0]],
                None,
            ),
        ),
        (
            "5: an appended id is already stored",
            patch(None, Vec::new(), Vec::new(), vec![&stored_append], None),
        ),
        (
            "6: a stored id below the cutoff is refused by the cutoff rule",
            patch(
                None,
                Vec::new(),
                Vec::new(),
                vec![&stored_append],
                Some(FeedEntryId::new(15)),
            ),
        ),
        (
            "6: an appended id is below the trim cutoff",
            patch(
                None,
                Vec::new(),
                Vec::new(),
                vec![&below_cutoff],
                Some(FeedEntryId::new(15)),
            ),
        ),
        (
            "7: the campaign id changed",
            patch(
                Some(&other_campaign),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            ),
        ),
    ];
    for (what, refused_patch) in &refusals {
        let refused = store
            .apply(refused_patch)
            .expect_err("a precondition violation is refused");
        assert!(
            matches!(refused, StoreFault::InvalidPatch { .. }),
            "precondition {what} is InvalidPatch, got {refused:?}"
        );
        assert_eq!(
            snapshot_docs(&store.load().expect("the store still loads")),
            baseline,
            "precondition {what} wrote nothing"
        );
    }

    // 8: documents without a campaign, on both an empty and an occupied store.
    let mut empty = harness.fresh();
    for (what, refused_patch) in [
        (
            "8: upserts without a stored campaign",
            patch(
                None,
                vec![&fixture.entities[0]],
                Vec::new(),
                Vec::new(),
                None,
            ),
        ),
        (
            "8: feed appends without a stored campaign",
            patch(None, Vec::new(), Vec::new(), vec![&fixture.feed[0]], None),
        ),
    ] {
        let refused = empty
            .apply(&refused_patch)
            .expect_err("documents require a stored campaign");
        assert!(
            matches!(refused, StoreFault::InvalidPatch { .. }),
            "precondition {what} is InvalidPatch, got {refused:?}"
        );
        assert!(matches!(empty.load(), Err(StoreFault::NoCampaign)));
    }

    // The empty patch is a valid no-op, with and without a stored campaign.
    store
        .apply(&patch(None, Vec::new(), Vec::new(), Vec::new(), None))
        .expect("an empty patch is a valid no-op");
    assert_eq!(
        snapshot_docs(&store.load().expect("the store still loads")),
        baseline
    );
    empty
        .apply(&patch(
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(FeedEntryId::new(5)),
        ))
        .expect("a trim-only patch carries no documents (precondition 8)");
    assert!(matches!(empty.load(), Err(StoreFault::NoCampaign)));
}

fn trimming_runs_before_appends(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let appended = feed_entry(30);

    store
        .apply(&patch(
            None,
            Vec::new(),
            Vec::new(),
            vec![&appended],
            Some(FeedEntryId::new(15)),
        ))
        .expect("a trim plus an append is applied");

    let loaded = store.load().expect("the patch was just committed");
    let ids: Vec<u64> = loaded.feed.iter().map(|entry| entry.id.get()).collect();
    assert_eq!(
        ids,
        vec![20, 30],
        "the trim dropped 10 and the append landed"
    );
}

fn version_gate(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut future = fixture.campaign.clone();
    future.schema_version = SchemaVersion { major: 2, minor: 0 };
    let mut store = harness.fresh();
    store
        .apply(&patch(
            Some(&future),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("`apply` stores typed documents ungated (N4)");

    match store.load() {
        Err(StoreFault::VersionRefused { found, supported }) => {
            assert_eq!(found, SchemaVersion { major: 2, minor: 0 });
            assert_eq!(supported, SchemaVersion::CURRENT);
        }
        other => panic!("a newer document major must be refused, got {other:?}"),
    }

    let mut older = fixture.campaign.clone();
    older.schema_version = SchemaVersion { major: 0, minor: 9 };
    let mut legacy_store = harness.fresh();
    legacy_store
        .apply(&patch(
            Some(&older),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("an older major is accepted");
    let loaded = legacy_store.load().expect("an older major loads");
    assert_eq!(loaded.campaign.schema_version, SchemaVersion::CURRENT);
}

fn bundles_round_trip(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut source = harness.fresh();
    source
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let first = ExportBundle::from_snapshot(
        &source.load().expect("setup: the fixture loads"),
        UnixTime::from_seconds(FIXED_TIME),
    );

    let decoded = decode_bundle(&bytes(&first)).expect("bundles are portable JSON");
    let mut restored = harness.fresh();
    import_into(&mut *restored, &decoded).expect("an empty store accepts a bundle");
    let loaded = restored.load().expect("the import committed");

    let mut second = ExportBundle::from_snapshot(&loaded, UnixTime::from_seconds(FIXED_TIME + 1));
    second.exported_at = first.exported_at;
    assert_eq!(bytes(&second), bytes(&first));
}

fn bundles_refuse_unloadable_envelopes(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let bundle = ExportBundle::from_snapshot(
        &store.load().expect("setup: the fixture loads"),
        UnixTime::from_seconds(FIXED_TIME),
    );
    let tampered = |rewrite: &dyn Fn(&mut serde_json::Value)| {
        let mut wire = serde_json::to_value(&bundle).expect("the bundle serializes");
        rewrite(&mut wire);
        wire.to_string()
    };

    match decode_bundle(&tampered(&|wire| {
        wire["format"] = serde_json::json!("some.other.tool");
    })) {
        Err(StoreFault::Corrupt { .. }) => {}
        other => panic!("a foreign format must be Corrupt, got {other:?}"),
    }
    match decode_bundle(&tampered(&|wire| {
        wire["format_version"] = serde_json::json!(2);
    })) {
        Err(StoreFault::FormatRefused { found, supported }) => {
            assert_eq!(found, 2);
            assert_eq!(supported, ExportBundle::CURRENT_FORMAT_VERSION);
        }
        other => panic!("a newer format version must be FormatRefused, got {other:?}"),
    }
    match decode_bundle(&tampered(&|wire| {
        wire["format_version"] = serde_json::json!(0);
    })) {
        Err(StoreFault::Corrupt { .. }) => {}
        other => panic!("a below-one format version must be Corrupt, got {other:?}"),
    }
}

fn import_refuses_an_occupied_store(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let bundle = ExportBundle::from_snapshot(
        &store.load().expect("setup: the fixture loads"),
        UnixTime::from_seconds(FIXED_TIME),
    );

    match import_into(&mut *store, &bundle) {
        Err(StoreFault::StoreNotEmpty) => {}
        other => panic!("a non-empty store must be StoreNotEmpty, got {other:?}"),
    }
}

/// `import_into` only treats `NoCampaign` as "empty": every other `load` fault
/// must pass through unchanged (N10).
fn import_propagates_a_non_empty_fault(harness: &mut dyn StoreHarness) {
    let fixture = Fixture::new();
    let mut store = harness.fresh();
    let mut future = fixture.campaign.clone();
    future.schema_version = SchemaVersion { major: 2, minor: 0 };
    store
        .apply(&patch(
            Some(&future),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("`apply` stores typed documents ungated (N4)");
    let snapshot = CampaignSnapshot {
        campaign: fixture.campaign.clone(),
        entities: fixture.entities.clone(),
        feed: fixture.feed.clone(),
    };
    let bundle = ExportBundle::from_snapshot(&snapshot, UnixTime::from_seconds(FIXED_TIME));

    match import_into(&mut *store, &bundle) {
        Err(StoreFault::VersionRefused { .. }) => {}
        other => panic!("a load fault other than NoCampaign must propagate, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// SQLite-only checks: `open`/`backup`/`sync`, DDL, raw-SQL fabrication
// ---------------------------------------------------------------------------

fn sqlite_only_checks(directory: &Path) {
    reopen_equals_the_previous_state(directory);
    backup_opens_and_equals_the_source(directory);
    rotation_keeps_complete_files_only(directory);
    a_foreign_database_is_refused(directory);
    a_garbage_file_is_refused(directory);
    a_zero_length_file_opens(directory);
    a_newer_ddl_version_is_refused(directory);
    a_stale_version_column_does_not_hide_a_newer_document(directory);
    projection_columns_are_extracted(directory);
}

fn reopen_equals_the_previous_state(directory: &Path) {
    let path = directory.join("reopen.sqlite");
    let fixture = Fixture::new();
    let mut store = SqliteStore::open(&path).expect("a fresh store opens");
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let before = snapshot_docs(&store.load().expect("setup: the fixture loads"));
    drop(store);

    let reopened = SqliteStore::open(&path).expect("the file reopens");

    assert_eq!(
        snapshot_docs(&reopened.load().expect("the committed state loads")),
        before
    );
}

fn backup_opens_and_equals_the_source(directory: &Path) {
    let path = directory.join("backup.sqlite");
    let target = directory.join("backup-copy.sqlite");
    let fixture = Fixture::new();
    let mut store = SqliteStore::open(&path).expect("a fresh store opens");
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    store.sync().expect("a checkpoint succeeds");

    store.backup(&target).expect("the backup is written");

    let copy = SqliteStore::open(&target).expect("the copy opens as a store");
    assert_eq!(
        snapshot_docs(&copy.load().expect("the copy loads")),
        snapshot_docs(&store.load().expect("the source loads"))
    );
}

fn rotation_keeps_complete_files_only(directory: &Path) {
    let path = directory.join("rotation.sqlite");
    let fixture = Fixture::new();
    let mut store = SqliteStore::open(&path).expect("a fresh store opens");
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    drop(store);
    for seconds in 1..=6 {
        fs::write(
            directory.join(format!("rotation.backup-{seconds}.sqlite")),
            b"complete",
        )
        .expect("the fabricated backup is writable");
    }
    fs::write(
        directory.join("rotation.backup-7.sqlite.tmp"),
        b"in progress",
    )
    .expect("the fabricated temporary is writable");
    fs::write(directory.join("rotation.backup-foreign.sqlite"), b"foreign")
        .expect("the foreign file is writable");

    let mut options = SqliteOptions::default();
    options.startup_backup = true;
    let reopened = SqliteStore::open_with(&path, options).expect("the store reopens and backs up");

    let mut stamps: Vec<u64> = fs::read_dir(directory)
        .expect("the directory is listable")
        .filter_map(|entry| {
            let name = entry.expect("entries are readable").file_name();
            let name = name.to_str()?.to_owned();
            let stamp = name
                .strip_prefix("rotation.backup-")?
                .strip_suffix(".sqlite")?;
            stamp.parse().ok()
        })
        .collect();
    stamps.sort_unstable();
    assert_eq!(
        stamps.len(),
        5,
        "rotation keeps five complete files: {stamps:?}"
    );
    assert_eq!(
        stamps[..4],
        [3, 4, 5, 6],
        "the oldest fabrications are gone"
    );
    assert!(!directory.join("rotation.backup-1.sqlite").exists());
    assert!(!directory.join("rotation.backup-2.sqlite").exists());
    assert!(directory.join("rotation.backup-7.sqlite.tmp").exists());
    assert!(directory.join("rotation.backup-foreign.sqlite").exists());
    drop(reopened);
}

fn a_foreign_database_is_refused(directory: &Path) {
    let path = directory.join("foreign.sqlite");
    let connection = Connection::open(&path).expect("a plain database opens");
    connection
        .execute_batch("CREATE TABLE unrelated (x TEXT);")
        .expect("a plain database takes DDL");
    drop(connection);

    assert_corrupt(SqliteStore::open(&path), "a foreign database");
}

fn a_garbage_file_is_refused(directory: &Path) {
    let path = directory.join("garbage.sqlite");
    fs::write(&path, b"definitely not a database").expect("the file is writable");

    assert_corrupt(SqliteStore::open(&path), "a non-store file");
}

fn a_zero_length_file_opens(directory: &Path) {
    let path = directory.join("empty.sqlite");
    fs::write(&path, []).expect("the empty file is writable");

    let store = SqliteStore::open(&path).expect("a zero-length file is fresh");

    assert!(matches!(store.load(), Err(StoreFault::NoCampaign)));
}

fn a_newer_ddl_version_is_refused(directory: &Path) {
    let path = directory.join("future.sqlite");
    drop(SqliteStore::open(&path).expect("a fresh store opens"));
    let connection = Connection::open(&path).expect("the store file opens");
    connection
        .pragma_update(None, "user_version", 99)
        .expect("user_version is writable");
    drop(connection);

    match SqliteStore::open(&path) {
        Err(StoreFault::DdlVersionRefused { found, supported }) => {
            assert_eq!(found, 99);
            assert!(supported >= 1, "the build knows at least DDL v1");
        }
        other => panic!("a newer DDL version must be refused, got {other:?}"),
    }
}

/// The gate reads the document's own `schema_version`: a newer major is
/// refused even though the extracted column still says `CURRENT` (the column
/// is a projection, never the gate).
fn a_stale_version_column_does_not_hide_a_newer_document(directory: &Path) {
    let path = directory.join("stale.sqlite");
    let fixture = Fixture::new();
    let entity = note(PlayerId::new());
    let id = entity.id().to_string();
    let mut store = SqliteStore::open(&path).expect("a fresh store opens");
    store
        .apply(&patch(
            Some(&fixture.campaign),
            vec![&entity],
            Vec::new(),
            Vec::new(),
            None,
        ))
        .expect("setup: the entity lands");
    drop(store);

    let connection = Connection::open(&path).expect("the store file opens");
    let (doc, column_major): (String, i64) = connection
        .query_row(
            "SELECT doc, schema_version_major FROM entities WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the entity row is there");
    assert_eq!(column_major, 1);
    let mut value: serde_json::Value = serde_json::from_str(&doc).expect("the doc is JSON");
    value["schema_version"] = serde_json::json!({ "major": 2, "minor": 0 });
    connection
        .execute(
            "UPDATE entities SET doc = ?1 WHERE id = ?2",
            rusqlite::params![value.to_string(), id],
        )
        .expect("the doc is overwritten");
    drop(connection);

    let reopened = SqliteStore::open(&path).expect("the file still opens");
    match reopened.load() {
        Err(StoreFault::VersionRefused { found, .. }) => assert_eq!(found.major, 2),
        other => panic!("a newer document major must be refused, got {other:?}"),
    }
}

/// The extracted columns are a projection of the documents, not an oracle: a
/// broken extraction would go unnoticed while only `load` reads the `doc`
/// column, so pin their values before a query depends on them (VER-3).
fn projection_columns_are_extracted(directory: &Path) {
    let path = directory.join("projection.sqlite");
    let fixture = Fixture::new();
    let mut store = SqliteStore::open(&path).expect("a fresh store opens");
    store
        .apply(&fixture.creation_patch())
        .expect("setup: the fixture lands");
    let connection = Connection::open(&path).expect("the file opens for inspection");

    for entity in &fixture.entities {
        let row: (
            String,
            String,
            String,
            Option<String>,
            String,
            i64,
            i64,
            i64,
        ) = connection
            .query_row(
                "SELECT kind, body_kind, owner, parent, name, rev,
                        schema_version_major, schema_version_minor
                   FROM entities WHERE id = ?1",
                rusqlite::params![entity.id().to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .expect("the entity projection row is there");
        let (kind, body_kind, owner, parent, name, rev, major, minor) = row;
        assert_eq!(kind, kind_slug(entity.kind()), "kind column");
        assert_eq!(
            body_kind,
            body_kind_slug(entity.kind().body_kind()),
            "body_kind column"
        );
        assert_eq!(owner, entity.owner().to_string(), "owner column");
        assert_eq!(
            parent,
            entity.parent().map(|id| id.to_string()),
            "parent column"
        );
        assert_eq!(name, entity.name(), "name column");
        assert_eq!(
            rev,
            i64::try_from(entity.rev()).expect("fixture revisions fit i64"),
            "rev column"
        );
        assert_eq!(
            (major, minor),
            (
                i64::from(entity.schema_version().major),
                i64::from(entity.schema_version().minor)
            ),
            "schema_version columns"
        );
    }

    let (ts, clock_s, audience): (i64, i64, String) = connection
        .query_row(
            "SELECT ts, clock_s, audience FROM feed WHERE id = ?1",
            rusqlite::params![
                i64::try_from(fixture.feed[0].id.get()).expect("the feed id fits i64")
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the feed projection row is there");
    assert_eq!(ts, fixture.feed[0].ts.seconds(), "ts column");
    assert_eq!(clock_s, fixture.feed[0].clock_s.seconds(), "clock_s column");
    assert_eq!(audience, "public", "audience column");

    let (major, minor): (i64, i64) = connection
        .query_row(
            "SELECT schema_version_major, schema_version_minor FROM campaign LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the campaign projection row is there");
    assert_eq!(
        (major, minor),
        (
            i64::from(fixture.campaign.schema_version.major),
            i64::from(fixture.campaign.schema_version.minor)
        ),
        "campaign schema_version columns"
    );
}

fn assert_corrupt(result: Result<SqliteStore, StoreFault>, what: &str) {
    match result {
        Err(StoreFault::Corrupt { .. }) => {}
        other => panic!("{what} must be Corrupt, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const FIXED_TIME: i64 = 1_750_000_000;

/// The adapter-local cast: the campaign, one entity per `BodyKind`, two feed
/// entries (ids 10 and 20).
struct Fixture {
    campaign: Campaign,
    entities: Vec<AnyEntity>,
    feed: Vec<FeedEntry>,
}

impl Fixture {
    fn new() -> Self {
        let owner = PlayerId::new();
        let mut campaign = CampaignBuilder::new("Contract fixture");
        campaign.dm("Dana");
        Self {
            campaign: campaign.build(),
            entities: vec![statblock(owner), note(owner), handout(owner)],
            feed: vec![feed_entry(10), feed_entry(20)],
        }
    }

    fn creation_patch(&self) -> StatePatch<'_> {
        patch(
            Some(&self.campaign),
            self.entities.iter().collect(),
            Vec::new(),
            self.feed.iter().collect(),
            None,
        )
    }
}

fn statblock(owner: PlayerId) -> AnyEntity {
    let mut builder = StatblockBuilder::character();
    builder.abilities(Abilities::all(10)).hp(12, 12);
    AnyEntity::from(
        builder
            .build_entity(EntityKind::Pc, EntityId::new(), "Mira", owner)
            .expect("abilities and hp were set"),
    )
}

fn note(owner: PlayerId) -> AnyEntity {
    note_with_id(EntityId::new(), owner, "The party opened the vault.")
}

fn note_with_id(id: EntityId, owner: PlayerId, text: &str) -> AnyEntity {
    AnyEntity::from(
        Entity::builder(
            EntityKind::Note,
            id,
            "Session 4",
            owner,
            NoteBody {
                text: text.to_owned(),
            },
        )
        .created_at(UnixTime::from_seconds(FIXED_TIME))
        .updated_at(UnixTime::from_seconds(FIXED_TIME))
        .build()
        .expect("a note kind pairs with a note body"),
    )
}

/// The wire slug for an entity kind, hardcoded so the projection check uses an
/// oracle independent of the serializer under test.
fn kind_slug(kind: EntityKind) -> String {
    match kind {
        EntityKind::Pc => "pc",
        EntityKind::Companion => "companion",
        EntityKind::Summon => "summon",
        EntityKind::Familiar => "familiar",
        EntityKind::Npc => "npc",
        EntityKind::Monster => "monster",
        EntityKind::Note => "note",
        EntityKind::Handout => "handout",
    }
    .to_owned()
}

/// The wire slug for a body family, hardcoded like [`kind_slug`].
fn body_kind_slug(body: BodyKind) -> String {
    match body {
        BodyKind::Statblock => "statblock",
        BodyKind::Note => "note",
        BodyKind::Handout => "handout",
    }
    .to_owned()
}

fn handout(owner: PlayerId) -> AnyEntity {
    AnyEntity::from(
        Entity::builder(
            EntityKind::Handout,
            EntityId::new(),
            "The vault map",
            owner,
            HandoutBody {
                content: HandoutContent::Text {
                    text: "A hurried sketch of the vault.".to_owned(),
                },
            },
        )
        .created_at(UnixTime::from_seconds(FIXED_TIME))
        .updated_at(UnixTime::from_seconds(FIXED_TIME))
        .build()
        .expect("a handout kind pairs with a handout body"),
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

fn patch<'a>(
    campaign: Option<&'a Campaign>,
    upserts: Vec<&'a AnyEntity>,
    removals: Vec<EntityId>,
    feed_appends: Vec<&'a FeedEntry>,
    feed_keep_from: Option<FeedEntryId>,
) -> StatePatch<'a> {
    StatePatch {
        campaign,
        upserts,
        removals,
        feed_appends,
        feed_keep_from,
    }
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

fn bytes<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("documents serialize")
}

/// Every document in load order, for whole-snapshot comparisons.
fn snapshot_docs(snapshot: &CampaignSnapshot) -> Vec<String> {
    let mut docs = vec![bytes(&snapshot.campaign)];
    docs.extend(snapshot.entities.iter().map(bytes));
    docs.extend(snapshot.feed.iter().map(bytes));
    docs
}

fn entity_docs(entities: &[AnyEntity]) -> BTreeMap<EntityId, String> {
    entities
        .iter()
        .map(|entity| (entity.id(), bytes(entity)))
        .collect()
}

fn feed_docs(feed: &[FeedEntry]) -> BTreeMap<FeedEntryId, String> {
    feed.iter().map(|entry| (entry.id, bytes(entry))).collect()
}

fn assert_sorted(snapshot: &CampaignSnapshot) {
    let entity_ids: Vec<EntityId> = snapshot.entities.iter().map(AnyEntity::id).collect();
    assert!(
        entity_ids.windows(2).all(|pair| pair[0] < pair[1]),
        "entities are ascending by id: {entity_ids:?}"
    );
    let feed_ids: Vec<FeedEntryId> = snapshot.feed.iter().map(|entry| entry.id).collect();
    assert!(
        feed_ids.windows(2).all(|pair| pair[0] < pair[1]),
        "feed is ascending by id: {feed_ids:?}"
    );
}
