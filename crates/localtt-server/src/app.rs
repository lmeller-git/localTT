//! The async façade and the single-writer actor (`docs/rfcs/server.md` 4.1,
//! 4.2 and 5).
//!
//! The actor owns the working set, the store and the command environment. It
//! runs on a dedicated thread with a current-thread Tokio runtime and
//! constructs one [`AsyncExecutor`] per command so the store borrow ends before
//! a save/backup. Read services and projections run on the same thread, which
//! keeps every view consistent with the last commit.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use localtt_core::types::{EntityId, FeedEntryId, PlayerId, RefId};
use localtt_engine::{
    authz::{self, CampaignView, EntityView, FeedEntryView},
    commands::{Command, CommandEnv, Event},
    content::{
        ContentEntry,
        ContentKind,
        ContentSummary,
        Provenance,
        ReferenceSource,
        SearchQuery,
    },
    executor::{AsyncExecutor, Executed},
    ports::store::{AsyncCampaignStore, CampaignSnapshot, StoreBackup},
    services::{self, SheetView},
    state::{LoadedCampaign, WorkingSet, WorkingSetMut},
};
pub use localtt_protocol::{ActorStatsDto, CommandOutcome, FeedPage, StreamEvent};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::Instrument;

use crate::error::AppError;

/// How many unread requests the actor channel buffers.
const REQUEST_BUFFER: usize = 64;

/// How many events one subscriber may lag before [`StreamEvent::Lagged`].
const EVENT_BUFFER: usize = 64;

/// The most content hits one search may return, regardless of the request.
const CONTENT_LIMIT: usize = 100;

/// A subscriber's projected event stream.
///
/// `recv` yields `None` once the actor is gone. A lagged receiver yields
/// [`StreamEvent::Lagged`] once and then continues with the oldest retained
/// event.
pub struct EventStream {
    receiver: broadcast::Receiver<StreamEvent>,
}

impl EventStream {
    /// Waits for the next event, `None` when the actor has shut down.
    #[inline]
    pub async fn recv(&mut self) -> Option<StreamEvent> {
        match self.receiver.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(StreamEvent::Lagged),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

/// An owned reference-content search request.
///
/// The server converts this to the engine's borrowed
/// [`SearchQuery`]. `limit` is capped at
/// 100 by [`AppHandle::search_content`]; an empty `kinds` searches every kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentQuery {
    /// The search text; the source trims it and matches case-insensitively.
    pub text: String,
    /// Kinds to include, in ranking-priority order; empty means all.
    pub kinds: Vec<ContentKind>,
    /// Maximum hits requested; zero yields no hits.
    pub limit: usize,
}

/// The single async seam over the actor.
///
/// Clone freely; every clone shares the actor, the request channel and the
/// content source.
#[derive(Clone)]
pub struct AppHandle {
    inner: Arc<Inner>,
}

struct Inner {
    sender: mpsc::Sender<Request>,
    content: Arc<dyn ReferenceSource>,
    join: Mutex<Option<JoinHandle<()>>>,
    started: Instant,
}

impl AppHandle {
    /// Starts the actor on a dedicated thread.
    ///
    /// `snapshot` must be the current durable contents of `store`; the caller
    /// normally obtains it from [`crate::session::start`]. `env` is owned by
    /// the actor between commands. `content` must be the same source the
    /// environment serves; content reads use the handle's clone directly and
    /// never enter the actor queue.
    ///
    /// The actor thread owns a current-thread Tokio runtime; store operations
    /// may move to the blocking pool, while reads and backups stay on the
    /// serialized actor thread.
    ///
    /// # Errors
    ///
    /// Returns the runtime-build or OS thread-spawn error if the actor cannot
    /// start.
    #[inline]
    pub fn start<S>(
        store: S,
        snapshot: CampaignSnapshot,
        env: Box<dyn CommandEnv>,
        content: Arc<dyn ReferenceSource>,
    ) -> std::io::Result<Self>
    where
        S: AsyncCampaignStore + StoreBackup + 'static,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (sender, receiver) = mpsc::channel(REQUEST_BUFFER);
        let started = Instant::now();
        let join = std::thread::Builder::new()
            .name("localtt-actor".to_owned())
            .spawn(move || Actor::new(store, snapshot, env).run(receiver, runtime))?;
        Ok(Self {
            inner: Arc::new(Inner {
                sender,
                content,
                join: Mutex::new(Some(join)),
                started,
            }),
        })
    }

    /// Executes one command and returns the acting client's projection.
    ///
    /// # Errors
    ///
    /// Propagates engine refusals and store faults; [`AppError::ActorGone`]
    /// when the actor is no longer running.
    #[inline]
    pub async fn execute(
        &self,
        actor: PlayerId,
        command: Command,
    ) -> Result<CommandOutcome, AppError> {
        self.request(|reply| Request::Execute {
            actor,
            command,
            reply,
        })
        .await
    }

