# localtt architecture

High-level map of how the project is put together. For product intent and
invariants, [`RFC-0001-localtt.md`](../RFC-0001-localtt.md) remains the source of
truth. Engine behaviour is in [`engine.md`](engine.md), the data model and test
suite in the crate docs linked in section 10, and the decision rationale in
[`decisions.md`](decisions.md).

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
  (static)       | Leptos forms + event stream   |
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
- `engine` depends on `core` only; it is pure logic plus orchestration through
  its own port traits (`engine::ports`, `engine::content`, `engine::export`).
- `adapter` depends on `engine` and `core`, implementing the engine's ports; the
  engine never depends on it.
- `protocol` depends on `engine` and `core`; it owns the transport DTOs and the
  projected values shared by the server and future clients. It has no HTTP,
  runtime or store dependency.
- `server` depends on `protocol`, `engine`, `adapter` and `core`; composition
  (wiring adapters into the engine) lives in `server`'s `localtt` binary
  (ENG-5).
- `web` is a nested Cargo workspace that depends on `protocol` and `core` only;
  it never links the server or its runtime. The browser reaches the actor through
  REST and the WebSocket.

`localtt-integration` is a leaf consumer: dev-only, depended on by nobody in
production, and the home of the end-to-end scenario suite.

### 2.1 Module map

- **engine**: `executor` (sync and async), `commands/`, `state`, `clock`,
  `rules/`, `dice/`, `authz/`, `services`, `content`, `export`, `error`,
  `ports/`. The behaviour contract is in [`engine.md`](engine.md).
- **adapter**: `store/` (memory, SQLite, backup, export), `load` (schema gate),
  `reference/` (snapshot reader, index, aliases, fixture source, Open5e
  converter), `rng` (`ChaChaRng`).
- **protocol**: `command`, `event`, `outcome`, `content`, `sheet`, `health`,
  `diagnostics`, `roster`, `error`; DTOs and projected values only.
- **server**: `app` (actor and `AppHandle`), `session`, `env`, `error`,
  `identity` (auth index), `http/` (router, routes, WebSocket, auth, request
  ids), `config`, `content`, `tui/`, and `src/bin/localtt.rs`.
- **web**: `app` (session and UI state), `http`, `commands`, `format`, `url`;
  pure command/format/URL builders compile natively.

## 3. Responsibilities per crate

- **core** (`crates/localtt-core`) -- the serializable domain data model and its
  validation: campaigns, players, ACLs, entities and bodies, statblocks, items,
  resources, statuses, spellcasting, dice AST, rolls, feed, clock, schema
  versions. Plus ergonomic builders (`src/builders.rs`). See
  `crates/localtt-core/DESIGN.md`.
- **engine** (`crates/localtt-engine`) -- rules (modifier pipeline, formulas,
  derived stats, warnings), commands, dice, clock arithmetic, `authz`/`view_of`,
  application services, and the ports the adapter implements (`ports/`, `content`,
  `export`). The command executor (`error`, `state`, `executor`, `clock`,
  `commands/`) and the rules, dice and authorization modules are built; their
  contract is in [`engine.md`](engine.md).
- **adapter** (`crates/localtt-adapter`) -- driven adapters implementing the
  engine's ports: the SQLite store, reference data (SRD snapshot, packs), and later
  PDF import/export. Currently the campaign store (`store`: memory + SQLite,
  async SQLite wrapper, export/import, backup), the schema-gated loader `load`,
  and the content reference (`reference/`: snapshot format, index, fixture, Open5e
  converter). The snapshot pipeline is described in section 8.
- **protocol** (`crates/localtt-protocol`) -- transport contract shared by the
  server and clients: command, event, warning, content, sheet, health,
  diagnostics, response and safe error DTOs, plus the actor's projected values
  (`CommandOutcome`, `FeedPage`, `StreamEvent`). It depends only on `core` and
  `engine`, so a WASM client can consume it without pulling in axum or tokio.
- **web** (`web/`) -- the Leptos 0.8 CSR browser client: token entry, join-link
  consumption and the `SignedOut`/`Connecting`/`Ready` session lifecycle, REST
  reads (health, campaign, entity/sheet, feed, reference), `/ws` resync, the core
  command forms (creation, inventory/handouts, HP/rest, dice, clock, save), the
  DM roster panel (create, one-time invite, reissue, revoke, two-step removal)
  and a debug panel over the gated diagnostics route with a bounded local
  event/error ring. Pure command, formatting and URL builders compile natively
  for tests; the UI is gated to `wasm32` and Trunk builds `web/dist`. It depends
  on `protocol` and `core`, not `server`.
