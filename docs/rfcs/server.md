# RFC: U7 -- the server, TUI-first

| | |
|---|---|
| **Status** | Spec, review fold (2026-10-20): folds the adversarial review of 2026-10-19/20. Pins the base-first order: B1 (`Executor::into_parts`, additive) before the TUI; B2 (async store and async executor, additive) before HTTP. The TUI is the first frontend; HTTP/WebSocket follows later. |
| **Scope** | `crates/localtt-server` (new modules `app`, `tui`, `protocol`, `error`, `src/bin/localtt.rs`); tests in `localtt-server` and `localtt-integration`. The base slices touch `localtt-engine` (`executor`, `state`, planned async ports) and `localtt-adapter` (planned async SQLite wrapper); no frozen target test is edited. |
| **Read with** | RFC-0001 3.1, 3.4, 5.2, 5.7, 5.8, 5.9, 5.11, 9, 10, 11; `docs/architecture.md` sections 3-5; `docs/decisions.md` ARCH-1/3/4, ENG-2/3/4/5/7/11/14/16/17/23/25/26, STORE-3/5/7/11/12, CONTENT-9/12/13; `docs/rfcs/server-findings.md` (F1-F7); `docs/rfcs/engine-executor.md` 4.4, 5.7, 10; `docs/rfcs/{authz,dice,store,content,rules}.md`; `crates/localtt-integration/DESIGN.md`. |
| **Pins** | Server module layout; `AppHandle`; actor ownership and the additive `Executor::into_parts`; per-viewer event dispatch; the async contract; data-root/backup/lock policy; M4's mutation token; the TUI-first milestone order. |

**As built (2026-10-20).** F1 (`CampaignView`), F4 (`session::start`), F5
(`env::ServerEnv`), the actor skeleton (`app::AppHandle`) and the M1 TUI
(`tui`, plus the `localtt` composition root) are implemented and covered by
engine/server unit tests. M2 creation (`CreateEntity` plus the TUI form) is
implemented; M3 sheet depth, B2, HTTP/WS and identity slices remain.

## 1. Goals

1. Pin a small server: one transport-agnostic core plus driving adapters, with the
   terminal UI as the first adapter and HTTP/WebSocket as a later one over the same
   core.
2. Pin the async surface: `AppHandle` (execute, query, subscribe, save, shutdown)
   is `async` from day one. The synchronous engine sits behind an actor thread
   until B2 lands.
3. **Base before features.** B1 is additive: `Executor::into_parts` lets the actor
   own the working set and environment between requests, so `save`/`sync` can run
   without a second store borrow and without editing the frozen target tests. B2
   adds an async store/executor path; the synchronous path stays for the frozen
   suite. B2 is required before HTTP.
4. Pin the v0 feature set and the engine prerequisite slices (section 8).
5. Pin projected state flow: `view_of` and the feed are the view model, the
   frontends render it, and the core never renders (section 4.4).
6. Pin operational safety: one campaign per process, a single-instance lock, a
   startup backup policy, a data root, and a per-run bearer token for M4 mutations.

## 2. Non-goals

- **No identity/auth in v0.** No join tokens, DM key or LAN binding; the local
  process acts as the campaign DM. M4 adds a per-run bearer token for mutations;
  M5 adds join/tokens (F2).
- **No WebSocket or network multi-client fan-out in M1.** The HTTP/WS adapter is
  M4; the TUI is the only network consumer in M1. The core per-viewer dispatcher
  is implemented and tested with two viewers in M1; the network fan-out is M4.
- **No remote TUI.** The TUI runs in the server process. A TUI over HTTP is
  possible later because the handle is transport-agnostic, not a v0 goal.
- **No multi-campaign.** One campaign per process (STORE-3, RFC-0001 5.9); no
  runtime campaign switching.
- **No frozen target test edits for the server.** B1/B2 are additive; a breaking
  engine migration needs its own approved unit and commit.
- **No remote content in the command path.** `ReferenceSource` stays local and
  blocking per CONTENT-12; a remote async dispatcher is a later unit.
- **No REST resource-per-field API.** One command endpoint (section 7).
- **No content packs, PDF, maps, chat, initiative.**

## 3. Client usage: the TUI flow (first client)

```
localtt tui [--campaign PATH] [--create --name NAME --dm NAME] [--actor ID]
            [--data DIR] [--no-backup]
```

With no `--campaign`, the path is `<data>/campaigns/default.sqlite`. The process
opens (or creates) the campaign, owns the store, starts the actor, and runs the
TUI task.

