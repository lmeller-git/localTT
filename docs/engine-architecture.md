# localtt engine, store and content architecture

| | |
|---|---|
| **Status** | Approved 2026-10-17 (section 12 answers recorded; section 11 folded into `docs/decisions.md`). No library code has been written against it yet. |
| **Supersedes** | Nothing. Answers RFC-0001 section 9.3 items 1 (port placement) and 2 (reference vendoring). |
| **Scope** | `localtt-engine`, `localtt-adapter` (store + content), the `localtt` composition root, and the cross-crate contracts (`localtt-core::ports`, `localtt-core::content`). |
| **Read with** | `RFC-0001-localtt.md` (intent and invariants), `docs/architecture.md` (as-built map), `docs/decisions.md` (decision log), `crates/localtt-core/DESIGN.md` (frozen wire contract). |
| **Audience** | Maintainer and the implementing agents. |

## 0. How to read this document

- Code blocks are **illustrative sketches**, not compiling Rust. Signatures will be pinned
  by per-unit RFCs and target tests (the `spec` skill) before any implementation.
- "MUST"/"SHOULD" carry the same weight as in RFC-0001. Section 11 collects the decisions
  (folded into `docs/decisions.md` as ENG/STORE/CONTENT entries); section 12 records the
  maintainer's answers, settled 2026-10-17.
- Nothing here changes the core wire contract. Section 8 states the (additive) core
  surface this design needs and its blast radius. Per RFC-0001 and `AGENTS.md`, any actual
  contract change is a separate, versioned proposal.

## 1. Goals

1. Answer, with evidence, how the serde data model relates to SQLite storage and to
   export/import/sharing, and prove round-trip fidelity.
2. Answer how the engine consumes external 5e content behind an abstraction that is not
   locked to one dataset: SRD 5.1 today, packs and future importers (PDF, other
   databases, web, custom formats) without redesign.
3. Define the engine: rules, dice, clock/expiry, authorization, commands, application
   services, events, and the synchronous single-writer executor.
4. Define exact ports, where they live, and how each is tested (in-memory + SQLite
   contract suites).
5. Produce a phased plan that fits the repo's `spec -> skeleton -> implement` workflow.

## 2. Non-goals

- No server work beyond naming the composition root and its wiring. REST/WS, join flow,
  DTOs and OpenAPI remain `localtt-server` unit specs.
- No rules-engine completeness: this document fixes the shape (modifier pipeline,
  breakdown, warnings) and defers the 5e rule catalogue to the rules unit.
- No content packs loader, no PDF import/export, no full-text search over descriptions,
  no Lua/WASM plugins. Hooks for all four are stated, none are built.
- No multi-process access to one campaign file, no CRDTs, no distributed state.
- No binary attachments (maps/images) yet. The design keeps a path to them open
  (section 5.4) but does not build it.

## 3. Ground rules inherited from RFC-0001 and AGENTS.md

- Dependency direction: `server -> engine -> core`; `adapter -> core`, never `engine`
  (RFC-0001 3.1, decisions ARCH-3). This forces port placement (section 4.1).
- Core stays I/O-free and dependency-light (ARCH-2). Ports are traits and data.
- No floats, no wall-clock reads in pure code, all randomness behind the `Rng` port
  (RFC-0001 2.9, decisions DATA-4).
- "Warn, never block" for rules; commands fail only for authorization, malformed input,
  or missing entities (RFC-0001 5.2).
- The wire format (`core::types`) is a versioned public contract (core DESIGN section 5).
- Fully offline play: no runtime network dependency, ever (RFC-0001 5.10).
- Every port ships an in-memory implementation and a contract suite run against every
  implementation (task constraint; RFC-0001 7).

## 4. Ports: placement and sketches

### 4.1 Placement rule (RFC-0001 9.3 item 1)

**Ports live in `localtt-core`, in a new `core::ports` module. The engine consumes them;
the adapter implements them.**

The dependency direction decides this: `adapter` may not depend on `engine` (ARCH-3), so
any trait that the SQLite store implements must be visible to `adapter`, and the engine
must depend on the same definition. That leaves `core`. This also matches RFC-0001 3.2,
which already assigns `ports/` to core, and keeps adapter wiring free of engine types.

Consequence: port data transfer types (`StatePatch`, `CampaignSnapshot`, `ContentEntry`,
`StoreFault`) also live in core, but **outside `types.rs`**, so the frozen wire contract
(documents in campaign files) is untouched. These types are never persisted as campaign
documents; they are the vocabulary between engine and adapters. Section 8 lists them as
the only core additions this design needs.

Two rules for the sketches below:

- Ports are object-safe (`&dyn CampaignStore`), because the server holds one boxed
  executor and tests swap implementations. No generics in port methods, no associated
  error types: `StoreFault` is a plain, `#[non_exhaustive]` enum. Backend-specific
  detail (rusqlite error text) goes into a `message: String` field documented as
  diagnostic-only.
- Ports are I/O-boundaries only. Pure logic (`rules`, `dice`, `authz`, formula
  evaluation) never takes a port.

### 4.2 `CampaignStore`

```rust
// core::ports::store (sketch)

/// Everything the store holds, materialized. Startup and export use this.
pub struct CampaignSnapshot {
    pub campaign: Campaign,
    pub entities: Vec<AnyEntity>,   // deterministic order: by EntityId (UUIDv7 sort)
    pub feed: Vec<FeedEntry>,       // ascending id
}

/// One command's durable effect, applied in a single transaction.
/// Constructed by the engine; the store treats documents opaquely.
pub struct StatePatch<'a> {
    /// Present when the campaign document changed (clock, settings, players).
    pub campaign: Option<&'a Campaign>,
    /// Full new revisions of touched entities (created or updated).
    pub upserts: Vec<&'a AnyEntity>,
    pub removals: Vec<EntityId>,
    /// Feed entries to append, ascending id.
    pub feed_appends: Vec<&'a FeedEntry>,
    /// When set, drop feed entries with `id < keep_from` (ring trimming).
    pub feed_keep_from: Option<FeedEntryId>,
}

/// Failure of any store operation.
#[non_exhaustive]
pub enum StoreFault {
    /// File (or embedded campaign) is empty; a campaign must be created first.
    NoCampaign,
    /// File's schema major is newer than this build supports (VER-2).
    VersionRefused { found: SchemaVersion, supported: SchemaVersion },
    /// File is not a localtt store, or a document failed to parse.
    Corrupt { message: String },
    /// A document or DDL migration failed.
    Migration { from: SchemaVersion, message: String },
    /// Advisory/IO failure (missing directory, permissions, disk full, locked).
    Unavailable { message: String },
    /// Any other backend failure; `message` is diagnostic, not user-facing.
    Backend { message: String },
}

/// The only persistence abstraction the engine knows.
///
/// Implementations are single-writer, synchronous and may block: the composition
/// root runs them behind the serialized command executor (section 7.2).
pub trait CampaignStore: Send {
    /// Loads the whole campaign. Called once at startup and by export.
    fn load(&self) -> Result<CampaignSnapshot, StoreFault>;

    /// Applies a patch in one transaction: all-or-nothing. The engine calls this
    /// before publishing anything (section 5.3).
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), StoreFault>;

    /// Online backup to `to` (SQLite backup API, not a file copy).
    fn backup(&mut self, to: &Path) -> Result<(), StoreFault>;

    /// True synchronization, mostly for tests and startup diagnostics.
    fn sync(&mut self) -> Result<(), StoreFault>;
}
```

Design notes:

- There is deliberately **no per-entity `load_entity`** on the port in v1. Startup loads
  everything into the working set (section 5.4). When the working set gains lazy bodies,
  the port grows `load_entity(id)`; adding a method to a core trait is additive.
- `apply` is the transactional unit of work per command. There is no `begin()/commit()`
  pair: with a single writer, a patch is the only transaction shape needed, and it makes
  crash safety trivial (section 5.3).
- In-memory implementation: `adapter::store::memory::MemoryStore`, a pure
  `CampaignSnapshot` holder, used by engine tests and the contract suite.
- SQLite implementation: `adapter::store::sqlite::SqliteStore`.

### 4.3 `ReferenceSource` and the content model

The engine needs a read-only view of content; the adapter owns how it is loaded. The
model is provider-neutral by construction (section 6.2): Open5e, packs, and future PDF or
web importers all produce the same `ContentEntry` snapshots.