- **server** (`crates/localtt-server`) -- driving adapter: async `AppHandle`,
  the single-writer actor (`app`), campaign startup/bootstrap (`session`), the
  process command environment (`env`), the server failure type (`error`), the
  transport contract re-exported from `protocol`, the HTTP/WS adapter (`http`:
  axum router, token, origin/CORS, body cap, auth rate limit, request-id tracing,
  REST handlers, diagnostic endpoint, WebSocket stream), data root/lock policy
  (`config`), runtime content loading
  (`content`) and the TUI (`tui`: pure reducer, pure renderer, terminal task,
  new-character form). The
  composition root is `src/bin/localtt.rs`; it can run the TUI, `tui --serve`, or
  headless `serve`. Optional `--web-dir` serves built browser assets through
  `ServeDir` with an SPA fallback and security headers, while `/api` and `/ws`
  stay guarded. Roster identity, invites, the DM key, stream revocation and the
  browser roster panel are built. The HTTP surface is documented in section 6;
  the browser test harness is in [`testing.md`](testing.md).
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

- Writes are **intents** (`SpendResource`, `ApplyDamage`, ...), not
  whole-document replacement, so simultaneous edits do not clobber each other.
  Lists are addressed by stable element id, never by index. Field-edit commands
  remain deferred.
- Commands are serialized through a single writer; each is one atomic transaction
  returning the new `rev`, any warnings, and the events it produced. At party
  scale this removes whole classes of races.
- Ports (engine-owned) the engine needs from outside: `CampaignStore` and its
  additive async twin `AsyncCampaignStore` (implemented by the adapter;
  `StoreBackup` is the separate file-capability trait the composition root uses),
  `ReferenceSource` and `Rng`; `SheetExporter`/`SheetImporter` are later. The
  command and dice layers sit behind `Rng` for determinism. There is no
  `EventSink` port in v1 (`ENG-7`): commands return their events.
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
  binds loopback by default, requires a roster token on every `/api` and `/ws`
  route except an origin-checked CORS preflight, resolves it to a `Viewer`, checks
  `Origin`, reflects allowed CORS origins, caps bodies at 1 MiB and rate-limits
  failed auth per client IP (ARCH-22). The DM token is printed at startup and kept
  owner-only in `<campaign>.dm.key` (`0600` on Unix); `--rotate-dm-key` replaces it
  and `--no-dm-key-file` opts out. `--lan` is the only way to bind a non-loopback
  address; it adds detected interface origins and matching `ws://` CSP entries,
  while `--lan-host` pins the invite base and its origin. DM-only roster routes
  create seats, mint or revoke player invites, and remove seats; the raw invite
  token is returned once as a `#token=` fragment and only its hash is stored. The
  TUI and the API share the actor; `localtt serve` runs headless and
  `localtt tui --serve` runs both. Sheet and health DTOs live in `protocol`, so
  the engine's `DerivedSheet` stays serde-free (ARCH-14).
  `/ws` uses the same router and middleware, subscribes the viewer before the
  upgrade, and serializes the projected `StreamEvent`s as `StreamEventDto` text
  frames; a lagged client resyncs over REST (ARCH-15). The stream records the
  token hash and revalidates it at subscribe; when a campaign change no longer
  resolves that hash to the viewer, the actor sends `Revoked`, drops the
  subscriber and the socket closes with policy code 1008.
  The browser does the same after a dropped socket, since events committed while
  it was offline were never delivered. It keeps one credential session
  (`SignedOut`/`Connecting`/`Ready`), persists the token only after the first
  read succeeds, and signs out with the login form's notice on a `401`, `429` or
  a `Revoked` stream while other failures keep the shell and set a dismissible
  banner (ARCH-20).
  API responses carry a generated `x-request-id` and trace method, path, status
  and latency. `--diagnostics` gates an authenticated diagnostics route exposing
  actor counters, subscriber count and queue state; it is off by default.
  `--web-dir` adds the public SPA/assets with `ServeDir`; API catch-alls remain
  authenticated JSON so unknown API paths never return HTML. Static responses
  carry no-store, `nosniff`, `DENY`, `no-referrer` and a generated CSP that
  permits the Trunk loader, WebAssembly and every enabled loopback/LAN `ws://`
  origin (ARCH-18, ARCH-23).

