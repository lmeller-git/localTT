//! The spawn-blocking async wrapper around the synchronous SQLite store
//! (server RFC 6.3).
//!
//! The wrapper shares one connection behind a mutex. Each async store operation
//! clones the handle and the owned patch, then locks and runs inside
//! `spawn_blocking`, so no mutex guard crosses an `.await` and no `rusqlite` call
//! runs on a runtime worker. `StoreBackup` stays synchronous by contract and is
//! called on the actor thread. A poisoned mutex or a failed blocking task is a
//! backend fault: the connection may be half-updated, so the actor treats it as
//! fatal.

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use async_trait::async_trait;
use localtt_core::types::{AnyEntity, Campaign, EntityId, FeedEntry, FeedEntryId};
use localtt_engine::ports::store::{
    AsyncCampaignStore,
    CampaignSnapshot,
    CampaignStore,
    StatePatch,
    StoreBackup,
    StoreFault,
};
use tokio::task::JoinError;

use crate::store::sqlite::SqliteStore;

/// An [`AsyncCampaignStore`] over the file-backed [`SqliteStore`].
///
/// The sync store stays available for startup, export and the frozen test
/// suite; the server wraps it for the command path.
#[derive(Debug)]
pub struct AsyncSqliteStore {
    inner: Arc<Mutex<SqliteStore>>,
}

impl AsyncSqliteStore {
    /// Wraps an already-open synchronous store.
    #[must_use]
    #[inline]
    pub fn new(store: SqliteStore) -> Self {
        Self {
            inner: Arc::new(Mutex::new(store)),
        }
    }
}

#[async_trait]
impl AsyncCampaignStore for AsyncSqliteStore {
    #[inline]
    async fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            let store = lock(&inner)?;
            store.load()
        })
        .await
        .map_err(task_fault)?
    }

    #[inline]
    async fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
        let inner = Arc::clone(&self.inner);
        let owned = OwnedStatePatch::new(patch);
        tokio::task::spawn_blocking(move || {
            let mut store = lock(&inner)?;
            let patch = owned.as_patch();
            store.apply(&patch)
        })
        .await
        .map_err(task_fault)?
    }
}

impl StoreBackup for AsyncSqliteStore {
    #[inline]
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault> {
        lock(&self.inner)?.backup(to)
    }

    #[inline]
    fn sync(&mut self) -> Result<(), StoreFault> {
        lock(&self.inner)?.sync()
    }
}

/// An owned mirror of [`StatePatch`], built before the blocking call because
/// `spawn_blocking` requires `'static`.
#[derive(Debug)]
struct OwnedStatePatch {
    campaign: Option<Campaign>,
    upserts: Vec<AnyEntity>,
    removals: Vec<EntityId>,
    feed_appends: Vec<FeedEntry>,
    feed_keep_from: Option<FeedEntryId>,
}

impl OwnedStatePatch {
    /// Clones every document and removal id out of a borrowed patch.
    fn new(patch: &StatePatch<'_>) -> Self {
        Self {
            campaign: patch.campaign.cloned(),
            upserts: patch
                .upserts
                .iter()
                .map(|entity| (*entity).clone())
                .collect(),
            removals: patch.removals.clone(),
            feed_appends: patch
                .feed_appends
                .iter()
                .map(|entry| (*entry).clone())
                .collect(),
            feed_keep_from: patch.feed_keep_from,
        }
    }

    /// Borrows the owned documents back into the store's patch shape.
    fn as_patch(&self) -> StatePatch<'_> {
        StatePatch {
            campaign: self.campaign.as_ref(),
            upserts: self.upserts.iter().collect(),
            removals: self.removals.clone(),
            feed_appends: self.feed_appends.iter().collect(),
            feed_keep_from: self.feed_keep_from,
        }
    }
}

/// Locks the connection, mapping poisoning to a fatal backend fault.
fn lock(inner: &Mutex<SqliteStore>) -> Result<MutexGuard<'_, SqliteStore>, StoreFault> {
    inner.lock().map_err(|_| StoreFault::Backend {
        message: "the SQLite connection mutex is poisoned".to_owned(),
    })
}