**M1 screen (explicit).** Header: campaign name, clock, acting player. Left: entity
list (name, kind, rev). Right: selected entity view. Bottom: feed, a status/error
line, key hints. The campaign roster/settings pane and the derived sheet pane are
M2 and M3; M1 does not need them. F1 (`CampaignView`) is an M1 prerequisite so the
header and roster read are safe from the start.

Actions in M1: list/select entities; apply damage, heal, set temp HP, death saves;
spend/restore a resource; add/remove/edit a status; short/long rest; cast a spell;
begin/end concentration; spend a hit die; roll an expression or a check; advance
or set the clock; save (WAL checkpoint); quit. Creation is M2. Rolls are entered
through a one-line prompt with a small parser; an invalid expression is a status
line error, not a crash.

The TUI is split so it is testable without a terminal:

- `tui::state::TuiState` is a pure reducer: `update(&mut self, TuiMsg) -> Vec<Effect>`;
- `tui::render` is a pure function over `&TuiState` and a `Frame`;
- the task owns the terminal, the input reader and the effect `JoinSet`; it is the
  only part not unit-tested.

## 4. Architecture

### 4.1 One core, many edges

```
        TUI adapter (M1, in-process)      HTTP/WS adapter (M4, remote)
                    \                          /
                     \                        /
                AppHandle (async API, Clone)
         execute / query / subscribe / save / shutdown
                            |
                 request channel + oneshot replies
                            v
   actor thread: state + store + env (owns all three)
                            |
              engine (sync today) -> adapter (sync today)
```

Frontends are **driving adapters** (ARCH-1): they call the core and consume its
event stream. The core does not know a frontend exists. The composition root
(`src/bin/localtt.rs`, ENG-5) chooses which adapters run:

| Mode | Core | TUI | HTTP/WS |
|---|---|---|---|
| `localtt tui` | yes | yes | no |
| `localtt tui --serve` | yes | yes | yes (M4) |
| `localtt serve` | yes | no | yes (M4) |

The rejected alternative is a renderer/backend trait called by the core: it
inverts the dependency direction, couples the domain to UI cadence, and cannot
express N remote clients with per-viewer projections. The shared abstraction is
the service API plus the projected event stream, not rendering.

### 4.2 The core loop: an actor owning the sync engine

`Executor` owns `Box<dyn WorkingSetMut>` and `Box<dyn CommandEnv>` and borrows
`&mut dyn CampaignStore` (`executor.rs:14-18`). It is `Send`, deliberately not
`Sync`; `rusqlite` is synchronous and may block (RFC-0001 5.9). The actor runs on a
dedicated OS thread and drains a `tokio::sync::mpsc` with `blocking_recv`, so a
SQLite write never blocks a runtime worker.

**Ownership (B1).** The actor owns `state: Box<dyn WorkingSetMut>`,
`store: S` (`S: CampaignStore + StoreBackup`) and `env: Box<dyn CommandEnv>`.
Between requests the executor does not exist. Per request:

```rust
let state = self.state.take().expect("state is present between requests");
let env = self.env.take().expect("environment is present between requests");
let mut executor = Executor::new(state, &mut self.store, env);
let result = executor.execute_as(actor, &command);
let (state, env) = executor.into_parts();     // additive engine API
self.state = Some(state);
self.env = Some(env);
// On success: dispatch events and build the command outcome.
// On a save request, with no executor alive:
self.store.sync()?;
```

`Executor::into_parts(self) -> (Box<dyn WorkingSetMut>, Box<dyn CommandEnv>)` drops
the store borrow and returns the owned state and environment. It is additive: the
existing `Executor::new`, `execute`, `execute_as`, `state` and every frozen target
test stay unchanged. The rejected alternative, making `Executor` borrow the state,
would change `Executor::new` at every call site, including the frozen target suite;
the additive recovery is smaller and equally sound.

Reads (`list_entities`, `get_entity`, `feed_after`, `campaign`, `sheet`) are actor
messages that borrow `&state` and call the sync services; they see a consistent
snapshot and share the single writer. This serializes reads behind writes and
behind each other. At party scale (under ~10 clients) it is irrelevant; a
read-only snapshot path is the follow-up if measurements say otherwise (section
6.6). Content reads are the exception: the handle holds
`Arc<dyn ReferenceSource>` and serves search/get/sources without the actor, so
content is never on the single-writer critical path. If the source does file IO,
the handle wraps the call in `spawn_blocking` (the source is `Send + Sync`); the
in-memory snapshot is the v0 fast path.

