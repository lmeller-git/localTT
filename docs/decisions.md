# Decision log

The load-bearing decisions behind the current code. Product-level decisions
(target ruleset, one campaign per run, invite-slot identity, ...) live in
`RFC-0001-localtt.md` section 9 and are not repeated here. This log records the
architecture and data decisions made while building, plus the items deliberately
deferred. Format: **decision -- why -- status** (Accepted unless noted).

## Architecture and boundaries

- **ARCH-1: Hexagonal layout across four crates** (`core`, `engine`, `adapter`,
  `server`), with `localtt-integration` as a leaf consumer. -- Keeps the domain
  pure and testable without I/O; lets storage, rules, reference data and transport
  evolve independently. -- Accepted.
- **ARCH-2: `core` depends on nothing but `serde`/`serde_json`/`uuid`/`thiserror`
  and contains no I/O.** -- Fast compiles, and it can later be extracted as the
  plugin-facing crate. -- Accepted.
- **ARCH-3: the dependency direction is one-way** (`server -> engine`, `adapter`, `core`;
  `adapter -> engine`, `core`; `engine -> core`; never `engine -> adapter`). -- Classic
  hexagonal: the application owns its ports and driven adapters implement them, so the
  engine never depends on storage while adapters may depend on the engine. Revised
  2026-10-17: the earlier wording ("`adapter -> core`, never `engine`") was a false
  constraint; `server` composes engine + adapters + core. -- Accepted, revised.
- **ARCH-4: builders live in core (`localtt_core::builders`), not in `types.rs`
  and not in the integration crate.** -- Keeps the wire contract in one file,
  gives every consumer (engine, PDF import, packs) the same constructor sugar, and
  avoids a second public path for the same item. -- Accepted (supersedes the
  earlier "builders in the integration crate" proposal).
- **ARCH-5: the command surface is provisional `&mut self` functions in
  `localtt_engine::commands`.** -- The real engine will likely be
  `(state, command) -> result`; keeping the names stable makes the rewrite
  mechanical. -- Accepted, expected to change.
- **ARCH-6: the schema-gated loader (`load_checked`) is adapter logic** and lives
  in `localtt_adapter::load`. -- "Load a persisted document behind a version gate"
  is store work, and relocating it also lets it peek the version without a full
  parse. -- Accepted.
- **ARCH-7: no `localtt-testkit` yet.** -- Extracting the fixtures is deferred
  until a second consumer exists, to avoid a dev-dependency cycle. -- Deferred.
- **ARCH-8: the server core is one actor owning the store, working set and
  environment; frontends are driving adapters and the TUI is first.** -- The sync
  `Executor` borrows the store and is not `Sync`; an actor thread gives a single
  writer, isolates blocking SQLite from the runtime, and keeps the core free of a
  renderer/backend trait. HTTP/WS later call the same `AppHandle`. -- Accepted
  (server RFC review, 2026-10-20).
- **ARCH-9: projected events are per-viewer ids plus refetch; `StreamEvent::
  Forgotten` covers both deletion and ACL narrowing.** -- A removed document cannot
  be projected after the commit, so the dispatcher keeps a per-subscriber known
  set and compares it with post-commit readability; ids keep the payload out of
  the core and `Lagged` forces a full resync. -- Accepted (server RFC review,
  2026-10-20).
- **ARCH-10: the TUI is a pure reducer plus pure renderer; the terminal task owns
  input and effect futures.** -- `update(&mut state, TuiMsg) -> Vec<Effect>` is
  testable without a terminal, and rendering over `&TuiState` is a pure function.
  The task reads keys on a dedicated thread and runs effects in a `JoinSet`, so
  slow saves and actor calls never block drawing or input; results return as
  messages. -- Accepted (M1 implementation, 2026-10-20).

## Data model

- **DATA-1: entity type-state `Entity<B: Body>` with a sealed `Body` trait**, plus
  `AnyEntity` as the runtime-tagged enum. -- Compile-time kind/body pairing with no
  dead variants, while storage, iteration and redaction still work with a runtime
  tag. A per-kind `Entity<Pc>`/`Entity<Monster>` design was rejected: the statblock
  method set is identical, and the marker zoo would leak into storage. -- Accepted.
- **DATA-2: one generic `Id<T>` with a `PhantomData<fn() -> T>` domain tag.** --
  ID domain separation with no per-type boilerplate; `EntityId = Id<AnyEntity>`.
  Trait impls are hand-written without a `T` bound. -- Accepted.
