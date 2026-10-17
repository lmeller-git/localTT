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
- **STORE-6: byte-for-byte document fidelity is a contract-test promise, not an
  aspiration.** -- Determinism guarantees (DATA-4/5) make it meaningful; the store
  contract suite asserts it for every fixture document. -- Accepted.
- **STORE-7: backup and `sync` are a capability trait (`StoreBackup`) the engine owns and
  never calls.** -- The engine's store dataflow stays `load` + `apply`; the composition root
  and tests use the capability, and file-backed stores provide the mechanism (`SqliteStore`
  via `rusqlite::backup`) behind a backend-neutral contract, so neither engine nor core
  contains backend concepts. The contract suite tests the trait per implementation; startup
  policy and `--no-backup` stay composition-root concerns (U7). -- Accepted (review
  2026-10-17).

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
- **CONTENT-6: the snapshot is embedded in the binary, parsed once; packs are files
  loaded at startup; no runtime network.** -- Single binary, offline play, no cache
  invalidation. -- Accepted.
- **CONTENT-7: lossiness is explicit: `needs_review` + `review_notes` per entry, a
  committed import report, and unmapped fields under `ext`.** -- Fidelity is auditable
  and re-minable when the mapping improves. -- Accepted.
- **CONTENT-8: hand-authored packs are TOML + frontmatter; importers emit snapshot JSON;
  the loader normalizes both into `ContentEntry`.** -- Maintainer decision (2026-10-17):
  packs are written and read by hand, so they get the human-friendlier format; the two
  formats meet in one in-memory model, so the port and the engine cannot tell them apart.
  Exact file layout is pinned in the packs unit. -- Accepted.

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