**Failure and shutdown.** The actor thread is the session. A panic (engine
assertion, store fault that aborts) closes the request channel; the handle maps
the closed channel to `AppError::ActorGone`. `shutdown` sends a `Shutdown` request,
waits for the actor to drain prior requests, and joins the thread; it is
idempotent and returns `Ok` if the actor is already gone. A failed command leaves
the actor alive; a panicked actor is fatal for the process (the TUI exits; the
HTTP adapter returns 503). The actor does not `catch_unwind` a half-mutated state.

### 4.3 The service API

`AppHandle` is a cheap `Clone` over the actor's channel; content reads use the
shared source directly.

```rust
pub struct CommandOutcome {
    pub executed: Executed,
    pub views: Vec<EntityView>,          // post-commit, acting viewer
    pub campaign: Option<CampaignView>,  // when CampaignChanged
    pub feed: Vec<FeedEntryView>,        // when FeedAppended, acting viewer
}

impl AppHandle {
    pub async fn execute(&self, actor: PlayerId, command: Command)
        -> Result<CommandOutcome, AppError>;
    pub async fn list_entities(&self, viewer: PlayerId)
        -> Result<Vec<EntityView>, AppError>;
    pub async fn get_entity(&self, viewer: PlayerId, id: EntityId)
        -> Result<EntityView, AppError>;
    pub async fn feed_after(&self, viewer: PlayerId, cursor: Option<FeedEntryId>, limit: usize)
        -> Result<FeedPage, AppError>;
    pub async fn campaign(&self, viewer: PlayerId)
        -> Result<CampaignView, AppError>;                                  // F1
    pub async fn sheet(&self, viewer: PlayerId, id: EntityId)
        -> Result<SheetView, AppError>;                                    // M3
    pub async fn search_content(&self, query: ContentQuery)
        -> Result<Vec<ContentSummaryDto>, AppError>;
    pub async fn content_entry(&self, id: &RefId)
        -> Result<Option<ContentEntryDto>, AppError>;
    pub async fn content_sources(&self) -> Vec<SourceInfoDto>;
    pub async fn subscribe(&self, viewer: PlayerId)
        -> Result<EventStream, AppError>;
    pub async fn save(&self) -> Result<(), AppError>;
    pub async fn shutdown(&self) -> Result<(), AppError>;
}
```

- `AppError::{Engine, Store, Content, ActorGone}` implements `Display` and
  `std::error::Error`; the HTTP adapter maps it to a stable code and status, never
  forwarding diagnostic messages (F6, 4.7).
- `FeedPage { entries: Vec<FeedEntryView>, trimmed_before: Option<FeedEntryId> }`.
  `trimmed_before` is the oldest retained feed id when the cursor is below the
  ring; it distinguishes "no new entries" from "you missed entries".
- `ContentQuery { text: String, kinds: Vec<ContentKind>, limit: usize }` is a
  server-owned, owned query; the actor converts it to the borrowed
  `SearchQuery<'_>` for the sync source.
- `save` means `StoreBackup::sync` (a WAL checkpoint). It is not a pending-state
  flush; commands are committed to SQLite before they are visible. Startup backup
  is a separate policy (section 4.5).
- `CommandOutcome.views`/`feed` remove the TUI's refetch after its own command;
  other clients refetch on `StreamEvent`s.

### 4.4 Projected state and the event stream

`view_of` is the one egress point (ARCH-4, ENG-8): `EntityView` and
`FeedEntryView` are `Serialize` and per-viewer. They are the shared view model;
the TUI draws them as text and the browser will draw them as HTML.

`StreamEvent` is server-level and deliberately payload-free:

```rust
pub enum StreamEvent {
    EntityChanged(EntityId),     // refetch through get_entity
    Forgotten(EntityId),         // delete or ACL narrowing: drop the cached entity
    CampaignChanged,             // refetch the campaign view
    FeedAppended(FeedEntryId),   // refetch through feed_after
    Lagged,                      // subscriber fell behind: full resync
}
```

`subscribe(viewer)` is async. The actor computes the viewer's initial **known set**
(every `EntityId` for which `services::get_entity(&state, id, viewer)` succeeds),
registers a per-subscriber `tokio::sync::broadcast::Sender<StreamEvent>`, and
returns the receiver wrapper. Per-subscriber broadcast gives non-blocking sends
and a natural `Lagged` signal; capacity 64.