- **DATA-3: hybrid `StatKey::Known | Custom`.** -- The engine understands a fixed
  set of stats, but homebrew stats must be storable; custom keys are validated and
  round-trip byte-identically. Promotion of a custom key to `Known` is a
  major-version event. -- Accepted.
- **DATA-4: no floats anywhere.** -- Deterministic bytes, `Eq`, and no NaN
  questions. `ChallengeRating` is stored in eighths; weight is integer ounces;
  sheet numerics are `i32` (signed so "warn but succeed" states are
  representable). -- Accepted.
- **DATA-5: `BTreeMap`/`BTreeSet` in serialized positions.** -- Stable, diffable
  document bytes across runs and platforms. -- Accepted.
- **DATA-6: enum tagging is part of the public contract** (internally tagged
  payload enums; untagged `Formula`/`RefOrText`/`OverrideValue`; transparent
  newtypes). Unknown object fields are ignored, but unknown enum tags are a parse
  **error** -- no silent reinterpretation. -- Accepted.
- **DATA-7: external references are namespaced plus a name snapshot.** -- A
  missing pack or a rename degrades to text instead of a broken sheet. -- Accepted.
- **DATA-8: statuses are the carrier for timed effect modifiers**, with group
  targets and a dice leaf, so effects like Bless are data rather than a separate
  channel. -- Accepted.
- **DATA-9: `FieldMask` is default-deny and section-level.**
  `None` means full read; `Write`, owners and the DM ignore masks. -- Accepted;
  finer granularity is deferred.
- **DATA-10: no `Default` for `Abilities`/`StatblockBody`/`CampaignSettings`.**
  -- A silent `10` in every ability (or invented policy settings) corrupts a sheet;
  the empty-able leaf sections do get `Default` so an importer can build a
  statblock field by field. -- Accepted.
- **DATA-11: column choices that follow from "mirror the paper sheet":** `senses`
  is free text (nothing computes it) while `speed` is structured (modifiers
  target it); `tools`/`languages` are single text fields, not id-carrying lists.
  -- Accepted; both are revisitable if a feature needs more structure.
- **DATA-12: the in-game calendar is derived, not stored.** -- `CampaignTime` is
  seconds since a campaign-relative origin; day, week and time of day are pure
  functions of it, so there is no calendar field to migrate and the calendar can
  never disagree with the clock. A week is Faerûn's ten-day *tenday*
  (`DAYS_PER_WEEK = 10`). Named months/eras/weekdays and a second, absolute
  "world" clock are deferred and additive. -- Accepted.

## Versioning and persistence

- **VER-1: `schema_version` on every entity and on the campaign, from day one.**
  -- Schema evolution is the long-term cost center; versioning early is cheap. --
  Accepted.
- **VER-2: a loader accepts same-or-older major, refuses a newer major, accepts a
  newer minor (dropping its unknown fields), and re-stamps the loaded document at
  `CURRENT`.** -- Refusing beats partial loading; re-stamping is honest once the
  newer minor's fields are dropped. Applies to both `Entity` and `Campaign`. --
  Accepted.
- **VER-3: one SQLite file per campaign, JSON documents in rows, only queried
  columns extracted.** -- Trivial backup and "restore next session"; the document
  structure is still moving. -- Accepted (the SQLite store is not built yet).

## Engine

Design and rationale: `docs/engine-architecture.md` section 11 (approved 2026-10-17).

- **ENG-1: ports live in `localtt-engine` (`engine::ports`); the adapter implements them.**
  -- With `adapter -> engine` allowed (ARCH-3) the application can own its ports and `core`
  stays pure data, gaining nothing: `StatePatch`, `CampaignSnapshot`, `StoreFault`, the
  content model and `ExportBundle` live in the engine outside the wire contract. Replaces
  the earlier `core::ports` placement, which rested on the false dependency rule. -- Accepted,
  revised 2026-10-17.
- **ENG-2: the command model is `(state, command) -> Transition`, and the executor
  persists the patch before publishing it.** -- Commands stay pure and testable; durable
  before visible removes rollback handling; a patch is exactly one transaction. --
  Accepted.
- **ENG-3: all reads go through `WorkingSet`; the default implementation is load-all;
  lazy/LRU is an implementation swap.** -- Party scale makes load-all trivial; the trait
  keeps the door open for attachments and large campaigns; reads return owned values so a
  lazy implementation is possible without borrow fights. -- Accepted.
- **ENG-4: `rev` is checked in the engine against in-memory state; the store is a dumb
  single writer.** -- One writer, one authority; SQL compare-and-swap would be dead code;
  the server maps conflicts to 409. -- Accepted.
