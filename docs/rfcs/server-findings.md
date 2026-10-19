# RFC: server findings -- gaps and friction the toy server plan must resolve

| | |
|---|---|
| **Status** | Findings (2026-10-19): the register of what the full-stack session tests and the audit they triggered say the toy server (U7) needs. This RFC pins no library code and is not a unit spec: each finding is independently actionable and carries evidence, impact, options, a recommendation and a rough size. Dispositions are inputs, not decisions. |
| **Scope** | Evidence: `crates/localtt-integration/tests/session_e2e.rs`. Surfaces audited: `localtt-engine::{commands, executor, error, state, services, authz, ports::store}`, `localtt-adapter::{store, rng, reference}`, `localtt-server` (placeholder). |
| **Read with** | RFC-0001 3.4, 5.2, 5.7, 5.8, 5.9, 5.11, 9.3; `docs/architecture.md` sections 3-5; `docs/decisions.md` ARCH-3, ENG-5/7/11/14/17, STORE-5/7/11, CONTENT-9; `docs/rfcs/{store,engine-executor,authz,dice,content}.md`; `crates/localtt-integration/DESIGN.md`. |
| **Pins** | Nothing. This is the findings register for the server plan; it becomes historical once that plan lands. |

## 1. Purpose and method

The e2e slice joined the finished units the way a client session does: fixtures,
then a raw creation patch, then `LoadedCampaign`, then an `Executor` over the
shipped adapter pieces, actor-scoped commands, the read services and a
close/reopen. The three tests are:

| Test | What it exercises |
|---|---|
| `a_session_over_sqlite_survives_close_and_reopen` | clock and expiry, a check plus a free roll with feed audiences, damage/spend/rest, an unauthorized write, the read services, restart byte fidelity |
| `authorization_holds_across_a_persisted_party` | the DM, owner, masked reader, grantless member and non-member matrix; masked views; feed visibility and paging |
| `failed_commands_leave_the_sqlite_file_unchanged` | one refused command per error class (Malformed, Conflict, Unauthorized, NotFound); no durable or visible write |

The slice changed no frozen target test and no engine semantic to pass; friction
is recorded here instead. A finding is one of:

- **gap** -- no supported client path exists;
- **friction** -- a path exists but every client hand-rolls it;
- **input** -- a server-unit design decision, not a missing API.

Every finding names its producer in code or docs, so a fix has a place to land.

## 2. What the server can already assume

The server plan does not need to revisit execution, persistence, authorization or
read projection; these are pinned:

| Capability | Where pinned |
|---|---|
| Apply -> assert -> store -> state, durable before visible; `Executed { warnings, events }` | `engine::executor`, ENG-2 |
| Per-entity `expected_rev` CAS and `EngineError::Conflict` | ENG-4, ENG-11; target suite |
| Actor-scoped execution and the per-command authorization pre-check | `engine::authz`, U6 suite |
| Viewer-facing reads: `services::{list_entities, get_entity, feed_after}` with `view_of` masking and audience filtering | `authz.md`, U6 suite |
| Events as ordered delivery hints (`EntityChanged`, `EntityRemoved`, `CampaignChanged`, `FeedAppended`) | `engine-executor.md` 5.7 |
| Warning vocabulary and the typed error taxonomy | `engine::error` |
| One store port, two backends; restart durability, byte fidelity, per-command atomicity | `store.md`, U1 suite |
| Shipped ports: `ChaChaRng`, fixture and directory reference sources, `SqliteStore` | `adapter::{rng, reference, store}` |

What follows is about **creation, identity, wire shape and the runtime loop**.

## 3. Findings

### F1 -- campaign and roster reads have no viewer-facing view (gap)

**Producer.** `services` exposes entities and feed only (`engine::services`).
`WorkingSet::campaign()` returns the raw `Campaign` (`engine::state`), whose
`players[]` carry `token_hash` (`core::types`), and `authz` has entity and feed
gates only. `docs/architecture.md` section 4 states the one-egress-point rule:
`view_of` is the only viewer-facing producer.

**Impact.** A server cannot hand a client the clock, the settings or the roster
without either projecting the raw document itself (a second egress point, with
drift risk) or leaking `token_hash`. Clock changes are already broadcast as
`Event::CampaignChanged`, but there is no safe read to pair with it, and the
roster visibility rules are unspecified (does a player see other players? what
does the DM see?).