```rust
// core::content (sketch)

/// Closed set of reference kinds (the `<kind>` segment of a RefId).
/// `feature` covers class/race/feat/trait features so monster actions and class
/// features share one shape (RFC-0001 4.4).
pub enum ContentKind {
    Spell, Item, Monster, Condition, Class, Subclass, Race, Background,
    Feat, Feature, Rule,
}

/// A source record adapted to the localtt model. This is what `get` returns and
/// what snapshots serialize. Never stored inside campaign documents.
pub struct ContentEntry {
    pub id: RefId,
    pub kind: ContentKind,
    pub name: String,
    /// Markdown; opaque (sanitization is a frontend concern, RFC-0001 8.7).
    pub description: String,
    /// Structured payload, typed per kind. Shares core types where one exists
    /// (`InlineItem` for items, `FeatureEntry`-shaped data for features).
    pub data: ContentData,
    pub provenance: Provenance,
    /// Conversion fidelity: true when the importer had to guess.
    pub needs_review: bool,
    /// Why the entry needs review, in human-readable form.
    pub review_notes: Vec<String>,
    /// Unmapped source fields, verbatim, for lossless re-export and later mapping.
    pub ext: Ext,
}

pub enum ContentData {
    Spell(SpellData),         // level, school, casting time, range, components,
                              // duration, concentration flag, classes, upcast text
    Item(InlineItem),         // reuses the core item shape including modifiers
    Monster(MonsterData),     // abilities, CR, AC, HP, speeds, senses, traits/actions
    Feature(FeatureData),     // category, text, modifiers, resource hint
    Text,                     // conditions, rules, classes, races, backgrounds, feats
}

pub struct Provenance {
    /// Namespace this entry was imported under, e.g. "srd2014" or "pack.mytome".
    pub namespace: String,
    /// Human-readable source document, e.g. "SRD 5.1 (CC-BY-4.0), via Open5e".
    pub source: String,
    /// Pinned source revision, e.g. a commit SHA plus dataset version.
    pub source_revision: String,
    /// License identifier and attribution text to surface in the UI.
    pub license: String,
    pub attribution: String,
    pub url: Option<String>,
}

/// Lightweight hit for lists and autocomplete.
pub struct ContentSummary {
    pub id: RefId,
    pub kind: ContentKind,
    pub name: String,
}

pub struct SearchQuery<'a> {
    pub text: &'a str,
    /// Empty means all kinds.
    pub kinds: &'a [ContentKind],
    pub limit: usize,
}

/// Read-only content access for the engine. Implementations are immutable after
/// construction (snapshots are pinned), so caching is trivial: load once.
pub trait ReferenceSource: Send + Sync {
    fn get(&self, id: &RefId) -> Option<ContentEntry>;
    /// Name/description search, deterministic order (section 6.5).
    fn search(&self, query: &SearchQuery<'_>) -> Vec<ContentSummary>;
    /// Ranked name autocomplete (section 6.5).
    fn autocomplete(&self, query: &SearchQuery<'_>) -> Vec<ContentSummary>;
    /// Sources (namespaces) currently loaded, for dedup UI and diagnostics.
    fn sources(&self) -> Vec<Provenance>;
}
```

Design notes:

- `get` returns owned `ContentEntry` (bounded size, one lookup per composed value);
  caching inside the implementation is allowed and trivial because snapshots are
  immutable per process.
- The layout/indexing implementation (in-memory maps, ranking) is adapter-internal.
- The in-memory implementation for tests is a small fixture source
  (`adapter::reference::fixture::FixtureSource`) with hand-written entries covering
  every `ContentKind`, duplicate names across kinds, unicode names, and an entry with
  `needs_review`.

### 4.4 `Rng`

```rust
// core::ports::rng (sketch)

/// Uniform die source. Contract: `roll_die(sides)` returns 1..=sides with equal
/// probability; `sides >= 2`. Callers cap `sides` and counts (section 7.5).
pub trait Rng {
    fn roll_die(&mut self, sides: u32) -> u32;
}
```

- Consumption: `engine::dice::roll(&DiceExpr, &mut dyn Rng)` and the `Roll` command.
- Implementations: `adapter::rng::ChaChaRng` (OS-seeded ChaCha, shipped) and
  `testkit::ScriptedRng` (a `VecDeque<u32>` fixture; lives next to the engine tests in
  `engine::testkit` or the integration crate).
- Contract suite: distribution sanity for a seeded implementation (chi-square-lite
  bounds, RFC-0001 7), range, and the `sides == 1` degenerate case.

### 4.5 `EventSink` (deferred)

```rust
// core::ports::events (sketch; NOT built in v1)
pub trait EventSink {
    fn publish(&self, event: DomainEventWithAudience);
}
```

Recommendation: **do not build this port yet.** The executor is synchronous and already
returns events to its caller (section 7.9); an extra publish path would be a second
egress point competing with `view_of` (RFC-0001 5.7) and untested fan-out logic. Add it
when a consumer exists that cannot be served by the command result, e.g. a background
scheduler that expires statuses without a command. The sketch stays here so the decision
is recorded, not so it gets implemented.

### 4.6 `SheetExporter` / `SheetImporter` (deferred)

```rust
// core::ports::sheet (sketch; NOT built now)
pub trait SheetExporter {
    fn export(&self, view: &EntityView, to: &mut dyn Write) -> Result<(), ExportFault>;
}
pub trait SheetImporter {
    fn import(&self, from: &mut dyn Read) -> Result<ImportOutcome, ImportFault>;
}
```

Both are adapter concerns (`adapter::pdf` later) and must speak **view** types, never core
documents, so a redacted sheet cannot leak. Deferred with RFC-0001 D12.

## 5. Storage interaction model

### 5.1 Format and fidelity (driving question 1, part 1)

**The storage format is the wire format: the exact bytes `serde_json` produces for
`Campaign`, `AnyEntity` and `FeedEntry` live in a `doc` TEXT column.** The versioned
schema policy (VER-1/VER-2, core DESIGN section 5) therefore applies unchanged to stored
files, and export is a re-serialization, not a translation.

Why this works:

- Documents are already the unit of change: commands produce whole entity revisions, so
  the store never needs to patch JSON in place.
- Determinism is already guaranteed by `BTreeMap`/`BTreeSet` in serialized positions
  (DATA-5) and no floats (DATA-4), so byte-identical rewrites are meaningful.
- The version gate already exists (`adapter::load::load_checked`, ARCH-6). The store
  adds only the column-backed gate (read `schema_version_*` before parsing) so a
  too-new file is refused without materializing it.

Fidelity contract, asserted by the store contract suite:

1. For every fixture `AnyEntity`, `store.apply(put)` then `store.load()` yields a
   document whose serialization is byte-identical to the original.
2. Same for `Campaign` and, per entry, the feed.
3. Export bundle -> import -> export is byte-identical modulo `exported_at`.

Extracted columns (decision VER-3: "only queried columns are extracted"):

| Column | Why it exists |
|---|---|
| `id` | Primary key; `load_entity` when it lands. |
| `kind`, `body_kind` | Filtering/diagnostics; cheap projection consistency check. |
| `owner`, `parent` | ACL-adjacent queries and integrity checks (`owner` must exist), server list filtering if it ever moves below the engine. |
| `name` | Sort/display in diagnostics; server search fallback. |
| `rev` | Startup diagnostics; a cross-check that the document's `rev` matches its column. |
| `schema_version_major/minor` | The version gate before parse, and migration dispatch. |

Explicitly **not** extracted in v1: ACL entries (no query needs them; `view_of` reads the
loaded document), players (roster lives in the campaign document, core DESIGN 3.3),
status expiry (engine runtime state, section 7.6), feed audiences (filtered in memory).
An `acl` projection table is the first thing to add when "list everything shared with
player X" moves into SQL; it is not needed while the working set is in memory.

### 5.2 DDL

```sql
-- v1 schema (sketch). DDL version lives in PRAGMA user_version.
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE campaign (
    id                   TEXT PRIMARY KEY,      -- singleton; enforced by the store
    schema_version_major INTEGER NOT NULL,
    schema_version_minor INTEGER NOT NULL,
    doc                  TEXT NOT NULL          -- Campaign JSON
);

CREATE TABLE entities (
    id                   TEXT PRIMARY KEY,      -- UUID string
    kind                 TEXT NOT NULL,
    body_kind            TEXT NOT NULL,
    owner                TEXT NOT NULL,
    parent               TEXT,
    name                 TEXT NOT NULL,
    rev                  INTEGER NOT NULL,
    schema_version_major INTEGER NOT NULL,
    schema_version_minor INTEGER NOT NULL,
    doc                  TEXT NOT NULL
);
CREATE INDEX entities_owner ON entities(owner);
CREATE INDEX entities_kind  ON entities(kind);

CREATE TABLE feed (
    id       INTEGER PRIMARY KEY,               -- FeedEntryId
    ts       INTEGER NOT NULL,
    clock_s  INTEGER NOT NULL,
    audience TEXT NOT NULL,
    doc      TEXT NOT NULL
);
```

- The singleton campaign is enforced with a `CHECK (id = ...)`-free approach: the store
  refuses a second row and `load` takes the only one; `NoCampaign` when empty.
- WAL + `NORMAL`: a power loss may lose the last committed command but cannot corrupt the
  database. Commands are user-typed bookkeeping; losing the last one is recoverable, a
  corrupt file is not. `busy_timeout = 5000`.
- Prepared statements are cached (`prepare_cached`) in the connection. One connection.
- `apply` uses `BEGIN IMMEDIATE` so there is never a deferred-to-write upgrade.

### 5.3 Transactions and the command commit

The executor is the only writer. For every command:

```
apply(state, command, env) -> Transition        // pure; state untouched
   |
   v
store.apply(&transition.patch)                  // one Durable transaction
   |  Ok
   v
state.apply(&transition.patch)                  // in-memory working set
   |
   v
return Executed { outcome, events }             // events only after durability
```