/// Maps a failed blocking task to the fatal backend class.
fn task_fault(error: JoinError) -> StoreFault {
    StoreFault::Backend {
        message: format!("the blocking store task failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
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
    use tempfile::TempDir;

    use super::*;

    const FIXED_TIME: i64 = 1_750_000_000;

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

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn the_wrapper_is_send_and_sync() {
        assert_send_sync::<AsyncSqliteStore>();
    }

    #[test]
    fn the_owned_patch_preserves_every_field() {
        let campaign = campaign();
        let entity = note(PlayerId::new());
        let feed = feed_entry(7);
        let removed = EntityId::new();
        let patch = StatePatch {
            campaign: Some(&campaign),
            upserts: vec![&entity],
            removals: vec![removed],
            feed_appends: vec![&feed],
            feed_keep_from: Some(FeedEntryId::new(3)),
        };

        let owned = OwnedStatePatch::new(&patch);
        let rebuilt = owned.as_patch();

        assert_eq!(
            rebuilt.campaign.map(|document| document.id),
            Some(campaign.id)
        );
        assert_eq!(rebuilt.upserts.len(), 1);
        assert_eq!(rebuilt.upserts[0].id(), entity.id());
        assert_eq!(rebuilt.removals, vec![removed]);
        assert_eq!(rebuilt.feed_appends.len(), 1);
        assert_eq!(rebuilt.feed_appends[0].id, feed.id);
        assert_eq!(rebuilt.feed_keep_from, Some(FeedEntryId::new(3)));
    }

    #[tokio::test]
    async fn a_poisoned_connection_is_a_backend_fault() {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("campaign.sqlite");
        let store = AsyncSqliteStore::new(SqliteStore::open(&path).expect("a fresh store opens"));
        let inner = Arc::clone(&store.inner);
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = inner.lock().expect("the mutex starts clean");
            panic!("poison the mutex");
        }));

        assert!(poisoned.is_err());
        assert!(matches!(
            store.load().await,
            Err(StoreFault::Backend { .. })
        ));
    }

    #[tokio::test]
    async fn async_apply_round_trips_and_reopens() {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("campaign.sqlite");
        let inner = SqliteStore::open(&path).expect("a fresh store opens");
        let mut store = AsyncSqliteStore::new(inner);
        let campaign = campaign();
        let entities = [note(PlayerId::new())];
        let feed = [feed_entry(1)];

        assert!(matches!(store.load().await, Err(StoreFault::NoCampaign)));
        store
            .apply(&creation_patch(&campaign, &entities, &feed))
            .await
            .expect("the async wrapper applies the creation patch");
        let loaded = store.load().await.expect("the patch was committed");
        assert_eq!(loaded.campaign.id, campaign.id);
        assert_eq!(loaded.entities.len(), 1);
        assert_eq!(loaded.feed.len(), 1);
        drop(store);

        let reopened = SqliteStore::open(&path).expect("the async write reopens");
        let reopened = CampaignStore::load(&reopened).expect("the campaign was durable");
        assert_eq!(reopened.entities.len(), 1);
        assert_eq!(reopened.feed.len(), 1);
    }

    #[tokio::test]
    async fn backup_and_sync_use_the_shared_connection() {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("campaign.sqlite");
        let backup_path = directory.path().join("backup.sqlite");
        let inner = SqliteStore::open(&path).expect("a fresh store opens");
        let mut store = AsyncSqliteStore::new(inner);
        let campaign = campaign();

        store
            .apply(&creation_patch(&campaign, &[], &[]))
            .await
            .expect("the campaign is stored");
        store.sync().expect("the WAL checkpoints");
        store
            .backup(&backup_path)
            .expect("the online backup is written");

        let backup = SqliteStore::open(&backup_path).expect("the backup opens");
        let loaded = CampaignStore::load(&backup).expect("the backup holds the campaign");
        assert_eq!(loaded.campaign.id, campaign.id);
    }
}
