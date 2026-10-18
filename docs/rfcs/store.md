# RFC: U1 -- the campaign store

| | |
|---|---|
| **Status** | Implemented (U1): the port, both backends and their suites are green; this RFC stays the unit's spec record. Spec-phase artifacts (this RFC and the target test) plus the review resolutions in sections 8 and 9 are folded into the sketches. |
| **Scope** | `localtt-engine::ports::store`, `localtt-engine::export`, `localtt-adapter::store` (`memory`, `sqlite`, `backup`, `export`), the target test `crates/localtt-integration/tests/store_target.rs`, the contract suite `crates/localtt-adapter/tests/store_contract.rs`. |
| **Read with** | `docs/engine-architecture.md` section 4.2 (port sketch), section 5 (storage interaction model), section 10 (U1); `docs/decisions.md` STORE-1..13, VER-2/3, ARCH-3/6, ENG-1; `crates/localtt-integration/DESIGN.md` (the fixture cast). |
| **Pins** | The `docs/engine-architecture.md` sketches are explicitly "not compiling Rust"; this RFC pins the store subset of them. Deviations from the approved sketch are marked and collected in sections 8 and 9. |

## 1. Goals

1. Pin the store port and its data transfer types so the engine (U3) and the
   composition root (U7) can be written against them: `CampaignStore` +
   `CampaignSnapshot`, `StatePatch`, `StoreFault` in `engine::ports::store`,
   implemented by the adapter.
2. Pin the two implementations behind that port: `MemoryStore` (engine tests,
   contract suite) and `SqliteStore` (the shipped one).
3. Pin export/import: `engine::export::ExportBundle`, plus the gated decode and
   the import workflow (RFC-0001 5.9, STORE-5), generic over the store port.
4. Pin backup and durability flushing as the engine-owned capability trait
   `StoreBackup` that the engine never calls: adapters implement it, the
   composition root and tests use it (review 2026-10-17).
5. Pin the guarantees the target test asserts: byte fidelity, snapshot ordering,
   per-patch atomicity, the version gates, and the empty-store behavior.
6. Match the implement-phase checklist of `docs/engine-architecture.md` section
   10 U1: DDL, extracted columns, WAL, backup, export/import, contract suite,
   `fixtures/campaigns/v1.0/`.

The target test is written first and lives at
`crates/localtt-integration/tests/store_target.rs`; section 3 is its map.

## 2. Non-goals

- **No lazy loading.** Load-all is the U1 working set (ENG-3); the port has no
  `load_entity`, no LRU, no attachments (design section 5.4).
- **No ACL projection, no feed-audience projection.** STORE-1 extracts only the
  documented columns; anything else waits for a query that needs it.
- **No merge/append import.** Import targets an empty store, all-or-nothing
  (STORE-5).
- **No multi-process access.** One process per campaign file, documented
  (STORE-3); SQLite locking plus `busy_timeout` is the accidental safety net.
- **No DDL v2 or document migrations.** The registries exist (empty) so the
  first real migration has a home (STORE-2); the unit only proves the gate and
  the hook shape, using a test-only migration.
- **No server wiring, no engine executor.** Startup backup policy, `--no-backup`
  and the binary are U7; command application is U3.

## 3. Client usage: the target test

`crates/localtt-integration/tests/store_target.rs` is ordinary client code: it
builds the golden party (`fixtures::party()`), turns it into a `StatePatch`,
drives both stores, and inspects the results. Seven tests:

| Test | What it pins |
|---|---|
| `memory_store_round_trips_the_party_campaign` | creation patch, load, byte fidelity, snapshot ordering |
| `sqlite_store_reopens_and_backs_up` | reopen durability, `StoreBackup::{sync, backup}`, backup is a readable store |
| `empty_stores_report_no_campaign` | `StoreFault::NoCampaign` on both implementations |
| `newer_schema_major_is_refused` | the load-time gate: a future major is refused, not partially loaded |
| `corrupt_store_file_is_refused` | a non-store file is `StoreFault::Corrupt`, not a panic |
| `failing_patch_applies_nothing` | patch preconditions + atomicity: a rejected patch leaves no trace |
| `export_bundle_round_trips_into_a_fresh_store` | gated decode, import into empty, re-export modulo `exported_at`, import-into-nonempty refused |