- **ENG-5: the composition root is `localtt-server`'s `[[bin]]`, named `localtt`.** -- No
  fifth crate whose only content is wiring; the library stays testable; a future headless
  CLI can reuse `build_app`. -- Accepted (RFC-0001 9.3 item 1, closed).
- **ENG-6: the expiry index is a `BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>`;
  timing wheels are rejected.** -- A party holds tens of statuses at discrete command
  points, so `O(log n)` is plenty; rewind is first-class and one range query answers
  "what is in `(old, new]`?"; soft expiry makes the index a candidate enumerator, not the
  truth. The wire keeps the absolute `expires_at`. Number argument in
  `docs/engine-architecture.md` section 7.6; revisit only if a benchmark proves
  otherwise. -- Accepted.
- **ENG-7: no `EventSink` port in v1; commands return events.** -- One synchronous egress
  model; a second publish path would compete with `view_of` and fan-out tests. Revisit
  only for timer-driven events that no command produces. -- Accepted.
- **ENG-8: `view_of` produces engine-owned view types, never redacted clones.** -- Core
  has no `Default` for critical sections (DATA-10); zeros would be ambiguous. View types
  carry `hidden` sections and a `writable` flag. -- Accepted.
- **ENG-9: the engine consumes the core calendar functions (DATA-12); crossing arithmetic
  is exact integer seconds plus `dawn_hour`.** -- No runtime state in core; no persisted
  calendar fields; the calendar cannot disagree with the clock. -- Accepted.
- **ENG-10: `Executed` drops the design's `reverted` field.** -- There is no rollback path
  and the field would always be false; a command either commits with warnings and events or
  returns an error before publish (ENG-2). -- Accepted (U3 engine, 2026-10-18).
- **ENG-11: U3 rev semantics: clock commands take no `expected_rev`, every U3 entity
  command takes `expected_rev: Option<u64>` as a uniform CAS.** -- The frozen core
  `Campaign` has no `rev`; campaign CAS returns with the server (U7) as a separate versioned
  core proposal if real concurrent clients make it worth a wire change (OQ-1/A). Deviation
  from design 5.5, which reserved the CAS for whole-document commands. -- Accepted
  (2026-10-18).
- **ENG-12: `clock::crossed_dawns` returns `i64`, signed.** -- One call represents forward
  and backward crossings; the design 7.6 sketch's `u32` could not express a rewind. --
  Accepted (2026-10-18).
- **ENG-13: the warning surface is pinned: `Warning { code, message, entity, element }`,
  five codes, and a two-variant `ElementRef`; both `#[non_exhaustive]`.** -- Design 7.3/7.4
  named both without shapes; additive growth stays possible. -- Accepted (2026-10-18).
- **ENG-14: `CommandEnv` is U3-minimal (`now()` only) and `Send`.** -- `reference()` and
  `roll()` arrive with U4/U5; `Send` keeps `Executor` `Send`. -- Accepted (2026-10-18).
- **ENG-15: `WorkingSet` gains `next_feed_id()`.** -- The design's transition sketch
  assigned feed ids without a source for them; the counter must come from the working set
  so trimming never rewinds it. -- Accepted (2026-10-18).
- **ENG-16: `WorkingSet`/`WorkingSetMut` are `Send` and `WorkingSetMut::apply` is
  infallible.** -- `Send` carries the executor's claim; an infallible apply removes a
  divergence path after the store commit (5.1). -- Accepted (2026-10-18).
- **ENG-17: `LoadedCampaign::from_snapshot` and `Executor::state()` are pinned.** -- The
  design sketched the struct and loop without the load seam or a read-back accessor; the
  target test drives both. -- Accepted (2026-10-18).
- **ENG-18: feed and event shapes are pinned:** expiry entries (5.2), event order (5.7),
  ring trim and the append cap (5.6), id allocation order, and the capacity-0 stance
  (OQ-3/A). -- Pins of design 7.6/7.9. -- Accepted (2026-10-18).
- **ENG-19: rest scope is pinned to RFC-0001 5.3 bookkeeping plus the optional clock
  move.** -- Recovery arithmetic (HP, death saves, exhaustion, hit dice) is OQ-5, deferred
  to U4+; the rest target is always rewritten as a checkpoint and a target that is also
  dawn-refilled stays one revision. -- Accepted (2026-10-18).