    /// Every entity `viewer` may read.
    ///
    /// # Errors
    ///
    /// [`AppError::ActorGone`] when the actor is no longer running.
    #[inline]
    pub async fn list_entities(&self, viewer: PlayerId) -> Result<Vec<EntityView>, AppError> {
        self.request(|reply| Request::ListEntities { viewer, reply })
            .await
    }

    /// One entity view, with the engine's missing-before-unauthorized order.
    ///
    /// # Errors
    ///
    /// Propagates the engine refusal; [`AppError::ActorGone`] when the actor is
    /// no longer running.
    #[inline]
    pub async fn get_entity(&self, viewer: PlayerId, id: EntityId) -> Result<EntityView, AppError> {
        self.request(|reply| Request::GetEntity { viewer, id, reply })
            .await
    }

    /// One feed page and the trim signal for the viewer.
    ///
    /// # Errors
    ///
    /// [`AppError::ActorGone`] when the actor is no longer running.
    #[inline]
    pub async fn feed_after(
        &self,
        viewer: PlayerId,
        cursor: Option<FeedEntryId>,
        limit: usize,
    ) -> Result<FeedPage, AppError> {
        self.request(|reply| Request::FeedAfter {
            viewer,
            cursor,
            limit,
            reply,
        })
        .await
    }

    /// The campaign and roster as `viewer` sees them.
    ///
    /// # Errors
    ///
    /// Propagates [`EngineError::Unauthorized`](localtt_engine::error::EngineError::Unauthorized) for a non-member;
    /// [`AppError::ActorGone`] when the actor is no longer running.
    #[inline]
    pub async fn campaign(&self, viewer: PlayerId) -> Result<CampaignView, AppError> {
        self.request(|reply| Request::Campaign { viewer, reply })
            .await
    }

    /// One derived sheet for the viewer.
    ///
    /// # Errors
    ///
    /// Propagates the engine refusal (missing, unauthorized, masked or not a
    /// statblock); [`AppError::ActorGone`] when the actor is no longer running.
    #[inline]
    pub async fn sheet(&self, viewer: PlayerId, id: EntityId) -> Result<SheetView, AppError> {
        self.request(|reply| Request::Sheet { viewer, id, reply })
            .await
    }

    /// Searches reference content without entering the actor queue.
    ///
    /// The query runs on the blocking pool because a source may read files.
    /// The requested limit is capped at 100.
    ///
    /// # Errors
    ///
    /// Propagates content faults; [`AppError::ContentTask`] when the blocking
    /// read cannot join.
    #[inline]
    pub async fn search_content(
        &self,
        query: ContentQuery,
    ) -> Result<Vec<ContentSummary>, AppError> {
        let content = Arc::clone(&self.inner.content);
        tokio::task::spawn_blocking(move || {
            let search = SearchQuery {
                text: &query.text,
                kinds: &query.kinds,
                limit: query.limit.min(CONTENT_LIMIT),
            };
            content.search(&search)
        })
        .await
        .map_err(|_| AppError::ContentTask)?
        .map_err(AppError::Content)
    }

    /// Resolves one reference entry by id without entering the actor queue.
    ///
    /// `Ok(None)` means no loaded source has the id.
    ///
    /// # Errors
    ///
    /// Propagates content faults; [`AppError::ContentTask`] when the blocking
    /// read cannot join.
    #[inline]
    pub async fn content_entry(&self, id: &RefId) -> Result<Option<ContentEntry>, AppError> {
        let content = Arc::clone(&self.inner.content);
        let id = id.clone();
        tokio::task::spawn_blocking(move || content.get(&id))
            .await
            .map_err(|_| AppError::ContentTask)?
            .map_err(AppError::Content)
    }

    /// Every loaded reference source, in load order.
    ///
    /// # Errors
    ///
    /// [`AppError::ContentTask`] when the blocking read cannot join.
    #[inline]
    pub async fn content_sources(&self) -> Result<Vec<Provenance>, AppError> {
        let content = Arc::clone(&self.inner.content);
        tokio::task::spawn_blocking(move || content.sources())
            .await
            .map_err(|_| AppError::ContentTask)
    }

    /// Subscribes `viewer` to the projected event stream.
    ///
    /// # Errors
    ///
    /// Propagates [`EngineError::Unauthorized`](localtt_engine::error::EngineError::Unauthorized) for a non-member;
    /// [`AppError::ActorGone`] when the actor is no longer running.
    #[inline]
    pub async fn subscribe(&self, viewer: PlayerId) -> Result<EventStream, AppError> {
        self.request(|reply| Request::Subscribe { viewer, reply })
            .await
    }