Notes:

- **Where it lives and why.** `localtt-integration` is the designated API
  playground and owns the deterministic fixture cast. The adapter crate cannot
  host it: `localtt-integration` already dev-depends on `localtt-adapter`, and a
  dev-dependency cycle in the other direction is not something to buy.
- **It does not compile yet, on purpose.** `engine::ports::store`,
  `engine::export` and `adapter::store` do not exist. The skeleton phase adds the
  declarations and the dev-dependencies (`tempfile` here; `rusqlite` with
  `bundled`, `tempfile` in the adapter) via `cargo add`, after which every test
  fails with `not yet implemented` until the implement phase.
- The test does not exercise the raw SQLite file; everything goes through the
  pinned API. Raw-DDL and PRAGMA checks belong to the contract suite.

## 4. Public API sketch

Sketches, not compilable code; signatures are the contract. `#[non_exhaustive]`
enums are matched by consumers with `..`.

### 4.1 `localtt_engine::ports::store`

```rust
/// Everything the store holds, materialized; startup and export use this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignSnapshot {
    pub campaign: Campaign,
    /// Ascending `EntityId` (UUIDv7 byte order).
    pub entities: Vec<AnyEntity>,
    /// Ascending `FeedEntryId`.
    pub feed: Vec<FeedEntry>,
}

/// One command's durable effect, applied in a single transaction.
/// Constructed by the engine; the store treats the documents opaquely.
#[derive(Debug)]
pub struct StatePatch<'a> {
    /// Present when the campaign document changed (clock, settings, players).
    pub campaign: Option<&'a Campaign>,
    /// Full new revisions of touched entities (created or updated).
    pub upserts: Vec<&'a AnyEntity>,
    pub removals: Vec<EntityId>,
    /// Feed entries to append, strictly ascending id.
    pub feed_appends: Vec<&'a FeedEntry>,
    /// When set, drop stored feed entries with `id < keep_from` (ring
    /// trimming); appends below the cutoff are rejected (section 5.3).
    pub feed_keep_from: Option<FeedEntryId>,
}

/// Failure of any store operation. `message` fields are diagnostic, not
/// user-facing; the server maps variants to status codes and must not forward
/// the messages (N11).
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum StoreFault {
    /// The store holds no campaign yet; create one first.
    NoCampaign,
    /// An empty-store operation (import) found a campaign already stored; a
    /// user-facing refusal, unlike `InvalidPatch` (B2; section 9).
    StoreNotEmpty,
    /// A document's major version is newer than this build supports (VER-2).
    VersionRefused { found: SchemaVersion, supported: SchemaVersion },
    /// The file is not a localtt store, a document did not parse, or a bundle
    /// envelope has a foreign `format`.
    Corrupt { message: String },
    /// The store file's DDL version is newer than this build knows;
    /// `user_version` is signed (N7; section 9).
    DdlVersionRefused { found: i32, supported: i32 },
    /// A document migration failed (B1; section 9).
    DocumentMigration { from: SchemaVersion, to: SchemaVersion, message: String },
    /// A DDL migration step failed (B1; section 9).
    DdlMigration { from: i32, to: i32, message: String },
    /// A bundle's `format_version` is newer than this build supports; the file
    /// is not corrupt, the build is old (S2; section 9).
    FormatRefused { found: u32, supported: u32 },
    /// Advisory/IO failure (missing directory, permissions, disk full, locked).
    Unavailable { message: String },
    /// Any other backend failure.
    Backend { message: String },
    /// The operation was refused before writing: a documented patch
    /// precondition failed (section 5.3). Nothing was applied.
    InvalidPatch { message: String },
}

/// The only persistence abstraction the engine knows.
///
/// Implementations are single-writer, synchronous and may block; the
/// composition root runs them behind the serialized command executor.
pub trait CampaignStore: Send {
    /// Loads the whole campaign. Called once at startup and by export.
    fn load(&self) -> Result<CampaignSnapshot, StoreFault>;

    /// Applies a patch in one transaction: all-or-nothing. Called before the
    /// engine publishes anything.
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;
}

/// Durable self-copy and durability flushing for a store implementation.
///
/// Separate from `CampaignStore`: the engine never calls these methods (its
/// dataflow is `load` + `apply`). The composition root and tests hold a
/// concrete store and use the capability. A file-backed implementation writes
/// a self-contained, consistent snapshot using the backend's own snapshot
/// mechanism, never a copy of live files (S7); a store with no file does not
/// implement the trait.
pub trait StoreBackup: Send {
    /// Writes a self-contained copy of the store's current contents to `to`.
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault>;

    /// Forces durability; tests and diagnostics.
    fn sync(&mut self) -> Result<(), StoreFault>;
}
```

