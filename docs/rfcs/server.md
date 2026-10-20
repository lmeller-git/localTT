# RFC: U7 -- the server, TUI-first

| | |
|---|---|
| **Status** | Spec, draft for review (2026-10-19): the architecture for the server unit. The TUI is the first frontend; HTTP/WebSocket follows in a later milestone. Pins the transport-agnostic core, the async `AppHandle`, the engine/adapter async-surface requirements (specified here, implemented later), and the milestone order. Implementation lands in slices on their own branches. |
| **Scope** | `crates/localtt-server` (new modules `app`, `tui`, `protocol`, `error`, `src/bin/localtt.rs`); tests in `localtt-server` and one server scenario in `localtt-integration`. The async-surface requirements constrain `localtt-engine` (`ports`, `content`, `commands`, `executor`) and `localtt-adapter` (`store`, `reference`), but no engine/adapter code lands with this RFC. |
| **Read with** | RFC-0001 3.1, 3.4, 5.2, 5.7, 5.8, 5.9, 5.11, 9, 10, 11; `docs/architecture.md` sections 3-5; `docs/decisions.md` ARCH-3, ENG-2/4/5/7/11/14/17, STORE-3/7/11, CONTENT-9/12; `docs/rfcs/server-findings.md` (F1-F7); `docs/rfcs/engine-executor.md` 4.4, 5.7, 10; `docs/rfcs/{authz,dice,store,content}.md`; `crates/localtt-integration/DESIGN.md`. |
| **Pins** | Server module layout; the `AppHandle` service API; the actor execution model; the TUI-first milestone order; the async-surface contract for later engine/adapter work; the decision that frontends are driving adapters and the core never renders. |

## 1. Goals

1. Pin a small server: one transport-agnostic core plus driving adapters, with the
   terminal UI as the first adapter and HTTP/WebSocket as a later one over the same
   core.
2. Pin the async surface: `AppHandle` (execute, query, subscribe, save, shutdown)
   is `async` from day one. The synchronous engine sits behind an actor until the
   engine and adapter grow an async surface.
3. Pin the engine/adapter async requirements (section 6): async store, async
   content, async command/executor paths, a dyn-compatible shape, and a
   `spawn_blocking` story for `rusqlite` and file IO. Implementation is a later
   unit; this RFC is its contract.
4. Pin the TUI as the first client and the composition root's usage modes:
   `localtt tui`, `localtt tui --serve`, and later `localtt serve`.
5. Pin the v0 feature set and the engine prerequisite slices it needs (section 8).
6. Pin how projected state flows: `view_of` and the feed are the view model, the
   frontends render it, and the core never renders (section 4.4).

## 2. Non-goals

- **No identity/auth in v0.** No join tokens, DM key, sessions or LAN binding; the
  local process acts as the campaign DM (section 4.5). F2/join is milestone M5.
- **No WebSocket or multi-client fan-out in M1.** The HTTP/WS adapter is M4; the
  event stream exists from M1 but the TUI is its only consumer.
- **No remote TUI.** The TUI runs in the server process. A TUI over HTTP is
  possible later because the handle abstraction is transport-agnostic, not a v0
  goal.
- **No multi-campaign.** One campaign per process (STORE-3, RFC-0001 5.9); no
  runtime campaign switching.
- **No engine/adapter code here.** The async surface is specified, not built.
- **No REST resource-per-field API.** One command endpoint (section 7).
- **No content packs, remote reference, PDF, maps, chat, initiative.**

## 3. Client usage: the TUI flow (first client)

The DM runs one process:

```
localtt tui --campaign ~/campaigns/vault.sqlite [--create --name Vault --dm Louis]
```

The process opens (or creates) the campaign, starts the core actor, and runs the
TUI task. A session looks like: the campaign pane shows clock, roster and
settings; the entity pane lists what the DM may read and shows one entity's sheet;
the feed pane shows rolls and expiry entries; keystrokes map to `AppHandle` calls
(apply damage, heal, spend/restore a resource, add/remove a status, short/long
rest, roll an expression or a check, advance/set the clock, create/delete an
entity from M2); replies update the TUI state, and the event stream triggers a
refresh. `s` forces durability (`StoreBackup::sync`).

The TUI is split so it is testable without a terminal:

- `tui::state::TuiState` is a pure reducer: `update(&mut self, TuiMsg) -> Vec<Effect>`;
- `tui::render` is a pure function over `&TuiState` and a `Frame`;
- the task owns the terminal, the input reader and the effect execution, and is
  the only part that is not unit-tested.

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
      actor (thread or task): Executor + SqliteStore + ServerEnv
                            |
              engine (sync today) -> adapter (sync today)
```

Frontends are **driving adapters** (ARCH-1): they call the core and consume its
event stream. The core does not know a frontend exists. The composition root
(`src/bin/localtt.rs`, ENG-5) decides which adapters run:

| Mode | Core | TUI | HTTP/WS |
|---|---|---|---|
| `localtt tui` | yes | yes | no |
| `localtt tui --serve` | yes | yes | yes (M4) |
| `localtt serve` | yes | no | yes (M4) |

The rejected alternative is a renderer/backend trait called by the core in a
usage-dependent branch: it inverts the dependency direction, couples the domain to
UI cadence, and cannot express N remote clients with per-viewer projections. The
shared abstraction is the service API plus the projected event stream, not
rendering.

### 4.2 The core loop: an actor owning the sync engine

The engine is synchronous: `Executor` owns the working set and the environment,
borrows `&mut dyn CampaignStore`, and is `Send` but not `Sync`
(`engine::executor`; `engine-executor.md` 4.4). `rusqlite` is synchronous and
blocks (RFC-0001 10). The server therefore runs one actor that owns the store, the
executor and the environment, and serves requests from a channel. Reasons:

1. **Self-reference.** `Executor<'a>` borrows the store it is constructed with;
   storing both in one struct needs self-reference. Locals inside an actor loop
   (thread or async task) hold both without any engine change.
2. **Single writer.** The engine serializes writers by design (ENG-4); a mailbox
   is the direct expression of that.
3. **Blocking isolation.** The actor runs on a dedicated OS thread for v0 and
   drains a `tokio::sync::mpsc` with `blocking_recv`, so a SQLite write never
   blocks a runtime worker. A tokio task with `block_in_place` is the equivalent
   alternative (open question 10.2).
4. **Future async.** When the engine/adapter ports become async (section 6), the
   actor becomes an ordinary async task (or disappears behind the handle); the
   `AppHandle` API and every frontend stay unchanged.

Reads (`list_entities`, `get_entity`, `feed_after`, campaign view, derived sheet)
are also actor messages for v0, so they see a consistent snapshot and share the
single writer. This is a known scaling limit: it serializes reads behind writes
and behind each other. At party scale (under ~10 clients) it is irrelevant; a
read-only snapshot path is the documented follow-up if measurements say otherwise
(section 6.6). This is deliberately F7 option 2 rather than its option-1
recommendation: the server is async from the start and the blocking store must not
sit behind a lock on a runtime worker.

### 4.3 The service API

`AppHandle` is a cheap `Clone` handle over the actor's channel:

```rust
impl AppHandle {
    pub async fn execute(&self, actor: PlayerId, command: Command)
        -> Result<Executed, EngineError>;
    pub async fn list_entities(&self, viewer: PlayerId) -> Vec<EntityView>;
    pub async fn get_entity(&self, viewer: PlayerId, id: EntityId)
        -> Result<EntityView, EngineError>;
    pub async fn feed_after(&self, viewer: PlayerId, cursor: Option<FeedEntryId>, limit: usize)
        -> Vec<FeedEntryView>;
    pub async fn campaign(&self, viewer: PlayerId) -> CampaignView;      // F1
    pub async fn sheet(&self, viewer: PlayerId, id: EntityId)
        -> Result<SheetView, EngineError>;                                // M3
    pub async fn search_content(&self, query: ContentQuery)
        -> Result<Vec<ContentSummary>, ContentFault>;
    pub fn subscribe(&self, viewer: PlayerId) -> EventStream;
    pub async fn save(&self) -> Result<(), StoreFault>;                   // sync/backup
    pub async fn shutdown(&self);
}
```

Rust frontends build engine `Command` values directly; the HTTP adapter maps DTOs
to the same values (section 7). `execute` returns the engine `Executed` (warnings
and events); the event stream carries change notifications (section 4.4).
`EventStream` is a broadcast-backed receiver wrapper; a lagged subscriber resyncs
through the feed cursor or entity `rev` (section 6.6). `CampaignView` and
`SheetView` are planned engine-owned views (F1, M3).