- **ENG-20: non-const `Formula` maxima degrade loudly (`FormulaUnevaluated`, no silent
  skip).** -- `Formula::Const` evaluates in U3: refills skip and warn, `RestoreResource`
  applies unclamped and warns (OQ-2/A). -- Accepted (2026-10-18).
- **ENG-21: the provisional helper paths (`apply_damage`, `heal`, `set_temp_hp`,
  `spend_resource`, `begin_concentration`, `level_up`, `summon_from_template`,
  `give_handout`) are a compatibility contract after the `commands/` split.** -- The frozen
  scenario suite keeps its import paths; the command effects reuse the same arithmetic, so
  command semantics cannot drift (6.4). -- Accepted (2026-10-18).
- **ENG-22: `clock::transitions` is absorbed by `commands::apply`; `clock` keeps only
  `crossed_dawns`.** -- The sweep produces feed entries and document deltas the transition
  owns; keeping `clock` pure arithmetic avoids a second owner of transition data. Deviation
  from design 7.6. -- Accepted (2026-10-18).
- **ENG-23: `Transition` fields are crate-private.** -- Clients see outcomes only through
  `Executed` after the commit (5.1), so a transition can be neither read nor built outside
  the engine. Deviation from design 7.2. -- Accepted (2026-10-18).
- **ENG-24: the rev-monotonicity check is an executor assertion.** -- Design 5.5 promised
  the check without a mechanism: the executor asserts `new.rev == old.rev + 1` per upsert
  before persisting and fails fast on violation (engine bug, never a client error; 5.1). --
  Accepted (2026-10-18).
- **ENG-25: the U3 spike folded its spec edits into `docs/rfcs/engine-executor.md` in the
  same commit as the code; recorded here for auditability.** -- `Transition` derives
  `Debug` (4.3), a statblock-only command on another entity kind is `Malformed` (5.4), the
  6.2 `next_feed_id` wording is tightened and 10 notes the create-path assertion. All
  match the tree. -- Accepted (2026-10-18).
- **ENG-26: the review fold pins exact and saturating domain edges (S1/S2).** --
  `clock::crossed_dawns` computes over `i128`, exact and panic-free across the whole `i64`
  time domain; the band below `i64::MIN + dawn_offset` previously overflowed in debug and
  wrapped in release, turning a rewind into a refill. Feed positions saturate at
  `u64::MAX` on load, in `cap_feed` (cutoff spelled `last - (c - 1)`) and when minting,
  and `stamp` saturates the entity revision, so a stored ceiling value never panics or
  wraps; an exhausted feed's duplicate append is refused loudly by the store's append
  validation. -- Accepted (2026-10-18).
- **ENG-27: the review fold pins one clock reading per transition and executor
  patch-shape assertions (N1-N3).** -- `finish` takes a single `env.now()` and stamps
  every document and feed entry with it; the executor pre-asserts the engine-built patch
  shape (upserts strictly ascending, feed appends strictly ascending and never below the
  trim cutoff) so an engine invariant break fails locally instead of surfacing as a store
  `InvalidPatch`; 6.1 now lists the private `commands/clock.rs` the implement phase added.
  -- Accepted (2026-10-18).
- **ENG-28: `Executor::into_parts` is additive and lets the server actor own the
  working set and environment between requests.** -- `into_parts(self)` returns the
  owned `Box<dyn WorkingSetMut>` and `Box<dyn CommandEnv>` and drops the store borrow,
  so `StoreBackup::sync` is callable with no executor alive. Keeping `Executor::new`
  unchanged avoids a call-site sweep across the frozen target suite; the alternative
  (borrowing the state) is rejected for that reason. -- Accepted (server RFC review,
  2026-10-20).
- **ENG-29: the async engine surface is additive; the sync path stays.** -- B2 adds
  `AsyncCampaignStore` plus an async `AsyncExecutor` and the adapter's
  `spawn_blocking` wrapper; `commands::apply` stays sync, `ReferenceSource` stays
  local/blocking (CONTENT-12), and `WorkingSet` stays `Send` (no `Sync` until a
  shared async read path exists). This keeps the frozen target suite compiling and
  avoids forcing the whole engine async for a remote content feature that CONTENT-12
  deliberately keeps behind a later dispatcher. -- Accepted (server RFC review,
  2026-10-20).
- **ENG-30: the campaign view is member-only and its roster exposes only id,
  display name and role.** -- A non-member is `Unauthorized`; every member sees the
  same roster, the campaign name/clock/settings and the default ruleset. The join
  token hash and last-seen stamp never enter the view, so the server cannot leak
  them by forwarding it. -- Accepted (F1, 2026-10-20).