Differences from the approved sketch (design section 4.2), settled in review
(2026-10-17):

- **Placement**: the port lives in `localtt-engine` (the engine owns its ports;
  the earlier `core::ports` placement rested on a false dependency rule). `core`
  gains nothing; see `docs/decisions.md` ARCH-3/ENG-1.
- `InvalidPatch` and `DdlVersionRefused` were additions in the first review
  (section 8); the adversarial review split `Migration` and added
  `StoreNotEmpty` and `FormatRefused` (section 9).
- `backup`/`sync` are not on `CampaignStore`; they are the separate capability
  trait `StoreBackup` in the same module. The engine owns the interface (all
  ports are engine-owned) but never calls it; adapters implement it (section
  4.3), and neither the engine nor `core` contains backend concepts (sections 8
  and 9).
- `StatePatch` and `CampaignSnapshot` keep public fields: they are
  workspace-internal DTOs and their literals are the constructors, so adding a
  field is a deliberate breaking change across the workspace, not an accident
  (N5). `#[non_exhaustive]` is not an option because the target test constructs
  `StatePatch` by literal.

### 4.2 `localtt_engine::export`

```rust
/// Versioned envelope for sharing a campaign as JSON (RFC-0001 5.9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExportBundle {
    /// Always `ExportBundle::FORMAT`.
    pub format: String,
    /// Envelope version, independent of document `schema_version`.
    pub format_version: u32,
    /// Excluded from round-trip comparisons (design section 5.1).
    pub exported_at: UnixTime,
    pub campaign: Campaign,
    /// Ascending `EntityId`.
    pub entities: Vec<AnyEntity>,
    /// Ascending `FeedEntryId`.
    pub feed: Vec<FeedEntry>,
}

impl ExportBundle {
    pub const FORMAT: &str = "localtt.campaign";
    pub const CURRENT_FORMAT_VERSION: u32 = 1;

    /// Wraps a loaded snapshot; the documents are cloned, not transformed.
    pub fn from_snapshot(snapshot: &CampaignSnapshot, exported_at: UnixTime) -> Self;
}
```

`ExportBundle` derives `Serialize` only (S1): writing a bundle is plain
`serde_json` serialization, and reading is `decode_bundle`, which parses an
adapter-private wire type and applies the gates before constructing the engine
envelope. There is no derived `Deserialize`, so the un-gated parse path cannot
be reached by accident.

### 4.3 `localtt_adapter::store`

```rust
// localtt_adapter::store::memory

#[derive(Debug, Default)]
pub struct MemoryStore { /* ... */ }

impl MemoryStore {
    pub fn new() -> Self;
}

impl CampaignStore for MemoryStore {
    fn load(&self) -> Result<CampaignSnapshot, StoreFault>;
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;
}

// localtt_adapter::store::sqlite

#[derive(Debug)]
pub struct SqliteStore { /* ... */ }

/// Opening options; `Default` is what `open` uses. `#[non_exhaustive]` keeps
/// later options additive (N6).
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct SqliteOptions {
    /// Online startup backup before DDL migrations (design section 5.7);
    /// skipped for a fresh file. `--no-backup` maps to `false` (U7).
    pub startup_backup: bool,
}