**Dispatch after a commit.** The actor deduplicates ids from `Executed.events` and,
for each subscriber, compares the known set with post-commit readability:

1. `EntityChanged(id)`: if the entity is readable now, send `EntityChanged` and
   insert the id into the known set; if it is not readable but was known, send
   `Forgotten` and remove it; if it is neither readable nor known, send nothing.
   ACL widening is the insert case; ACL narrowing is the `Forgotten` case.
2. `EntityRemoved(id)`: if the id was known, send `Forgotten` and remove it. The
   known set is the pre-commit visibility, so the actor does not need the deleted
   document.
3. `CampaignChanged`: send to every subscriber; `campaign(viewer)` applies the
   per-viewer projection.
4. `FeedAppended(id)`: look up the entry in `state.feed()` and send only if
   `authz::entry_visible` admits the viewer. A trimmed entry is skipped.

Coalesce at most one `EntityChanged`/`Forgotten` per id, one `CampaignChanged` and
one feed event per id per commit. A lagged receiver gets `Lagged` and performs a
full read (`list_entities` + `feed_after` + `campaign`). Clients subscribe before
their initial reads; events that arrive during the read are applied idempotently
(refetch/drop by id).

RFC-0001 5.11 describes pushing the full filtered entity on change. That is a
later optimization for the WS adapter; the v0 core contract is ids plus refetch.
`CommandOutcome` covers the acting client without a round trip.

### 4.5 Campaign lifecycle, identity and persistence

- **Data root.** `--data <DIR>` overrides. The default is the platform local data
  directory via a maintained `directories`-style crate (Linux
  `$XDG_DATA_HOME/localtt` or `~/.local/share/localtt`; macOS
  `~/Library/Application Support/localtt`; Windows `%LOCALAPPDATA%\localtt`).
  Layout: `campaigns/`, `reference/`, `backups/`.
- **Open/create at startup.** `--campaign` defaults to
  `<data>/campaigns/default.sqlite`. `--create` bootstraps through the F4 helper
  (`CampaignBuilder` plus a creation patch) when the store is empty;
  `--create --name NAME --dm NAME` (defaults: "Untitled Campaign", the OS user).
  An empty store without `--create` is a startup error. A stored campaign with one
  DM uses that seat; with several, `--actor <player-id>` is required.
- **Single instance.** The composition root takes an exclusive advisory lock on
  `<campaign>.lock` before opening the store and holds it for the process
  lifetime; a second process fails fast with a clear message (STORE-3 is a rule,
  not just documentation).
- **Startup backup.** On by default for SQLite, before DDL migrations (STORE-3/12);
  `--no-backup` disables it. Backups go to `<campaign dir>/backups/` with the
  store's `.tmp` + rename protocol. A failed startup backup aborts the open; a
  failed rotation is best-effort (STORE-14).
- **Durability wording.** A committed command is in SQLite before it is visible;
  power-loss durability follows STORE-3 (`synchronous=NORMAL`), which may lose the
  last committed command on power loss. `save` checkpoints the WAL; it does not
  create a backup.
- **Identity v0.** The local process is the DM. M4 uses a per-run bearer token
  that grants the DM actor; M5 adds player join/tokens (F2). `Player.token_hash` is
  never returned by any view.
- **Content data.** `<data>/reference/` holds the runtime SRD snapshot; missing
  content is a warning at startup (the campaign still opens) and content-dependent
  commands fail closed through the empty/default source.

### 4.6 Frontends

- **TUI (M1).** An async task over `ratatui` + `crossterm` (candidates, added via
  `cargo add`). Input runs on a dedicated thread using `crossterm::event::poll`
  with a timeout and an atomic stop flag, feeding a channel; the runtime task
  never blocks on the terminal. Effects are spawned in a `JoinSet` with a
  monotonic `RequestId`; results arrive as `TuiMsg`, so a slow `save` (SQLite's
  busy timeout is 5s) cannot freeze rendering. The task selects over input,
  `EventStream`, effect results and a tick. A `TerminalGuard` restores raw
  mode/alternate screen on `Drop`; a panic hook restores it and prints the panic.
  Resize is a reducer message; Ctrl-C is a quit message; `AppError`/`EngineError`
  are shown on the status line. Rendering is independent of in-flight effects.
  The module is behind a `tui` feature (default on for the binary); the headless
  build uses `--no-default-features` once `serve` exists (M4).