- **ENG-31: entity creation is an actor-scoped `CreateEntity` command; the engine
  owns identity stamping and the access rules.** -- The draft may carry the id and
  body, but a nonzero revision is malformed; the engine sets `rev = 1`, both
  timestamps and the current schema version. A player may create only documents
  they own and may set no ACL grants; the DM may create for any roster member and
  set grants. Id collisions are `EngineError::AlreadyExists`; the executor's
  revision assertion now accepts a missing prior document only at `rev = 1`. The
  command payload is boxed to keep `Command` small. Delete/field/ACL commands
  remain deferred. -- Accepted (M2, 2026-10-20).

## Storage

Design and rationale: `docs/engine-architecture.md` sections 5 and 11.

- **STORE-1: storage format is the wire document verbatim in a `doc` column; extracted
  columns are the projection `id/kind/body_kind/owner/parent/name/rev/schema_version`,
  no ACL table in v1.** -- VER-3 plus a justification per column; ACL and other
  projections arrive only when a query needs them. -- Accepted.
- **STORE-2: DDL version in `PRAGMA user_version`, hand-rolled ordered migration steps;
  document migrations are pure `Value -> Value` functions in the adapter; fixtures per
  version committed under `fixtures/campaigns/`.** -- Same philosophy as ARCH-6 and
  VER-2; the registry exists now (empty) so v1 has a migration home. -- Accepted.
- **STORE-3: WAL + `synchronous=NORMAL` + `busy_timeout`; one process per campaign file,
  documented; automatic online backup on startup before migrations, rotate 5.** -- Safe
  against corruption and copy-while-running (RFC-0001 8.11); losing the last committed
  command on power loss is acceptable for a game log. -- Accepted.
- **STORE-4: one transaction per command, `apply(patch)`, `BEGIN IMMEDIATE`; the file is
  the only truth and the engine state is rebuilt from it at startup.** -- Matches the
  single-writer executor; crash recovery is restart. -- Accepted.
- **STORE-5: export/import is a versioned `ExportBundle`; import targets an empty store
  and is all-or-nothing; sharing supports both the `.sqlite` file and the bundle.** --
  The bundle is the escape hatch (RFC-0001 5.9); the file is the normal sharing path. --
  Accepted.
- **STORE-6: byte-for-byte document fidelity is a contract-test promise for documents
  applied through `apply` at `CURRENT`, not a promise about stored bytes.** -- Determinism
  guarantees (DATA-4/5) make canonical round-trips meaningful; hand-edited or older-minor
  documents are canonicalized on load (S14). -- Accepted, revised (adversarial review
  2026-10-17).
- **STORE-7: backup and `sync` are a capability trait (`StoreBackup`) the engine owns and
  never calls.** -- The engine's store dataflow stays `load` + `apply`; the composition root
  and tests use the capability, and file-backed implementations provide the mechanism
  (`SqliteStore` via `rusqlite::backup`) behind a backend-neutral contract, so neither the
  engine nor `core` contains backend concepts. The contract suite tests the trait against
  file-backed implementations only; startup policy and `--no-backup` stay
  composition-root concerns (U7). -- Accepted (review 2026-10-17).
- **STORE-8: `StoreFault` keeps failure classes distinct: document vs DDL migration,
  `InvalidPatch` (engine bug) vs `StoreNotEmpty` (user refusal), `FormatRefused` vs
  `Corrupt`; `user_version` fields are `i32`.** -- Resolves the adversarial review B1/B2/S2
  and keeps the U7 status mapping (409/500/upgrade) possible; every fault stays typed
  instead of overloading `Backend`. -- Accepted (adversarial review 2026-10-17).
- **STORE-9: `ExportBundle` derives `Serialize` only; `decode_bundle` parses an
  adapter-private wire type.** -- Removes the un-gated `Deserialize` path; writing bundles
  is plain serialization, reading is the single gated entry point (RFC S1/S2). -- Accepted
  (adversarial review 2026-10-17).
- **STORE-10: the store contract suite lives in `localtt-adapter/tests` with
  builder-made fixtures; the migration hook is a `#[cfg(test)]` unit test in
  `doc_migrations`; the "in-memory implementation + contract suite" rule is scoped to
  engine-consumed ports.** -- No dev-dependency cycle with the integration crate, no
  test-only public seam, and `StoreBackup` is capability-tested against file-backed
  stores (RFC S3/S4/S6). -- Accepted (adversarial review 2026-10-17).