impl SqliteStore {
    /// Opens the campaign file, creating it when missing or zero-length: sets
    /// the connection pragmas outside any transaction, verifies file identity
    /// (`application_id` magic; a foreign database is `Corrupt`; S10), the DDL
    /// version (refusing a newer one) and that the version's tables are present,
    /// takes an atomic startup backup when requested, then applies pending DDL
    /// migrations in one transaction (S8).
    pub fn open(path: &Path) -> Result<Self, StoreFault>;
    pub fn open_with(path: &Path, options: SqliteOptions) -> Result<Self, StoreFault>;
}

impl CampaignStore for SqliteStore {
    fn load(&self) -> Result<CampaignSnapshot, StoreFault>;
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;
}

// localtt_adapter::store::backup

// `impl StoreBackup for SqliteStore` (the trait is engine-owned; section 4.1):
// `backup` captures the store through the backend's own snapshot mechanism
// (never a copy of live files); `sync` flushes WAL writes to durable storage.
// `open_with`'s startup backup and rotation use the same mechanism, written to
// a `.tmp` name and renamed atomically (S9; design section 5.7).
impl StoreBackup for SqliteStore {
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault>;
    fn sync(&mut self) -> Result<(), StoreFault>;
}

// localtt_adapter::store::export

/// Decodes a bundle text: envelope format/version, then every document through
/// the same schema gate as `load` (same-or-older major accepted and re-stamped,
/// newer refused). A foreign `format` is `Corrupt`; a newer `format_version` is
/// `FormatRefused` (S2). Parses an adapter-private wire type; `ExportBundle`
/// deliberately does not derive `Deserialize` (S1).
pub fn decode_bundle(text: &str) -> Result<ExportBundle, StoreFault>;