**Options.**

1. Engine slice: `authz::view_of_campaign` plus
   `services::campaign_view(state, viewer)` returning an engine-owned view
   (name, clock, settings, roster) that never includes `token_hash`.
2. Server-local projection with documented rules.
3. Minimal: expose clock and settings only, no roster.

**Recommendation.** Option 1, with the visibility rules decided in the server
plan. It is the same shape as the entity and feed services and keeps egress in
one place; the view can start small and grow additively. Option 2 is the toy
workaround if the plan wants to defer engine work.

**Size.** S-M (engine slice with target tests). Toy-blocking for any safe client
read of the campaign.

### F2 -- roster, players and invites have no command path (gap)

**Producer.** No `Command` variant touches players (`engine::commands`). The e2e
suite appends `Player` values directly to `campaign.players`
(`session_e2e.rs::add_member`). `Campaign` is frozen and has no `rev` (ENG-11),
and `Player.token_hash` exists for the join model (RFC-0001 5.8). RFC-0001 5.2
lists "create invite, revoke, reissue token" as candidate commands.

**Impact.** The join flow (DM key, invite slots, per-slot tokens) has no engine
surface: roster edits happen as whole-campaign store writes, outside authz,
events and validation. The toy server can do this itself, but every roster edit
is a hand-rolled patch and a full campaign upsert, and nothing emits
`CampaignChanged` unless it goes through a command.

**Options.**

1. Engine commands (DM-only): add/remove player, set role, reissue invite; token
   minting and hashing stay in the server.
2. Composition-root writes plus server-owned tokens for the toy; engine commands
   later.
3. Full invite/token commands, including `token_hash` management in the engine.

**Recommendation.** Option 2 for the toy, option 1 when the server needs in-band
roster edits. Keep token minting and verification in the server (transport and
security concern) and persist only the hash through `Player.token_hash`.

**Size.** M (commands plus join flow); S for the composition-root version.

### F3 -- entity lifecycle commands are absent and creates are refused by the executor (gap)

**Producer.** `Command` has no create/update/delete/owner/ACL variants.
`Executor::commit` asserts that every upsert replaces a loaded document with
`rev + 1` (`engine::executor`), while `StatePatch.upserts` and
`LoadedCampaign::apply` already accept inserts. `docs/rfcs/engine-executor.md`
section 2 defers entity lifecycle to a later unit, and its design notes pin "the
first create commands must start at `rev = 1`; no U3 command creates". RFC-0001
5.2 lists the catalogue. The e2e suite creates through a raw `StatePatch`
(`session_e2e.rs::seeding_patch`).

**Impact.** The server cannot create a character, note, handout or summon
through a command: no authorization, no events, no revision semantics, no feed.
All in-app creation is either store seeding (a workspace-internal DTO) or
deferred to import.

**Options.**

1. Minimal lifecycle slice: `CreateEntity` (DM; `rev = 1`), `DeleteEntity`
   (`Event::EntityRemoved` gets its first producer), and the executor assertion
   extended for creates; spec and target tests per the repo workflow.
2. Full catalogue: field edits, owner reassignment, share/unshare,
   summon-from-template.
3. Toy scope: no creation; bootstrap from fixtures or a bundle and run commands
   on existing entities only.

**Recommendation.** Decide in the server plan. If the toy supports character
creation, option 1 is the smallest coherent unit; option 2 stays a later slice.
The engine side is smaller than it looks: only the `commit` assertion and the
command surface are missing, because the working set and the store already
handle creates.

**Size.** M (new unit: spec -> target tests -> implementation) or 0 if deferred.

### F4 -- no packaged campaign bootstrap (friction)

**Producer.** `CampaignStore` is `load` plus `apply`
(`engine::ports::store`). `StoreFault::NoCampaign` says "create one first", but
nothing creates one. `import_into` (`adapter::store::export`) is the only
packaged creator: it checks emptiness itself and applies one `ExportBundle`. The
e2e suite hand-rolls the same creation patch (`session_e2e.rs::seeding_patch`).

**Impact.** The composition root (ENG-5: the `localtt` binary in
`localtt-server`) must hand-roll `CampaignBuilder` plus `StatePatch` and its own
emptiness guard; `apply` itself has no emptiness gate, only the store's patch
preconditions (a campaign id must match the stored one; entities and feed
require a stored campaign). There is no CLI story for a new campaign (`--new`?
create-if-missing? `--import <bundle>`?).