- **HTTP/WS (M4).** `axum` over the same handle. **Mutations require a per-run
  256-bit bearer token**, generated at startup, printed once to stderr and
  optionally written to `<data>/server.token` with `0600`; `Origin` is checked
  against a loopback/config allowlist; the bind is loopback by default; request
  bodies are capped (1 MiB) and auth failures are rate-limited. `--lan` remains
  M5 and is explicit opt-in. The WS adapter subscribes per connection and forwards
  `StreamEvent`s (or projected payloads after a refetch); lagged clients resync.
- **CLI (later).** Thin HTTP-client subcommands once HTTP exists; the composition
  root stays one binary.

### 4.7 Protocol, content and error mapping (M4)

Transport-only DTOs in `protocol.rs`:

- command DTOs (tagged enum with an unknown fallback for the engine's
  `#[non_exhaustive]` variants);
- DTOs for `Event`/`Warning`/`WarningCode`/`ElementRef`/`EngineError`;
- **content DTOs**: `ContentEntryDto`, `ContentSummaryDto`, `SourceInfoDto`.
  `ContentEntry` is deliberately serde-free (`content.rs:91-93`) and
  `ContentSummary` is not `Serialize`; the server maps them. F6's scope includes
  these; the handle exposes `content_entry` and `content_sources` so the API table
  is implementable.

`EntityView`/`FeedEntryView` are already `Serialize` and are returned directly.
Golden-JSON tests pin them in M1 so the web wire shape does not drift silently
while the TUI bypasses HTTP. Diagnostic messages (`StoreFault`, `ContentFault`,
`EngineError::Malformed`) are never forwarded; they map to a stable code plus an
HTTP status (findings RFC F6, section 4.1).

## 5. The app we are building (fitness check)

| Product flow (RFC-0001) | Seam |
|---|---|
| DM opens a campaign on their machine and uses a local UI | in-process TUI + actor; one process owns the file and the lock |
| DM acts on entities: HP, resources, statuses, rests, concentration | existing `Command` variants; `execute` |
| DM rolls and the table sees a feed with audiences | `roll` + `feed_after`; per-viewer dispatch |
| Characters/items/handouts are created and edited | character creation is built (M2); item/handout and edit commands remain M3 |
| Players join later and see only what they may read | `view_of` per actor; HTTP/WS in M4; join in M5 |
| Reconnect and catch up | feed cursor + `trimmed_before` + entity `rev` refetch |
| SRD reading, later from a user endpoint | sync `ReferenceSource` now; async dispatcher later (CONTENT-12) |

This is not a drift from RFC-0001: the invariable parts (intent commands, one
egress point, one writer, one campaign per file) are in the core. Only transport
and identity strength are staged. The review-fold corrections are contract-level,
not product-level.

## 6. The async surface (B2; implementation deferred)

The runtime must never block on IO. Today the actor thread guarantees that. B2
adds an async store/executor path for the server; it is additive so the frozen
target suite keeps compiling.

### 6.1 Principles

1. `AppHandle` and every frontend boundary are async already; only the core
   internals are sync. B2 moves the store call, not the command logic.
2. `AsyncCampaignStore` is used behind `dyn`, so it uses `async_trait` (boxed
   futures); native `async fn` in traits is not dyn-compatible on the pinned MSRV.
   One allocation per command is acceptable at party scale.
3. No blocking call on a runtime worker: `rusqlite`, file snapshots, exports and
   backups run on the actor thread or through `spawn_blocking`.
4. No lock held across `.await` unless it is a `tokio::sync` lock; the SQLite
   mutex is locked inside the blocking closure.

### 6.2 Engine changes

B2 is additive: `AsyncCampaignStore` plus an async executor path. The synchronous
`CampaignStore`/`Executor` remain for the frozen target tests and in-memory test
stores; the server uses the async path. If the maintainer later approves a
breaking migration, the two paths can merge.

| Item | B2 change | Notes |
|---|---|---|
| `ports::store::AsyncCampaignStore::{load, apply}` | new async trait | `apply(&mut self, patch: &StatePatch<'_>)`; `Send`; `dyn` via `async_trait` |
| `executor::AsyncExecutor::{execute, execute_as}` | new async path | borrows `&mut dyn WorkingSetMut`, `&mut dyn AsyncCampaignStore`, `&mut dyn CommandEnv` |
| `commands::{apply, apply_as}` | stays sync | content stays local; only the store call is async |
| `content::ReferenceSource` | stays sync | CONTENT-12: the unit port is local/blocking; the async dispatcher is later |
| `state::WorkingSet` | stays `Send` | `&mut dyn WorkingSetMut` is `Send`; no `Sync` is needed because `apply` is sync. Add `Sync` only when a shared async read path lands |
| `StoreBackup` | stays sync | checkpoint/backup run on the actor thread; not on the async command hot path |
| `Executor::into_parts` | B1 | additive; recovered state/env work for both paths |