/// Imports a decoded bundle into an empty store: one `apply`, all-or-nothing.
/// A store that already holds a campaign is refused with `StoreNotEmpty` (B2);
/// the emptiness check matches `Err(NoCampaign)` from `load` and lets every
/// other fault through (N10).
pub fn import_into(store: &mut dyn CampaignStore, bundle: &ExportBundle) -> Result<(), StoreFault>;
```

Both entry points are generic over the store port, mirroring the backup capability:
`import_into` takes any `&mut dyn CampaignStore`, so a bundle restores into the SQLite
store, the memory store, or a future backend with no adapter-specific path. Decode is
format work (one envelope, one gate); import is dataflow (one `apply` through the port),
and neither is tied to SQLite.

A bundle written by the server uses plain `serde_json` serialization of
`ExportBundle` (a `Serialize`-only type; serialization cannot fail a gate). Only
decode is special, because only decode needs the version gate.

## 5. Guarantees

### 5.1 Byte fidelity

`apply` stores `serde_json::to_string` of each typed document in its `doc`
column. Serialization of the typed documents is total (no fallible custom
`Serialize` impls), so this step cannot fail in practice; a hypothetical failure
surfaces as `Backend`, never `InvalidPatch`. For any document at
`schema_version = CURRENT` -- and `apply` only ever receives those -- the bytes
a client gets back by re-serializing `load`'s output are byte-identical to the
document that was applied: campaign, every entity, every feed entry (STORE-6,
design section 5.1). Export -> import -> export is byte-identical modulo
`exported_at`.

Stored bytes are not preserved verbatim for documents that did not come through
`apply`: `load` parses through the gate, and the core types re-stamp at
`CURRENT` and drop unknown fields (`Deserialize` behavior, unchanged), so a
hand-edited or older-minor `doc` is canonicalized on load. The contract suite
therefore asserts canonical-apply -> canonical-load, which is the promise that
matters (S14).

### 5.2 Ordering

`CampaignSnapshot::entities` is ascending by `EntityId`, `feed` ascending by
`FeedEntryId`, regardless of the order patches supplied. Both implementations
sort on load (SQL `ORDER BY`, memory `BTreeMap`/sorted walk). The target test
asserts this; callers may rely on it for byte-stable exports. The contract suite
supplies shuffled insertion order to prove the sort: the target test's fixture
is already ascending, so it cannot (N3).

### 5.3 Atomicity and apply preconditions

One `apply` is one transaction in both implementations: either every upsert,
removal, campaign update and feed append lands, or none does.

`apply` validates before writing and rejects, with `InvalidPatch`, a patch that
violates a documented precondition:

1. an entity id appears twice in `upserts`;
2. an entity id appears in both `upserts` and `removals`;
3. `removals` names an id the store does not hold;
4. `feed_appends` is not strictly ascending by id;
5. a `feed_appends` id is already stored -- the feed is append-only, never
   upserted. (S12's case, a stored id that `feed_keep_from` would trim in the
   same patch, is refused by 6 before this state check runs; the fault and its
   "nothing applied" effect are identical, only the diagnostic names the
   cutoff.)
6. an appended id is below `feed_keep_from` (S11): the engine computes the
   cutoff from its own state, so this is divergence, not user input;
7. `campaign` is `Some` and carries a different id than the stored campaign;
8. no campaign is stored, the patch does not create one, and it carries any
   upserts, removals or feed appends -- entities live inside a campaign (S13).

Campaign semantics are explicit (S13): `campaign: Some` on an empty store
creates the campaign; `Some` with the stored id updates it; `Some` with a
different id is rejected by 7. The campaign table never holds a second row: the
preconditions are the singleton guard, so the campaign always goes through the
same validate-then-write path. An empty patch is a valid no-op.

`feed_keep_from` trimming runs before appends are inserted: stored entries with
`id < keep_from` are dropped, and appended entries cannot be below the cutoff
(6). `rev` monotonicity is not a store precondition: the engine asserts it
against in-memory state before producing the patch (design section 5.5, ENG-4).

A violation means state divergence between engine and store (single writer, so
the engine is the only possible source): fail fast, write nothing. `SqliteStore`
validates inside its `BEGIN IMMEDIATE` transaction, so the preconditions and the
write are atomic and a second handle cannot slip a conflicting patch between
them; `MemoryStore` validates on a copy and swaps only on success, so neither can
half-apply. A backend failure inside the transaction (for example a disk error)
rolls back in SQLite.

### 5.4 Durability and visibility

`apply` returns only after commit. The engine treats durable-before-visible as
its invariant (ENG-2): if `apply` fails, engine state is unchanged. A process
crash after commit but before the caller publishes anything is safe: the next
`load` returns the committed file. `load` reads the campaign, the entities and
the feed in one read transaction, so a concurrent commit cannot tear the
snapshot. SQLite runs WAL + `synchronous = NORMAL`, so a power loss can lose the
last committed command but cannot corrupt the file (STORE-3).
`StoreBackup::sync` forces a checkpoint/fsync and reports a checkpoint that could
not complete (a reader holds the WAL) as `Unavailable`, never as success; the
tests use it before backup copies.

### 5.5 Version gates

- **Documents**: `load` reads the document's own `schema_version` before
  parsing anything (VER-2); the extracted version columns are a query
  projection and are never the gate, so a stale column cannot hide a newer
  document. A newer major is `VersionRefused { found,
  supported: SchemaVersion::CURRENT }`. A same-or-older major loads; the parse
  re-stamps at `CURRENT` (existing core behavior, unchanged).
- **DDL**: `open` refuses a `PRAGMA user_version` (signed `i32`) newer than the
  build knows (`DdlVersionRefused`) and otherwise applies pending migrations in
  one transaction, after the connection pragmas and the startup backup (S8). A
  magic-carrying file whose tables do not match its `user_version` -- a dropped
  table, or tables present at version 0 -- is `Corrupt`, not `Unavailable`: it is
  damaged or tampered, not temporarily unreadable (S10).
- **Bundles**: `decode_bundle` checks `format` (foreign -> `Corrupt`) and
  `format_version` (newer -> `FormatRefused`; `CURRENT_FORMAT_VERSION` is 1 and
  there is no older version to accept yet), then runs every document through
  the same gate as `load` before parsing it.

`apply` itself is deliberately not gated: it receives typed documents the engine
just built at `CURRENT` in-process, and the gate exists for documents entering
from outside (campaign files and bundles). The target test uses it to fabricate
a future-major store and the contract suite may do the same with raw SQL; that
is not a license for server code to skip the gate on load (N4).

### 5.6 Error model

`StoreFault` (section 4.1) is `#[non_exhaustive]`, `Clone + Debug + Error`.
Messages are diagnostic only: the U7 server maps variants to status codes and
must not forward `message` text (it can carry paths, SQL and IO detail) (N11).
The engine maps faults into its own error type in U3.