- **Durable before visible.** If `store.apply` fails, the engine state is unchanged and
  the error surfaces; no rollback logic, no dirty-state repair.
- **Crash safety.** If the process dies after commit and before publish, the next startup
  loads the committed file: the command is simply already done. The in-memory state is
  always reconstructible from the file.
- `Transition` carries the patch, warnings and events (section 7.2). The `StatePatch` is
  the only mutation shape crossing the port.
- Why not `begin()/commit()` on the trait: it pushes lifetime and rollback handling into
  every caller for zero gain. A patch is exactly one command's effect.

### 5.4 Working set: load-all now, swappable later

Commands and view services read through one small trait:

```rust
// engine::state (sketch)

pub struct EntityHeader {
    pub id: EntityId, pub kind: EntityKind, pub name: String,
    pub owner: PlayerId, pub parent: Option<EntityId>,
    pub acl: Vec<AclEntry>, pub rev: u64, pub updated_at: UnixTime,
}

/// Read view commands and services use. By design, reads return owned values so
/// an implementation may lazily load a body without fighting the borrow checker.
pub trait WorkingSet {
    fn campaign(&self) -> &Campaign;
    fn entity(&self, id: EntityId) -> Result<Option<AnyEntity>, EngineError>;
    fn headers(&self) -> Vec<EntityHeader>;
    fn feed(&self) -> Vec<FeedEntry>;
    fn expiry_candidates_until(&self, until: CampaignTime) -> Vec<(EntityId, StatusId)>;
}

/// Mutated only by the executor, after the store commit.
pub trait WorkingSetMut: WorkingSet {
    fn apply(&mut self, patch: &StatePatch<'_>) -> Result<(), EngineError>;
}
```

Default implementation, `LoadedCampaign`:

```rust
pub struct LoadedCampaign {
    campaign: Campaign,
    entities: BTreeMap<EntityId, AnyEntity>,
    feed: VecDeque<FeedEntry>,
    next_feed_id: u64,
    expiries: BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>,
}
```

Sizing, which justifies load-all today (party scale, users are a DM and up to ~8 players):

| Quantity | Typical | Pathological | Read cost |
|---|---|---|---|
| Entities | 30-100 | ~1,000 | one `SELECT id, doc` pass |
| Body JSON | 2-8 KB | ~50 KB | < 1 MB typical; ~50 MB pathological |
| Feed ring | 200 entries | 2,000 entries | bounded by `feed_capacity` |
| Startup load | < 100 ms | < 2 s | dominated by JSON parse |

`entity()` clones a document (a few KB): commands touch one to three entities, so this is
noise, and it is what allows a future lazy implementation to slot in. When the body grows
(images/maps hosted as attachments, or campaigns an order of magnitude larger), the swap
is a new `WorkingSetMut` implementation that keeps headers plus an LRU of bodies and
fetches via a new `CampaignStore::load_entity`; no command, rule or view code changes.
Attachments themselves are out of scope; when they land, their bytes belong in a separate
table or sidecar files referenced from `ext`, never inside the entity JSON.

Two invariants, property-tested against command sequences:

1. `headers()` reflects `entities` (no header drift).
2. `expiries` equals the index recomputed from `entities` (section 7.6).

### 5.5 Optimistic concurrency (`rev`)

- `rev` is checked **in the engine**, against the authoritative in-memory state, never in
  the store. The store has a single writer and no other client, so a SQL-level
  compare-and-swap would be dead code.
- Commands that replace a whole document (`SetField` on a whole-body path, `SetClock`,
  settings updates) take `expected_rev: Option<u64>`; a mismatch is
  `EngineError::Conflict { entity, expected, actual }`, which the server maps to 409.
- The command layer owns stamping: on every touched entity it sets
  `rev = old.rev + 1`, `updated_at = env.now()`. Creates start at `rev = 1`. The executor
  asserts monotonicity before persisting.
- Path-level field edits and intent commands do not require `expected_rev` (RFC-0001
  5.2); they read-modify-write inside the single writer, which is exactly why intent
  commands exist.
- Two processes opening one campaign file is not supported in v1 (section 5.7).

### 5.6 Migrations

Two layers, both required from day one:

1. **DDL migrations**: `PRAGMA user_version` holds the DDL version. `adapter::store::schema`
   owns an ordered `const DDL_MIGRATIONS: &[fn(&Connection) -> Result<(), StoreFault>]`
   list (one entry today: create v1). Applied inside one transaction at open; a newer DDL
   version than the build knows refuses to open (same philosophy as VER-2).
2. **Document migrations**: pure `serde_json::Value -> Value` functions in
   `adapter::store::doc_migrations`, keyed by `(major, minor)`, applied after the gate and
   before `load_checked`. The registry is **empty at v1.0** because no older version
   exists yet. It exists now so the first breaking change has a home that is not a
   redesign.
   - Fixtures: one committed whole-campaign bundle per historical version under
     `fixtures/campaigns/v<major>.<minor>/`, plus a test that every fixture loads to
     `CURRENT` and re-exports deterministically. The repo already generates
     byte-deterministic fixture campaigns (integration DESIGN section 4); committing their
     serialized form is a copy, not new tooling.
   - Migrations run before the store is opened for writing and only after a backup
     (section 5.7).

### 5.7 WAL, backup, single process

- One campaign file, one server process, documented. No advisory lock file in v1: SQLite
  locking plus `busy_timeout` prevents corruption if a second process is started by
  accident, but in-memory state could go stale, so the startup banner and docs say "one
  server per campaign file". A lock file is a cheap later addition.
- Automatic online backup **on startup, before migrations**, using
  `rusqlite::backup` (never a file copy of a live WAL database, RFC-0001 8.11). Rotation:
  keep the 5 newest, named `<stem>.backup-<unix>.sqlite` next to the campaign.
- A `--no-backup` flag for scripted test runs; the default is on.
- Full WAL checkpoint on clean shutdown; not needed for correctness.

### 5.8 Export, import, sharing, portability

- **Export bundle** (a new versioned core format, `core::export`):

  ```rust
  pub struct ExportBundle {
      pub format: String,             // "localtt.campaign"
      pub format_version: u32,        // 1
      pub exported_at: UnixTime,      // excluded from round-trip comparisons
      pub campaign: Campaign,
      pub entities: Vec<AnyEntity>,   // ascending EntityId
      pub feed: Vec<FeedEntry>,       // ascending id
  }
  ```

  `format_version` is separate from document `schema_version`: it versions the envelope,
  not the documents, which keep their own gate. Import refuses unknown envelope versions.
- **Import**: decode -> gate every document (`load_checked`) -> require the target store to
  be empty -> one `store.apply` with all upserts plus the campaign. All-or-nothing.
  Merge/append import is explicitly out of scope.
- **Sharing a campaign** (two supported paths, different guarantees):
  1. Ship the `.sqlite` file. Single-file property preserved (RFC-0001 2.1, 5.9). The
     recipient needs the same build or older-major-compatible (VER-2).
  2. Ship an export bundle. Portable JSON; imports into a fresh file.
- **Portability when reference data is external**: the campaign stores `RefId` plus name
  snapshot; the engine emits `UnresolvedReference` warnings during derivation rather than
  computing wrong numbers; an explicit `FreezeReference` command converts one referenced
  element into its inline copy (CONTENT-4, section 6.7). SRD content is embedded in the
  binary, so only user packs can be missing.
- **Player sheet backup**: export of the entities visible to that player, produced from
  `view_of` output (not core documents). Deferred; the bundle above is DM-level.

### 5.9 Store contract suite

`crates/localtt-adapter/tests/store_contract.rs` runs one generic suite against both
implementations via a harness:

```rust
fn store_contract(harness: &mut dyn StoreHarness) {
    // load/apply/load fidelity per fixture document (byte-for-byte)
    // patch atomicity: a patch with a failing entry leaves nothing behind
    // feed append + trim semantics; monotonic ids across reopen
    // rev passthrough, upsert/remove, campaign singleton
    // version gate: newer major refused; older major accepted
    // migration hooks: a registered fake migration transforms a fixture
    // backup produces an openable store with equal contents
    // reload equals prior state (the "store mirrors state" property, section 9)
}
```

Both `MemoryStore` and `SqliteStore` (tempdir) run it. `just test` already gates tests;
no new recipe needed beyond the dev-dependencies.

## 6. Content pipeline and `RefOrText` (driving question 2)

### 6.1 Concrete source and license

**Vendored dataset: SRD 5.1 (2014), digitized by Open5e under the `wotc-srd` document,
pinned by commit and SHA-256.** Evidence:

- SRD 5.1 is released by Wizards of the Coast under CC-BY-4.0 (irrevocable), in addition
  to the legacy OGL 1.0a; the canonical CC file is published at
  `dndbeyond.com/srd` (`SRD_CC_v5.1.pdf`). SRD 5.2/SRD 5.2.1 are 5.5e-rules documents
  and are out of scope: the product targets 2014 (RFC-0001 header).
