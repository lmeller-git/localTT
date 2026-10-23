# localtt architecture

High-level map of how the project is put together. For product intent and
invariants, `RFC-0001-localtt.md` remains the source of truth; for the data model
and the test suite, see the crate docs linked in section 8. The decision log is
`docs/decisions.md`.

## 1. What this is

A locally hosted web app that digitizes the D&D 5e character sheet and adds light
bookkeeping automation for an in-person or voice-call party. The DM runs a server
on their own machine; players join from a browser on the same LAN. It is a
note-taking and bookkeeping aid, not a virtual tabletop and not a rules referee:
computed values are suggestions, and nothing blocks on rules (warnings, not
errors).

## 2. Crate map

```
                 +-------------------------------+
  browser -----> | localtt-web (driving, WASM)   |
  (static)       | Leptos reads + event stream   |
                 +---------------+---------------+
                                 | REST / WS
                 +---------------v---------------+
                 | localtt-server  (driving)     |
  TUI / HTTP --->| routing, join flow, static,   |
  WS clients     | event fan-out, composition    |
                 +---------------+---------------+
                                 | protocol DTOs
                 +---------------v---------------+
                 | localtt-protocol (contract)   |
                 +---------------+---------------+
                                 | depends on
                 +---------------v---------------+
                 | localtt-engine (application)  |
                 | rules, commands, dice, clock, |
                 | authz/view_of, services,      |
                 | ports (traits)                |
                 +---------------+---------------+
                                 | depends on
                 +---------------v---------------+
                 | localtt-core (domain)         |
                 | types (data, serde), builders;|
                 | ids, errors                   |
                 +---------------^---------------+
                                 | implements engine ports
                 +---------------+---------------+
                 | localtt-adapter (driven)      |
                 | sqlite store, reference data, |
                 | content packs, later: pdf     |
                 +-------------------------------+
```

Dependency direction, enforced by convention and reviewed:

- `core` has **no I/O** (only `serde`, `serde_json`, `uuid`, `thiserror`). It can
  later be extracted as a plugin-facing crate.
- `engine` depends on `core` only; it is pure logic plus orchestration generic
  over its own port traits (`engine::ports`, `engine::content`, `engine::export`).
- `adapter` depends on `engine` and `core`, implementing the engine's ports; the
  engine never depends on it.
- `protocol` depends on `engine` and `core`; it owns the transport DTOs and the
  projected values shared by the server and future clients. It has no HTTP,
  runtime or store dependency.
- `server` depends on `protocol`, `engine`, `adapter` and `core`; composition
  (wiring adapters into the engine) lives there or in a thin binary crate, still
  an open question (RFC-0001 9.3).
- `web` is a nested Cargo workspace that depends on `protocol` and `core` only;
  it never links the server or its runtime. The browser reaches the actor through
  REST and the WebSocket.

`localtt-integration` is a leaf consumer: dev-only, depended on by nobody in
production, and the home of the end-to-end scenario suite.

## 3. Responsibilities per crate

- **core** (`crates/localtt-core`) -- the serializable domain data model and its
  validation: campaigns, players, ACLs, entities and bodies, statblocks, items,
  resources, statuses, spellcasting, dice AST, rolls, feed, clock, schema
  versions. Plus ergonomic builders (`src/builders.rs`). See
  `crates/localtt-core/DESIGN.md`.
- **engine** (`crates/localtt-engine`) -- rules (modifier pipeline, formulas,
  derived stats, warnings), commands, dice, clock arithmetic, `authz`/`view_of`,
  application services, and the ports the adapter implements (`ports/`, `content`,
  `export`). U3 lands the command executor (`error`, `state`, `executor`, `clock`,
  `commands/`; implemented for the U3 slice, later units extend the catalogue,
  `docs/rfcs/engine-executor.md`).
- **adapter** (`crates/localtt-adapter`) -- driven adapters implementing the
  engine's ports: the SQLite store, reference data (SRD snapshot, packs), and later
  PDF import/export. Currently the campaign store (`store`: memory + SQLite,
  async SQLite wrapper, export/import, backup), the schema-gated loader `load`,
  and the content reference (`reference/`: snapshot format, index, fixture, Open5e
  converter; built in U2, `docs/rfcs/content.md`).