- **STORE-11: the composition root owns the concrete store; the executor borrows
  `&mut dyn CampaignStore`.** -- Keeps `backup`/`sync` callable for policy after the
  store stops being boxed, and preserves single-writer ergonomics (RFC S5). -- Accepted
  (adversarial review 2026-10-17).
- **STORE-12: SQLite files carry an `application_id` magic; startup backups are
  `.tmp` + rename and rotation counts only complete files.** -- A foreign valid database
  is `Corrupt` instead of silently adopted, and a crash mid-backup cannot evict a good
  backup (RFC S9/S10). -- Accepted (adversarial review 2026-10-17).
- **STORE-13: the `apply` preconditions are closed as listed in RFC section 5.3
  (campaign singleton explicit, entities require a campaign, feed appends never collide
  and never fall below `feed_keep_from`); trimming runs before appends.** -- Violations
  are engine bugs in a single-writer system; fail fast, write nothing (RFC S11-S13). --
  Accepted (adversarial review 2026-10-17).
- **STORE-14: the post-implementation adversarial review hardened atomicity and fault
  classification without changing any public signature.** -- `apply` validates inside its
  `BEGIN IMMEDIATE` transaction, so the preconditions and the write are atomic and a
  second handle cannot interleave a conflicting patch; `load` reads its three queries in
  one read transaction; `sync` reports a blocked checkpoint as `Unavailable` instead of
  success; a magic-carrying file whose tables do not match its `user_version` is
  `Corrupt`, not `Unavailable` or `DdlMigration`; backup rotation is best-effort, so an
  unprunable entry does not fail the open. -- Each closes a silent-failure path the
  approved RFC left open (RFC section 5.3/5.4/5.5, section 6.1). -- Accepted
  (adversarial review 2026-10-17).
- **STORE-15: the composition root takes an exclusive `<campaign>.lock` and enables
  the startup backup by default.** -- STORE-3's one-process rule was documentation
  only; two writers would keep divergent in-memory working sets even with SQLite
  serializing writes. `--no-backup` opts out, and a failed startup backup aborts the
  open rather than risking an unmigrated file. -- Accepted (server RFC review,
  2026-10-20).

## Content and reference data

Design and rationale: `docs/engine-architecture.md` sections 6 and 11.

- **CONTENT-1: vendor SRD 5.1 as an Open5e `wotc-srd` snapshot, pinned by commit and
  SHA-256, redistributed under WotC's CC-BY-4.0 with attribution surfaced in the app.**
  -- Verified scale (1,016 records) and field coverage; the alternatives are OGL-murky
  (dnd5eapi, archived) or a brittle PDF parser. Re-pinning is a review event. -- Accepted.
- **CONTENT-2: ingestion and serving are separate, provider-neutral layers; importers
  emit a common snapshot format and hand-authored packs normalize into the same model
  (CONTENT-8); `ReferenceSource` is the only read port.** -- PDF, web, other databases
  and custom formats are future `Importer`s, not engine changes; `ext` carries unmapped
  fields verbatim. -- Accepted.
- **CONTENT-3: namespaces are source-scoped (`srd2014`, `pack.<id>`); slugs come from the
  source where stable, collisions are converter errors, and renames go through an
  `aliases.json` one-hop table; campaigns are never rewritten automatically.** -- Refs are
  the join key forever; alias resolution keeps old sheets working. -- Accepted.
- **CONTENT-4: campaign files keep refs + name snapshots; unresolved refs warn and freeze
  is explicit.** -- RFC-0001 DATA-7 plus a real escape hatch; silent mechanics loss is
  prevented by the warning. -- Accepted (RFC-0001 9.3 item 2, closed).
- **CONTENT-5: autocomplete is an in-memory ranked prefix index with deterministic tiers;
  FTS5, fuzzy and trigram search are deferred behind the port.** -- 1,016 entries,
  microsecond lookups, integer/string ranking only; description search ships as substring
  with the same budget; `just bench` guards the 10,000-entry, 10 ms budget. -- Accepted.
- **CONTENT-6: (superseded by CONTENT-9) the snapshot is embedded in the binary,
  parsed once; packs are files loaded at startup; no runtime network.** -- Single
  binary, offline play, no cache invalidation. -- Superseded 2026-10-17: runtime
  source directories replace embedding (CONTENT-9); offline play is kept because
  local sources are always sufficient.
- **CONTENT-7: lossiness is explicit: `needs_review` + `review_notes` per entry, a
  committed import report, and unmapped fields under `ext`.** -- Fidelity is auditable
  and re-minable when the mapping improves. -- Accepted.