- Open5e's `data/v1/wotc-srd/` (repo `open5e/open5e-api`) is a structured digitization of
  exactly that document: verified counts via the live v1 API, `document__slug=wotc-srd`:
  319 spells, 322 monsters, 237 magic items, 15 conditions, 9 races, 12 classes,
  1 background, 1 feat, 37 weapons, 18 armor, 45 rule sections; 1,016 records total.
  Structured fields we need exist (spell level/school/casting time/range/components/
  concentration flag/classes; monster abilities/CR/AC/HP/speeds/senses/traits/actions).
- Licensing hygiene: Open5e's repo license explicitly does **not** relicense the SRD
  content it embeds; our right to redistribute comes from WotC's CC-BY-4.0. We ship the
  required attribution (SRD 5.1 notice + "via Open5e, commit <sha>") in the snapshot and
  surface it in the UI. The repo is MIT for code; we vendor no Open5e code.
- Fallbacks, documented but not chosen: `5e-bits/5e-database` (the dnd5eapi data) was
  archived in October 2026 and is OGL-based, a murkier redistribution story; extracting
  the official PDF ourselves means owning a brittle parser forever.
- `open5e-api` v1 is in maintenance; the pin makes that irrelevant. Re-pinning is a
  deliberate, reviewable snapshot upgrade (section 6.3).

### 6.2 Provider-neutral model: importers vs providers

The maintainer's requirement: never be locked to one source. The design separates
**ingestion** from **serving**:

- **Ingestion** (`adapter::reference::import`): pluggable converters that read some
  source format and emit our snapshot format. One converter today
  (`open5e::convert`, run by a dev binary against a pinned checkout); future converters
  for PDF exports, other databases, or a one-shot web fetch. Ingestion is run explicitly
  by us (release); it never runs at play time. Hand-authored packs are not converter
  input: they are TOML + frontmatter, normalized by the pack loader (section 6.6).
- **Serving** (`adapter::reference::index`): one implementation of
  `core::ports::ReferenceSource` over N loaded snapshots. A snapshot is
  `{ provenance, entries: Vec<ContentEntry> }`; sources compose by namespace, and nothing
  in the engine or in the campaign file knows which converter produced an entry.
- **`ext` is the lossless seam**: any source field we cannot map is preserved verbatim
  under a namespaced `ext` key, so a later converter revision can mine it without the
  original source. `needs_review` marks entries where the mapping guessed.
- A future live web provider is still an importer: offline-first forbids runtime network
  (RFC-0001 5.10). The DM runs an import once; the result is a snapshot file on disk.

Snapshot file (committed for SRD; per-pack later):

```json
{
  "snapshot_version": 1,
  "provenance": { "namespace": "srd2014", "source": "SRD 5.1 (CC-BY-4.0) via Open5e",
                   "source_revision": "<commit-sha>", "license": "CC-BY-4.0",
                   "attribution": "This work includes material taken from the System Reference Document 5.1 ...",
                   "url": "https://dnd.wizards.com/resources/systems-reference-document" },
  "entries": [ { "id": "srd2014:spell/fireball", "...": "ContentEntry" } ]
}
```