## 6. HTTP and WebSocket surface

Every API request and WebSocket upgrade requires `Authorization: Bearer
<roster token>`. `/ws` may instead use `?token=` because the browser WebSocket
API cannot set headers. A CORS preflight is origin-checked and unauthenticated.
The default bind is loopback; `Origin` must be in the allowlist; bodies are
capped at 1 MiB; failed authentication is rate-limited per client IP (8 failures
per 60-second window). Responses carry a generated `x-request-id`, including
auth failures and unknown API paths.

| Method and path | Purpose |
|---|---|
| `GET /api/v1/health` | version, campaign id/name, clock summary |
| `GET /api/v1/diagnostics` | opt-in via `--diagnostics`; campaign summary, uptime and `ActorStatsDto`; 404 when disabled |
| `GET /api/v1/campaign` | `CampaignView` with the authenticated viewer and a roster without token hashes |
| `POST /api/v1/campaign/save` | `StoreBackup::sync` (WAL checkpoint); DM-only |
| `GET /api/v1/roster` | DM-only `RosterAdminEntry` list; `has_invite` is a hash-presence flag |
| `POST /api/v1/roster` | DM-only; create a player seat and its first invite, 201 with `InviteDto` |
| `POST /api/v1/roster/{id}/invite` | DM-only reissue; replace the hash and return the new `InviteDto` |
| `DELETE /api/v1/roster/{id}/invite` | DM-only revoke; clear the hash |
| `DELETE /api/v1/roster/{id}` | DM-only removal; transfer entities to the calling DM |
| `GET /api/v1/entities?kind=` | `list_entities` for the viewer |
| `GET /api/v1/entities/{id}` | `get_entity` view |
| `GET /api/v1/entities/{id}/sheet` | derived sheet |
| `POST /api/v1/commands` | tagged `CommandDto`; returns `CommandOutcomeDto` |
| `GET /api/v1/feed?after=&limit=` | `FeedPageDto` with `trimmed_before`; default limit 100, capped at 500 |
| `GET /api/v1/reference/search?q=&kind=&limit=` | content summaries; comma-separated kinds, default limit 20, capped at 100 |
| `GET /api/v1/reference/{*id}` | `ContentEntryDto`; the ref id may contain `/` |
| `GET /api/v1/reference/sources` | `SourceInfoDto` list |
| `GET /ws` | per-viewer `StreamEventDto` text frames |
| `GET /` and static paths | built browser assets when `--web-dir` is set; public, with SPA fallback and security headers |

Roster routes mint a 256-bit token server-side. The raw token appears once in
`invite_url` as `#token=<raw>`; the store keeps only its SHA-256 hash. Invite
bases are chosen in this order: `--lan-host`, an allowed non-loopback request
`Origin`/`Host`, the first detected LAN address (default-route interface first),
then loopback. A loopback invite is flagged in the banner and the roster panel
because it only works on the DM's machine.

The WebSocket handler subscribes the viewer before completing the upgrade and
sends projected `StreamEventDto` frames. A lagged client gets `Lagged` and
resyncs over REST. Each subscriber stores the token hash that opened it; after a
campaign change the actor revalidates that hash, sends `Revoked`, drops the
subscriber and closes with policy code 1008. The browser clears its stored token
and stops reconnecting on `Revoked`, `401` or `429`.

Static assets are public; `/api` and `/ws` stay guarded. `--web-dir` enables the
SPA and assets through `ServeDir`; API catch-alls return authenticated JSON so
unknown API paths never return HTML. Static responses carry no-store, `nosniff`,
`DENY`, `no-referrer` and a generated CSP that permits the Trunk loader,
WebAssembly and every enabled loopback/LAN `ws://` origin.

`localtt` can run the TUI, `tui --serve`, or headless `serve`. The DM key is
stored owner-only in `<campaign file>.dm.key` (`0600` on Unix); a matching file
is reused, a mismatch or a missing file with an existing seat hash fails closed,
`--rotate-dm-key` replaces both, and `--no-dm-key-file` mints a throwaway key
each start. `--lan` is the only way to bind a non-loopback address.

## 7. Persistence and versioning

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

## 8. Content pipeline