`CommandEnv::{now, roll}` and `Rng::roll_die` stay synchronous. The environment's
`reference()` accessor keeps returning `&dyn ReferenceSource`.

**Why no `WorkingSet: Sync`.** The review's failing probe assumed
`commands::apply` becomes async. It does not: content is local and `apply` is
pure. The async executor holds `&mut dyn WorkingSetMut` across the store await,
and `&mut T` is `Send` when `T: Send`. If a future read path holds
`&dyn WorkingSet` across an await (shared snapshots), add `Sync` then and record
the ENG-16 supersede.

### 6.3 Adapter async story

- **SQLite.** Keep `rusqlite` synchronous. Wrap the connection as
  `Arc<std::sync::Mutex<rusqlite::Connection>>` (`Connection: Send`, not `Sync`)
  and run each operation inside `spawn_blocking`, locking inside the closure so no
  guard crosses `.await`. `apply` stays one transaction. A poisoned mutex maps to
  `StoreFault::Backend`; the actor treats it as fatal (the connection may be
  half-updated).
- **Borrowed patch, owned crossing.** `StatePatch<'_>` borrows the owned
  `Transition`. The async `apply` may hold that borrow in its future, but
  `spawn_blocking` requires `'static`, so the adapter builds an owned mirror
  (`OwnedStatePatch`: cloned campaign/entities/feed, owned removal ids) before the
  blocking call. This is the one extra allocation per command; pin it here so the
  borrowed trait signature is not mistaken for a `'static` one.
- **Backup/paths.** `StoreBackup::backup(&mut self, to: &Path)` stays sync; if it
  moves off the actor thread, clone to `PathBuf` before the blocking call.
- **Startup.** Open, DDL verification and migrations run before the tokio runtime
  starts (or inside `spawn_blocking`), not on a runtime worker.
- **MemoryStore.** Gets a trivial async impl for server tests.
- **Remote reference (later).** CONTENT-12's async backend dispatcher, not an
  async `ReferenceSource`; cache/backoff policy is its own slice.

### 6.4 Send/lifetime requirements to pin now

- `Box<dyn CampaignStore + Send>`; `Box<dyn ReferenceSource>` (already
  `Send + Sync`); `Box<dyn CommandEnv + Send>`.
- Async store futures are `Send`; the actor task can move across executor threads.
- `StatePatch<'_>` may borrow across the async `apply`; the adapter owns the
  mirror before `spawn_blocking`. `Transition` is owned, so no state borrow
  crosses the await.
- `SqliteStore`/`Executor` remain `Send`, not `Sync`.

### 6.5 Migration order

1. **B1:** `Executor::into_parts`; actor ownership tests (section 8).
2. **B2:** `AsyncCampaignStore` + `AsyncExecutor` + async SQLite wrapper + owned
   patch mirror; server switches to the async executor. The sync path stays.
3. Optionally replace the actor thread with a tokio task once the async path is
   proven; `AppHandle` does not change.
4. Remote content dispatcher; WS fan-out over the async store.

### 6.6 Performance notes and triggers

- The actor serializes reads and writes. Trigger to revisit: measured read latency
  under concurrent remote clients (M4+), or a large `load`. The fix is a
  snapshot/`RwLock` read path, not a redesign.
- Content reads bypass the actor via `Arc<dyn ReferenceSource>`.
- Per-subscriber broadcast is non-blocking; lagged clients resync. Capacity 64.
- Coalesce events per commit; `CommandOutcome` carries the acting client's views.
- `list_entities` and `feed_after` clone; optimize only when the M4 trigger fires.

## 7. API v0 (HTTP contract, M4; the TUI uses the handle)

All mutating requests and WS upgrades require
`Authorization: Bearer <per-run token>`; loopback bind and `Origin` allowlist by
default; 1 MiB body cap; auth-failure rate limit.