**Options.**

1. Server-side helper over `CampaignBuilder` and the concrete store's `apply`;
   no port change.
2. Adapter convenience (`SqliteStore::create` / `new_campaign`).
3. Extend the port with `create`.

**Recommendation.** Option 1; the plan decides the UX. Option 3 is rejected by
`store.md` 5.3 and STORE-5: the engine dataflow stays `load` plus `apply`, and
`import_into` remains the packaged check for the bundle path.

**Size.** S. Toy-blocking at startup.

### F5 -- no shipped `CommandEnv`; every client hand-rolls the glue (friction)

**Producer.** `CommandEnv` (`engine::commands`) has `now`, plus defaulted
`reference` and `roll`. The workspace has eleven test-local implementations
(three each in `engine::commands` tests, plus `engine::executor` tests, the
adapter property suite, the integration target and property suites, and
`session_e2e.rs::SessionEnv`). The adapter ships `ChaChaRng` and the reference
sources, but no public wall clock; the glue `dice::roll(plan, &mut rng)` is
repeated in every env. `backup.rs` keeps its `SystemTime` helper private. The
runtime content source and data root are CONTENT-9 (U7); `FixtureSource` is
explicitly the in-memory test source.

**Impact.** The composition root must assemble and keep in sync what the tests
already assemble by hand; a forgotten `roll` fails closed at the first roll
(`EngineError::Environment`), which is correct but late, and there is no shared
place for the wall-clock policy.

**Options.**

1. Server-local `Env` (composition root per ENG-5): `SystemTime`, the shipped
   `ChaChaRng`, the runtime `ReferenceSource`.
2. Adapter-shipped env (`adapter::env`) reused by the binary and the integration
   suite.
3. Leave the glue to each client.

**Recommendation.** Option 1 for the binary; revisit option 2 if a second
non-test consumer appears. Keep the trait as is (ENG-14).

**Size.** S. Toy-blocking: nothing executes without an env.

### F6 -- events, warnings and errors have no wire form (input)

**Producer.** `Event`, `Warning`, `WarningCode`, `ElementRef` and `EngineError`
have no serde derives (`engine::commands`, `engine::error`), though the engine
already depends on serde. `docs/rfcs/engine-executor.md` section 10 assigns
"Server DTO mapping for commands, events and warnings" to U7. `StoreFault` and
`EngineError` carry diagnostic strings that must not be forwarded
(`engine::ports::store`, `engine::error`).

**Impact.** No client contract exists for command results. `#[non_exhaustive]`
on `Event`, `WarningCode` and `EngineError` means direct derives have no
unknown-variant handling, and forwarding messages would leak internals. The
server also needs command DTOs; `Command` itself is not and should not be
wire-serializable.

**Options.**

1. Server DTOs with explicit mappings and an unknown fallback for engine
   non-exhaustive variants; engine types stay Rust-only.
2. Serde derives on engine types; the client wire is tied to engine releases and
   unknown variants have no handling.
3. Hybrid: serde for closed core value types, DTOs for engine enums.

**Recommendation.** Option 1. Add a mapping test per variant so a new engine
variant fails as an explicit choice, not as an accidental wire change.

**Size.** M, part of the server unit. Toy-blocking for any API surface.

### F7 -- single-writer loop: reads, fan-out and the blocking store share one serialization point (input)

**Producer.** `Executor<'a>` owns the working set and the env, borrows
`&mut dyn CampaignStore`, and is `Send` but deliberately not `Sync`
(`engine::executor`; `engine-executor.md` 4.4). The store is synchronous and may
block (RFC-0001 5.9). `Executed.events` carry ids only, and RFC-0001 5.11 says
the server pushes the full filtered entity on change and resumes from the last
feed id. There is no `EventSink` (ENG-7) and no campaign CAS (ENG-11).

**Impact.** Read endpoints cannot borrow `Executor::state()` outside the
writer's serialization: a read either holds the same lock, clones quickly under
it, or reloads from the store. The fan-out must build each recipient's payload
(through `view_of` and feed filtering) before releasing the lock. Clock and
settings writes are last-write-wins; the single writer is the only protection.