### 5.7 Threading and progress

Both stores are `Send`, not `Sync`: one owner (the executor) mutates them. All
calls are blocking; there is no lock-free or wait-free claim. `MemoryStore` is
infallible except for validation and the load gate; `SqliteStore` blocks on IO
and on SQLite locks bounded by `busy_timeout = 5000` ms, surfacing `Unavailable`
on timeout.

## 6. Internal structure

### 6.1 `adapter::store::sqlite`

- One `rusqlite::Connection`, prepared statements cached; the DDL of design
  section 5.2 verbatim (`campaign`, `entities` + two indexes, `feed`).
- `open` sets `busy_timeout`, `PRAGMA journal_mode = WAL`,
  `PRAGMA synchronous = NORMAL` and `PRAGMA foreign_keys = ON` **outside** any
  transaction (`journal_mode` cannot change inside one), then verifies file
  identity and DDL version, takes the startup backup, and only then runs DDL
  migrations plus `PRAGMA user_version = N` inside one transaction (S8).
- File identity: creation sets `PRAGMA application_id` to a fixed localtt magic
  value; `open` refuses a present file with a different magic as `Corrupt`. A
  missing or zero-length file is fresh; `user_version = 0` with the magic and no
  tables is a fresh store mid-initialization (S10). `open` then verifies the
  tables its `user_version` promises actually exist: the v1 migration runs in one
  transaction, so a v1 file missing a table, or a v0 file holding tables, cannot
  be a crash artifact and is `Corrupt`.
- `apply` extracts the columns from the typed documents and runs the writes in
  one `BEGIN IMMEDIATE` transaction.
- `schema.rs` owns `PRAGMA user_version` and an ordered migration list (one
  entry today: create v1); `fn(&Connection) -> Result<(), StoreFault>` steps
  report `DdlMigration` on failure. `doc_migrations.rs` owns the empty
  `(major, minor) -> fn(Value) -> Value` registry applied after the gate and
  before parsing.
- `StoreBackup::backup` and `open_with`'s startup backup capture the store
  through the backend's own snapshot mechanism, write to
  `<stem>.backup-<unix>.sqlite.tmp`, then rename to
  `<stem>.backup-<unix>.sqlite`; rotation keeps the five newest complete files
  and never counts a `.tmp` (S9). A same-second repeat replaces its own target;
  startup takes at most one backup per open. Rotation is best-effort: it runs
  after the backup has landed, so an unlistable directory or an entry that cannot
  be pruned (a directory named like a backup) is skipped rather than failing the
  open.

### 6.2 `adapter::store::memory`

A plain `CampaignSnapshot` holder, preserved insertion-order-free (maps keyed by
id). It implements the same preconditions and the same load gate so the contract
suite can run one suite against both. It is the store used by engine tests in
U3+; it has no file and does not implement `StoreBackup`, which is scoped as a
capability, not an engine-consumed port (S6).

### 6.3 Contract suite

The "every port ships an in-memory implementation plus a contract suite" rule
applies to the ports the engine consumes (`CampaignStore` here; `Rng` and
`ReferenceSource` in their units). `StoreBackup` is a capability interface: it
is contract-tested against file-backed stores only, and `MemoryStore`
deliberately does not implement it (S6).

`crates/localtt-adapter/tests/store_contract.rs` runs a shared suite over
`&mut dyn CampaignStore` (via a small harness) for `MemoryStore` and
`SqliteStore` in a tempdir, plus a SQLite-only block for `open`/`backup`/`sync`
and DDL checks. Fixtures are adapter-local, built through `localtt_core::builders`
(one entity per `BodyKind`, plus the campaign and feed): the integration golden
cast is not reachable from the adapter without a dev-dependency cycle, so the
fidelity promise here is per builder fixture, and the full golden-cast check
lives in the target test (S3/S4).