Content is user-supplied runtime data, not embedded in the binary. The server
loads one or more source directories at startup; the repository fixture under
`crates/localtt-adapter/data/srd2014/` is the workspace and test default.

- One namespace directory per source: `index.json` (version, provenance,
  name/offset rows), `entries.jsonl` (bodies read lazily by byte offset),
  optional `aliases.json` and `report.json`.
- `index.json` is written last and acts as the completeness marker. Writes are
  deterministic: sorted entries, temporary file plus rename.
- A namespace directory name must equal its `provenance.namespace`. Non-namespace
  names and backup names (`*.bak`, `*.tmp`, `*.old`) are pruned. A missing or
  invalid `index.json` or `entries.jsonl` is a fatal source error.
- Reads are strict: a body that fails to parse fails lookup and search alike.
  `Ok(None)` means "no such entry"; `Err` means the source is broken.
- Aliases are namespace-scoped, one-hop and live-first. A foreign-namespace
  alias, a live-id collision, a chain or a cross-namespace target is `Corrupt`.
- Autocomplete is an in-memory ranked prefix index: exact, prefix, word-prefix,
  then substring, with deterministic integer/string ordering. FTS5, fuzzy and
  trigram search are deferred behind the same port.
- The Open5e converter is `localtt-content-convert` in `localtt-adapter`. The
  snapshot format and conversion mapping are documented in the converter module;
  regeneration and licence terms are in
  [`crates/localtt-adapter/data/README.md`](../crates/localtt-adapter/data/README.md).
- Sources are immutable per process; swapping or re-pinning data requires a
  restart. Direct async REST backends and remote search are deferred; the local
  `ReferenceSource` stays blocking.

## 9. Testing

- `just test` is the gate: native tests (`--all-features` and
  `--no-default-features`, including doc tests), loom over the pure crates,
  shuttle, and the nested web workspace. Miri is currently commented out of the
  recipe; the pure-crate list is kept for when it returns. `just lint` and
  `just check` cover format/lints and docs plus a feature-powerset check. Web
  tests are four layers: native tests for pure formatting/URL logic; headless
  WASM tests that mount the login shell and cover session/auth behaviour; an
  ignored browser E2E suite run by `just e2e` through geckodriver against an
  in-process seeded server and the built `web/dist` assets; and the ignored
  `just ui-shot` scenario loop, which reuses that harness to write desktop/narrow
  screenshots plus `report.md` and `manifest.json` under `target/ui/latest`. The
  browser journeys cover login/creation/save, hit points with temporary hit
  points and rests, handouts, reconnect resync, restart persistence, wrong-token
  rejection, an expired session, a rejected form, outage recovery and the DM
  roster invite/revoke/remove journey, the diagnostics panel, the debug/error ring
  and request-id correlation; native router tests pin the gate, counters and
  headers. `just web-build` runs Trunk. See [`docs/testing.md`](testing.md).
- Core's contract is pinned by `crates/localtt-core/tests/types.rs` (shape and
  serde) and the integration suite
  (`crates/localtt-integration/tests/scenarios.rs`) by whole-campaign round trips
  and the golden-character catalog.
- Concurrency is a static `Send + Sync` contract for the data types; there is no
  contention behaviour in core. The engine is synchronous and single-writer, so
  there is no engine concurrency surface; the server actor is covered by its own
  router and actor tests.
- Deliberately absent: mutation testing and snapshot (`insta`) tests. The repo has
  no recipe for the former; the latter is deferred until snapshots are worth
  diffing.

## 10. Documentation map

- [`RFC-0001-localtt.md`](../RFC-0001-localtt.md) - product intent, invariants
  and open product questions.
- [`docs/engine.md`](engine.md) - engine behaviour contract: rules, dice,
  authorization and read services.
- [`crates/localtt-core/DESIGN.md`](../crates/localtt-core/DESIGN.md) - the
  domain data model, serde contract, version policy and builders.
- [`crates/localtt-integration/DESIGN.md`](../crates/localtt-integration/DESIGN.md)
  - the scenario suite and golden-character catalog.
- [`docs/decisions.md`](decisions.md) - the decision log.
- [`docs/testing.md`](testing.md) - native, WASM, browser E2E and UI snapshot
  layers, selectors and artifacts.
- [`TODO.md`](../TODO.md) - the consolidated deferred-work backlog.
- [`AGENTS.md`](../AGENTS.md) - workflow, style and the commands to run.