Determinism: converter tests run it twice and compare bytes; entries sorted by `RefId`;
`BTreeMap` for every map; no floats (weights in ounces, values as integer coins, CR in
eighths; core's `ChallengeRating`).

The snapshot JSON is the **importer** interchange format (the SRD snapshot is committed
and embedded). Hand-authored packs are **TOML + frontmatter** instead (section 12 Q7):
friendlier to write and read by hand; the pack loader normalizes them into the same
`ContentEntry` model, so `ReferenceSource`, the engine and the campaign file cannot tell
the two formats apart. Exact file layout is pinned in the packs unit (U8).

### 6.3 Namespace, `RefId`, slug stability

- `RefId = <namespace>:<kind>/<slug>` already exists in core (types.rs, pinned by tests).
  The namespace is the *source*: `srd2014` for the embedded snapshot, `pack.<pack_id>` for
  user packs. Kinds are the `ContentKind` list above; the slug is the source's own slug
  where it has one (Open5e does, stable and lowercase-kebab), else derived from the name
  by the converter (lowercase, ASCII-fold, non-alphanumerics to `-`, collapse, trim).
- Collisions inside a namespace are a converter **error**, resolved by the converter with
  an explicit suffix rule that is part of the snapshot report, never by silent overwrite.
- **Slug stability policy**: because the SRD is a frozen document under an irrevocable
  license, re-pinning Open5e cannot change the rules text, but it could change a slug.
  The converter keeps an `aliases.json` (`old RefId -> new RefId`); `ReferenceSource::get`
  follows one alias hop; the engine never rewrites campaigns automatically. Pack upgrades
  use the same mechanism. Renames are review events, not silent migrations.
- Name snapshots (`ContentRef.name`) remain the display fallback and are unaffected.

### 6.4 Import mapping and lossiness

Per-kind mapping (converter; the shape of the work, not its final field list):

| Kind | Mapped | Lossy / `needs_review` candidates |
|---|---|---|
| `spell` | level, school, casting time, range, components (V/S/M + material), duration text, concentration flag, classes, description, upcast text | damage/DC not parsed into structured formulas (description carries them); duration seconds parsing is deliberately not attempted yet |
| `monster` | abilities, CR (string -> eighths), AC, HP, speeds, senses, languages, saves, skills, traits/actions/reactions/legendary actions -> `FeatureData` | recharge-by-roll -> `Resource` with `Recharge::Manual` and a review note; multiattack stays action text; attack lines keep free-text damage |
| `item` | name, kind, description, requires_attunement, weight lb -> oz, value string -> `Coins` | +1/+2/+3 suffixes are heuristic -> modifiers only when unambiguous, otherwise `needs_review`; properties mapped when the source shape matches |
| `condition` | name, description | none known |
| `class` / `subclass` / `race` / `background` / `feat` / `feature` | name, description, level/feature lists as text | mechanics are description text in v1 |
| `rule` | section title, text | none known; sections are display-only |

Every unmapped source field lands under `ext["src.open5e"]` verbatim. The converter emits
a review report (`report.json` next to the snapshot: counts per kind, review counts, the
exact review notes) that is committed with the snapshot, so fidelity is auditable.

### 6.5 Autocomplete

Requirement: as the player types "fireb" while editing a sheet, return ranked matches,
kind-filterable, fast enough to feel local.

Decision: **in-memory ranked prefix matching; FTS5, fuzzy matching and trigram indexes are
explicitly deferred.**

- Index: for each snapshot, a `BTreeMap<String /*lowercased name*/, Vec<EntryRef>>` plus
  a by-`RefId` map. Alias names are indexed too (section 6.3).
- Ranking tiers (deterministic, no floats):
  1. exact case-insensitive name match
  2. name starts with the query
  3. a word inside the name starts with the query
  4. name contains the query
  Tie-breaks, in order: kind priority (the caller's kind filter order, then a fixed
  order for unfiltered calls), namespace priority (packs before SRD, so homebrew wins
  its own display slot), then `name` ascending, then `RefId` ascending. Every comparison
  is total and integer/string-based, so results are stable across runs and platforms.
- Kind filtering happens before ranking (the caller passes `kinds`).
- Latency budget: **p99 <= 10 ms server-side for 10,000 entries**, a 10x margin over the
  SRD scale. Reality: 1,016 entries in a `BTreeMap`; a prefix range scan is O(log n + k)
  in the tens of microseconds; even the substring tier (linear scan of ~1,000 strings)
  is well under 1 ms. Budget is asserted by a bench (`just bench`) with an in-memory
  fixture source scaled to 10,000 synthetic entries, not by a wall-clock unit test.
- Description search (`search`, RFC-0001 5.10 "FTS5 or equivalent"): ships as a
  name-and-description substring scan in the same implementation, same budget. It is
  adequate at SRD scale and for packs up to a few thousand entries. When it isn't
  (large packs, ranked full-text), the replacement is an implementation detail behind
  `ReferenceSource`: bundled SQLite already ships FTS5, and a `tantivy` index remains an
  option. The port signature does not change.

### 6.6 Caching and embedding

- The SRD snapshot is generated by
  `cargo run -p localtt-adapter --bin localtt-content-convert` (dev tool) and committed at
  `crates/localtt-adapter/data/srd2014.json` with its SHA-256 recorded in the source tree
  and in the report.
- It is embedded in the binary with `include_bytes!` and parsed once (lazily, via
  `OnceLock`, on first reference use) into the index. Estimated size: 1.5-3 MB of JSON,
  20-60 ms one-time parse; acceptable for a desktop-hosted binary, and it keeps the
  single-binary story (RFC-0001 10, `rust-embed` row) with zero runtime files.
- Packs (later) are TOML + frontmatter files found on startup, normalized by the pack
  loader into the same entry model and index (section 6.2). Adding/removing packs
  requires a restart in v1.
- No invalidation logic: snapshots are immutable per process. A snapshot version bump is
  a code/release event, not runtime state.

### 6.7 What lives in the campaign file, what stays external

**External by default, with an explicit freeze escape hatch** (RFC-0001 9.3 item 2):

- In the campaign file: `RefId` + `ContentRef.name` snapshot (already in core, DATA-7),
  inline documents the user typed or edited, and nothing else from a snapshot.
- External: the SRD snapshot (embedded in the binary, so effectively always present) and
  pack files.
- When a reference cannot be resolved at derivation time, the rules layer emits
  `UnresolvedReference`; the displayed value falls back to what is representable (name
  snapshot, no mechanics) instead of silently computing without modifiers.
- `FreezeReference { entity, element }` resolves one element and rewrites it as its inline
  form (e.g. `InventoryItem::Ref -> InventoryItem::Inline`). This is a normal command with
  warnings, not an error. A campaign-wide "freeze everything used" variant is a later,
  additive convenience.

## 7. The engine

### 7.1 Module layout

```
crates/localtt-engine/src/
  lib.rs
  error.rs          EngineError, Warning, WarningCode
  state.rs          EntityHeader, WorkingSet, WorkingSetMut, LoadedCampaign
  executor.rs       Executor (owns WorkingSetMut + ports), command loop, events
  commands/
    mod.rs          Command enum + apply() dispatch + Transition
    entities.rs     create/update/delete/owner/acl/share
    hp.rs           damage, heal, temp HP, death saves
    resources.rs    spend/restore/set, hit dice
    rests.rs        short/long rest
    statuses.rs     add/remove/edit duration, concentration
    inventory.rs    add/remove/equip/attune, give handout
    casting.rs      cast spell (slot spend, proposal)
    clock.rs        advance/set, dawn recharge, expiry sweep
    dice.rs         roll command
    players.rs      invites, tokens
    content.rs      freeze reference
  rules/
    mod.rs          derive(), DerivedSheet, DerivedValue, Contribution
    formula.rs      Formula evaluation (static and roll contexts)
    pipeline.rs     modifier collection and ordering, stacking warnings
    warnings.rs     rule warnings (over-attunement, two body armors, ...)
  dice/
    parser.rs       notation -> DiceExpr (hand-rolled recursive descent)
    roller.rs       DiceExpr + Rng -> DiceResult
    compose.rs      fold modifier contributions into a roll expression
  clock/            pure arithmetic: dawn crossings, expiry transitions
  authz/
    mod.rs          can_read/can_write, view_of, EntityView, feed visibility
    view.rs         view types (mirror, not redacted clones)
  service/
    mod.rs          read-side services (list, get, derived, feed, autocomplete)
  testkit.rs        ScriptedRng, fixture builders (cfg(test) or feature "testkit")
```

`commands.rs` (the provisional `&mut` functions, ARCH-5) is replaced by `commands/`; the
function names and semantics carry over into the `Command` variants where they map
one-to-one (integration DESIGN section 5 predicted this rewrite as mechanical).

Adapter and composition root:

```
crates/localtt-adapter/src/
  lib.rs
  load.rs                 exists (schema gate)
  store/
    mod.rs                re-exports; store contract suite behind `pub mod contract`
    memory.rs             MemoryStore (in-memory CampaignStore)
    sqlite.rs             SqliteStore
    schema.rs             DDL + DDL migrations
    doc_migrations.rs     pure JSON document migrations
    export.rs             ExportBundle encode/decode, import planning
    backup.rs             rusqlite backup + rotation
  reference/
    mod.rs                IndexedSources (ReferenceSource)
    snapshot.rs           snapshot format + serde
    import/
      mod.rs              Importer trait + report types
      open5e.rs           pinned Open5e wotc-srd converter
    fixture.rs            FixtureSource (test/fixture entries)
    aliases.rs            alias table + one-hop resolution
  rng.rs                  ChaChaRng (OS-seeded)
  bin/localtt-content-convert.rs   dev conversion tool
  data/srd2014.json       generated snapshot (committed)
  data/srd2014.report.json

crates/localtt-server/
  src/lib.rs              router builder, DTO mapping, WS fan-out (later)
  src/main.rs             [[bin]] name = "localtt": CLI, wiring, startup banner
```

The composition root is `localtt-server`'s `[[bin]]`, named `localtt` for the installed
command, with the library kept separate so handler tests do not need the process. No fifth
crate: its only content today would be `main.rs` wiring, and the server already depends on
everything needed. If a headless CLI composition ever appears, it can call the same
`server::build_app(ports)` entry point without moving crates.

### 7.2 Command model

```rust
// engine::commands (sketch)

/// Serializable-in-principle catalogue; server DTOs map into it (no serde on the
/// engine type, so the HTTP wire format is not coupled to engine refactors).
pub enum Command {
    // clock
    AdvanceClock { by_seconds: i64, expected_rev: Option<u64> },
    SetClock { to: CampaignTime, expected_rev: Option<u64> },
    // entities
    CreateEntity { spec: NewEntitySpec },
    DeleteEntity { entity: EntityId },
    SetField { entity: EntityId, path: FieldPath, value: Value, expected_rev: Option<u64> },
    SetOwner { entity: EntityId, owner: PlayerId },
    SetAcl { entity: EntityId, acl: Vec<AclEntry> },
    GiveHandout { handout: EntityId, recipient: EntityId },
    FreezeReference { entity: EntityId, element: ElementRef },
    // hp / resources / rests
    ApplyDamage { entity: EntityId, amount: i32 },
    Heal { entity: EntityId, amount: i32 },
    SetTempHp { entity: EntityId, amount: i32 },
    SetDeathSaves { entity: EntityId, value: DeathSaves },
    SpendResource { entity: EntityId, resource: ResourceId, amount: i32 },
    RestoreResource { entity: EntityId, resource: ResourceId, amount: i32 },
    SpendHitDie { entity: EntityId, resource: ResourceId },
    ShortRest { entity: EntityId, advance_seconds: Option<i64> },
    LongRest { entity: EntityId, advance_seconds: Option<i64> },
    // casting / statuses
    CastSpell { entity: EntityId, spell: SpellEntryId, slot: ResourceId, upcast_slots: u32, concentrate: bool },
    BeginConcentration { entity: EntityId, status: StatusSpec },
    EndConcentration { entity: EntityId },
    AddStatus { entity: EntityId, status: StatusSpec },
    RemoveStatus { entity: EntityId, status: StatusId },
    EditStatusDuration { entity: EntityId, status: StatusId, duration: StatusDuration },
    // inventory
    AddItem { entity: EntityId, item: ItemSpec, qty: i32 },
    RemoveItem { entity: EntityId, entry: InventoryEntryId },
    SetItemState { entity: EntityId, entry: InventoryEntryId, state: ItemState },
    SetAttuned { entity: EntityId, entry: InventoryEntryId, attuned: bool },
    // dice and seats
    Roll { actor: PlayerId, entity: Option<EntityId>, expr: DiceExpr, context: RollContext, audience: Option<Audience> },
    CreateInvite { name: String, role: Role },
    RevokeInvite { player: PlayerId },
    ReissueToken { player: PlayerId },
}

/// Everything a command may consult that is not the working set. Port access is
/// limited to content lookup (read-only, deterministic per process) and dice.
pub trait CommandEnv {
    fn now(&self) -> UnixTime;
    fn reference(&self) -> &dyn ReferenceSource;
    fn roll(&mut self, expr: &DiceExpr) -> DiceResult;
}

/// Pure with respect to `state`: computes what should happen and what it costs.
/// State is not modified; the executor persists and then applies.
pub fn apply(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError>;

pub struct Transition {
    pub patch: StatePatch<'static>,   // owned documents; the store borrows
    pub warnings: Vec<Warning>,
    pub events: Vec<Event>,
}

pub enum Event {
    EntityChanged { id: EntityId, rev: u64, kind: EntityKind },
    EntityRemoved { id: EntityId },
    CampaignChanged,                 // clock, settings, players
    FeedAppended { ids: Vec<FeedEntryId> },
}
```

Executor:

```rust
pub struct Executor {
    state: Box<dyn WorkingSetMut>,
    store: Box<dyn CampaignStore>,
    reference: Arc<dyn ReferenceSource>,
    rng: Box<dyn Rng>,
    clock: ClockSource,             // wall time for UnixTime stamps; injectable
}

impl Executor {
    pub fn execute(&mut self, command: &Command) -> Result<Executed, EngineError> {
        // 1. apply(command) -> Transition      (pure, against current state)
        // 2. store.apply(&transition.patch)    (durable, one transaction)
        // 3. state.apply(&transition.patch)    (publish)
        // 4. Ok(Executed { warnings, events, reverted: false })
    }
}
```

- Single writer: the executor is `Send` but not `Sync`; the server serializes access
  (mutex or actor thread). At party scale either is fine; the engine does not care.
- Authorization happens inside `apply` via the actor carried in the command (each command
  variant has or implies the actor; the sketch folds it into the server DTO mapping).
  `apply` returns `Unauthorized` before producing a patch.
- Rules never reject: commands that violate 5e expectations succeed with warnings
  (`spend_resource` at 0, over-attunement, multiple concentration from a manual edit).
- Feed entries are produced as part of the transition (a roll creates its `Roll` and its
  `FeedEntry`; clock advance creates `StatusExpired` entries); the engine computes the
  ring trim cutoff (`feed_keep_from`) centrally.
- `expected_rev` semantics are defined in 5.5.

### 7.3 Error taxonomy

```rust
#[non_exhaustive]
pub enum EngineError {
    /// The actor may not read/write the target entity or perform the command.
    Unauthorized { actor: PlayerId, entity: Option<EntityId> },
    /// No such entity / element id inside an entity.
    NotFound { entity: EntityId, element: Option<ElementRef> },
    /// Optimistic concurrency mismatch (server: 409).
    Conflict { entity: EntityId, expected: u64, actual: u64 },
    /// Request-level invalid input (server: 400): bad dice notation, bad field
    /// path, cap exceeded, unknown content reference on an add command.
    Malformed { reason: String },
    /// The dice expression exceeded configured caps (count/sides/depth).
    TooLarge { what: &'static str, limit: u32 },
    /// Persistence failure, transparent.
    Store(StoreFault),
}
```

Rules-like conditions are not errors at any layer (RFC-0001 2.2): they are `Warning`s.
`StoreFault` stays confined to the store boundary; the engine wraps it.

### 7.4 Rules

```rust
pub struct DerivedSheet {
    pub values: BTreeMap<StatKey, DerivedValue>,
    pub warnings: Vec<Warning>,
}

pub struct DerivedValue {
    /// What the engine computes, when it knows how (None for text-only stats).
    pub computed: Option<i32>,
    /// The player's sparse override, if any (RFC-0001 2.3).
    pub override_value: Option<OverrideValue>,
    /// `override ?? computed` where both exist; the only value a client shows as
    /// "the" value. Both stay visible so drift is obvious (RFC-0001 8.2).
    pub effective: OverrideValue,
    pub breakdown: Vec<Contribution>,
}

pub struct Contribution {
    pub source: ContributionSource,   // Item(entry) | Feature(id) | Status(id) | Base
    pub label: String,                // "Ring of Protection", "Bless", "Armor (chain)"
    pub op: ContributionOp,           // Set/Add/Min/Max/Advantage/Disadvantage
    pub value: Option<i32>,           // None for advantage/disadvantage
}

pub fn derive(entity: &StatblockEntity, ctx: &RulesContext) -> DerivedSheet;
```

- `RulesContext` carries `now: CampaignTime`, and a **materialized** content map
  (`&BTreeMap<RefId, ContentEntry>`) for exactly the refs the entity mentions. The service
  layer resolves refs before calling `derive`; rules stay pure (RFC-0001 3.2) and
  unit-testable with literal maps.
- Pipeline order (5e shape, RFC-0001 5.1): collect base values -> apply `Set` ops
  (armor base AC) -> apply `Add` ops -> apply `Min`/`Max` clamps -> advantage and
  disadvantage cancel pairwise, leftovers become roll hints. Only modifiers whose `when`
  condition holds contribute: `Equipped`/`Attuned` from the inventory entry,
  `StatusActive { name }` from non-hidden statuses (case-insensitive, core DESIGN 3.9),
  `Custom` from table state (warn: not modeled; ignored).
- Dice leaves in static contexts evaluate to their flat part (0) with a
  `DiceInStaticContext` warning (core DESIGN 3.9); roll contexts compose them as
  attributed terms via `dice::compose`.
- Warnings returned alongside values, never errors: two body armors, over-attunement
  (more than 3), strength requirement unmet, spending at 0, negative HP, multiple
  concentration statuses, `UnresolvedReference`, `StaleOverride` (override set while the
  computed value later changed), `ZeroQuantity`.
- Known-not-done: multiclass slot tables, pact magic specifics, temp-HP/death-save
  interaction rules, cross-entity derivation (RFC-0001 4.3) all live in the rules unit(s)
  and are asserted against the golden-character catalog.

### 7.5 Dice

- Parser: hand-rolled recursive descent per RFC-0001 10; `NdM`, `kh/kl`, advantage/
  disadvantage, multi-term `+`/`-`, `*`. AST is the core type (it is persisted in `Roll`).
  Caps: count <= 100, sides <= 1000, depth <= 16, terms <= 32; exceeding returns
  `TooLarge` (input error, not a rules warning).
- Roller: `roll(expr, rng) -> DiceResult` fills per-die values and `kept`, per-term
  totals and attribution labels. Advantage/disadvantage on a composed expression rolls
  the d20 term twice and keeps one, preserving nat-1/nat-20 detection (RFC-0001 5.6).
- Composition: `compose(base: DiceExpr, contributions: &[Contribution]) -> DiceExpr`
  appends attributed terms (`TermResult::source` labels).
- Determinism: everything routes through `Rng`; the executor owns the concrete RNG, so a
  test can replay a whole campaign's rolls.

### 7.6 Clock and the expiry index

- The clock is seconds on the campaign document (core `Clock { now_s }`). Rounds are
  derived (6 s, RFC-0001 5.3); nothing round-shaped is stored.
- The engine owns the only runtime structure: a global expiry index.

```rust
// state::LoadedCampaign (sketch)
expiries: BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>,
```

- **Why a sorted map and not a hierarchical timing wheel** (party scale, tens of
  statuses):
  - Workload: ~10 entities x ~5 statuses = ~50 live timers typical; a pathological
    campaign (1,000 entities, 2 statuses each) is ~2,000. Timer operations happen on
    discrete commands (a handful per minute), not thousands per second.
    A `BTreeMap` insert/remove/range is O(log n) = ~11 comparisons at n = 2,000,
    tens of nanoseconds; the measured cost is dominated by JSON serialization of the
    entity itself. A timing wheel's O(1) insert buys nothing at this rate; its
    hierarchical variant is designed for millions of timers per second.
  - **Clock rewind is a first-class operation** (RFC-0001 5.3, L2): re-running `advance`
    over a rewound clock must re-fire transitions. A sorted map answers "what is in
    (old, new]?" with one range query; a wheel must be re-bucketed or made reversible.
  - **Soft expiry** (L2, confirmed) means the index is not the truth: truth is the
    predicate `now >= expires_at` evaluated per status; the index only enumerates
    candidates. Rebuilding it after a load or an inconsistency is `O(n log n)` over the
    loaded bodies and is property-tested against a recomputation.
  - Cancellation is cheap: removing a status removes its index entry (or leaves a stale
    entry that is validated on read; both are fine, the invariant test catches drift).
- Operations:
  - Status added/edited/removed -> index entry maintained by `state.apply` (it sees the
    old and new documents and can diff the `Timed` statuses).
  - Clock advances/sets -> `clock::transitions(old, new, index)` yields:
    - newly expired `(entity, status)` pairs (old not expired, new expired) -> feed
      entries of kind `StatusExpired` to the owner and the DM (RFC-0001 5.3);
    - un-expired pairs (rewind) -> silent, plus one `DawnRewind`-style warning when the
      move crosses a dawn instant backwards (dawn recharge is not reversed, owed to the
      discrete nature of recharge).
    - dawn instants in `(old, new]` -> `Recharge::Dawn` resources refilled; the count
      is exact integer arithmetic over `SECONDS_PER_DAY` and `settings.dawn_hour`.
      A dawn instant is a day-shifted midnight, so `days_since_start` on the
      dawn-shifted times is the count; `div_euclid` floors, so pre-origin times
      (rewind) behave.
- The core calendar (DATA-12, landed: `days_since_start`, `weeks_since_start`,
  `day_of_week`, `time_of_day`, tenday weeks) is the single source of calendar
  truth. The engine consumes it for crossing arithmetic and for view/display; no
  calendar-derived state is ever persisted, and no runtime state moves into core
  (constraint). Engine-visible API sketch:
  `clock::crossed_dawns(old: CampaignTime, new: CampaignTime, dawn_hour: u8) -> u32`.
- Rest commands take an explicit `advance_seconds` (server UI proposes 3600/28800, L1
  "prompt, skippable"); the engine never invents time, which keeps tests deterministic.

### 7.7 Authorization and views

- `authz::can_read(entity, viewer)` / `can_write(entity, viewer)` implement RFC-0001
  5.7: owner full rw, DM full rw, `AclLevel::Write` rw, `AclLevel::Read`
  read-with-mask, `field_mask` default-deny (DATA-9).
- `view_of(entity, viewer, resolved_content) -> EntityView`. **View types are
  engine-owned and distinct from core documents.** They are mirrors with optional
  sections, never redacted clones: core deliberately has no `Default` for critical
  sections (DATA-10), so zeroing a hidden `Abilities` would be indistinguishable from a
  real zero. Sketch:

  ```rust
  pub struct EntityView {
      pub id: EntityId,
      pub kind: EntityKind,
      pub owner: PlayerId,
      pub parent: Option<EntityId>,
      pub rev: u64,
      pub writable: bool,
      /// Sections the viewer may not see, so a UI can render "hidden".
      pub hidden: BTreeSet<ViewField>,
      pub name: Option<String>,
      pub statblock: Option<StatblockView>,   // each section Optional, same rule
  }
  ```

  The exhaustive field-by-field mapping is pinned by the `authz` unit spec and its
  matrix test (every viewer role x ACL state x kind x egress path, RFC-0001 5.7).
- Feed visibility: `Public` -> everyone; `RollerAndDm` -> roller + DM; `RollerOnly` ->
  roller only (open question OQ-6 confirms DM visibility semantics). The DM can always
  read the file; this is social secrecy (RFC-0001 2.11), documented.
- Derived values in views are computed for the target entity once, then masked per
  viewer; the WS fan-out sends per-recipient views (RFC-0001 5.11), never documents.

### 7.8 Services and read paths

Thin functions over the executor/working set; the server calls only these:

- `list_entities(viewer) -> Vec<EntityView>` (headers filtered by `can_read`, bodies
  masked; headers-only variant when the caller does not need bodies).
- `get_entity(id, viewer) -> Result<EntityView, EngineError>`.
- `derived(id, viewer) -> Result<DerivedSheet, EngineError>` (values + breakdowns +
  warnings; overrides preserved; RFC-0001 9.3 item 9 says engine always returns both).
- `feed_after(viewer, cursor, limit) -> Vec<FeedEntryView>`.
- `autocomplete(query, kinds, limit)` (pass-through to the port; no authz).
- `export(viewer)` (DM only) via the store; player sheet export later.

### 7.9 Events and the feed

- `Event`s (section 7.2) are delivery hints for the server: they carry ids and revs, not
  documents. The server re-reads through `view_of` for each recipient, which keeps the
  single egress point intact.
- `FeedEntry`s are persisted domain records, not delivery hints; the ring is trimmed on
  append by the engine-computed `feed_keep_from`.
- No `EventSink` port in v1 (section 4.5).

## 8. Error and versioning story

### 8.1 What changes in core, and how

All additions, no document changes, no `SchemaVersion` bump:

| Addition | Kind | Blast radius |
|---|---|---|
| `core::ports` (traits, `CampaignSnapshot`, `StatePatch`, `StoreFault`) | New public module; additive | None for existing files or consumers. Adapter/engine gain a dependency direction they already have. `cargo hack --feature-powerset` unaffected (no features). |
| `core::content` (`ContentEntry`, `ContentKind`, `ContentData`, `Provenance`, `ContentSummary`, `SearchQuery`) | New public module; additive | None: never stored in campaign documents; only `RefId` + `ContentRef.name` are (unchanged, DATA-7). |
| `core::export` (`ExportBundle`) | New public format; versioned by `format_version` | New files only; import refuses unknown envelope versions. |
| Calendar functions (landed in core as DATA-12; additive) | Pure derived functions | None: `CampaignTime` arithmetic only, no new stored fields or schema change. The engine consumes them in `clock`; no persisted calendar state. Levelled at day/week (tenday)/time-of-day; named months and an absolute world clock stay deferred. |

No blocking core change is required to start any unit. If, during a unit, a wire change
turns out to be necessary, it follows the RFC-0001 / core DESIGN process: a separate
minimal proposal, a version bump, a migration, and fixtures per version.

### 8.2 Versioning surfaces

| Surface | Versioned by | Policy |
|---|---|---|
| Campaign/entity documents | `SchemaVersion` major.minor | Existing VER-2 policy: refuse newer major, accept newer minor, re-stamp on load. |
| Export bundle | `ExportBundle::format_version` | Refuse unknown; no in-place upconvert in v1. |
| Content snapshot | `snapshot_version` + pinned source revision | Regenerated per release; unknown version refused at load; aliases bridge renames. |
| Store DDL | `PRAGMA user_version` | Refuse a newer DDL version; ordered migration steps. |
| HTTP API | server-owned URL/DTO version | Server unit; DTOs never expose engine types directly. |

### 8.3 Failure behavior

- Newer document major -> `StoreFault::VersionRefused` -> server refuses to start with a
  clear message (never partial load).
- Newer DDL version -> same.
- Corrupt document -> `StoreFault::Corrupt`; startup stops; the startup backup from the
  previous run is the recovery path.
- Simulated corrupt/migration tests live in the store contract suite; fixtures per
  version live under `fixtures/campaigns/`.

## 9. Testing strategy

| Layer | What is tested | How |
|---|---|---|
| `core::ports` data | serde of export bundles; patch/snapshot construction | unit + round-trip tests |
| Store | one contract suite, two implementations (memory, SQLite tempdir) | `store_contract` harness (5.9); fidelity is byte-for-byte |
| Store/state | "store mirrors state": after any command sequence, `store.load()` equals the engine state, serialized | proptest over command sequences; reload-and-compare |
| Engine commands | command semantics: overspend warns, temp HP, concentration replace, rev monotonicity, conflict on stale rev, feed ring trimming | unit tests + proptest invariants |
| Expiry | index equals recomputation; advance/rewind transitions; dawn crossing counts; un-expire is silent | proptest (clock ranges, rewind) + fixed cases |
| Rules | golden characters (integration catalog): every derived stat with breakdown; overrides win but computed stays; warnings | integration suite, `derive` against fixture entities |
| Dice | parser round-trip on ASTs, caps, per-die results, advantage semantics, distribution sanity | unit + seeded RNG + contract suite for `Rng` impls |
| Authz | full matrix: viewer role x ownership/ACL/mask x entity kind x egress (list, get, derived, feed, WS payload) | table-driven matrix test; a property asserts hidden section content never appears in the serialized view |
| Content | converter determinism (byte-identical rerun), alias resolution, ranking order stability, autocomplete/description budgets | fixture source + committed snapshot hash + `just bench` |
| Migrations | every committed fixture version loads and migrates; newer major refused | fixture-driven tests |
| Concurrency | engine is synchronous, single-writer, no shared state: nothing to loom/shuttle. The server's serialization point (mutex/actor) is tested where it lands (server unit), with shuttle/loom if it holds shared state | documented; revisit in server unit |

Deliberate omissions: no snapshot (`insta`) tests yet (PROC-2: worth diffing only once
the engine exists); no mutation testing (no recipe, PROC-2); wall-clock latency tests
(the budgets live in benches, not CI asserts).

## 10. Phased plan

Workflow per unit: `spec` (RFC + client-facing target test) -> `skeleton` (`todo!()`
declarations, target tests fail with `not yet implemented`) -> `implement` (spike first,
then unit by unit). Dependencies are added with `cargo add` as each unit needs them
(`rusqlite` with the `bundled` feature, `proptest`, `tempfile`, `rand_chacha`, `clap`,
`axum`/`tokio` in the server unit). No production code before this document is approved.

**Phase D -- this document.** Approve, fold section 11 into `docs/decisions.md`, answer
section 12, then start U1.

**U1 -- store (first, because it pins ports and the fidelity claim).**
- Spec `docs/rfcs/store.md` + target test: client code that builds the fixture campaign,
  applies a patch, reloads, compares bytes; failure cases: newer major refused, corrupt
  document refused, patch atomicity.
- Skeleton: `core::ports::store`, `adapter::store::{memory, sqlite, schema,
  doc_migrations, export, backup}`, store contract suite shape.
- Implement: memory store, SQLite store + DDL, gate behind extracted columns, WAL/backup,
  export/import, contract suite green; integration suite still green.
- Also lands: `fixtures/campaigns/v1/` fixture bundle, `core::export::ExportBundle`.

**U2 -- content (parallel-safe after U1's `core::ports` exists; self-contained).**
- Spec `docs/rfcs/content.md` + target test: load the embedded SRD source, `get` a
  spell/monster/item, autocomplete "fireb" returns Fireball first, kind filter, alias
  resolution, unmatched ref -> `None` (engine warns later).
- Skeleton: `core::content`, `adapter::reference::{snapshot, index, fixture, aliases}`,
  converter bin, `Importer` trait.
- Implement: Open5e converter against the pinned checkout, committed snapshot + report +
  hash, deterministic ordering, prefix ranking, attribution, budget bench.

**U3 -- engine core (needs U1's ports and store; content lookups only for commands that
need them).**
- Spec `docs/rfcs/engine-executor.md` + target test: executor over the memory store;
  clock advance expires a status and appends feed entries; spend-at-zero warns and
  succeeds; stale `rev` conflicts; reload matches state.
- Skeleton: `engine::{state, executor, error, commands, clock}`, `Command` enum slice
  (clock, statuses, hp, resources, rests), `WorkingSet`/`WorkingSetMut`, `Event`,
  `Warning`.
- Implement: `LoadedCampaign` + expiry index, command slice, rev stamping, durable-before-
  visible commit, feed trimming, property tests.

**Later units, in order:**
- U4 rules: `engine::rules` (pipeline, formulas, derived values, warnings) + golden
  character assertions in the integration suite.
- U5 dice: parser, roller, composition, `Rng` impls + contract suite; `Roll` command.
- U6 authz: `view_of` + view types + the matrix test (before any networking).
- U7 server: `[[bin]] localtt`, axum REST + WS, join flow, DTO mapping, per-recipient
  fan-out, OpenAPI/TS generation, composition + startup banner.
- U8 packs: TOML + frontmatter pack loader (manifest, alias handling); `FreezeReference`
  command.
- U9 deferred: FTS5/tantivy if search outgrows the in-memory index; PDF export/import;
  player sheet export; attachments (maps/images) with the lazy-body swap.

## 11. Decisions (folded into `docs/decisions.md`)

All entries below were accepted in review (2026-10-17) and copied into
`docs/decisions.md` under the same ids.

### Engine

- **ENG-1: ports live in `core::ports`; adapter implements them.** -- `adapter` may not
  depend on `engine` (ARCH-3), so any trait the store implements must be visible to both;
  RFC-0001 3.2 already assigns `ports/` to core. DTOs (`StatePatch`, `CampaignSnapshot`,
  `StoreFault`, content types) live in core outside `types.rs`, so the wire contract is
  untouched. -- Accepted.
- **ENG-2: the command model is `(state, command) -> Transition`, and the executor
  persists the patch before publishing it.** -- Commands stay pure and testable; durable
  before visible removes rollback handling; a patch is exactly one transaction. -- Accepted.
- **ENG-3: all reads go through `WorkingSet`; the default implementation is load-all;
  lazy/LRU is an implementation swap.** -- Party scale makes load-all trivial; the trait
  keeps the door open for attachments and large campaigns; reads return owned values so a
  lazy implementation is possible without borrow fights. -- Accepted.
- **ENG-4: `rev` is checked in the engine against in-memory state; the store is a dumb
  single writer.** -- One writer, one authority; SQL compare-and-swap would be dead code.
  Server maps conflicts to 409. -- Accepted.
- **ENG-5: the composition root is `localtt-server`'s `[[bin]]`, named `localtt`.** -- No
  fifth crate whose only content is wiring; library stays testable; a future headless CLI
  can reuse `build_app`. -- Accepted (RFC-0001 9.3 item 1, closed).
- **ENG-6: the expiry index is a `BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>`;
  timing wheels are rejected.** -- Tens of timers at discrete command points; rewind is
  first-class; soft expiry makes the index a candidate enumerator, not the truth. Number
  argument in section 7.6. -- Accepted.
- **ENG-7: no `EventSink` port in v1; commands return events.** -- One synchronous
  egress model; a second publish path would compete with `view_of` and fan-out tests.
  Revisit only for timer-driven events that no command produces. -- Accepted.
- **ENG-8: `view_of` produces engine-owned view types, never redacted clones.** -- Core
  has no `Default` for critical sections (DATA-10); zeros would be ambiguous. View types
  carry `hidden` sections and a `writable` flag. -- Accepted.
- **ENG-9: the engine consumes the core calendar functions (DATA-12, landed); crossing
  arithmetic is exact integer seconds plus `dawn_hour`.** -- No runtime state in core;
  no persisted calendar fields; the calendar cannot disagree with the clock. -- Accepted.

### Store

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

### Content

- **CONTENT-1: vendor SRD 5.1 as an Open5e `wotc-srd` snapshot, pinned by commit and
  SHA-256, redistributed under WotC's CC-BY-4.0 with attribution surfaced in the app.** --
  Verified scale (1,016 records) and field coverage; the alternatives are OGL-murky
  (dnd5eapi, archived) or a brittle PDF parser. Re-pinning is a review event. -- Accepted.
- **CONTENT-2: ingestion and serving are separate, provider-neutral layers; importers
  emit a common snapshot format and hand-authored packs normalize into the same model
  (CONTENT-8); `ReferenceSource` is the only read port.** -- The maintainer requirement:
  PDF, web, other databases and custom formats are future `Importer`s, not engine
  changes. `ext` carries unmapped fields verbatim. -- Accepted.
- **CONTENT-3: namespaces are source-scoped (`srd2014`, `pack.<id>`); slugs come from the
  source where stable, collisions are converter errors, and renames go through an
  `aliases.json` one-hop table; campaigns are never rewritten automatically.** -- Refs are
  the join key forever; alias resolution keeps old sheets working. -- Accepted.
- **CONTENT-4: campaign files keep refs + name snapshots; unresolved refs warn and freeze
  is explicit.** -- RFC-0001 DATA-7 plus a real escape hatch; silent mechanics loss is
  prevented by the warning. -- Accepted (RFC-0001 9.3 item 2, closed).
- **CONTENT-5: autocomplete is an in-memory ranked prefix index with deterministic
  tiers; FTS5, fuzzy and trigram search are deferred behind the port.** -- 1,016 entries,
  microsecond lookups, integer/string ranking only; description search ships as substring
  with the same budget; `just bench` guards the 10,000-entry, 10 ms budget. -- Accepted.
- **CONTENT-6: the snapshot is embedded in the binary, parsed once; packs are files
  loaded at startup; no runtime network.** -- Single binary, offline play, no cache
  invalidation. -- Accepted.
- **CONTENT-7: lossiness is explicit: `needs_review` + `review_notes` per entry, a
  committed import report, and unmapped fields under `ext`.** -- Fidelity is auditable
  and re-minable when the mapping improves. -- Accepted.
- **CONTENT-8: hand-authored packs are TOML + frontmatter; importers emit snapshot JSON;
  the loader normalizes both into `ContentEntry`.** -- Maintainer decision (section 12
  Q7): packs are written and read by hand, so they get the human-friendlier format; the
  two formats meet in one in-memory model. Exact file layout is pinned in the packs unit.
  -- Accepted.

## 12. Question answers (settled 2026-10-17)

All eight questions were answered in review. The answers bind the unit specs; where an
answer changed the design, the affected section was updated in the same pass.

1. **Player-only `RollerOnly` visibility** -> **roller-only, both roles**: the feed entry
   is delivered to the roller and to no one else, the DM included (the DM can still read
   the file). RFC-0001 5.6 read literally.
2. **Freeze scope** -> **single-element first**: campaign-wide "freeze all used" stays a
   later additive convenience (section 6.7).
3. **SRD namespace spelling** -> **`srd2014`**, matching the core test example
   (`srd2014:spell/fireball`) and unambiguous against a future 2024 SRD (section 6.3).
4. **Fixtures directory** -> **root `fixtures/`**: store tests and the integration suite
   share one source (U1, section 10).
5. **Converter binary home** -> **`localtt-adapter` `src/bin/`** (the dev/release tool
   `localtt-content-convert`, section 6.6).
6. **Rest clock default** -> confirmed: the engine takes explicit `advance_seconds`; the
   server proposes **1 h (short) / 8 h (long)** and the DM skips or confirms (section 7.6).
7. **Content pack format** -> **TOML + frontmatter** (maintainer decision; the snapshot
   JSON recommendation was not taken). Recorded as CONTENT-8: importers keep the snapshot
   JSON, the pack loader normalizes packs into `ContentEntry` (sections 6.2, 6.6); exact
   file layout pinned in the packs unit.
8. **`localtt` startup flags** -> **deferred to the server unit**; nothing in this design
   fixes or blocks the flag surface (`--campaign` positional vs flag, `--lan`, `--port`,
   `--no-backup`).

## 13. Explicit answers to the two driving questions

### (1) Does the serde protocol work as the storage format and as the sharing/export format?

**Yes, validated with one caveat.** The wire documents (`Campaign`, `AnyEntity`,
`FeedEntry`) are stored verbatim in JSON `doc` columns, next to a small extracted
projection (`id/kind/body_kind/owner/parent/name/rev/schema_version`) that exists for the
version gate, integrity checks and future SQL queries. The existing determinism
guarantees (DATA-4/5) and the existing version policy (VER-1/VER-2, `load_checked`) carry
over unchanged; fidelity is a contract-test promise asserted byte-for-byte. Export is
re-serialization of the same documents in a versioned envelope; import gates and applies
in one transaction into an empty store. The caveat is the one VER-3 already names: only
queried fields are extracted, so anything that wants SQL-level filtering of ACLs or status
expiry must first add a projection or move that work into the engine -- both are additive.

The store interaction model: one SQLite file, one writer (the executor), one transaction
per command (`apply(patch)`), durable-before-visible, WAL + startup online backup,
load-all working set behind `WorkingSet` (LRU is a swap), `rev` checked in the engine,
DDL + document migrations with fixtures per version, and the single-process documented.

**Deferred**: lazy body loading/LRU (trait ready, no consumer pressure), ACL projection
tables, binary attachments (maps/images; separate table or sidecar when they land),
merge-import, player-scoped export bundles, multi-process access.

### (2) How do we read the 5e source and keep `RefOrText` honest?

We vendor **SRD 5.1 (CC-BY-4.0)** as an **Open5e `wotc-srd` snapshot pinned by commit
and SHA-256**, converted by our own tool into a **provider-neutral snapshot**, embedded in
the binary, and served behind `ReferenceSource` (`get`/`search`/`autocomplete`).
Autocomplete is a deterministic in-memory ranked prefix index (exact > prefix >
word-prefix > substring, kind-filtered), with a 10x budget margin over SRD scale; FTS5,
fuzzy and trigram search are deferred behind the unchanged port. Campaign files store
`RefId` + name snapshots, never source documents; unresolved references warn during
derivation; `FreezeReference` converts one element inline for portability. Ingestion is
pluggable (Open5e today; packs, PDF, other databases, one-shot web fetch later), because
`ext` preserves unmapped fields and every importer emits the same snapshot format.

**Deferred**: description FTS5/tantivy, fuzzy search, pack loader (U8), PDF import, live
web providers (incompatible with offline play; they become one-shot importers if ever),
structured parsing of spell damage/DCs and monster recharge-by-roll (kept as text with
`needs_review`), slug-rename automation (aliases only).