- load/apply/load byte fidelity for every local fixture document;
- ordering guarantees under shuffled patch order (the target test's fixtures are
  pre-sorted, so only the suite proves the sort) (N3);
- every apply precondition of section 5.3 -> `InvalidPatch`, nothing applied,
  including the campaign-singleton matrix and the append/trim rules;
- version gate: newer document major refused, same/older accepted; newer DDL
  refused; newer bundle `format_version` -> `FormatRefused`, foreign `format` ->
  `Corrupt`;
- import: an empty store accepts, a non-empty store is `StoreNotEmpty`;
- reopen equals prior state; `StoreBackup::backup` opens and equals the source,
  and rotation leaves only complete files (SQLite-only block);
- file identity: a foreign valid SQLite file (no magic) is `Corrupt`; a fresh or
  zero-length file opens (SQLite-only block);
- `NoCampaign` on both implementations; `Corrupt` on a non-store file.

The document-migration hook is a `#[cfg(test)]` unit test inside
`doc_migrations.rs`: the registry is `pub(crate)`, so the external suite cannot
register a test migration, and a unit test needs no public test seam (S3).

### 6.4 Fixtures and migration homes

- `fixtures/campaigns/v<major>.<minor>/` (root, per the review answer to Q4)
  gets the serialized party as a bundle; U1 commits `v1.0/` plus its
  regenerate-and-compare test, so future document migrations have real input.
- The store reads only through `engine::ports::store`; nothing adapter-internal
  leaks into engine or server code.

### 6.5 Dependencies and layout

- `localtt-engine`: plus `serde` (the `ExportBundle` envelope derives it) and `thiserror`
  (`StoreFault`); `localtt-core` gains nothing.
