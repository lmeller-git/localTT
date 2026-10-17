//! U1 target test: the campaign store, as a client uses it.
//!
//! This file is the fixed point for `docs/rfcs/store.md`: client code written
//! against the pinned API before any library code exists. It intentionally does
//! not compile until the U1 skeleton lands (`localtt_engine::ports::store` and
//! `localtt_adapter::store` are not implemented yet); that is the spec-phase
//! contract, not an oversight.

use std::{collections::BTreeMap, fs};

use localtt_adapter::store::{export, memory::MemoryStore, sqlite::SqliteStore};
use localtt_core::types::{
    AnyEntity,
    Audience,
    Campaign,
    CampaignTime,
    EntityId,
    FeedEntry,
    FeedEntryId,
    FeedPayload,
    Id,
    PlayerId,
    SchemaVersion,
    UnixTime,
};
use localtt_engine::{
    export::ExportBundle,
    ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreBackup, StoreFault},
};
use localtt_integration::{fixtures, roundtrip::to_json};
use serde::Serialize;
use tempfile::TempDir;

/// The fixed wall-clock stamp the fixture cast uses.
const FIXED_TIME: i64 = 1_750_000_000;

/// Serializes a core document; the fixture suite compares exact bytes.
fn bytes<T: Serialize>(value: &T) -> String {
    to_json(value).expect("core documents serialize")
}

/// Entity documents keyed by id, so comparison does not depend on order.
fn entity_bytes(entities: &[AnyEntity]) -> BTreeMap<EntityId, String> {
    entities
        .iter()
        .map(|entity| (entity.id(), bytes(entity)))
        .collect()
}

/// Feed entries keyed by id, so comparison does not depend on order.
fn feed_bytes(entries: &[FeedEntry]) -> BTreeMap<FeedEntryId, String> {
    entries
        .iter()
        .map(|entry| (entry.id, bytes(entry)))
        .collect()
}

/// The party's whole initial durable state as one creation patch.
fn party_patch<'a>(
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

/// The DM's session-start entry.
fn session_hint(dm: PlayerId) -> FeedEntry {
    FeedEntry {
        id: FeedEntryId::new(1),
        ts: UnixTime::from_seconds(FIXED_TIME),
        clock_s: CampaignTime::from_seconds(0),
        actor: Some(dm),
        entity: None,
        audience: Audience::Public,
        payload: FeedPayload::Hint {
            text: "Session 4 begins; the party enters the vault.".to_owned(),
        },
    }
}

/// Asserts a loaded snapshot equals the documents that were applied, in the
/// snapshot's documented order.
fn assert_snapshot(
    snapshot: &CampaignSnapshot,
    campaign: &Campaign,
    entities: &[AnyEntity],
    feed: &[FeedEntry],
) {
    assert_eq!(bytes(&snapshot.campaign), bytes(campaign));
    assert_eq!(entity_bytes(&snapshot.entities), entity_bytes(entities));
    assert_eq!(feed_bytes(&snapshot.feed), feed_bytes(feed));
    assert!(
        snapshot
            .entities
            .windows(2)
            .all(|pair| pair[0].id() < pair[1].id()),
        "snapshots order entities by id"
    );
    assert!(
        snapshot.feed.windows(2).all(|pair| pair[0].id < pair[1].id),
        "snapshots order feed entries by id"
    );
}

#[test]
fn memory_store_round_trips_the_party_campaign() {
    let party = fixtures::party();
    let entities = party.entities();
    let feed = [session_hint(party.dm)];

    let mut store = MemoryStore::new();
    store
        .apply(&party_patch(&party.campaign, &entities, &feed))
        .expect("a fresh store accepts the party");

    let loaded = store.load().expect("the party was just committed");
    assert_snapshot(&loaded, &party.campaign, &entities, &feed);
}

#[test]
fn sqlite_store_reopens_and_backs_up() {
    let dir = TempDir::new().expect("a temp dir for the store");
    let path = dir.path().join("campaign.sqlite");
    let backup_path = dir.path().join("campaign.backup.sqlite");
    let party = fixtures::party();
    let entities = party.entities();
    let feed = [session_hint(party.dm)];

    let mut store = SqliteStore::open(&path).expect("a new file opens");
    store
        .apply(&party_patch(&party.campaign, &entities, &feed))
        .expect("a fresh store accepts the party");
    store.sync().expect("syncing a committed store");
    store.backup(&backup_path).expect("an online backup");
    drop(store);

    let reopened = SqliteStore::open(&path).expect("reopening the campaign");
    let loaded = reopened.load().expect("the party persisted");
    assert_snapshot(&loaded, &party.campaign, &entities, &feed);

    let backup = SqliteStore::open(&backup_path).expect("the backup opens");
    let restored = backup.load().expect("the backup holds the party");
    assert_snapshot(&restored, &party.campaign, &entities, &feed);
}