- **CONTENT-8: hand-authored packs are TOML + frontmatter; importers emit snapshot JSON;
  the loader normalizes both into `ContentEntry`.** -- Maintainer decision (2026-10-17):
  packs are written and read by hand, so they get the human-friendlier format; the two
  formats meet in one in-memory model, so the port and the engine cannot tell them apart.
  Exact file layout is pinned in the packs unit. -- Accepted.
- **CONTENT-9: runtime sources replace embedding.** The app loads converted snapshot
  directories at startup, configured by CLI paths with a filesystem default; nothing is
  compiled in (`include_bytes!`/`OnceLock` from CONTENT-6 are out). The repository still
  commits the SRD snapshot as data under `crates/localtt-adapter/data/srd2014/` (fixture
  terms in its `README.md`), and `SHIPPED_DATA_ROOT` names the in-checkout root for tests
  and the workspace default; the installed default and flags are U7. -- Data is
  user-supplied and can be swapped or re-pinned without a rebuild; the single-binary story
  is consciously relaxed to "binary + user-configured data sources". -- Accepted (RFC
  `docs/rfcs/content.md`; pre-spec decision 2026-10-17; source model corrected 2026-10-18).
- **CONTENT-10: snapshots are an index + lazy JSONL directory.** One namespace directory
  per source: `index.json` (version, provenance, name/offset rows, written last as the
  completeness marker), `entries.jsonl` (bodies read lazily by offset), optional
  `aliases.json` and `report.json`; writes are deterministic (sorted entries, temp +
  rename). -- "Read lazily from disk at runtime once a link is followed" (maintainer);
  names and aliases are small enough to hold eagerly. -- Accepted (RFC section 5).
- **CONTENT-11: `ReferenceSource` is fallible; `Ok(None)` stays distinct from failure.**
  All read methods return `Result<_, ContentFault>`: `Ok(None)` is "no such entry" (a
  warning later, CONTENT-4), `Err` is a broken source. Reads are strict: a body that
  fails to parse fails `get` and `search`/`autocomplete` alike; skip-and-diagnose is
  deferred. -- A lazy disk source can fail mid-read; the engine must not conflate
  user-visible absence with infrastructure failure. -- Accepted (RFC section 5; review
  2026-10-17 pinned the strict-reads half).
- **CONTENT-12: direct async REST is in scope; the unit port stays local and blocking.**
  At runtime an async backend dispatcher (`load_link_from_ref`-style) routes a link load
  by namespace to the correct backend, and a backend may be a live RESTful server. The
  local snapshot source (CONTENT-9/10) is one backend behind the blocking
  `ReferenceSource`; the dispatcher, a REST backend and any remote search are later
  units, and `SnapshotReader` is the local reader seam, not a remote one. -- The runtime
  needs live sources without forcing offline play through the network; the frozen U2
  target test pins the blocking local API. -- Accepted (maintainer note, review
  2026-10-17).
- **CONTENT-13: payload evolution is serde-additive.** The engine payload types are the
  content wire contract: added struct fields must be `Option<T>` + `#[serde(default)]`
  (or carry a documented `Default`) and need no version bump; new enum variants,
  renames, removals and changed field types are incompatible (DATA-6: unknown enum tags
  are errors) and bump `CURRENT_SNAPSHOT_VERSION`, so the gate refuses the mismatch at
  open instead of a lazy `Corrupt` on `get`. Snapshots are read-only at runtime, so an
  ignored unknown field never loses data. -- Resolves the "widening is additive" vs
  "version bumps on shape change" contradiction (review S2). -- Accepted (adversarial
  review 2026-10-17).
- **CONTENT-14: data-root discovery policy.** At least one root is required
  (`Unavailable` otherwise); a namespace directory's name must equal its
  `provenance.namespace` (`Corrupt` otherwise); non-namespace names and backup names
  (`*.bak`, `*.tmp`, `*.old`) are pruned silently; a namespace directory missing
  `index.json`/`entries.jsonl` is fatal `Unavailable`; `index.json` is gated before
  `entries.jsonl` presence is checked; sources are immutable per process, so swapping or
  re-pinning data requires a restart. -- Directory scanning is the weak point of a
  copy-able data directory: a stray backup must never load or block startup, a genuine
  mismatch must fail loudly, and a live rewrite must not be observed as a torn pair
  (review S3/N2/N7). -- Accepted (adversarial review 2026-10-17).