    /// Flushes the store durability boundary.
    ///
    /// # Errors
    ///
    /// Propagates store faults; [`AppError::ActorGone`] when the actor is no
    /// longer running.
    #[inline]
    pub async fn save(&self) -> Result<(), AppError> {
        self.request(|reply| Request::Save { reply }).await
    }

    /// Samples the actor counters and channel state.
    ///
    /// The queue depth is read before the stats request enters the actor, so it
    /// is the number of other requests already waiting.
    ///
    /// # Errors
    ///
    /// [`AppError::ActorGone`] when the actor is no longer running.
    #[inline]
    pub async fn stats(&self) -> Result<ActorStatsDto, AppError> {
        let queue_capacity = self.inner.sender.max_capacity();
        let queue_depth = queue_capacity.saturating_sub(self.inner.sender.capacity());
        let mut stats = self.request(|reply| Request::Stats { reply }).await?;
        stats.queue_capacity = queue_capacity;
        stats.queue_depth = queue_depth;
        Ok(stats)
    }

    /// Whole time since the actor was started.
    ///
    /// The value is measured on the handle's clock and is not persisted.
    #[must_use]
    #[inline]
    pub fn uptime(&self) -> Duration {
        self.inner.started.elapsed()
    }

    /// Stops the actor and joins its thread.
    ///
    /// Idempotent: a second call after the actor is gone returns `Ok`.
    ///
    /// # Errors
    ///
    /// [`AppError::ActorGone`] when the actor thread panicked.
    #[inline]
    pub async fn shutdown(&self) -> Result<(), AppError> {
        let (reply, response) = oneshot::channel();
        if self
            .inner
            .sender
            .send(Request::Shutdown { reply })
            .await
            .is_ok()
        {
            let _ = response.await;
        }
        let join = {
            self.inner
                .join
                .lock()
                .expect("the actor join handle mutex is not poisoned")
                .take()
        };
        if let Some(join) = join {
            tokio::task::spawn_blocking(move || join.join())
                .await
                .map_err(|_| AppError::ActorGone)?
                .map_err(|_| AppError::ActorGone)?;
        }
        Ok(())
    }

    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, AppError>>) -> Request,
    ) -> Result<T, AppError>
    where
        T: Send + 'static,
    {
        let (reply, response) = oneshot::channel();
        self.inner
            .sender
            .send(build(reply))
            .await
            .map_err(|_| AppError::ActorGone)?;
        response.await.map_err(|_| AppError::ActorGone)?
    }
}