| Method and path | Purpose |
|---|---|
| `GET /api/v1/health` | version, campaign id/name, clock summary |
| `GET /api/v1/campaign` | `CampaignView`; roster without `token_hash` (F1) |
| `POST /api/v1/campaign/save` | `StoreBackup::sync` (checkpoint) |
| `GET /api/v1/entities?kind=` | `list_entities` for the actor |
| `GET /api/v1/entities/{id}` | `get_entity` view |
| `GET /api/v1/entities/{id}/sheet` | derived sheet (`rules::derive`, M3) |
| `POST /api/v1/commands` | tagged command DTO; returns `CommandOutcome` |
| `GET /api/v1/feed?after=&limit=` | `FeedPage` (entries + `trimmed_before`) |
| `GET /api/v1/reference/search?q=&kind=` | `ContentSummaryDto` list |
| `GET /api/v1/reference/{id}` | `ContentEntryDto` |
| `GET /api/v1/reference/sources` | `SourceInfoDto` list |
| `GET /ws` | per-viewer `StreamEvent` stream |

`CampaignView` and `SheetView` shapes are provisional until F1/M3 land; the
endpoint names are pinned so the web client does not drift.

## 8. Base slices, prerequisites and milestones

### 8.1 Base slices

| Slice | Contents | Size |
|---|---|---|
| **B1** (first) | `Executor::into_parts` (additive); model of actor state/env recovery; engine tests for state/RNG continuity and save between requests | S |
| **B2** (before M4) | `AsyncCampaignStore`, `AsyncExecutor`, async SQLite wrapper, `OwnedStatePatch`, Send/lifetime tests; sync path retained | M |
| **B3** (M4) | F6 protocol DTOs plus content DTOs; golden JSON; error mapping | M |

### 8.2 Feature prerequisites

| Slice | Enables | Size |
|---|---|---|
| F4 bootstrap helper | `--create`, one campaign-start path | S (server-side) |
| F5 `ServerEnv` | executing anything | S (server-side) |
| F1 campaign view | M1 header/roster, `GET /campaign` | S-M (engine) |
| F3 create/delete entity | creation is built; deletion remains | M (engine) |
| Inventory/handout commands | item handling, handout give | M (engine) |
| Sheet service | `GET /sheet`, derived values | S (engine) |
| Feed gap (`FeedPage`) | reconnect honesty | S (server) |
| F2 roster/invite | multiple players, join | M (engine + server, M5) |

### 8.3 Milestones

- **B1 -- robust base.** `Executor::into_parts` plus tests; no frozen test edits,
  no call-site churn. Land before any server code.
- **F1 -- campaign view.** `authz::view_of_campaign` plus
  `services::campaign_view`; member-only, with id, name, default ruleset, clock,
  public settings and a roster of id/display name/role. No `token_hash` or
  `last_seen`.
- **M1 -- TUI walking skeleton.** Actor (`into_parts` recovery), `ServerEnv`,
  bootstrap helper, `AppHandle`, per-viewer stream registry, TUI reducer/render/task,
  `localtt tui` over the existing engine surface; single-instance lock, data root,
  startup backup. Tests: actor over `BackupSpy<MemoryStore>`, two-viewer dispatch,
  reducer, `TestBackend` smoke, golden `EntityView`/`FeedEntryView` JSON, and one
  SQLite integration scenario (create, execute, save, restart).
- **M2 -- creation (built).** `CreateEntity` plus the TUI new-character form;
  deletion remains for a later lifecycle slice.
- **M3 -- items and sheet.** Inventory/handout commands; sheet service; item and
  sheet panes.
- **B2 -- async store.** Async store/executor and adapter wrapper; may run in
  parallel with M1-M3. Required before M4.
- **M4 -- HTTP/WS + web.** B3 DTOs, `axum`, per-run token, `--serve`, WS
  projection/fan-out, headless mode; then the browser client.
- **M5 -- identity.** F2 roster/invite, join tokens, DM key, `--lan`.

## 9. Decisions recorded

- **D1: one core, many edges.** Frontends are driving adapters; the core never
  renders; the composition root selects adapters.
- **D2: actor over the sync core; async handle.** Content is outside the actor.
- **D3: TUI first, HTTP later, one core.**
- **D4: one campaign per process, startup-scoped; autosave per command;**
  single-instance lock.
- **D5: explicit actor parameter; local DM in v0; M4 per-run token; M5 join.**
- **D6: projected views are the view model; DTOs are transport-only.**
- **D7: async contract pinned; B2 additive; sync path retained.**
- **D8: B1 uses additive `into_parts`, not a breaking `Executor::new` change;**
  frozen target tests stay untouched.