- **CONTENT-15: aliases are namespace-scoped and live-first.** An alias must be in its
  snapshot's own namespace (a foreign-namespace alias is `Corrupt`), the existing
  validation rules stay (no live-id collision, no duplicate, live target in the same
  snapshot, no chains), and `get` checks the live id before the alias. -- A user-supplied
  source must not be able to shadow another namespace's ids through its alias table
  (review S4). -- Accepted (adversarial review 2026-10-17).

## Builders and test ergonomics

- **BLD-1: `StatblockBuilder::build` is fallible and requires `abilities` + `hp`.**
  -- It is the mechanism that enforces DATA-10. -- Accepted.
- **BLD-2: `build_entity` finishes header + body in one call; `entity_builder`
  returns the core `EntityBuilder` for header control.** -- A one-expression
  common path, with an escape hatch for `parent`/`acl`/`rev`/timestamps without
  duplicating the header setters. -- Accepted.
- **BLD-3: `*_with_id` builder variants exist for deterministic element ids.** --
  Fixtures must serialize byte-for-byte, so element ids are supplied explicitly
  rather than reseeded after the fact. -- Accepted.
- **BLD-4: fixtures use fixed UUIDs and fixed times.** -- Makes snapshot tests and
  round-trip diffing possible; fresh ids only where determinism does not matter. --
  Accepted.
- **BLD-5: `give_handout` returns `Option<InventoryEntryId>`.** -- A text handout
  genuinely has no inventory effect; returning an invented unused id was a lie. --
  Accepted.

## Process and tooling

- **PROC-1: `just` is the task runner; `just test`/`just lint`/`just check` are
  the gate, and `cargo` is not invoked directly for those.** -- One place to know
  the full matrix (including miri/loom/shuttle). -- Accepted.
- **PROC-2: no mutation testing yet; no `insta` snapshot tests yet.** -- The repo
  has no mutation-testing recipe, and snapshots are not worth diffing before the
  engine lands. -- Deferred.
- **PROC-3: miri covers `localtt-core` and `localtt-engine` only.** --
  `rusqlite` is C FFI and file-backed store tests need a real filesystem; miri
  can execute neither, so the SQLite store and its tests stay covered by the
  normal, sanitizer and coverage runs instead. -- Accepted.

## Deferred and open

- **`AnyEntity` / `InventoryItem::Inline` boxing** -- public shape change, deferred
  (carries an `#[expect(clippy::large_enum_variant)]`).
- **`Entity::validate` split** -- so a loader can name a kind/body mismatch as a
  typed error instead of a serde string.
- **`#[non_exhaustive]` pass** on payload enums before 1.0 / crate extraction.
- **`Audience` as a set** -- "DM reveals to one player"; needs a feed-document
  migration.
- **`StatusActive` by ref** in addition to name; **`weight_oz`** pounds input
  sugar; **`ViewField`** finer granularity; per-type **`ext`**.
- **Phase E**: engine rules, the `view_of` authorization matrix, a deterministic
  `Rng` fixture, and roll/feed tests. -- Plan and unit order:
  `docs/engine-architecture.md` section 10 (U3-U6); still open.
- **`fixtures/campaigns/v1.0/`** (the serialized party bundle plus its
  regenerate-and-compare test) -- RFC goal 6 and section 6.4 list it as a U1
  deliverable, but it has no consumer until the first document migration lands, so
  v1.0 ships without it (the `STORE-2` "fixtures per version" clause is
  unhonoured).
- **`i64` bound on `feed_appends` ids and entity `rev`** -- `MemoryStore` accepts a
  `u64` beyond `i64::MAX` where `SqliteStore` returns `Backend`; engine tests run
  on `MemoryStore`, so the divergence is invisible until production. Close it with
  a shared `InvalidPatch` precondition (RFC section 5.3) or document the SQLite
  bound.
- **Runtime data-root packaging** (CONTENT-9/S6) -- the server RFC pins `--data` and
  a platform data-dir default (`campaigns/`, `reference/`, `backups/`); how
  `crates/localtt-adapter/data/` is installed alongside an installed binary remains
  open. `SHIPPED_DATA_ROOT` stays build-time. U7.
- **Async backend dispatcher, live REST backend and remote search** (CONTENT-12) --
  a later unit; the local blocking port stays unchanged.
- **Skip-and-diagnose search over unreadable bodies** (CONTENT-11/N8) -- deferred with
  the operator diagnostics story.
- **Backup file permissions** -- SQLite creates the destination `0644`, widening a
  deliberately private `0600` campaign; copy the source mode on Unix.