- `localtt-adapter`: plus `localtt-engine` (path; it implements the engine's ports) and
  `rusqlite` (`bundled`), dev plus `tempfile`.
- `localtt-integration`: dev plus `tempfile`.
- Added with `cargo add` in the skeleton phase, never hand-pinned versions.
- Module layout: `crates/localtt-adapter/src/store/{mod.rs, memory.rs,
  sqlite.rs, backup.rs, export.rs, schema.rs, doc_migrations.rs}`; `store` re-exports
  the public items above. `schema` and `doc_migrations` are `pub(crate)`; the external
  contract suite drives raw SQL through its own `rusqlite` dependency when it needs to
  fabricate a version or inspect a file.

## 7. API cross-check against the target test

Every item the test uses, and where it is pinned: `MemoryStore::new` /
`Default` (4.3); `SqliteStore::open` (4.3) and `StoreBackup::{backup, sync}`
(4.1); `CampaignStore::{load, apply}` (4.1); `StatePatch` fields (4.1);
`CampaignSnapshot` fields + ordering (4.1, 5.2); `StoreFault::{NoCampaign,
StoreNotEmpty, VersionRefused, Corrupt, InvalidPatch}` (4.1);
`ExportBundle::from_snapshot` + `exported_at` (4.2);
`export::{decode_bundle, import_into}` (4.3). The sketch elements the test does
not use: `open_with`/`SqliteOptions` and `StatePatch::feed_keep_from` (justified
by design sections 5.3 and 5.7 and exercised by the contract suite), and the
remaining `StoreFault` variants (`DocumentMigration`, `DdlMigration`,
`DdlVersionRefused`, `FormatRefused`, `Unavailable`, `Backend`; used by U3/U7
and the contract suite).

## 8. First review decisions (settled 2026-10-17)

All three questions were answered in review; the sketches above already
incorporate the answers. The original options are kept in brief for the record.

1. **`StoreFault::InvalidPatch` and `DdlVersionRefused`** -> **accepted (option A)**: both
   variants are added. "Refused before writing" and "too-new DDL" stay typed; the server
   maps them in U7, and `Backend` keeps meaning "the backend failed". Rejected: overloading
   `Backend` (a client bug would read as a backend failure) and encoding a DDL number as a
   `SchemaVersion`.
2. **Where backup lives** -> **engine-owned capability trait, adapter-implemented**:
   `StoreBackup` in `engine::ports::store` (section 4.1), separate from `CampaignStore`.
   The engine never calls it (dataflow stays `load` + `apply`); the composition root and
   tests use it; file-backed stores provide the mechanism. No backup or backend concept
   reaches the engine or `core` contracts (U7 policy: startup backup, `--no-backup`).
   Rejected: `backup`/`sync` on `CampaignStore` (a memory store cannot implement file
   semantics) and backend-only inherent methods (not generic, backend-shaped API).
3. **Export/import entry points** -> **accepted (option A), made generic**:
   `ExportBundle::from_snapshot` in `localtt-engine::export`;
   `adapter::store::export::{decode_bundle, import_into}` with `import_into` taking any
   `&mut dyn CampaignStore`, so every implementation imports through the same dataflow and
   nothing is tied to SQLite. Rejected: default methods on the port (moves workflow into
   the engine trait and every mock inherits it).

## 9. Adversarial review resolutions (2026-10-17)

A second, adversarial review pass preceded the skeleton. Every finding below was
resolved before this revision; the sketches incorporate all of them, and the
decisions are recorded as STORE-8..13 in `docs/decisions.md`.

Blockers:

- **B1** `StoreFault::Migration` split into `DocumentMigration` and
  `DdlMigration`: a failed DDL step no longer needs a fabricated
  `SchemaVersion`.
- **B2** `StoreNotEmpty` added for "import into an occupied store", and the
  target test asserts it. `InvalidPatch` stays the engine-bug class (a failed
  section 5.3 precondition), which keeps the U7 mapping (409 vs 500) possible.

Should-fix:

- **S1** `ExportBundle` derives `Serialize` only; `decode_bundle` owns an
  adapter-private wire type. The un-gated parse path is gone.
- **S2** a newer bundle `format_version` is `FormatRefused`; `Corrupt` is for a
  foreign format or a broken file.
- **S3/S4** the contract suite stays in `localtt-adapter/tests` with
  builder-made local fixtures; the migration hook is a `#[cfg(test)]` unit test
  in `doc_migrations.rs`; the golden cast stays in `localtt-integration`.
- **S5** the composition root keeps ownership of the concrete store; the engine
  executor borrows (`&mut dyn CampaignStore`) instead of boxing it (design
  section 7.2 updated).
- **S6** the "in-memory + contract suite" rule is scoped to engine-consumed
  ports; `StoreBackup` is capability-tested against file-backed stores only.
- **S7** `StoreBackup`'s doc is backend-neutral; SQLite details live in adapter
  documentation.
- **S8** connection pragmas run outside any transaction; DDL plus
  `user_version` run inside one.
- **S9** backups are written to a `.tmp` name and renamed; rotation counts
  complete files only.
- **S10** a `PRAGMA application_id` magic distinguishes foreign valid
  databases; missing or zero-length files are fresh.
- **S11/S12/S13** section 5.3 grew preconditions 5-8: feed appends never
  collide with stored ids and never fall below the trim cutoff; the campaign
  singleton is explicit; entities require a campaign.
- **S14** the byte-fidelity claim is stated honestly: canonical documents at
  `CURRENT` round-trip byte-identically; stored bytes are not preserved
  verbatim for hand-edited or older-minor documents.

Nits, all folded: N1 the design `Transition` sketch is fixed to own its
documents (the `'static` borrow could not compile) with a borrowing `patch()`
accessor; N2 the design `StoreFault` is synced with this RFC; N3 the shuffled
order case moved to the contract suite; N4 `apply` stays ungated with the
rationale in section 5.5; N5 `StatePatch` keeps public fields (workspace
DTO; literals are the constructor); N6 `SqliteOptions` is `#[non_exhaustive]`
with `Default`; N7 `i32` for `user_version`; N8 the adapter crate doc is
updated when the engine dependency lands; N9 fixtures use
`fixtures/campaigns/v<major>.<minor>/`; N10 `import_into` matches
`Err(NoCampaign)` specifically; N11 `message` fields are never forwarded;
N12 the `Rng` sketch gains `Send`.