enum Request {
    Execute {
        actor: PlayerId,
        command: Command,
        reply: oneshot::Sender<Result<CommandOutcome, AppError>>,
    },
    ListEntities {
        viewer: PlayerId,
        reply: oneshot::Sender<Result<Vec<EntityView>, AppError>>,
    },
    GetEntity {
        viewer: PlayerId,
        id: EntityId,
        reply: oneshot::Sender<Result<EntityView, AppError>>,
    },
    FeedAfter {
        viewer: PlayerId,
        cursor: Option<FeedEntryId>,
        limit: usize,
        reply: oneshot::Sender<Result<FeedPage, AppError>>,
    },
    Campaign {
        viewer: PlayerId,
        reply: oneshot::Sender<Result<CampaignView, AppError>>,
    },
    Sheet {
        viewer: PlayerId,
        id: EntityId,
        reply: oneshot::Sender<Result<SheetView, AppError>>,
    },
    Subscribe {
        viewer: PlayerId,
        reply: oneshot::Sender<Result<EventStream, AppError>>,
    },
    Save {
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    Stats {
        reply: oneshot::Sender<Result<ActorStatsDto, AppError>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

/// The coarse class of an actor request, used for counters and span names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestKind {
    Execute,
    Read,
    Subscribe,
    Save,
    Stats,
    Shutdown,
}

impl RequestKind {
    fn label(self) -> &'static str {
        match self {
            Self::Execute => "execute",
            Self::Read => "read",
            Self::Subscribe => "subscribe",
            Self::Save => "save",
            Self::Stats => "stats",
            Self::Shutdown => "shutdown",
        }
    }
}

impl Request {
    fn kind(&self) -> RequestKind {
        match self {
            Self::Execute { .. } => RequestKind::Execute,
            Self::ListEntities { .. }
            | Self::GetEntity { .. }
            | Self::FeedAfter { .. }
            | Self::Campaign { .. }
            | Self::Sheet { .. } => RequestKind::Read,
            Self::Subscribe { .. } => RequestKind::Subscribe,
            Self::Save { .. } => RequestKind::Save,
            Self::Stats { .. } => RequestKind::Stats,
            Self::Shutdown { .. } => RequestKind::Shutdown,
        }
    }
}

struct Subscriber {
    viewer: PlayerId,
    sender: broadcast::Sender<StreamEvent>,
    known: BTreeSet<EntityId>,
}

struct Actor<S> {
    state: Option<Box<dyn WorkingSetMut>>,
    env: Option<Box<dyn CommandEnv>>,
    store: S,
    subscribers: Vec<Subscriber>,
    stats: ActorStatsDto,
}

impl<S> Actor<S>
where
    S: AsyncCampaignStore + StoreBackup,
{
    fn new(store: S, snapshot: CampaignSnapshot, env: Box<dyn CommandEnv>) -> Self {
        Self {
            state: Some(Box::new(LoadedCampaign::from_snapshot(snapshot))),
            env: Some(env),
            store,
            subscribers: Vec::new(),
            stats: ActorStatsDto::default(),
        }
    }

    fn run(mut self, mut receiver: mpsc::Receiver<Request>, runtime: tokio::runtime::Runtime) {
        runtime.block_on(async move {
            while let Some(request) = receiver.recv().await {
                if self.handle(request).await {
                    break;
                }
            }
        });
    }

    async fn handle(&mut self, request: Request) -> bool {
        let kind = request.kind();
        self.stats.requests = self.stats.requests.saturating_add(1);
        match kind {
            RequestKind::Execute => self.stats.commands = self.stats.commands.saturating_add(1),
            RequestKind::Read => self.stats.reads = self.stats.reads.saturating_add(1),
            RequestKind::Save => self.stats.saves = self.stats.saves.saturating_add(1),
            RequestKind::Subscribe | RequestKind::Stats | RequestKind::Shutdown => {}
        }
        let (stop, failed) = self
            .handle_request(request)
            .instrument(tracing::debug_span!("actor.request", kind = kind.label()))
            .await;
        if failed {
            self.stats.errors = self.stats.errors.saturating_add(1);
        }
        stop
    }

    async fn handle_request(&mut self, request: Request) -> (bool, bool) {
        match request {
            Request::Execute {
                actor,
                command,
                reply,
            } => {
                let failed = self.execute(actor, command, reply).await;
                (false, failed)
            }
            Request::ListEntities { viewer, reply } => {
                let _ = reply.send(Ok(services::list_entities(self.working_set(), viewer)));
                (false, false)
            }
            Request::GetEntity { viewer, id, reply } => {
                let result =
                    services::get_entity(self.working_set(), id, viewer).map_err(AppError::Engine);
                let failed = result.is_err();
                let _ = reply.send(result);
                (false, failed)
            }
            Request::FeedAfter {
                viewer,
                cursor,
                limit,
                reply,
            } => {
                let _ = reply.send(Ok(self.feed_page(viewer, cursor, limit)));
                (false, false)
            }
            Request::Campaign { viewer, reply } => {
                let result =
                    services::campaign_view(self.working_set(), viewer).map_err(AppError::Engine);
                let failed = result.is_err();
                let _ = reply.send(result);
                (false, failed)
            }
            Request::Sheet { viewer, id, reply } => {
                let reference = self
                    .env
                    .as_ref()
                    .expect("the actor always owns an environment")
                    .reference();
                let result = services::derived_sheet(self.working_set(), reference, id, viewer)
                    .map_err(AppError::Engine);
                let failed = result.is_err();
                let _ = reply.send(result);
                (false, failed)
            }
            Request::Subscribe { viewer, reply } => {
                let result = self.subscribe(viewer);
                let failed = result.is_err();
                let _ = reply.send(result);
                (false, failed)
            }
            Request::Save { reply } => {
                let result = self.store.sync().map_err(AppError::Store);
                let failed = result.is_err();
                let _ = reply.send(result);
                (false, failed)
            }
            Request::Stats { reply } => {
                self.retain_live_subscribers();
                let mut stats = self.stats;
                stats.subscribers = self.subscribers.len();
                let _ = reply.send(Ok(stats));
                (false, false)
            }
            Request::Shutdown { reply } => {
                let _ = reply.send(());
                (true, false)
            }
        }
    }

    async fn execute(
        &mut self,
        actor: PlayerId,
        command: Command,
        reply: oneshot::Sender<Result<CommandOutcome, AppError>>,
    ) -> bool {
        let state = self
            .state
            .take()
            .expect("the actor always owns a working set");
        let env = self
            .env
            .take()
            .expect("the actor always owns an environment");
        let mut executor = AsyncExecutor::new(state, &mut self.store, env);
        let result = executor.execute_as(actor, &command).await;
        let (state, env) = executor.into_parts();
        self.state = Some(state);
        self.env = Some(env);

        let response = match result {
            Ok(executed) => {
                let outcome = self.outcome(actor, executed);
                self.dispatch(&outcome.executed);
                Ok(outcome)
            }
            Err(error) => Err(AppError::Engine(error)),
        };
        let failed = response.is_err();
        let _ = reply.send(response);
        failed
    }

    fn outcome(&self, actor: PlayerId, executed: Executed) -> CommandOutcome {
        let state = self.working_set();
        let mut views = Vec::new();
        let mut campaign = None;
        let mut feed = Vec::new();
        for event in &executed.events {
            match event {
                Event::EntityChanged { id, .. } => {
                    if !views.iter().any(|view: &EntityView| view.id == *id)
                        && let Ok(view) = services::get_entity(state, *id, actor)
                    {
                        views.push(view);
                    }
                }
                Event::EntityRemoved { .. } => {}
                Event::CampaignChanged => {
                    if campaign.is_none() {
                        campaign = services::campaign_view(state, actor).ok();
                    }
                }
                Event::FeedAppended { ids } => {
                    for id in ids {
                        if feed.iter().any(|entry: &FeedEntryView| entry.id == *id) {
                            continue;
                        }
                        if let Some(entry) = feed_view(state, actor, *id) {
                            feed.push(entry);
                        }
                    }
                }
                _ => {}
            }
        }
        CommandOutcome {
            executed,
            views,
            campaign,
            feed,
        }
    }

    fn dispatch(&mut self, executed: &Executed) {
        self.retain_live_subscribers();
        if self.subscribers.is_empty() {
            return;
        }

        let mut changed = BTreeSet::new();
        let mut removed = BTreeSet::new();
        let mut feed = BTreeSet::new();
        let mut campaign_changed = false;
        for event in &executed.events {
            match event {
                Event::EntityChanged { id, .. } => {
                    changed.insert(*id);
                }
                Event::EntityRemoved { id } => {
                    removed.insert(*id);
                }
                Event::CampaignChanged => campaign_changed = true,
                Event::FeedAppended { ids } => feed.extend(ids.iter().copied()),
                _ => {}
            }
        }
        for id in &removed {
            changed.remove(id);
        }

        let Self {
            state, subscribers, ..
        } = self;
        let state = state
            .as_ref()
            .expect("the actor always owns a working set")
            .as_ref();
        for subscriber in subscribers {
            for id in &changed {
                if services::get_entity(state, *id, subscriber.viewer).is_ok() {
                    subscriber.known.insert(*id);
                    let _ = subscriber.sender.send(StreamEvent::EntityChanged(*id));
                } else if subscriber.known.remove(id) {
                    let _ = subscriber.sender.send(StreamEvent::Forgotten(*id));
                }
            }
            for id in &removed {
                if subscriber.known.remove(id) {
                    let _ = subscriber.sender.send(StreamEvent::Forgotten(*id));
                }
            }
            if campaign_changed {
                let _ = subscriber.sender.send(StreamEvent::CampaignChanged);
            }
            for id in &feed {
                if feed_view(state, subscriber.viewer, *id).is_some() {
                    let _ = subscriber.sender.send(StreamEvent::FeedAppended(*id));
                }
            }
        }
    }

    fn retain_live_subscribers(&mut self) {
        self.subscribers
            .retain(|subscriber| subscriber.sender.receiver_count() > 0);
    }

    fn subscribe(&mut self, viewer: PlayerId) -> Result<EventStream, AppError> {
        let state = self.working_set();
        services::campaign_view(state, viewer).map_err(AppError::Engine)?;
        let known = state
            .headers()
            .into_iter()
            .filter(|header| services::get_entity(state, header.id, viewer).is_ok())
            .map(|header| header.id)
            .collect();
        let (sender, receiver) = broadcast::channel(EVENT_BUFFER);
        self.subscribers.push(Subscriber {
            viewer,
            sender,
            known,
        });
        Ok(EventStream { receiver })
    }

    fn feed_page(&self, viewer: PlayerId, cursor: Option<FeedEntryId>, limit: usize) -> FeedPage {
        let state = self.working_set();
        let entries = services::feed_after(state, viewer, cursor, limit);
        let trimmed_before = cursor.and_then(|cursor| {
            state
                .feed()
                .first()
                .map(|entry| entry.id)
                .filter(|oldest| cursor < *oldest)
        });
        FeedPage {
            entries,
            trimmed_before,
        }
    }

    fn working_set(&self) -> &dyn WorkingSet {
        self.state
            .as_ref()
            .expect("the actor always owns a working set")
            .as_ref()
    }
}

/// One visible feed entry by id, or `None` when absent or hidden.
fn feed_view(state: &dyn WorkingSet, viewer: PlayerId, id: FeedEntryId) -> Option<FeedEntryView> {
    state
        .feed()
        .into_iter()
        .find(|entry| entry.id == id)
        .and_then(|entry| {
            authz::entry_visible(state.campaign(), &entry, viewer).then_some(FeedEntryView {
                id: entry.id,
                ts: entry.ts,
                clock_s: entry.clock_s,
                actor: entry.actor,
                entity: entry.entity,
                payload: entry.payload,
            })
        })
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use localtt_adapter::{reference::FixtureSource, rng::ChaChaRng, store::MemoryStore};
    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            AclEntry,
            AnyEntity,
            Audience,
            Campaign,
            CampaignTime,
            EntityId,
            EntityKind,
            FeedEntry,
            FeedEntryId,
            FeedPayload,
            KnownStat,
            PlayerId,
            StatKey,
            UnixTime,
        },
    };
    use localtt_engine::{
        commands::{Command, Event},
        content::ReferenceSource,
        error::EngineError,
        ports::store::{
            AsyncCampaignStore,
            CampaignSnapshot,
            CampaignStore,
            StatePatch,
            StoreFault,
        },
    };
    use tokio::{sync::broadcast, time::timeout};

    use super::*;
    use crate::env::ServerEnv;

    struct BackupSpy<S> {
        inner: S,
        syncs: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl<S: AsyncCampaignStore> AsyncCampaignStore for BackupSpy<S> {
        async fn load(&self) -> Result<CampaignSnapshot, StoreFault> {
            self.inner.load().await
        }

        async fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault> {
            self.inner.apply(patch).await
        }
    }

    impl<S: AsyncCampaignStore> StoreBackup for BackupSpy<S> {
        fn backup(&mut self, _to: &Path) -> Result<(), StoreFault> {
            Err(StoreFault::Unavailable {
                message: "the backup spy has no file".to_owned(),
            })
        }

        fn sync(&mut self) -> Result<(), StoreFault> {
            self.syncs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Fixture {
        campaign: Campaign,
        dm: PlayerId,
        owner: PlayerId,
        other: PlayerId,
        entity: AnyEntity,
        entity_id: EntityId,
    }

    fn fixture(acl: Vec<AclEntry>) -> Fixture {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("Dungeon Master");
        let owner = builder.player("Owner");
        let other = builder.player("Other");
        let campaign = builder.build();
        let mut stats = StatblockBuilder::character();
        stats.abilities(Abilities::all(10)).hp(32, 32);
        let mut entity = stats
            .build_entity(EntityKind::Pc, EntityId::new(), "Aria", owner)
            .expect("abilities and hp are set");
        entity.acl = acl;
        let entity_id = entity.id;
        Fixture {
            campaign,
            dm,
            owner,
            other,
            entity: AnyEntity::from(entity),
            entity_id,
        }
    }

    fn seed(fixture: &Fixture) -> MemoryStore {
        let mut store = MemoryStore::new();
        CampaignStore::apply(
            &mut store,
            &StatePatch {
                campaign: Some(&fixture.campaign),
                upserts: vec![&fixture.entity],
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            },
        )
        .expect("the fixture is a valid patch");
        store
    }

    fn content() -> Arc<dyn ReferenceSource> {
        Arc::new(FixtureSource::new())
    }

    fn env(content: Arc<dyn ReferenceSource>) -> Box<dyn CommandEnv> {
        Box::new(ServerEnv::with_rng(content, ChaChaRng::from_seed([11; 32])))
    }

    fn start_app(fixture: &Fixture) -> (AppHandle, Arc<AtomicUsize>) {
        let store = seed(fixture);
        let snapshot = CampaignStore::load(&store).expect("the fixture loaded");
        let syncs = Arc::new(AtomicUsize::new(0));
        let store = BackupSpy {
            inner: store,
            syncs: Arc::clone(&syncs),
        };
        let content = content();
        let app = AppHandle::start(store, snapshot, env(Arc::clone(&content)), content)
            .expect("the actor starts");
        (app, syncs)
    }

    fn hp(view: &EntityView) -> i32 {
        match view.body.as_ref().expect("a visible body") {
            authz::EntityViewBody::Statblock(body) => {
                body.hp.as_ref().expect("HP is visible").current
            }
            _ => panic!("expected a statblock view"),
        }
    }

    #[tokio::test]
    async fn execute_projects_the_actor_and_dispatches_one_change() {
        let fixture = fixture(Vec::new());
        let (app, syncs) = start_app(&fixture);
        let mut stream = app.subscribe(fixture.dm).await.expect("the DM subscribes");
        let mut other_stream = app
            .subscribe(fixture.other)
            .await
            .expect("another member subscribes");

        let campaign = app
            .campaign(fixture.owner)
            .await
            .expect("a member reads the campaign");
        assert_eq!(campaign.roster.len(), 3);

        let outcome = app
            .execute(
                fixture.dm,
                Command::ApplyDamage {
                    entity: fixture.entity_id,
                    amount: 12,
                    expected_rev: None,
                },
            )
            .await
            .expect("the DM may damage");
        assert_eq!(hp(&outcome.views[0]), 20);
        assert!(outcome.campaign.is_none());
        assert!(outcome.feed.is_empty());
        assert!(outcome.executed.events.iter().any(|event| matches!(
            event,
            Event::EntityChanged { id, .. } if *id == fixture.entity_id
        )));

        let stored = app
            .get_entity(fixture.dm, fixture.entity_id)
            .await
            .expect("the DM reads the updated entity");
        assert_eq!(hp(&stored), 20);
        assert_eq!(
            stream.recv().await,
            Some(StreamEvent::EntityChanged(fixture.entity_id))
        );
        assert!(
            timeout(Duration::from_millis(50), other_stream.recv())
                .await
                .is_err(),
            "an unrelated member must not receive an event for an unreadable entity"
        );
        assert!(
            app.list_entities(fixture.other)
                .await
                .expect("the call runs")
                .is_empty()
        );

        app.save().await.expect("the spy saves");
        assert_eq!(syncs.load(Ordering::SeqCst), 1);

        app.shutdown().await.expect("the actor stops");
        assert!(matches!(
            app.list_entities(fixture.dm).await,
            Err(AppError::ActorGone)
        ));
        app.shutdown().await.expect("shutdown is idempotent");
    }

    #[tokio::test]
    async fn sheet_reads_derive_through_the_actor() {
        let fixture = fixture(Vec::new());
        let (app, _syncs) = start_app(&fixture);

        let sheet = app
            .sheet(fixture.dm, fixture.entity_id)
            .await
            .expect("the DM reads Aria's sheet");
        assert_eq!(sheet.entity, fixture.entity_id);
        assert_eq!(sheet.rev, 0);
        assert!(
            sheet
                .sheet
                .values
                .contains_key(&StatKey::Known(KnownStat::Ac))
        );

        assert!(matches!(
            app.sheet(fixture.other, fixture.entity_id).await,
            Err(AppError::Engine(EngineError::Unauthorized {
                actor,
                entity: Some(id)
            })) if actor == fixture.other && id == fixture.entity_id
        ));
        app.shutdown().await.expect("the actor stops");
    }

    #[tokio::test]
    async fn content_reads_use_the_shared_source_without_the_actor() {
        let fixture = fixture(Vec::new());
        let (app, _syncs) = start_app(&fixture);
        let query = ContentQuery {
            text: "fire".to_owned(),
            kinds: vec![ContentKind::Spell],
            limit: 10,
        };

        let hits = app
            .search_content(query.clone())
            .await
            .expect("the spell search succeeds");
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            vec!["fixture:spell/fire-bolt"]
        );

        let entry = app
            .content_entry(&hits[0].id)
            .await
            .expect("the entry read succeeds")
            .expect("the fixture entry exists");
        assert_eq!(entry.name, "Fire Bolt");
        assert_eq!(
            entry.description,
            "A mote of fire streaks toward a creature in range."
        );
        assert!(
            !app.content_sources()
                .await
                .expect("sources read")
                .is_empty()
        );

        app.shutdown().await.expect("the actor stops");
        assert_eq!(
            app.search_content(query)
                .await
                .expect("content survives the actor"),
            hits
        );
    }

    #[tokio::test]
    async fn giving_a_handout_is_dm_only_and_refuses_a_non_handout() {
        let fixture = fixture(Vec::new());
        let (app, _syncs) = start_app(&fixture);
        let command = Command::GiveHandout {
            handout: fixture.entity_id,
            recipient: fixture.entity_id,
            expected_rev: None,
        };

        assert!(matches!(
            app.execute(fixture.owner, command.clone()).await,
            Err(AppError::Engine(EngineError::Unauthorized {
                actor,
                entity: None
            })) if actor == fixture.owner
        ));
        assert!(matches!(
            app.execute(fixture.dm, command).await,
            Err(AppError::Engine(EngineError::Malformed { .. }))
        ));
        app.shutdown().await.expect("the actor stops");
    }

    #[tokio::test]
    async fn a_non_member_cannot_subscribe() {
        let fixture = fixture(Vec::new());
        let (app, _syncs) = start_app(&fixture);
        let outsider = PlayerId::new();
        assert!(matches!(
            app.subscribe(outsider).await,
            Err(AppError::Engine(EngineError::Unauthorized {
                actor,
                entity: None
            })) if actor == outsider
        ));
        app.shutdown().await.expect("the actor stops");
    }

    #[tokio::test]
    async fn a_hidden_entity_is_forgotten_by_a_known_subscriber() {
        let fixture = fixture(Vec::new());
        let store = seed(&fixture);
        let snapshot = CampaignStore::load(&store).expect("the fixture loaded");
        let mut actor = Actor::new(
            BackupSpy {
                inner: store,
                syncs: Arc::new(AtomicUsize::new(0)),
            },
            snapshot,
            env(content()),
        );
        let (sender, mut receiver) = broadcast::channel(8);
        actor.subscribers.push(Subscriber {
            viewer: fixture.other,
            sender,
            known: BTreeSet::from([fixture.entity_id]),
        });

        actor.dispatch(&Executed {
            warnings: Vec::new(),
            events: vec![Event::EntityChanged {
                id: fixture.entity_id,
                rev: 2,
                kind: EntityKind::Pc,
            }],
        });

        assert_eq!(
            receiver.recv().await.expect("an event"),
            StreamEvent::Forgotten(fixture.entity_id)
        );
    }

    #[tokio::test]
    async fn a_removed_entity_is_forgotten() {
        let fixture = fixture(Vec::new());
        let store = seed(&fixture);
        let snapshot = CampaignStore::load(&store).expect("the fixture loaded");
        let mut actor = Actor::new(
            BackupSpy {
                inner: store,
                syncs: Arc::new(AtomicUsize::new(0)),
            },
            snapshot,
            env(content()),
        );
        let (sender, mut receiver) = broadcast::channel(8);
        actor.subscribers.push(Subscriber {
            viewer: fixture.other,
            sender,
            known: BTreeSet::from([fixture.entity_id]),
        });
        actor
            .state
            .as_mut()
            .expect("the actor owns a working set")
            .apply(&StatePatch {
                campaign: None,
                upserts: Vec::new(),
                removals: vec![fixture.entity_id],
                feed_appends: Vec::new(),
                feed_keep_from: None,
            });

        actor.dispatch(&Executed {
            warnings: Vec::new(),
            events: vec![Event::EntityRemoved {
                id: fixture.entity_id,
            }],
        });

        assert_eq!(
            receiver.recv().await.expect("an event"),
            StreamEvent::Forgotten(fixture.entity_id)
        );
    }

    #[tokio::test]
    async fn feed_pages_report_a_trimmed_cursor() {
        let fixture = fixture(Vec::new());
        let mut store = MemoryStore::new();
        let feed = [
            FeedEntry {
                id: FeedEntryId::new(3),
                ts: UnixTime::from_seconds(0),
                clock_s: CampaignTime::from_seconds(0),
                actor: None,
                entity: None,
                audience: Audience::Public,
                payload: FeedPayload::Hint {
                    text: "three".to_owned(),
                },
            },
            FeedEntry {
                id: FeedEntryId::new(4),
                ts: UnixTime::from_seconds(0),
                clock_s: CampaignTime::from_seconds(0),
                actor: None,
                entity: None,
                audience: Audience::Public,
                payload: FeedPayload::Hint {
                    text: "four".to_owned(),
                },
            },
        ];
        CampaignStore::apply(
            &mut store,
            &StatePatch {
                campaign: Some(&fixture.campaign),
                upserts: vec![&fixture.entity],
                removals: Vec::new(),
                feed_appends: feed.iter().collect(),
                feed_keep_from: None,
            },
        )
        .expect("the feed is a valid patch");
        let snapshot = CampaignStore::load(&store).expect("the fixture loaded");
        let content = content();
        let app = AppHandle::start(
            BackupSpy {
                inner: store,
                syncs: Arc::new(AtomicUsize::new(0)),
            },
            snapshot,
            env(Arc::clone(&content)),
            content,
        )
        .expect("the actor starts");

        let page = app
            .feed_after(fixture.dm, Some(FeedEntryId::new(1)), 10)
            .await
            .expect("the DM reads the feed");
        assert_eq!(page.trimmed_before, Some(FeedEntryId::new(3)));
        assert_eq!(
            page.entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![FeedEntryId::new(3), FeedEntryId::new(4)]
        );

        let page = app
            .feed_after(fixture.dm, Some(FeedEntryId::new(3)), 10)
            .await
            .expect("the DM reads the feed");
        assert_eq!(page.trimmed_before, None);
        assert_eq!(
            page.entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![FeedEntryId::new(4)]
        );
        app.shutdown().await.expect("the actor stops");
    }

    #[tokio::test]
    async fn stats_count_requests_errors_and_live_subscribers() {
        let fixture = fixture(Vec::new());
        let (app, _syncs) = start_app(&fixture);
        let _stream = app.subscribe(fixture.dm).await.expect("the DM subscribes");
        app.list_entities(fixture.dm)
            .await
            .expect("the DM lists entities");
        assert!(
            app.get_entity(fixture.other, fixture.entity_id)
                .await
                .is_err(),
            "a hidden entity is an error"
        );

        let stats = app.stats().await.expect("stats are readable");

        assert!(stats.requests >= 4);
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.reads, 2);
        assert_eq!(stats.commands, 0);
        assert_eq!(stats.saves, 0);
        assert_eq!(stats.subscribers, 1);
        assert_eq!(stats.queue_capacity, 64);
        assert_eq!(stats.queue_depth, 0);
        assert!(app.uptime() > Duration::ZERO);

        app.shutdown().await.expect("the actor stops");
    }
}