- **protocol** (`crates/localtt-protocol`) -- transport contract shared by the
  server and clients: command, event, warning, content, sheet, health, response
  and safe error DTOs, plus the actor's projected values (`CommandOutcome`,
  `FeedPage`, `StreamEvent`). It depends only on `core` and `engine`, so a WASM
  client can consume it without pulling in axum or tokio.
- **web** (`web/`) -- the Leptos 0.8 CSR browser client: token entry, REST reads
  (health, campaign, entity/sheet, feed, reference) and `/ws` resync. Pure
  formatting and URL builders compile natively for tests; the UI is gated to
  `wasm32` and Trunk builds `web/dist`. This is the M4c read-only shell; mutation
  forms follow. It depends on `protocol` and `core`, not `server`.
- **server** (`crates/localtt-server`) -- driving adapter: async `AppHandle`,
  the single-writer actor (`app`), campaign startup/bootstrap (`session`), the
  process command environment (`env`), the server failure type (`error`), the
  transport contract re-exported from `protocol`, the HTTP/WS adapter (`http`:
  axum router, token, origin/CORS, body cap, auth rate limit, REST handlers,
  WebSocket stream), data root/lock policy (`config`), runtime content loading
  (`content`) and the TUI (`tui`: pure reducer, pure renderer, terminal task,
  new-character form). The
  composition root is `src/bin/localtt.rs`; it can run the TUI, `tui --serve`, or
  headless `serve`. Optional `--web-dir` serves built browser assets through
  `ServeDir` with an SPA fallback and security headers, while `/api` and `/ws`
  stay guarded. Join/identity remains. The architecture, TUI-first milestone
  order and async-surface contract are in `docs/rfcs/server.md`; the gaps and
  friction recorded by the full-stack e2e slice are in
  `docs/rfcs/server-findings.md`.
- **integration** (`crates/localtt-integration`) -- the end-to-end scenario suite
  and API playground. See `crates/localtt-integration/DESIGN.md`.

## 4. The domain model at a glance

- **One entity model, many kinds.** PCs, companions, summons, familiars, NPCs,
  monsters, notes and handouts are all `Entity<B>` with an owner and an ACL. Six
  kinds share one statblock body; the body family is checked at build and parse
  time.
- **Store base data, derive the rest.** Derived values are never persisted; a
  displayed value is `override ?? computed`, and the engine always returns both
  the value and a breakdown with sources.
- **Content is data.** Items and features can be pure text or carry mechanics;
  official and homebrew content share one shape. References are namespaced
  (`<source>:<kind>/<slug>`) and always stored with a name snapshot so a missing
  pack degrades to text.
- **One clock.** Campaign time is a DM-controlled integer (seconds). Status
  durations are absolute expiry times on it; rounds and the in-game calendar
  (day, ten-day week, time of day) are derived, never stored. There is no turn
  engine. An absolute in-world clock and history is a planned additive layer.
- **One egress point.** `view_of(entity, viewer)` in the engine is the only
  function that produces viewer-facing representations; REST and WebSocket output
  both pass through it. Core provides only the `AclEntry`/`FieldMask`/`ViewField`
  data.
- **Audience, not secrecy by cryptography.** Rolls and feed entries carry an
  audience (`Public`, `RollerAndDm`, `RollerOnly`); "secret" means hidden from
  other players, and the DM can read the file.

## 5. Command and runtime model

- Writes are **intents** (`spend_resource`, `apply_damage`, ...) or path-level
  field edits, not whole-document replacement, so simultaneous edits do not
  clobber each other. Lists are addressed by stable element id, never by index.
- Commands are serialized through a single writer; each is one atomic transaction
  returning the new `rev`, any warnings, and the events it produced. At party
  scale this removes whole classes of races.
- Ports (engine-owned) the engine needs from outside: `CampaignStore` and its
  additive async twin `AsyncCampaignStore` (implemented by the adapter;
  `StoreBackup` is the separate file-capability trait the composition root uses),
  `ReferenceSource`, `Rng`, `EventSink`, later
  `SheetExporter`/`SheetImporter`. The command and dice layers sit behind `Rng`
  for determinism.
