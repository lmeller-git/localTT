# RFC: U1 -- the campaign store

| | |
|---|---|
| **Status** | Draft. Spec phase: this RFC and its target test are the only artifacts; no library code exists for this unit yet. Review answers (2026-10-17) are folded into the sketches; the questions are closed in section 8. |
| **Scope** | `localtt-engine::ports::store`, `localtt-engine::export`, `localtt-adapter::store` (`memory`, `sqlite`, `backup`, `export`), the target test `crates/localtt-integration/tests/store_target.rs`, the contract suite `crates/localtt-adapter/tests/store_contract.rs`. |
| **Read with** | `docs/engine-architecture.md` section 4.2 (port sketch), section 5 (storage interaction model), section 10 (U1); `docs/decisions.md` STORE-1..7, VER-2/3, ARCH-3/6, ENG-1; `crates/localtt-integration/DESIGN.md` (the fixture cast). |
| **Pins** | The `docs/engine-architecture.md` sketches are explicitly "not compiling Rust"; this RFC pins the store subset of them. Deviations from the approved sketch are marked and collected in section 8. |

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
   `fixtures/campaigns/v1/`.

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
    /// When set, drop feed entries with `id < keep_from` (ring trimming),
    /// existing and appended ones alike.
    pub feed_keep_from: Option<FeedEntryId>,
}

/// Failure of any store operation. `message` fields are diagnostic, not
/// user-facing; the server maps variants to status codes.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum StoreFault {
    /// The store holds no campaign yet; create one first.
    NoCampaign,
    /// A document's major version is newer than this build supports (VER-2).
    VersionRefused { found: SchemaVersion, supported: SchemaVersion },
    /// The file is not a localtt store, a document did not parse, or a bundle
    /// envelope is unknown.
    Corrupt { message: String },
    /// The store file's DDL version is newer than this build knows, or a
    /// migration failed.
    DdlVersionRefused { found: u32, supported: u32 },   // added in review; section 8
    Migration { from: SchemaVersion, message: String },
    /// Advisory/IO failure (missing directory, permissions, disk full, locked).
    Unavailable { message: String },
    /// Any other backend failure.
    Backend { message: String },
    /// The operation was refused before writing: a documented patch
    /// precondition failed. Nothing was applied.   // added in review; section 8
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
/// concrete store and use the capability; a file-backed implementation
/// provides the mechanism of its own backend (a database's online backup
/// API; never a copy of live files). A store with no file does not implement
/// it.
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
- `InvalidPatch` and `DdlVersionRefused` are additions (section 8, accepted):
  failures stay typed instead of overloading `Backend`.
- `backup`/`sync` are not on `CampaignStore`; they are the separate capability
  trait `StoreBackup` in the same module. The engine owns the interface (all
  ports are engine-owned) but never calls it; adapters implement it (section
  4.3), and neither the engine nor `core` contains backend concepts (section 8).

### 4.2 `localtt_engine::export`

```rust
/// Versioned envelope for sharing a campaign as JSON (RFC-0001 5.9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Opening options; `Default` is what `open` uses.
#[derive(Clone, Debug)]
pub struct SqliteOptions {
    /// Online startup backup before DDL migrations (design section 5.7);
    /// skipped for a fresh file. `--no-backup` maps to `false` (U7).
    pub startup_backup: bool,
}

impl SqliteStore {
    /// Opens the campaign file, creating it when missing: checks the DDL
    /// version (refusing a newer one), takes the startup backup, then applies
    /// pending DDL migrations.
    pub fn open(path: &Path) -> Result<Self, StoreFault>;
    pub fn open_with(path: &Path, options: SqliteOptions) -> Result<Self, StoreFault>;
}

impl CampaignStore for SqliteStore {
    fn load(&self) -> Result<CampaignSnapshot, StoreFault>;
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;
}

// localtt_adapter::store::backup

// `impl StoreBackup for SqliteStore` (the trait is engine-owned; section 4.1):
// `backup` through the SQLite online-backup API (never a copy of a live WAL
// database); `sync` as WAL checkpoint / fsync. `open_with`'s startup backup and
// rotation use the same mechanism (design section 5.7).
impl StoreBackup for SqliteStore {
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault>;
    fn sync(&mut self) -> Result<(), StoreFault>;
}

// localtt_adapter::store::export

/// Decodes a bundle text: envelope format/version, then every document through
/// the same schema gate as `load_checked` (older major accepted and re-stamped,
/// newer refused). Plain `serde_json` deserialization of `ExportBundle`
/// bypasses the gate and is for tests only.
pub fn decode_bundle(text: &str) -> Result<ExportBundle, StoreFault>;

/// Imports a decoded bundle into an empty store: one `apply`, all-or-nothing.
/// A store that already holds a campaign is refused.
pub fn import_into(store: &mut dyn CampaignStore, bundle: &ExportBundle) -> Result<(), StoreFault>;
```