### 4.4 Projected state and the event stream

`view_of` is already the one egress point (ARCH-4, ENG-8): `EntityView` and
`FeedEntryView` are `Serialize` and per-viewer. They are the shared view model;
the TUI draws them as text and the browser will draw them as HTML.

`EventStream` yields the engine event vocabulary (`EntityChanged`, `EntityRemoved`,
`CampaignChanged`, `FeedAppended`) with ids only, no projected payloads. The actor
filters the stream per viewer: an entity or feed id the viewer may not read is not
delivered. Subscribers then re-query with their own viewer to get projected data.
RFC-0001 5.11 describes pushing the full filtered entity on change; that is an
optimization for the WS adapter, not the core contract. For the single-user TUI,
the command reply is usually enough, and the stream mainly serves `--serve` mode.

### 4.5 Campaign lifecycle, identity and persistence

- **Open/create at startup.** `--campaign <path>` opens a `SqliteStore`;
  `--create` bootstraps a campaign through the F4 helper (`CampaignBuilder` plus
  a creation patch) when the store is empty; otherwise an empty store is a startup
  error, not a request error.
- **Autosave.** Every committed command is durable (ENG-2). `save` is an explicit
  `StoreBackup::sync`/backup, not a pending-state flush.
- **Identity v0.** The local process acts as the campaign DM: `--create` seeds the
  DM seat; a loaded campaign uses its DM seat, or `--actor <player-id>` when there
  is more than one. When HTTP lands, the remote actor arrives as a dev header
  (loopback only) until join/tokens (M5, F2).
- **One campaign per process.** No runtime switch; a second process must not open
  the same file (STORE-3).

### 4.6 Frontends

- **TUI (M1).** An async task over `ratatui` + `crossterm` (candidates, added via
  `cargo add` and license-checked). Input is read on a dedicated thread and
  forwarded over a channel so the runtime task never blocks; rendering is a pure
  function of `TuiState`. The TUI calls `AppHandle` directly, in-process.
- **HTTP/WS (M4).** `axum` over the same handle: REST queries, one command
  endpoint, and a WebSocket that fans out changes per viewer: its subscription
  filters ids, then it refetches through `view_of`/`feed_after` before pushing.
  Loopback by default; `--lan` is M5.
- **CLI (later).** Thin HTTP-client subcommands once HTTP exists; the composition
  root stays one binary.

### 4.7 Protocol and error mapping (M4)