- The engine's command logic, content reads and pure rules stay synchronous. The
  server (`localtt-server`) drives them through `AsyncExecutor` and
  `AsyncCampaignStore`: a dedicated actor thread owns `Box<dyn WorkingSetMut>`,
  the store and `Box<dyn CommandEnv>`, runs a current-thread Tokio runtime, and
  receives `AppHandle` requests over a Tokio channel with `oneshot` replies. Per
  command the actor builds an `AsyncExecutor`, calls `into_parts` to recover the
  owned state/environment, then builds the acting client's view. `AsyncSqliteStore`
  wraps the synchronous SQLite store and moves each operation to the blocking
  pool with an owned patch mirror. Subscribers receive id-only `StreamEvent`s
  from a per-viewer broadcast and refetch through their own authorization. This
  is ARCH-8/ARCH-9/ARCH-12.
  `AppHandle` also holds an `Arc<dyn ReferenceSource>`; content search and entry
  reads bypass the actor on the blocking pool, so reference reads never queue
  behind SQLite writes (ARCH-11).
- The TUI is split into a pure reducer (`TuiState::update -> Vec<Effect>`), a
  pure renderer over `&TuiState`, and a terminal task. The task owns the input
  thread and a `JoinSet` of effect futures; effect results come back as `TuiMsg`,
  so a slow save never blocks rendering or input. The `/` browser searches and
  reads reference entries through the handle. The composition root resolves the
  data root, takes the campaign lock, opens the SQLite store (with the backup
  policy), bootstraps the actor and mounts the TUI (ARCH-10).
- The REST adapter (`http`) is a driving adapter over the same `AppHandle`: it
  binds loopback by default, requires a per-run bearer token on every route except
  an origin-checked CORS preflight, checks `Origin`, reflects allowed CORS origins,
  caps bodies at 1 MiB and rate-limits failed auth. The token is printed once and
  written owner-only by default (`0600` on Unix). The TUI and the API share the
  actor; `localtt serve` runs headless and `localtt tui --serve` runs both. Sheet
  and health DTOs live in `protocol`, so the engine's `DerivedSheet` stays
  serde-free (ARCH-14). `/ws` uses the same router and middleware, subscribes the
  viewer before the upgrade, and serializes the projected `StreamEvent`s as
  `StreamEventDto` text frames; a lagged client resyncs over REST (ARCH-15).
  `--web-dir` adds the public SPA/assets with `ServeDir`; API catch-alls remain
  authenticated JSON so unknown API paths never return HTML. Static responses
  carry no-store, `nosniff`, `DENY`, `no-referrer` and a CSP that permits the
  Trunk loader and WebAssembly (ARCH-18).

## 6. Persistence and versioning

- **One SQLite file per campaign**; entity documents are JSON in `doc` columns,
  with only the queried columns (`id`, `kind`, `body_kind`, `owner`, `parent`,
  `name`, `rev`, `schema_version_major`, `schema_version_minor`) extracted. ACL
  entries are not projected in v1 (no query needs them).
- **Schema versioning from day one**: every entity and the campaign carry a
  `SchemaVersion`. A loader refuses a newer major rather than partially loading
  it; a newer minor is accepted and its unknown fields dropped; a loaded document
  is re-stamped at the build's version. Migrations are pure functions over the
  JSON.
- Reference data (SRD and packs) lives outside the campaign file. Export/import to
  plain JSON is the escape hatch.

## 7. Testing

- `just test` is the gate (normal, `--no-default-features`, miri over
  `localtt-core`/`localtt-engine`, loom, shuttle, echeneis, plus the nested web
  workspace). `just lint` and `just check` cover format/lints and docs plus a
  feature-powerset check. Web tests are two layers: native tests for pure
  formatting/URL logic, and one headless WASM smoke test that mounts the login
  shell; `just web-build` runs Trunk.
- Core's contract is pinned by `crates/localtt-core/tests/types.rs` (shape and
  serde) and the integration suite
  (`crates/localtt-integration/tests/scenarios.rs`) by whole-campaign round trips
  and the golden-character catalog.
- Concurrency is a static `Send + Sync` contract for the data types; there is no
  contention behaviour in core to test. Engine-level concurrency will be tested
  where the command executor lands.
- Deliberately absent: mutation testing and snapshot (`insta`) tests. The repo has
  no recipe for the former; the latter is deferred until the engine makes
  snapshots worth diffing.

## 8. Documentation map

- `RFC-0001-localtt.md` -- product intent, invariants, deferred/open product
  questions (still current).
- `crates/localtt-core/DESIGN.md` -- the domain data model, serde contract,
  version policy, builders.
- `crates/localtt-integration/DESIGN.md` -- the scenario suite, golden-character
  catalog, and the review ledger.
- `docs/decisions.md` -- the decision log.
- `AGENTS.md` -- workflow, style, and the commands to run.