Both entry points are generic over the store port, mirroring the backup capability:
`import_into` takes any `&mut dyn CampaignStore`, so a bundle restores into the SQLite
store, the memory store, or a future backend with no adapter-specific path. Decode is
format work (one envelope, one gate); import is dataflow (one `apply` through the port),
and neither is tied to SQLite.

A bundle written by the server uses plain `serde_json` serialization of
`ExportBundle` (a derived type; serialization cannot fail a gate). Only decode
is special, because only decode needs the version gate.

## 5. Guarantees

### 5.1 Byte fidelity

For a document applied through `apply`, the serialization `load` returns is
byte-identical to the document that was applied: campaign, every entity, every
feed entry (STORE-6, design section 5.1). The store keeps the exact `serde_json`
bytes in `doc` columns and parses on load; nothing rewrites a document. Export
-> import -> export is byte-identical modulo `exported_at`.

### 5.2 Ordering

`CampaignSnapshot::entities` is ascending by `EntityId`, `feed` ascending by
`FeedEntryId`, regardless of the order patches supplied. Both implementations
sort on load (SQL `ORDER BY`, memory `BTreeMap`/sorted walk). The target test
asserts this; callers may rely on it for byte-stable exports.

### 5.3 Atomicity and apply preconditions

One `apply` is one transaction in both implementations: either every upsert,
removal, campaign update and feed append lands, or none does.

`apply` validates before writing and rejects, with `InvalidPatch`, a patch that
violates a documented precondition:

1. an entity id appears twice in `upserts`;
2. an entity id appears in both `upserts` and `removals`;
3. `removals` names an id the store does not hold;
4. `feed_appends` is not strictly ascending by id;
5. `campaign` is `Some` and carries a different id than the stored campaign.

A violation means state divergence between engine and store (single writer, so
the engine is the only possible source): fail fast, write nothing. `SqliteStore`
validates before `BEGIN IMMEDIATE`; `MemoryStore` validates on a copy and swaps
only on success, so neither can half-apply. A backend failure inside the
transaction (for example a disk error) rolls back in SQLite.

### 5.4 Durability and visibility

`apply` returns only after commit. The engine treats durable-before-visible as
its invariant (ENG-2): if `apply` fails, engine state is unchanged. A process
crash after commit but before the caller publishes anything is safe: the next
`load` returns the committed file. SQLite runs WAL + `synchronous = NORMAL`, so
a power loss can lose the last committed command but cannot corrupt the file
(STORE-3). `StoreBackup::sync` forces a checkpoint/fsync; the tests use it
before backup copies.

### 5.5 Version gates

- **Documents**: `load` reads `schema_version_major` from the extracted columns
  before parsing anything (VER-2). A newer major is `VersionRefused { found,
  supported: SchemaVersion::CURRENT }`. A same-or-older major loads; the parse
  re-stamps at `CURRENT` (existing core behavior, unchanged).
- **DDL**: `open` refuses a `PRAGMA user_version` newer than the build knows
  (`DdlVersionRefused`; review 2026-10-17) and applies pending migrations in one
  transaction otherwise.
- **Bundles**: `decode_bundle` checks `format` and `format_version` (unknown ->
  `Corrupt`), then gates campaign and entity documents on their raw JSON before
  parsing them, reusing the `load_checked` gate.

`apply` itself is deliberately not gated: it receives documents the engine just
built at `CURRENT`, and a client simulating a future file (the target test does)
writes through it. The gate that matters is on load.