Transport-only DTOs in `protocol.rs`: command DTOs (tagged enum with a fallback
for the engine's `#[non_exhaustive]` variants), and DTOs for
`Event`/`Warning`/`WarningCode`/`ElementRef`/`EngineError`, which have no serde
today. `EntityView`/`FeedEntryView`/content types are already `Serialize` and are
returned directly. Agent rule: diagnostic messages (`StoreFault`, `ContentFault`,
`EngineError::Malformed`) are never forwarded; they map to a stable code plus an
HTTP status (findings RFC F6, section 4.1).

## 5. The app we are building (fitness check)

The architecture must express these flows; each maps to a concrete seam:

| Product flow (RFC-0001) | Seam |
|---|---|
| DM opens a campaign on their machine and uses a local UI | in-process TUI + actor; one process owns the file |
| DM acts on entities: HP, resources, statuses, rests, concentration | `Command` variants already implemented; `execute` |
| DM rolls and the table sees a feed with audiences | `roll` command + `feed_after`; per-viewer filtering |
| Characters/items/handouts are created and edited | M2/M3 engine slices (F3, inventory); creation is the known gap |
| Players join later and see only what they may read | `view_of` per actor over the same handle; HTTP/WS in M4 |
| Reconnect and catch up | feed cursor + entity `rev` refetch (RFC-0001 5.11) |
| SRD reading, later from a user endpoint | `ReferenceSource` behind the handle; async when remote (M-later) |

This is not a drift from RFC-0001: the invariable parts (intent commands, one
egress point, one writer, one campaign per file) are all in the core; only the
transport and the identity strength are staged.

## 6. The async surface (requirements; implementation deferred)

The runtime must never block on IO. Today that is guaranteed by the actor thread.
When the engine/adapter grow an async surface, the actor becomes an async task and
the following traits change. This section is the contract so the change is a
compile-driven refactor rather than a redesign.

### 6.1 Principles

1. `AppHandle` and every frontend boundary are async already; only the core
   internals are sync. The migration moves the sync boundary inward.
2. Ports are used behind `dyn` (`Box<dyn CampaignStore>`, `&mut dyn ...`), and the
   engine public API keeps that shape. Native `async fn` in traits is stable but
   **not dyn-compatible** on Rust 1.89, so the async ports use `async_trait`
   (boxed futures) or a hand-rolled `Pin<Box<dyn Future + Send + '_>>` alias.
   One heap allocation per call is acceptable at party scale.
3. No blocking call on a runtime worker: `rusqlite`, file snapshots, exports and
   backups go through `spawn_blocking` or the actor.
4. No lock held across `.await` unless it is a `tokio::sync` lock; shared reader
   state prefers message passing or a snapshot.

### 6.2 Engine traits to make async

| Trait / function | Change | Notes |
|---|---|---|
| `ports::store::CampaignStore::{load, apply}` | `async fn` | `apply(&mut self)`; stays `Send`; `dyn` via `async_trait` |
| `ports::store::StoreBackup::{backup, sync}` | `async fn` | file IO; used by the composition root, not the engine |
| `content::ReferenceSource::{get, search, autocomplete, sources}` | `async fn` | already `Send + Sync`; required by the remote REST source (CONTENT-12) |
| `commands::{apply, apply_as}` | `async fn` | awaits content reads; `CommandEnv` unchanged in shape |
| `executor::Executor::{execute, execute_as}` | `async fn` | awaits `apply` and `store.apply` |
| `state::WorkingSet` reads | stay sync | load-all is in memory; a lazy/LRU set would go async (ENG-3 door) |

`CommandEnv::{now, roll}` and `Rng::roll_die` stay synchronous: `now` is trivial
and `roll_die` is pure CPU. The environment's `reference()` accessor keeps
returning `&dyn ReferenceSource`; the async move is in the content methods it
exposes.

### 6.3 Adapter async story

- **SQLite.** Keep `rusqlite` synchronous, wrap the connection as
  `Arc<std::sync::Mutex<rusqlite::Connection>>` (`Connection: Send`, not `Sync`)
  and run each operation inside `spawn_blocking`, locking inside the closure so no
  guard crosses `.await`. `apply` stays one transaction. This is a wrapper type,
  the existing schema/DDL code is unchanged.
- **Files (snapshots, export, backup).** `spawn_blocking`.
- **Remote reference (later).** An async HTTP client (e.g. `reqwest` with rustls)
  implementing `ReferenceSource`, `Send + Sync`; cache/backoff policy is its own
  slice (CONTENT-12).

### 6.4 Send/lifetime requirements to pin now

- `Box<dyn CampaignStore + Send>`, `Box<dyn ReferenceSource>` (`Send + Sync`
  already), `Box<dyn CommandEnv + Send>`.
- Futures returned by port methods are `Send` so the actor task can move across
  executor threads.
- `StatePatch<'_>` borrows live across the `apply().await`; the async signature
  must keep the lifetime, e.g. `async fn apply(&mut self, patch: &StatePatch<'_>)`.

### 6.5 Migration order (the later unit)

1. Engine traits and `apply`/`execute` async behind `async_trait`; keep sync
   in-memory test envs working (small `block_on` shims or async test runtimes).
2. Adapter: async SQLite wrapper, async backup, async file sources.
3. Replace the actor thread with a tokio task, or drop the actor for an async
   `AppCore` behind a `tokio::sync::Mutex`.
4. Async remote reference source.
5. WS fan-out over the async store.

Each step leaves `AppHandle` unchanged.

### 6.6 Performance notes and triggers

- The actor serializes reads and writes. Trigger to revisit: measured read
  latency under concurrent remote clients (M4+), or a store whose `load` is large.
  The fix is a snapshot/`RwLock` read path, not a redesign.
- Bounded channels give per-client backpressure; unbounded is v0 simplicity for a
  local process.
- WS fan-out: filter per subscriber, cap queues, drop-and-resync on overflow
  (feed cursor or `rev` refetch).

## 7. API v0 (HTTP contract, M4; the TUI uses the handle)

| Method and path | Purpose |
|---|---|
| `GET /api/v1/health` | version, campaign id/name, clock summary |
| `GET /api/v1/campaign` | campaign view: name, clock, settings, roster without `token_hash` (F1) |
| `POST /api/v1/campaign/save` | `StoreBackup::sync`/backup |
| `GET /api/v1/entities?kind=` | `list_entities` for the actor |
| `GET /api/v1/entities/{id}` | `get_entity` view |
| `GET /api/v1/entities/{id}/sheet` | derived sheet (`rules::derive`, M3) |
| `POST /api/v1/commands` | one tagged command DTO; returns `{ warnings, events }` |
| `GET /api/v1/feed?after=&limit=` | projected feed page |
| `GET /api/v1/reference/search?q=&kind=` | content search |
| `GET /api/v1/reference/{id}` | content entry |
| `GET /api/v1/reference/sources` | provenance/attribution |
| `GET /ws` (M4) | projected change stream |

## 8. Prerequisite slices and milestones

### 8.1 Engine/adapter prerequisite slices

| Slice | Enables | Size |
|---|---|---|
| F4 bootstrap helper | `--create`, one campaign-start path | S (server-side, no engine change) |
| F5 `ServerEnv` | executing anything | S (server-side) |
| F1 campaign view | `GET /campaign`, TUI campaign pane | S-M (engine `authz`/`services`) |
| F3 create/delete entity | character/note/handout creation | M (engine unit) |
| Inventory/handout commands | item handling, handout give | M (engine unit) |
| Sheet service | `GET /sheet`, derived values | S (engine `services` over `rules::derive`) |
| F6 protocol/DTOs | HTTP contract | M (server) |
| F2 roster/invite | multiple players, join | M (engine + server, M5) |

### 8.2 Milestones

- **M1 -- TUI walking skeleton.** `AppCore` actor, `ServerEnv`, bootstrap helper,
  `AppHandle`, TUI reducer/render/task, `localtt tui --campaign [--create]` over
  the existing engine surface (reads, commands, rolls, feed, clock, save).
  Tests: actor over `MemoryStore`, reducer, `TestBackend` render smoke.
- **M2 -- creation.** F1 + F3 engine slices land (parallel branches); campaign
  pane and create/delete-character flows in the TUI.
- **M3 -- items and sheet.** Inventory/handout commands; sheet service; item and
  sheet panes.
- **M4 -- HTTP/WS + web.** Protocol DTOs, `axum` adapter, `--serve`, WS
  projection/fan-out, headless mode; then the browser client.
- **M5 -- identity.** F2 roster/invite, join tokens, DM key, `--lan`.

## 9. Decisions recorded

- **D1: one core, many edges.** Frontends are driving adapters; the core never
  renders; usage mode selects adapters in the composition root. The renderer
  trait called from the core is rejected.
- **D2: actor over the sync core; async handle.** Rejected: `Mutex<AppCore>`
  (self-reference and blocking on runtime workers).
- **D3: TUI first, HTTP later, one core.** The TUI is an in-process adapter; the
  web app later uses the same core over HTTP/WS.
- **D4: one campaign per process, startup-scoped; autosave per command.**
- **D5: explicit actor parameter; DM-only local identity in v0.**
- **D6: projected views are the view model; DTOs are transport-only.**
- **D7: async surface pinned as a contract (section 6), implemented later.**

## 10. Open questions

1. **TUI stack:** `ratatui` + `crossterm` (recommended) and whether the TUI is
   feature-gated so a headless server build skips those dependencies.
2. **Actor placement:** dedicated OS thread (recommended for v0) vs tokio task
   with `block_in_place`.
3. **TUI scope in M1:** read-only plus the existing act commands, with creation
   in M2 -- or a minimal entity-creation form in M1 once F3 lands?
4. **Campaign creation parameters:** `--name`/`--dm` plus optional seats, or
   import a fixture bundle as the only bootstrap in v0?
5. **`--serve` in M1 or M4:** the TUI could expose HTTP before M4 to test the
   protocol early; it is not required for the TUI-first goal.
6. **Campaign view shape (F1):** roster visibility rules (does a player see the
   roster? what does the DM see?) belong to that slice.