#[test]
fn empty_stores_report_no_campaign() {
    let memory = MemoryStore::new();
    assert!(matches!(memory.load(), Err(StoreFault::NoCampaign)));

    let dir = TempDir::new().expect("a temp dir for the store");
    let sqlite = SqliteStore::open(&dir.path().join("campaign.sqlite")).expect("a new file opens");
    assert!(matches!(sqlite.load(), Err(StoreFault::NoCampaign)));
}

#[test]
fn newer_schema_major_is_refused() {
    let dir = TempDir::new().expect("a temp dir for the store");
    let path = dir.path().join("campaign.sqlite");
    let party = fixtures::party();
    let mut future = party.campaign.clone();
    future.schema_version = SchemaVersion { major: 2, minor: 0 };

    let mut store = SqliteStore::open(&path).expect("a new file opens");
    let patch = StatePatch {
        campaign: Some(&future),
        upserts: Vec::new(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    };
    store
        .apply(&patch)
        .expect("apply writes documents as given; the gate is on load");

    match store.load() {
        Err(StoreFault::VersionRefused { found, supported }) => {
            assert_eq!(found, future.schema_version);
            assert_eq!(supported, SchemaVersion::CURRENT);
        }
        other => panic!("a future major must be refused, got {other:?}"),
    }
}

#[test]
fn corrupt_store_file_is_refused() {
    let dir = TempDir::new().expect("a temp dir for the store");
    let path = dir.path().join("not-a-store.sqlite");
    fs::write(&path, b"this file is not a localtt store").expect("writing the file");

    match SqliteStore::open(&path) {
        Err(StoreFault::Corrupt { .. }) => {}
        other => panic!("a non-store file must be refused as corrupt, got {other:?}"),
    }
}

#[test]
fn failing_patch_applies_nothing() {
    let dir = TempDir::new().expect("a temp dir for the store");
    let path = dir.path().join("campaign.sqlite");
    let party = fixtures::party();
    let entities = party.entities();
    let mut store = SqliteStore::open(&path).expect("a new file opens");
    store
        .apply(&party_patch(&party.campaign, &entities, &[]))
        .expect("a fresh store accepts the party");
    let before = store.load().expect("the party was just committed");

    // A patch carrying a campaign that belongs to a different campaign must be
    // rejected as a whole: the goblin it also carries never lands.
    let goblin = AnyEntity::from(fixtures::goblin(party.player));
    let mut other_campaign = party.campaign.clone();
    other_campaign.id = Id::new();
    let patch = StatePatch {
        campaign: Some(&other_campaign),
        upserts: vec![&goblin],
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    };
    assert!(matches!(
        store.apply(&patch),
        Err(StoreFault::InvalidPatch { .. })
    ));

    let after = store
        .load()
        .expect("the failed patch left the store readable");
    assert_eq!(bytes(&after.campaign), bytes(&before.campaign));
    assert_eq!(
        entity_bytes(&after.entities),
        entity_bytes(&before.entities)
    );
    assert_eq!(feed_bytes(&after.feed), feed_bytes(&before.feed));
    assert!(
        !after
            .entities
            .iter()
            .any(|entity| entity.id() == goblin.id())
    );
}

#[test]
fn export_bundle_round_trips_into_a_fresh_store() {
    let dir = TempDir::new().expect("a temp dir for the store");
    let party = fixtures::party();
    let entities = party.entities();
    let feed = [session_hint(party.dm)];

    let mut source =
        SqliteStore::open(&dir.path().join("campaign.sqlite")).expect("a new file opens");
    source
        .apply(&party_patch(&party.campaign, &entities, &feed))
        .expect("a fresh store accepts the party");

    let first = ExportBundle::from_snapshot(
        &source.load().expect("the party was just committed"),
        UnixTime::from_seconds(FIXED_TIME),
    );
    let decoded = export::decode_bundle(&bytes(&first)).expect("bundles are portable JSON");

    let mut restored = MemoryStore::new();
    export::import_into(&mut restored, &decoded).expect("import into an empty store");
    let loaded = restored.load().expect("the bundle just persisted");
    assert_snapshot(&loaded, &party.campaign, &entities, &feed);

    // Re-export equals the first bundle modulo the documented `exported_at`.
    let mut second = ExportBundle::from_snapshot(&loaded, UnixTime::from_seconds(FIXED_TIME + 1));
    second.exported_at = first.exported_at;
    assert_eq!(bytes(&second), bytes(&first));

    // Import refuses a store that already holds a campaign.
    let mut occupied = MemoryStore::new();
    occupied
        .apply(&party_patch(&party.campaign, &entities, &[]))
        .expect("a fresh store accepts the party");
    assert!(matches!(
        export::import_into(&mut occupied, &second),
        Err(StoreFault::StoreNotEmpty)
    ));
}