### 5.6 Error model

`StoreFault` (section 4.1) is `#[non_exhaustive]`, `Clone + Debug + Error`.
Messages are diagnostic only. The server will map variants to HTTP status codes
in U7; the engine maps them into its own error type in U3.

### 5.7 Threading and progress

Both stores are `Send`, not `Sync`: one owner (the executor) mutates them. All
calls are blocking; there is no lock-free or wait-free claim. `MemoryStore` is
infallible except for validation and the load gate; `SqliteStore` blocks on IO
and on SQLite locks bounded by `busy_timeout = 5000` ms, surfacing `Unavailable`
on timeout.

## 6. Internal structure

### 6.1 `adapter::store::sqlite`

- One `rusqlite::Connection`, prepared statements cached; the DDL of design
  section 5.2 verbatim (`campaign`, `entities` + two indexes, `feed`;
  `PRAGMA journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`).
- `apply` extracts the columns from the typed documents and runs the writes in
  one `BEGIN IMMEDIATE` transaction.
- `schema.rs` owns `PRAGMA user_version` and an ordered migration list (one
  entry today: create v1). `doc_migrations.rs` owns the empty
  `(major, minor) -> fn(Value) -> Value` registry applied after the gate and
  before parsing.
- `StoreBackup::backup` uses `rusqlite::backup`; `open_with`'s startup backup names files
  `<stem>.backup-<unix>.sqlite` next to the campaign and keeps the five newest
  (design section 5.7).

### 6.2 `adapter::store::memory`

A plain `CampaignSnapshot` holder, preserved insertion-order-free (maps keyed by
id). It implements the same preconditions and the same load gate so the contract
suite can run one suite against both. It is the store used by engine tests in
U3+; it has no file and does not implement `StoreBackup`.

### 6.3 Contract suite

`crates/localtt-adapter/tests/store_contract.rs` runs a shared suite over
`&mut dyn CampaignStore` (via a small harness) for `MemoryStore` and
`SqliteStore` in a tempdir, plus a SQLite-only block for `open`/`backup`/`sync`
and DDL checks:

- load/apply/load byte fidelity for every fixture document;
- ordering guarantees; feed append and `feed_keep_from` trimming;
- each apply precondition of section 5.3 -> `InvalidPatch`, nothing applied;
- campaign singleton (create once, update with the same id, refuse a second);
- version gate: newer major refused, same/older accepted; newer DDL refused;
- the migration hook: a test-registered migration transforms a fixture;
- reopen equals prior state; `StoreBackup::backup` opens and equals the source
  (SQLite-only block);
- `NoCampaign`, `Corrupt` on a non-store file.

### 6.4 Fixtures and migration homes

- `fixtures/campaigns/v1/` (root, per the review answer to Q4) gets the
  serialized party as a bundle plus its regenerate-and-compare test, so future
  document migrations have real input.
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
  the public items above. `schema` and `doc_migrations` are `pub(crate)`, except the
  small `schema` surface the contract suite drives through SQLite itself.

## 7. API cross-check against the target test

Every item the test uses, and where it is pinned: `MemoryStore::new` /
`Default` (4.3); `SqliteStore::open` (4.3) and `StoreBackup::{backup, sync}`
(4.1); `CampaignStore::{load, apply}` (4.1); `StatePatch` fields (4.1);
`CampaignSnapshot` fields + ordering (4.1, 5.2); `StoreFault::{NoCampaign,
VersionRefused, Corrupt, InvalidPatch}` (4.1); `ExportBundle::from_snapshot` +
`exported_at` (4.2); `export::{decode_bundle, import_into}` (4.3). The sketch
elements the test does not use: `open_with`/`SqliteOptions` and
`StatePatch::feed_keep_from` (justified by design sections 5.3 and 5.7 and
exercised by the contract suite), and the remaining `StoreFault` variants
(`Migration`, `Unavailable`, `Backend`, `DdlVersionRefused`; used by U3/U7 and
the contract suite).

## 8. Review decisions (settled 2026-10-17)

All three questions were answered in review; the sketches above already incorporate the
answers. The original options are kept in brief for the record.

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