- **D9: per-viewer dispatch uses a known-visibility set; `Forgotten` covers delete
  and ACL narrowing; ids plus refetch in v0.**
- **D10: `ReferenceSource` stays sync (CONTENT-12); remote content is an async
  dispatcher later.**
- **D11: M4 mutations use a per-run bearer token; loopback plus `Origin`
  allowlist; 1 MiB cap; auth rate limit.**
- **D12: `CommandOutcome` carries post-commit views/feed for the acting client;
  other clients refetch on events.**

## 10. Review fold dispositions

| Review finding | Disposition |
|---|---|
| 1.1 actor/save ownership | Fixed by B1 `into_parts`; additive, no frozen test changes |
| 1.2 removal/ACL dispatch | Fixed by the known set + `Forgotten` (4.4) |
| 1.3 sync `subscribe`/broadcast | Fixed: async `subscribe`, per-subscriber broadcast, `Lagged` |
| 1.4 spoofable M4 header | Fixed: per-run bearer token, `Origin`, caps, rate limit |
| 1.5 single-instance not enforced | Fixed: `<campaign>.lock` |
| 1.6 campaign view infallible/scope | Fixed: `Result<CampaignView, AppError>`; F1 in M1 |
| 1.7 content not wire-ready | Fixed: content DTOs plus handle methods |
| 1.8 CONTENT-12 contradiction | Fixed: `ReferenceSource` stays sync; dispatcher later |
| 1.9 data root/backup | Fixed: `--data`, layout, backup policy, `--no-backup` |
| 1.10 TUI bypasses wire | Fixed: golden JSON tests in M1 |
| 1.11 feed trim gap | Fixed: `FeedPage.trimmed_before` |
| 1.12 M1 panes | Fixed: explicit M1 screen; roster/sheet in M2/M3 |
| 2.1 `WorkingSet: Sync` | Corrected: not needed after content stays sync; add with a shared read path |
| 2.2 `spawn_blocking` borrow | Fixed: borrowed trait, owned `OwnedStatePatch`/`PathBuf` |
| 2.3 `AppCore` alternative | Removed; B1 ownership is the single model |
| 2.4 content serialized behind writer | Fixed: content outside the actor |
| 2.5 view shapes | Marked provisional until F1/M3 |
| 2.6 deps/features | Pinned: tokio features, `tui` feature, B3 deps |
| 2.7 multi-campaign | Recorded as a one-way door (D4) |
| 3.1 ids-only refetch cost | Fixed: `CommandOutcome` + coalescing; full push later |
| 3.2 actor failure | Fixed: `AppError::ActorGone`, shutdown ack/join |
| 3.3 broadcast/backpressure | Fixed: per-subscriber broadcast with `Lagged` |
| 3.4 read cloning | Deferred with the M4 trigger |
| 3.5 TUI cancellation | Fixed: `JoinSet` + `RequestId` |
| 3.6 TUI stalls on save | Fixed: effects off the render path |
| 3.7 mutex poisoning | Pinned: `StoreFault::Backend`, actor fatal |
| 3.8 startup blocking | Pinned: open/migrations before the runtime |
| 4.1 filter tests | Added to M1 (two viewers) |
| 4.2 MemoryStore has no backup | Fixed: `BackupSpy<MemoryStore>` test double |
| 4.3 TUI failure modes | Fixed: guard, panic hook, resize, error line |
| 4.4 durable wording | Fixed: committed-before-visible; STORE-3 power-loss semantics |
| 4.5 `save` semantics | Fixed: WAL checkpoint, not a flush |
| 4.6 citations | Fixed: `rules.md` added; service names; RFC-0001 5.9 |
| 4.7 filter before two viewers | Not deferred: the dispatcher is security-critical and tested with two viewers in M1 |

## 11. Open questions

1. **TUI stack:** `ratatui` + `crossterm` (recommended); default-on `tui` feature.
2. **Data-root crate:** `directories` vs `etcetera` vs a small local resolver.
3. **F1 roster visibility:** what players see about each other (F1 decides; the
   DM sees the roster, no `token_hash` anywhere).
4. **B2 shape:** additive `AsyncCampaignStore`/`AsyncExecutor` (recommended) vs an
   approved breaking migration that updates the frozen target tests.
5. **M4 token storage:** stderr only, or a `0600` file under the data root.
6. **Backup retention:** count- or age-based; STORE-12 rotation is best-effort.