**Options.**

1. A `Mutex` (or `RwLock`) app state holding store and executor; commands and
   reads serialize.
2. An actor thread owning the executor, commands over a channel, reads served
   from cloned snapshots or a shared read model.
3. Reload a `CampaignSnapshot` per read.

**Recommendation.** Option 1 for the toy; option 2 if load or async integration
makes blocking visible. Keep the synchronous store off an async executor thread
`spawn_blocking`-style (RFC-0001 5.9).

**Size.** M (design; small code in a toy). Toy-blocking: it is the architecture
of the loop.

## 4. Inputs to the server plan (not gaps)

### 4.1 Error mapping sketch

| Class | Status candidates | Notes |
|---|---|---|
| `Malformed` | 400 | client input; do not forward `reason` |
| `Unauthorized` | 403 | actor plus optional entity |
| `NotFound` | 404 | entity plus optional element |
| `Conflict` | 409 | `expected`/`actual` are client-useful |
| `TooLarge` | 400 or 413 | dice caps; pick one in the plan |
| `Store(StoreFault::NoCampaign)` | startup error | not a request error |
| `Store` others | 500 or 503 | `Unavailable` is 503; `InvalidPatch` is an engine bug (500) |
| `Environment`, `Content` | 5xx | server-side faults; never forward messages |

`StoreFault::VersionRefused` / `FormatRefused` are upgrade conditions (409/426);
STORE-8 anticipates the U7 mapping.

### 4.2 Event to client update

| Event | Client update |
|---|---|
| `EntityChanged` | refetch the entity through `get_entity` (per-viewer), or push the full view per RFC-0001 5.11 |
| `EntityRemoved` | drop it from clients |
| `CampaignChanged` | refetch the campaign view (F1) |
| `FeedAppended` | append through `feed_after` or push the entries |
| warnings | command response only, never the feed |

### 4.3 Feed content asymmetry

Feed appends come from rolls, clock expiry and concentration hints; HP,
resource and rest changes are entity events with no feed entry (by design). A
client must not treat the feed as a changelog of entity state; entity `rev`
refetch is the sync path.

### 4.4 Identity and join

`PlayerId` is the actor everywhere; no session store exists. Join token minting,
verification and reissue live in the server; only `Player.token_hash` is
persisted (RFC-0001 5.8). DM key handling is still open (RFC-0001 9).

### 4.5 Content and data root

Runtime source selection and the default data directory are CONTENT-9 (U7);
`SHIPPED_DATA_ROOT` is build-time only, and `FixtureSource` is a test source,
not the runtime default.

### 4.6 Startup and backup

Startup backup policy and `--no-backup` are composition-root concerns (STORE-7);
`StoreBackup` is the capability trait behind the concrete store. SQLite runs
behind the single writer (STORE-3, RFC-0001 5.9).

## 5. Disposition

| Finding | Kind | Candidate home | Size | Toy-blocking |
|---|---|---|---|---|
| F1 campaign/roster view | gap | engine (`authz`, `services`) | S-M | yes for safe reads; a server-local projection works around |
| F2 roster and invites | gap | engine (commands) plus server (tokens) | M | no; composition-root writes suffice |
| F3 entity lifecycle | gap | engine (new unit) | M | depends on scope; no for commands on existing entities |
| F4 campaign bootstrap | friction | server | S | yes, at startup |
| F5 `CommandEnv` | friction | server | S | yes, nothing executes without it |
| F6 wire forms | input | server | M | yes, for any API surface |
| F7 writer/read loop | input | server | M (design) | yes, it is the loop |

Suggested split: the server plan owns F4-F7; F1 and F2 are self-contained engine
slices that can move in parallel; F3 needs a scope call first.

## 6. Open questions

- **Toy scope:** does the server create entities and players in-app, or bootstrap
  from fixtures or a bundle and run commands on existing entities? This drives
  F2 and F3.
- **Roster visibility:** what does a player see about other players, and what
  does the DM see? This drives F1.
- **Transport shape:** full views pushed on events (RFC-0001 5.11) versus ids
  plus client refetch; reconnect semantics. This drives F6 and F7.
- **Error envelope:** one response shape for command results, warnings and
  errors. This drives F6.
- **Identity:** DM key persistence, token minting and join UX. This drives F2
  and section 4.4.
