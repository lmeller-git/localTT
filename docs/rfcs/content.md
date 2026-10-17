# RFC: U2 -- content access and the SRD snapshot

| | |
|---|---|
| **Status** | Skeleton committed 2026-10-17 (`7977049` on `u2-content`): the declarations exist with `todo!()` bodies and the eleven target tests fail with `not yet implemented` until the implement phase. Revised 2026-10-17 after the pre-implementation adversarial review (S1-S6, N1-N9): CONTENT-12..15 below, the section 5 policy amendments, and the fold into `docs/decisions.md` and `docs/engine-architecture.md`. |
| **Scope** | `localtt-engine::content`, `localtt-adapter::reference` (`source`, `index`, `snapshot`, `fixture`, `aliases`, `import::open5e`), the converter bin `localtt-content-convert`, the target test `crates/localtt-integration/tests/content_target.rs`, the contract suite `crates/localtt-adapter/tests/reference_contract.rs`, and the committed data at `crates/localtt-adapter/data/srd2014/`. |
| **Read with** | `docs/engine-architecture.md` sections 4.3 and 6 (driving question 2), section 9 (testing), section 10 (U2); `docs/decisions.md` CONTENT-1..15; `crates/localtt-core/DESIGN.md` (`RefId`, `Namespace`, payload types); `crates/localtt-integration/DESIGN.md` (the fixture cast). |
| **Pins** | The `docs/engine-architecture.md` sketches are explicitly "not compiling Rust"; this RFC pins the content subset of them and amends the design where the maintainer's decisions changed it (section 8 marks every deviation). |

## 1. Goals

1. Pin the engine-owned content model and read port: `engine::content::{ContentKind,
   ContentEntry, ContentData, Provenance, ContentSummary, SearchQuery, ContentFault,
   ReferenceSource}` with a fallible `ReferenceSource` whose `Ok(None)` stays distinct
   from a failure.
2. Pin the runtime snapshot format: one directory per namespace, `index.json` (eager
   names, offsets, version, provenance) + `entries.jsonl` (lazy bodies) + optional
   `aliases.json` + `report.json`, with the index written last as the completeness
   marker.
3. Pin the strict loader and its failure classes: `IndexedSources::{from_dirs,
   from_readers}` over `SnapshotReader`s; missing root or namespace file ->
   `Unavailable`, malformed content -> `Corrupt`, newer snapshot ->
   `SnapshotVersionRefused`, duplicate namespace -> `NamespaceConflict`.
4. Pin ranking for `autocomplete` and `search`: four name tiers (exact, name prefix,
   word prefix, substring), a description tier below them for `search`, and a total
   sort key `(tier, kind priority, namespace, name, id)`.
5. Pin one-hop alias resolution: `get` follows an alias, search never surfaces alias
   ids, invalid tables are refused at load.
6. Pin the in-memory `FixtureSource` and the contract suite that runs against both it
   and `IndexedSources`.
7. Pin the ingestion seam (`Importer`), the Open5e converter mapping and report,
   `write_source` determinism, and the `localtt-content-convert` binary surface.
8. Record the decisions that amended the approved architecture (runtime sources replace
   embedding; index + JSONL; fallible port; async backend dispatch; serde-additive
   payload evolution; discovery and alias scoping) as CONTENT-9..15, folded into
   `docs/decisions.md` on 2026-10-17.

The target test is written first and lives at
`crates/localtt-integration/tests/content_target.rs`; section 3 is its map.

## 2. Non-goals

- **No network code in this unit.** Offline play (RFC-0001 5.10) holds: local
  directories are always sufficient. Direct async REST is in scope for the runtime:
  a later unit adds a backend dispatcher (a `load_link_from_ref`-style async call
  routed by namespace; CONTENT-12) whose backends include local snapshots and live
  RESTful servers. U2 pins only the local, blocking backend that the dispatcher
  wraps; the dispatcher itself and any REST client are out of scope.
- **No live backend as a `SnapshotReader`.** A remote backend is not a
  `SnapshotReader`; that trait is the local reader seam (sections 4.2 and 6.2).
  Routing live backends is the dispatcher's job.
- **No embedding.** CONTENT-6's `include_bytes!` + `OnceLock` plan is amended by
  CONTENT-9: the app reads configured source directories at startup, and the
  repository ships the converted snapshot as data.
- **No packs loader** (U8), no PDF/web importers, no pack TOML.
- **No FTS5, tantivy, fuzzy or trigram search**, and no description index. `search`
  tests descriptions by reading bodies; its latency bench and any indexing are
  deferred behind the unchanged port (CONTENT-5).
- **No full stat block extraction.** U2 payloads are the minimal typed subset
  (spell level/school/conc/ritual; monster abilities/CR/AC/HP); everything else stays
  in the description text and in `ext` until a later, additive widening.
- **No monster feature entries and no item property mapping.** The raw source records
  are preserved verbatim under `ext`, so a later converter revision can mine them.
- **No engine integration.** Rules derivation, executor wiring and `CommandEnv` are
  U3+; this unit only ships the port and its implementations.
- **No report SHA-256 and no runtime data-root resolution.** The hash is deferred
  (section 9); `SHIPPED_DATA_ROOT` is the build-time in-checkout constant used by the
  tests and the workspace default, not an installed-layout promise, and installed
  default paths plus CLI flags are composition-root and packaging work (U7).
- **No cross-source merge semantics** beyond namespace composition: duplicate names
  across kinds are expected, duplicate namespaces are refused.

## 3. Client usage: the target test

`crates/localtt-integration/tests/content_target.rs` is ordinary client code: it opens
the shipped `srd2014` data (via `SHIPPED_DATA_ROOT`), reads typed entries, ranks
autocomplete hits, writes one temp snapshot to exercise aliases, and probes the strict
loader. Eleven tests:

| Test | What it pins |
|---|---|
| `srd_spell_payload_is_typed` | `get` on a shipped spell: kind, name, description text, `SpellData` fields, entry provenance namespace |
| `srd_monster_payload_is_typed` | `MonsterData`: abilities, challenge rating, AC, HP |
| `srd_item_payload_reuses_the_core_shape` | `ContentData::Item(InlineItem)`, the attunement presence rule, item kind mapping |
| `autocomplete_fireb_returns_fireball_first` | ranked prefix hit: `"fireb"` -> Fireball first |
| `autocomplete_ranks_name_prefixes_above_word_prefixes` | tier order: name prefix (`Fireball`) before word prefix (`Wand of Fireballs`) |
| `kind_filter_order_decides_ties` | exact-name tie broken by the caller's kind filter order (`Shield` spell vs item) |
| `aliases_resolve_one_hop` | `write_source` + a directory source: `get(alias)` returns the target entry |
| `unknown_reference_is_none_not_a_fault` | `Ok(None)` for an unknown ref and for `srd2014:background/sage` |
| `sources_report_the_snapshot_provenance` | `sources()`: namespace, license, pinned revision, attribution, url |
| `a_missing_root_is_unavailable` | strict roots: a missing root is `ContentFault::Unavailable` |
| `a_corrupt_index_is_refused` | a malformed `index.json` is `ContentFault::Corrupt` |

Notes:

- **Skeleton exists; the tests still fail on purpose.** `engine::content` and
  `adapter::reference` are declared with `todo!()` bodies, so all eleven tests fail
  with `not yet implemented` until the implement phase (the spec-phase note said
  "does not compile yet"; the skeleton commit is `7977049`).
- **The shipped assertions need committed data.**
  `crates/localtt-adapter/data/srd2014/` is generated by the converter in the
  implement phase (section 6.4). Until then the tests panic at `todo!()`, before
  touching the data.
- **`srd2014:background/sage` is the honest negative case.** The golden cast
  (`localtt-integration/src/fixtures.rs`) references it, but SRD 5.1 contains only
  Acolyte; the ref resolves to `Ok(None)` and the rules layer warns later (CONTENT-4).
  This RFC does not change the fixture; section 9 records the finding for U8.
- The test does not inspect the snapshot files raw; wire-format checks, version
  refusal and duplicate-namespace cases belong to the contract suite (section 6.6).

## 4. Public API sketch

Sketches, not compilable code; signatures are the contract. `#[non_exhaustive]` enums
are matched by consumers with `..`.

### 4.1 `localtt_engine::content`

```rust
/// Closed set of reference kinds (the `<kind>` segment of a `RefId`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Spell, Item, Monster, Condition, Class, Subclass, Race, Background, Feat, Feature, Rule,
}

impl ContentKind {
    /// Every kind, in the fixed priority order used for unfiltered ranking.
    pub const ALL: [ContentKind; 11];

    /// The `<kind>` segment spelling, e.g. `"spell"`.
    #[must_use]
    pub fn as_str(self) -> &'static str;
}

/// A source record adapted to the localtt model. This is what `get` returns and
/// what snapshots serialize; never stored inside campaign documents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentEntry {
    pub id: RefId,
    pub kind: ContentKind,
    pub name: String,
    /// Markdown; opaque (sanitization is a frontend concern, RFC-0001 8.7).
    pub description: String,
    /// Structured payload, typed per kind.
    pub data: ContentData,
    pub provenance: Provenance,
    /// Conversion fidelity: true when the importer had to guess.
    pub needs_review: bool,
    /// Why the entry needs review, in human-readable form.
    pub review_notes: Vec<String>,
    /// Unmapped source fields, verbatim, under `ext["src.<format>"]`.
    pub ext: Ext,
}

/// Structured payload, typed per kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentData {
    Spell(SpellData),
    /// Reuses the core item shape, including modifiers.
    Item(InlineItem),
    Monster(MonsterData),
    /// Class, race, feat and monster features.
    Feature(FeatureData),
    /// Conditions, rules, classes, races, backgrounds, feats.
    Text,
}

/// The minimal U2 spell payload; the rest of the stat block is description text
/// and `ext` until the rules unit widens it (additive).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellData {
    pub level: u8,
    pub school: String,
    pub concentration: bool,
    pub ritual: bool,
}

/// The minimal U2 monster payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterData {
    pub abilities: Abilities,
    pub challenge_rating: ChallengeRating,
    pub armor_class: i32,
    pub hit_points: i32,
}

/// A feature-shaped payload (class/race/feat/trait entries).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureData {
    pub category: FeatureCategory,
    pub text: String,
}

/// Provenance of one imported snapshot, recorded per entry and reported by
/// `sources()`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Source namespace, e.g. `srd2014` or `pack.mytome`.
    pub namespace: Namespace,
    /// Human-readable source document, e.g. `"SRD 5.1 (CC-BY-4.0), via Open5e"`.
    pub source: String,
    /// Pinned source revision, e.g. a commit SHA plus dataset version.
    pub source_revision: String,
    /// License identifier and attribution text to surface in the UI.
    pub license: String,
    pub attribution: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Lightweight hit for lists and autocomplete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentSummary {
    pub id: RefId,
    pub kind: ContentKind,
    pub name: String,
}

/// One search or autocomplete request.
#[derive(Clone, Copy, Debug)]
pub struct SearchQuery<'a> {
    pub text: &'a str,
    /// Empty means all kinds; order is the kind priority (section 5.3).
    pub kinds: &'a [ContentKind],
    pub limit: usize,
}

/// Failure of a content operation. `message` fields are diagnostic, not
/// user-facing; the server maps variants to status codes without forwarding the
/// messages (same rule as `StoreFault`).
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ContentFault {
    /// The source root or a required file is missing or unreadable.
    Unavailable { message: String },
    /// A snapshot file, index row or entry body failed to parse or validate.
    Corrupt { message: String },
    /// A snapshot's on-disk version is newer than this build supports.
    SnapshotVersionRefused { found: u32, supported: u32 },
    /// Two loaded sources share one namespace.
    NamespaceConflict { namespace: Namespace },
}

/// Read-only content access for the engine. Implementations are immutable after
/// construction (snapshots are pinned), so caching is trivial: load once.
pub trait ReferenceSource: Send + Sync {
    /// Resolves one entry, following a one-hop alias (section 5.4). `Ok(None)`
    /// means "not in any loaded source"; a failure means the source is broken.
    fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault>;

    /// Name-and-description search, deterministic order (section 5.3).
    fn search(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault>;

    /// Ranked name autocomplete (section 5.3).
    fn autocomplete(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault>;

    /// Sources (namespaces) currently loaded, in load order.
    fn sources(&self) -> Vec<Provenance>;
}
```

Design notes:

- `ContentEntry` is deliberately serde-free (the same mechanism as `ExportBundle`,
  STORE-9): the adapter owns crate-private wire types and the only parse path goes
  through them, gated. `ContentKind`, `ContentData`, the payloads and `Provenance`
  derive serde because the wire types embed them (`index.json` embeds `Provenance`;
  `ContentKind` is an index row field).
- `ContentData` uses the same internally-tagged `"type"` shape as core's
  `InventoryItem`, so the future server DTO mapping is mechanical.
- `SearchQuery` borrows its text and kinds: no allocation per call, and the struct is
  `Copy`.
- `ContentKind` has no `parse`: ids carry the kind segment; the enum is a typed view
  over it, and `as_str` is the single spelling source.

### 4.2 `localtt_adapter::reference`

```rust
/// The committed data root that ships with this repository: `<crate>/data`.
/// The `srd2014` namespace lives under it.
pub const SHIPPED_DATA_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");

/// The current on-disk snapshot version.
pub const CURRENT_SNAPSHOT_VERSION: u32 = 1;

/// One loaded snapshot, read for indexing. This is the seam where a future
/// HTTP or pack reader plugs in: `IndexedSources` only needs these four calls.
pub trait SnapshotReader: Send + Sync {
    /// Namespace and licensing of the snapshot.
    fn provenance(&self) -> &Provenance;

    /// Every entry's summary (id, kind, name), eager; bodies stay behind
    /// `entry`.
    fn summaries(&self) -> Result<Vec<ContentSummary>, ContentFault>;

    /// One entry body by id; no alias resolution here.
    fn entry(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault>;

    /// The `(alias, target)` rows of this snapshot.
    fn aliases(&self) -> Result<Vec<(RefId, RefId)>, ContentFault>;
}

/// The runtime `ReferenceSource`: N loaded snapshots composed by namespace.
pub struct IndexedSources { /* private */ }

impl IndexedSources {
    /// Loads every namespace directory under each root, in root order and
    /// ascending directory-name order (section 5.1).
    pub fn from_dirs(roots: &[PathBuf]) -> Result<Self, ContentFault>;

    /// Composes already-open readers; the fixture source and future
    /// non-directory readers use this.
    pub fn from_readers(readers: Vec<Box<dyn SnapshotReader>>) -> Result<Self, ContentFault>;
}

impl ReferenceSource for IndexedSources { /* get/search/autocomplete/sources */ }

/// Everything `write_source` needs for one namespace.
#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    pub provenance: Provenance,
    /// The catalog; `write_source` sorts by `RefId`.
    pub entries: Vec<ContentEntry>,
    /// `(alias, target)` rows; validated on write and on load.
    pub aliases: Vec<(RefId, RefId)>,
    /// Fidelity report written next to the snapshot.
    pub report: ImportReport,
}

/// Writes one namespace snapshot under `root/<namespace>/`: `entries.jsonl`,
/// `aliases.json` (when any), `report.json`, then `index.json` last as the
/// completeness marker; each file via temp + rename.
pub fn write_source(root: &Path, snapshot: &SourceSnapshot) -> Result<(), ContentFault>;

/// An in-memory `ReferenceSource` over a fixed fixture catalog: every
/// `ContentKind`, duplicate names across kinds, a unicode name, and one entry
/// with `needs_review`.
pub struct FixtureSource { /* private */ }

impl FixtureSource {
    /// The fixture catalog as a source.
    #[must_use]
    pub fn new() -> Self;

    /// The fixture catalog plus an explicit alias table; an invalid table is
    /// refused (`Corrupt`).
    pub fn with_aliases(aliases: Vec<(RefId, RefId)>) -> Result<Self, ContentFault>;

    /// A copy of the fixture entries, for expectations in the contract suite.
    #[must_use]
    pub fn catalog() -> Vec<ContentEntry>;
}

impl ReferenceSource for FixtureSource { /* delegates to the shared index */ }
```

Notes:

- **Deviation from the design sketch (design section 4.3):** `get`, `search` and
  `autocomplete` are fallible. A lazy disk source can fail mid-read; the engine must
  tell "unknown ref -> warn" (CONTENT-4) from "source broken -> fail".
- `IndexedSources::from_dirs` consumes the maintainer's runtime model: one or more
  filesystem roots plus CLI-configured paths, resolved at startup (U7 wires the
  flags). At least one root is required (`Unavailable` otherwise; N7).
  `SHIPPED_DATA_ROOT` is a build-time constant naming the in-checkout data directory
  for the tests and the workspace default; it is not an installed-layout promise, and
  the runtime default root is U7 packaging work (S6, section 9).
- `SnapshotReader`/`from_readers` are the **local reader** seam: the in-memory
  fixture, the directory reader and (later) the pack loader implement it. A live
  remote backend is not a `SnapshotReader`: it is served by the runtime's async
  backend dispatcher (CONTENT-12), which routes a link load by namespace to a local
  snapshot or a remote backend. The blocking port and the dispatcher are separate
  layers; nothing in the engine or in `IndexedSources` changes when the dispatcher
  lands.
- `FixtureSource` and `IndexedSources` share one implementation of ranking, aliasing
  and validation; the fixture differs only in where its entries come from.

### 4.3 `localtt_adapter::reference::import` and the converter binary

```rust
/// One-time conversion from an external source format to a `SourceSnapshot`.
pub trait Importer {
    /// The format identifier recorded in the report, e.g. `"open5e"`.
    fn format(&self) -> &'static str;

    /// Reads the source tree and produces a snapshot; never touches the
    /// network.
    fn import(&self, input: &Path) -> Result<SourceSnapshot, ImportError>;
}

/// What an import produced: counts per kind, the review queue, the pinned
/// revision. Written as `report.json` next to the snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub format: String,
    pub source_revision: String,
    /// Entries per kind.
    pub counts: BTreeMap<ContentKind, usize>,
    /// Review notes per entry id.
    pub review: BTreeMap<RefId, Vec<String>>,
}

/// Import failure. `file` is dataset-relative, for diagnostics.
#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The input tree or a required file is missing or unreadable.
    Unavailable { message: String },
    /// A source file did not parse, or a record cannot be mapped.
    Corrupt { file: String, message: String },
    /// Two source records produce the same `RefId`; resolved by the converter
    /// before writing, never by silent overwrite.
    Collision { id: RefId, first: String, second: String },
}

/// The Open5e `wotc-srd` converter (section 6.3).
#[derive(Clone, Debug)]
pub struct Open5eImporter { /* private */ }

impl Open5eImporter {
    /// `source_revision` is the pinned upstream revision recorded in the
    /// provenance and report (the tool does not read git).
    #[must_use]
    pub fn new(namespace: Namespace, source_revision: String) -> Self;
}

impl Importer for Open5eImporter { /* format/import */ }
```

Converter binary:

```
localtt-content-convert --input <dataset-dir> --output <data-root> \
    --namespace <ns> --source-revision <rev>
```

- All four flags required; `--help` prints usage; an unknown or malformed flag exits
  2 with usage on stderr.
- Success prints the per-kind counts and exits 0; an import or write failure prints
  the error to stderr and exits 1.
- Hand-rolled flag parsing (no `clap`): four flags do not justify a dependency
  (AGENTS.md "vet new dependencies").
- Importers are one-shot ingestion. A one-shot web fetch is an importer; a live
  RESTful server serving links at runtime is a backend of the async dispatcher
  (CONTENT-12), not an importer and not a `SnapshotReader`.

## 5. Guarantees

### 5.1 Runtime source layout and strict loading

```
<root>/<namespace>/index.json      version + provenance + name/offset rows
<root>/<namespace>/entries.jsonl   one entry per line, sorted by RefId
<root>/<namespace>/aliases.json    optional; (alias, target) rows
<root>/<namespace>/report.json     the import report (informational)
```

`index.json`:

```json
{
  "snapshot_version": 1,
  "provenance": {
    "namespace": "srd2014",
    "source": "SRD 5.1 (CC-BY-4.0), via Open5e",
    "source_revision": "b22294a4ebd8f5652135ad784eaa20070c4e62ff",
    "license": "CC-BY-4.0",
    "attribution": "This work includes material taken from the System Reference Document 5.1 ...",
    "url": "https://dnd.wizards.com/resources/systems-reference-document"
  },
  "rows": [
    { "id": "srd2014:spell/fireball", "kind": "spell", "name": "Fireball", "offset": 0 }
  ]
}
```

- `rows` is ascending by `RefId`; `offset` is the byte offset of the entry's line in
  `entries.jsonl` and is strictly ascending. Bodies are lazy: opening reads the index
  and aliases only.
- `index.json` is written **last** (temp + rename): its presence marks the namespace
  complete. A missing index means the source was never finished or was removed, and
  is `Unavailable`, never silence.
- Discovery: at least one root is required and every configured root must be an
  existing directory (`Unavailable` otherwise; N7). Within a root, candidate names
  are scanned in ascending order. A namespace directory's name must equal the
  `provenance.namespace` in its `index.json` (`Corrupt` otherwise; S3): the
  directory name selects the source, the provenance is checked against it. Before
  opening, names that do not parse as a `Namespace` and backup names (`*.bak`,
  `*.tmp`, `*.old`) are pruned silently, so a stray copy never loads and never
  blocks startup. Root order is preserved across roots; with the default source
  first, the fixture/CLI order is the `sources()` order.
- Failure classes, closed:

| Condition | Fault |
|---|---|
| no roots configured | `Unavailable` |
| root missing, or not a directory | `Unavailable` |
| namespace directory without `index.json` | `Unavailable` |
| namespace directory without `entries.jsonl` | `Unavailable` |
| namespace directory name != `provenance.namespace` | `Corrupt` |
| malformed `index.json`, entry line or `aliases.json` | `Corrupt` (entry bodies are lazy: the fault surfaces on the read, not the open) |
| index row id/kind mismatch, duplicate id, entry provenance mismatch | `Corrupt` |
| `snapshot_version != CURRENT_SNAPSHOT_VERSION` | `SnapshotVersionRefused` |
| two sources with the same namespace | `NamespaceConflict` |
| invalid alias table (collision, duplicate, dead target, chain, foreign namespace) | `Corrupt` |

- `from_readers` runs the same composition checks (namespace uniqueness, alias
  validation, at least one reader) without a directory layout; `from_dirs` is a thin
  discovery layer over it.
- Check order is normative: `index.json` is read and gated before `entries.jsonl`
  presence is checked, so a malformed index is `Corrupt` even when `entries.jsonl`
  is also missing (`a_corrupt_index_is_refused` in the target test; N2).
- Sources are immutable per process. Adding, removing or re-pinning a namespace while
  a server runs is unsupported: `write_source` rewrites `entries.jsonl` before
  `index.json`, so a live reader can observe a torn pair (the packs restart rule,
  design section 6.6).
- Reads are strict: a body that fails to parse (torn line, hand-edited file) fails
  `get` and `search`/`autocomplete` alike with `Corrupt`. Skipping bad bodies with
  diagnostics is deferred (section 9, N8); the operator repairs by re-running the
  converter.

### 5.2 Lookup semantics

- A live id of a loaded namespace -> `Ok(Some(entry))`, body read lazily.
- An alias id -> `Ok(Some(target entry))`; the returned entry's id is the target's.
  A live id is checked before the alias table (defence in depth; load validation
  already refuses an alias that collides with a live id, section 5.4).
- An id in a namespace that is not loaded, or absent from a loaded namespace ->
  `Ok(None)`.
- A read or parse failure -> `Err(ContentFault)`; never a silent `None`. A body that
  disappeared or changed under a live index surfaces on `get` as `Corrupt`.

`Ok(None)` is the empty answer that U3 turns into `UnresolvedReference` warnings
(CONTENT-4); `Err` is a broken source and fails the read.

### 5.3 Ranking

`search` and `autocomplete` share matching and ordering. The query text is trimmed
of ASCII whitespace and lowercased; an empty query yields no hits. The comparison key
is `(tier, kind_priority, namespace, name, id)`, all ascending:

- **Tier** (smaller first), for name matches:
  0. lowercased name equals the query (exact, case-insensitive);
  1. the lowercased name starts with the query;
  2. a word of the name starts with the query (words are split on non-alphanumeric
     characters);
  3. the lowercased name contains the query.
  `search` adds **tier 4**: the lowercased description contains the query. An entry
  already matched by name is never demoted to or duplicated in tier 4.
- **kind_priority**: the index of the kind in the caller's `kinds` slice (first
  occurrence) or, when unfiltered, in `ContentKind::ALL`. Filtering happens before
  ranking, so every candidate has a priority.
- **namespace**: ascending string order. `pack.mytome` sorts before `srd2014`, so
  homebrew wins its display slot (design section 6.5).
- **name**: ascending byte order, then **id** (a `RefId`) ascending as the final
  total tie-break. Every key is integer or string based: results are stable across
  runs and platforms, and a rerun of the same query returns identical vectors.

Empty `kinds` means all kinds. `limit == 0` yields no hits. Results are truncated to
`limit` after sorting.

`search` and `autocomplete` operate over the loaded local sources: the eager
summaries are local, and `search` tier 4 reads bodies through the local readers.
Remote backends are not searched: the dispatcher routes by-ref loads only
(CONTENT-12, section 9).

### 5.4 Aliases

- `aliases.json` rows are `(alias, target)`; `get(alias)` returns the target entry.
- Validation (on write and on load): the alias must be in the snapshot's own
  namespace; the alias must not equal a live id of the snapshot; the alias must not
  appear twice; the target must be a live id of the same snapshot; the target must
  not itself be an alias (no chains). Any violation is `Corrupt`. Namespace scoping
  (S4) is what keeps a user-supplied source from shadowing another namespace's ids
  through its alias table; `get` additionally checks the live id before the alias
  (section 5.2).
- Aliases are id-only: alias names are not indexed and never appear in `search` or
  `autocomplete` results. This deliberately drops the design's "alias names are
  indexed too" (section 6.5); see section 8.
- One hop only, by construction: a resolved lookup always lands on a live id.

### 5.5 Search and autocomplete

- `autocomplete` matches names only (tiers 0-3): the sheet-editing path.
- `search` matches names (tiers 0-3) plus description substrings (tier 4): the
  "find anything" path. It reads bodies lazily per query, so its cost is
  proportional to the catalog size (~1,000 bodies at SRD scale). The 10,000-entry,
  10 ms budget bench is deferred (section 9); the port does not change when the
  implementation gains an index.
- Both return `ContentSummary { id, kind, name }`, one hit per entry.

### 5.6 Writes, determinism and versioning

- `write_source` sorts entries by `RefId`, writes LF-terminated `entries.jsonl`,
  computes offsets from the bytes actually written, and writes `index.json` last.
  Writing the same `SourceSnapshot` twice produces byte-identical files: no
  timestamps, every map ordered, entries sorted.
- Re-writing a namespace without aliases removes a stale `aliases.json`.
- A namespace directory is **not** atomic as a whole. A crash mid-write can leave new
  entries behind an old index (reads then fail `Corrupt`, the line and the row
  disagree) or no index at all (`Unavailable`). Re-running the converter repairs it;
  the tool is offline and idempotent.
- `snapshot_version` gates reads: a version this build does not know is refused, not
  partially read. The version bumps only when the on-disk shape changes
  (`CURRENT_SNAPSHOT_VERSION`).
- Payload evolution (CONTENT-13): the engine payload types (`ContentData` and its
  structs) are the content wire contract, so the versioning rule and serde
  compatibility must agree. Adding a struct field is compatible only as
  `Option<T>` + `#[serde(default)]` (or a documented `Default`): old snapshots keep
  loading with the default, older builds ignore the unknown field in new snapshots,
  and no version bump is needed. New enum variants, renames, removals and changed
  field types are incompatible (DATA-6 makes an unknown enum tag an error) and bump
  `CURRENT_SNAPSHOT_VERSION`, so the open-time gate refuses the mismatch up front
  instead of a lazy `Corrupt` on the first `get`. Snapshots are read-only at
  runtime, so dropping an unknown field never loses data. The contract suite pins
  both directions with a hand-written v1 entry fixture (section 6.6).

### 5.7 Error model

`ContentFault` is `#[non_exhaustive]`, `Clone + Debug + Error`; `message` fields are
diagnostic only and U7 must not forward them (the `StoreFault` rule). `ImportError`
is the converter's own type; `write_source` reports `ContentFault` (`Unavailable` for
IO, `Corrupt` for an invalid snapshot). The engine maps faults into its own error
type in U3.

`write_source` validates before touching the first file, and validates what the
loader validates (N6): every entry's id matches its `kind` and sits in the snapshot
namespace with provenance equal to the snapshot's, the catalog has no duplicate ids,
the alias table passes section 5.4 (including namespace scoping), and
`report.source_revision == provenance.source_revision` (the report and the provenance
cannot disagree). Report counts are informational and not recomputed. A violation is
`Corrupt` and nothing is written.

### 5.8 Threading and progress

`ReferenceSource: Send + Sync`; implementations are immutable after construction and
every read takes `&self`. All calls are blocking; there is no lock-free or wait-free
claim. The directory reader opens files per body read (no interior mutability), so
`Sync` is structural. At U2 scale the eager index is a few thousand summaries and
lookups are microseconds; the slow path is the deferred description scan.

## 6. Internal structure

### 6.1 Module layout

```
crates/localtt-engine/src/content.rs        the model + port (4.1)

crates/localtt-adapter/src/reference/
  mod.rs        re-exports; SHIPPED_DATA_ROOT
  source.rs     SnapshotReader; DirSnapshot (crate-private directory reader)
  index.rs      IndexedSources (the port implementation)
  snapshot.rs   SourceSnapshot, write_source, CURRENT_SNAPSHOT_VERSION,
                crate-private wire types + read/validate helpers
  fixture.rs    FixtureSource; MemorySnapshot (crate-private)
  aliases.rs    AliasTable (crate-private)
  import/
    mod.rs      Importer, ImportReport, ImportError
    open5e.rs   Open5eImporter

crates/localtt-adapter/src/bin/localtt-content-convert.rs
crates/localtt-adapter/data/srd2014/{index.json, entries.jsonl, report.json}
```

### 6.2 `IndexedSources`

- Construction: open each reader; read `summaries` (names) and `aliases` eagerly;
  refuse duplicate namespaces; validate and build each snapshot's `AliasTable`
  against its live ids and its own namespace (section 5.4, S4); keep the sources in
  load order. Namespaces are the join key; there is no cross-source merge.
- `get`: find the source by the id's namespace; resolve a one-hop alias only when the
  id is not live (live-first, section 5.2); delegate to the reader (lazy body).
- `search`/`autocomplete`: scan the eager summaries of the loaded sources, match,
  rank, truncate. For tier 4, `search` asks each reader for the bodies it must test
  (`MemorySnapshot` scans memory, `DirSnapshot` seeks the JSONL). Both run locally; a
  live REST backend is not composed here and is not searched (CONTENT-12).
- Rationale: bodies stay out of memory until a link is followed or a search needs
  them (the maintainer's "read lazily from disk at runtime" requirement), while names
  and aliases are small enough to hold eagerly.

### 6.3 Open5e converter mapping

Input: the pinned dataset directory (`data/WOTC_5e_SRD_v5.1` of
`open5e/open5e-api`, commit `b22294a4ebd8f5652135ad784eaa20070c4e62ff`), holding
eleven required files: `spells`, `monsters`, `magicitems`, `weapons`, `armor`,
`conditions`, `classes`, `races`, `backgrounds`, `feats`, `sections` (all `.json`).
A missing file is `Unavailable`; a file that does not parse, a record that cannot be
mapped, or an empty derived slug is `Corrupt { file, message }`.

Slug rule: lowercase ASCII; every maximal run of characters outside `[a-z0-9]`
becomes `-`; leading/trailing `-` trimmed. The dataset has no slug fields; names are
unique per kind for the pinned revision (verified for all kinds), and the merged
`item` kind has no collisions either. A collision is `ImportError::Collision` with
both source record names, never a silent overwrite.

| Kind | Source | Payload |
|---|---|---|
| `spell` | `spells.json` | `Spell(SpellData { level: level_int, school, concentration: field == "yes", ritual: field == "yes" })` |
| `monster` | `monsters.json` | `Monster(MonsterData { abilities from the six int fields, challenge_rating parsed from the CR string, armor_class, hit_points })` |
| `item` | `magicitems.json` + `weapons.json` + `armor.json` | `Item(InlineItem { name, description, kind, requires_attunement, weight_oz, value, properties: None, modifiers: [], resource: None, ext })` |
| `condition` | `conditions.json` | `Text` |
| `class` | `classes.json` | `Text` |
| `subclass` | `classes.json` `subtypes[]` | `Text` |
| `race` | `races.json` + `subtypes[]` (subraces) | `Text` |
| `background` | `backgrounds.json` | `Text` |
| `feat` | `feats.json` | `Text` |
| `rule` | `sections.json` | `Text` |

Per-kind notes:

- **Ids**: `<namespace>:<kind>/<slug>`; subclasses and subraces are flat entries of
  their own kind (`srd2014:subclass/path-of-the-berserker`, `srd2014:race/high-elf`),
  so a sheet's `RefOrText` resolves them directly.
- **Item kind**: `weapons.json` -> `Weapon`; `armor.json` -> `Shield` when the name
  is `Shield`, else `Armor`; `magicitems.json` -> `type` starts with `"Weapon"` ->
  `Weapon`, with `"Armor (shield)"` -> `Shield`, with `"Armor"` -> `Armor`, anything
  else (`Wand`, `Staff`, `Ring`, `Rod`, `Scroll`, `Potion`, `Wondrous item`) ->
  `Wondrous`.
- **Attunement**: `requires_attunement` is true when the `requires-attunement` key
  is present (its text may add a qualifier).
- **Weight and value**: `weight` is parsed as `<n> lb.`, `<num>/<den> lb.` (with
  `num * 16 % den == 0`) or `<n> oz.` into `weight_oz`; `cost` is parsed as
  `<n> <cp|sp|ep|gp|pp>` into `Coins`. A value that does not match the pattern is
  left `None` with a review note. All pinned records parse cleanly.
- **Description**: markdown, assembled per kind so nothing displayable is dropped:
  - spell: stat line (level + school, casting time, range, components with material,
    duration plus concentration marker), then `desc`, then an "At Higher Levels."
    paragraph when `higher_level` is non-empty;
  - monster: `special_abilities` and `actions` rendered as bold-name paragraphs
    (reactions and legendary actions, when the source adds them, follow the same
    shape); the structured originals stay in `ext`;
  - item: `desc`, plus an italic attunement qualifier paragraph when present
    (weapons and armor carry no `desc`);
  - class: `features.desc`; subclass: `desc`;
  - race/subrace: `desc` plus `traits` when present;
  - background: `desc`; feat: `desc` plus `effects_desc` bullets;
  - condition/rule: `desc`.
- **`ext["src.open5e"]`** holds the source record verbatim, for every converted
  record: the lossless seam for later mapping (design section 6.2).
- **Review**: U2 does not guess, so the pinned dataset produces `needs_review: false`
  and an empty report review map. The mechanism is exercised by the fixture source
  and the contract suite; a later converter revision that parses mechanics out of
  prose is where notes appear.
- **Aliases**: the pinned dataset has none (first snapshot); the converter emits an
  empty table and the runtime format carries the mechanism.
- **Report**: `format: "open5e"`, the pinned `source_revision`, counts per kind, the
  review map. Entries are sorted by `RefId` before writing and all maps are
  `BTreeMap`s, so a rerun against the same revision is byte-identical.

Expected scale for the pinned revision (a sanity check for the implement phase, not
a contract): 319 spells, 322 monsters, 292 items (237 magic + 37 weapons + 18 armor),
15 conditions, 24 class/subclass entries, 13 race/subrace entries, 1 background,
1 feat, 26 rules -> 1,013 entries.

Provenance for `srd2014` is pinned as:

```json
{
  "namespace": "srd2014",
  "source": "SRD 5.1 (CC-BY-4.0), via Open5e",
  "source_revision": "b22294a4ebd8f5652135ad784eaa20070c4e62ff",
  "license": "CC-BY-4.0",
  "attribution": "This work includes material taken from the System Reference Document 5.1 (\"SRD 5.1\") by Wizards of the Coast LLC and available at https://dnd.wizards.com/resources/systems-reference-document. The SRD 5.1 is licensed under the Creative Commons Attribution 4.0 International License available at https://creativecommons.org/licenses/by/4.0/legalcode.",
  "url": "https://dnd.wizards.com/resources/systems-reference-document"
}
```

The target test asserts the revision, the license and that the attribution contains
"System Reference Document 5.1"; re-pinning is a review event (CONTENT-1), and the
converter records the given revision verbatim rather than reading git.

### 6.4 Converter binary and the committed data

```
cargo run --package localtt-adapter --bin localtt-content-convert -- \
  --input path/to/open5e/data/WOTC_5e_SRD_v5.1 \
  --output crates/localtt-adapter/data \
  --namespace srd2014 \
  --source-revision b22294a4ebd8f5652135ad784eaa20070c4e62ff
```

The committed result `crates/localtt-adapter/data/srd2014/` is what the target test
and the app's default source read. The `--input` argument is the dataset directory
itself (the repo layout's `data/WOTC_5e_SRD_v5.1`), not the checkout root; the
revision is recorded, not verified. Re-running against the same revision must
reproduce the committed files byte-identically; the contract suite asserts write
determinism, and the implement phase commits the generated data. A SHA-256 of the
report and snapshot is deferred (section 9).

### 6.5 Fixture source

`FixtureSource::new()` composes a crate-private `MemorySnapshot` through
`IndexedSources::from_readers`, so fixture and directory sources share one
implementation of ranking, aliasing and validation.

The fixed catalog covers:

- one entry of every `ContentKind` (the `ContentData` variants are each exercised:
  `Spell`, `Item`, `Monster`, `Feature`, `Text`);
- a duplicate name across two kinds, to pin the kind-order tie-break;
- a unicode name, to pin case-insensitive lowercase matching;
- one `needs_review` entry with a note;
- a stable namespace (`fixture`) whose provenance fields are literals.

`with_aliases` is the public seam the contract suite uses to drive alias tables
(valid and invalid) without writing files.

### 6.6 Contract suite

`crates/localtt-adapter/tests/reference_contract.rs` runs one shared suite over a
`ReferenceSource` harness for `FixtureSource` and `IndexedSources` (a tempdir
snapshot written by `write_source`). The shared suite covers the port-level subset
both implementations can express through the harness (`fresh`); failure-class and
raw-file cases live in the per-implementation test bodies, which may write files and
call constructors directly (N1). Like `store_contract`, fixtures are adapter-local
and built through this crate's public API; the golden cast stays in
`localtt-integration`.

- `get`: every fixture kind reachable; unknown id -> `Ok(None)`; alias -> target;
  unknown namespace -> `Ok(None)`;
- ranking: exact > name prefix > word prefix > substring; `search` name matches
  above description matches; description match found; kind-filter order ties;
  namespace ties; empty query, empty kinds, `limit == 0`; rerun stability;
- aliases: alias colliding with a live id, duplicate alias, dead target, chain,
  self-alias, alias in a foreign namespace -> `Corrupt` via `with_aliases`; alias ids
  absent from results;
- payload compatibility (section 5.6): an entry line missing a later optional field
  parses to its default, and an unknown field in a line is ignored;
- directory source: `write_source` -> `from_dirs` round trip (entry equality,
  sorted output, byte determinism on rewrite, stale `aliases.json` removed);
- strict loader: no roots, missing root, namespace dir without `index.json`, without
  `entries.jsonl` -> `Unavailable`; malformed index / entry line / alias file ->
  `Corrupt`; a malformed index with `entries.jsonl` also missing is `Corrupt`
  (check order, N2); entry line disagreeing with its index row -> `Corrupt` on `get`
  and on `search`, not on open (lazy, N8); a directory whose name disagrees with its
  `provenance.namespace` -> `Corrupt`; `*.bak` copies pruned silently; newer
  `snapshot_version` -> `SnapshotVersionRefused`; duplicate namespace across roots ->
  `NamespaceConflict`; invalid directory names and plain files pruned;
- `sources()`: one provenance per loaded namespace, in load order.

The suite needs no new dependencies: `tempfile` and `serde_json` are already
adapter dev-dependencies.

### 6.7 Dependencies and layout

- `localtt-engine` gains nothing: `serde` and `thiserror` are already dependencies
  (`ContentKind`/payloads derive serde; `ContentFault` derives the error).
- `localtt-adapter` gains nothing: `serde`, `serde_json`, `thiserror` and
  `tempfile` are already present; the converter bin needs no `clap`.
- `localtt-integration` gains nothing: `tempfile` is already a dev-dependency.
- No new workspace dependency is added by U2.

## 7. API cross-check against the target test

Used by the test, where pinned:

- `reference::{IndexedSources::from_dirs, SHIPPED_DATA_ROOT, SourceSnapshot,
  write_source}` and the `SourceSnapshot`/`ImportReport` fields (4.2, 4.3);
- `content::{ReferenceSource::{get, autocomplete, sources}, SearchQuery fields,
  ContentEntry fields, ContentData::{Spell, Item, Monster} + payload fields,
  ContentKind::{Spell, Item, Monster}, ContentFault::{Unavailable, Corrupt},
  Provenance fields}` (4.1);
- `localtt-core::{RefId, Namespace, ItemKind, Ext}` for literals and assertions.

Sketch elements the test does not use, and their justification:

- `ContentData::{Feature, Text}`, `FeatureData`, `ContentKind::Feature`: exercised
  by the fixture catalog and the contract suite; keeping them now avoids a redesign
  when packs land (design section 4.3).
- `ContentKind::ALL`: defines the unfiltered kind priority (5.3) and is used by the
  contract suite.
- `search`: the engine's find path (design section 7.8); pinned here, exercised by the
  contract suite.
- `FixtureSource`, `SnapshotReader`, `from_readers`,
  `CURRENT_SNAPSHOT_VERSION`: the test fixture path and the local reader seam
  (6.5, 4.2, CONTENT-12).
- `Importer`, `Open5eImporter`, `ImportError`: the converter bin and its report
  (4.3, 6.4).

## 8. Decisions (folded 2026-10-17)

Items 1-3 are the maintainer's pre-spec answers; items 4-8 were spec-phase
deviations; items 9-12 came out of the pre-implementation adversarial review of this
spec. All of them are folded into `docs/decisions.md` as CONTENT-9..15 (folded
2026-10-17), and every noted design edit has been applied to
`docs/engine-architecture.md`; the applied list is at the end of this section. This
section remains the unit-local record of what the RFC changed.

1. **CONTENT-9: runtime sources replace embedding.** The app loads converted
   snapshot directories at startup, configured by CLI paths with a filesystem
   default; nothing is compiled into the binary. Amends CONTENT-6: no
   `include_bytes!`, no `OnceLock`, no embedded parse. The repository still commits
   the SRD snapshot as data under `crates/localtt-adapter/data/srd2014/` and
   `SHIPPED_DATA_ROOT` names it; U7 wires the flags and defaults. Rationale: the
   maintainer forks/clones Open5e and serves the data, so data can be swapped or
   re-pinned without a rebuild; the single-binary story is consciously relaxed to
   "binary + a data directory".
   *Design edits applied*: section 4.3 sketch (fallible trait, `Namespace`
   provenance, minimal `FeatureData` comment) and its notes; section 5.8
   "embedded in the binary" sentence; section 6.2 serving bullet and snapshot
   example; section 6.3 "embedded snapshot" and the Open5e-slug sentence; section
   6.5 alias sentence; section 6.6 rewritten; section 6.7 "embedded" sentence;
   section 8.2 table row; section 9 Content row; section 10 U2; section 13(2);
   the document status line.
2. **CONTENT-10: index + JSONL lazy snapshot.** One namespace directory per source
   with `index.json` (version, provenance, names, offsets), `entries.jsonl` (lazy
   bodies), optional `aliases.json` and `report.json`; `index.json` is the
   completeness marker written last; writes are deterministic. Amends the single-JSON
   snapshot sketch in design section 6.2. Rationale: "read lazily from disk at
   runtime once a link is followed" (maintainer); names are small enough to hold
   eagerly.
   *Design edits applied*: section 6.2 snapshot example replaced by the directory
   layout and the serving bullet; section 6.6 rewritten around the runtime data
   directory.
3. **CONTENT-11: `ReferenceSource` is fallible; `Ok(None)` stays distinct.** All
   three read methods return `Result<_, ContentFault>`: `Ok(None)` is "no such
   entry" (a warning later), `Err` is a broken source. Amends design section 4.3's
   infallible sketch. Rationale: a lazy disk source can fail mid-read, and the engine
   must not conflate user-visible absence with infrastructure failure.
   *Design edits applied*: section 4.3 trait sketch and its notes.
4. **Source seam now, dispatcher later.** `SnapshotReader` +
   `IndexedSources::from_readers` are the *local* reader seam (fixture, directory,
   later the pack loader). A live RESTful backend is not a `SnapshotReader`: the
   runtime's async backend dispatcher routes a link load by namespace to a local
   snapshot or a remote backend (CONTENT-12), and U2 ships no network code, so
   offline play is unchanged.
5. **Aliases are id-only.** Alias names are not indexed and never appear in results;
   `get` is the only alias-aware call. Drops "alias names are indexed too" from
   design section 6.5. Rationale: alias ids exist for compatibility; surfacing an
   alias as a distinct autocomplete hit would offer the player a dead-looking
   duplicate.
6. **Minimal payloads.** `SpellData`/`MonsterData`/`FeatureData` carry exactly the
   fields the target test pins; everything else is description text plus `ext`.
   Widening them stays compatible through the CONTENT-13 rule (serde-optional
   fields; incompatible changes bump the snapshot version).
   *Design edits applied*: section 6.4 gains a sentence that U2 maps the minimal
   typed payloads and defers the listed extraction; section 4.3's `ContentData`
   comment matches the U2 `FeatureData` shape (category, text).
7. **The converter is a workspace bin, not a shell script.** `localtt-adapter`'s
   `src/bin/localtt-content-convert.rs` keeps conversion next to the format it
   writes and needs no new dependency (design section 10 U2 already scheduled a
   converter bin).
8. **Testing row update.** Design section 9's Content row becomes: converter
   determinism (byte-identical rerun), strict-loader failure classes, alias scoping,
   ranking order stability, payload compatibility, committed snapshot provenance; the
   autocomplete and description budgets move to the deferred bench. The contract
   suite covers the port-level subset through the harness and leaves raw-file cases
   to the per-implementation bodies (N1).
9. **CONTENT-12: direct async REST is in scope; the unit port stays local and
   blocking.** At runtime a backend dispatcher (`load_link_from_ref`-style, async)
   routes a link load by namespace to the correct backend, and a backend may be a
   live RESTful server. U2 builds one backend: the local snapshot source behind the
   blocking `ReferenceSource`. The dispatcher, a REST backend and any remote search
   are later units; search/autocomplete stay local-index operations for now.
   Rationale: the runtime needs live sources without forcing the offline path
   through the network, and the frozen target test pins the blocking local API.
   *Design edits applied*: section 6.2 "live web provider is still an importer"
   replaced by the dispatcher split; section 13(2) same; RFC section 2 non-goals and
   section 4.2 notes updated.
10. **CONTENT-13: serde-additive payload evolution.** The engine payload types are
    the content wire contract: added struct fields must be `Option<T>` +
    `#[serde(default)]` (or carry a documented `Default`) and need no version bump;
    new enum variants, renames, removals and changed field types are incompatible and
    bump `CURRENT_SNAPSHOT_VERSION`, which the open-time gate refuses loudly instead
    of a lazy `Corrupt` on `get` (S2). *Design edits applied*: section 8.2 table row
    and section 9 Content row mention the rule; the RFC pins it in section 5.6.
11. **CONTENT-14: discovery policy.** A configured root needs at least one entry;
    a namespace directory's name must equal its `provenance.namespace` (`Corrupt`
    otherwise); backup names (`*.bak`, `*.tmp`, `*.old`) and non-namespace names are
    pruned silently; a namespace directory missing required files stays fatal
    (`Unavailable`); sources are immutable per process, so swapping or re-pinning
    data requires a restart (S3, N7). *Design edits applied*: section 6.6 restart
    sentence; the RFC pins it in section 5.1.
12. **CONTENT-15: aliases are namespace-scoped.** An alias must be in its
    snapshot's namespace; a foreign-namespace alias is `Corrupt`; `get` checks the
    live id before the alias (S4). *Design edits applied*: section 6.2 construction
    and `get` bullets; the RFC pins it in sections 5.2 and 5.4.

## 9. Deferred items (not blocking the skeleton)

- **Description-search bench.** `just bench` with a 10,000-entry synthetic fixture
  and the p99 <= 10 ms budget (CONTENT-5); U2 pins the semantics and defers the
  measurement.
- **Body caching in `IndexedSources`.** Correctness does not need it; a body is read
  per resolve. Add a cache only when a profile asks for it.
- **SHA-256 of the committed snapshot/report** for integrity pinning (importer
  hardening).
- **Non-ASCII slug folding.** The current rule turns non-ASCII runs into `-`;
  deterministic but lossy for names that are not ASCII-transliterable. A fold table
  is a converter revision, not a format change.
- **Runtime data-root resolution and packaging** (S6): where an installed binary
  finds its default data directory (exe-relative `../share/localtt` or a platform
  data dir) and how `crates/localtt-adapter/data/` is installed; `--data` and the
  other flags are composition-root work (U7). `SHIPPED_DATA_ROOT` stays the
  build-time constant for tests and the workspace default.
- **Remote backends and remote search** (CONTENT-12): the async backend dispatcher,
  a live RESTful backend, and any search/autocomplete fan-out to remote backends;
  a later unit, designed against the frozen local port rather than inside it.
- **Search diagnostics on unreadable bodies** (N8): skip-and-diagnose instead of
  failing the whole search; deferred with the operator diagnostics story.
- **Item property mapping and monster feature extraction** — later converter
  revisions mine `ext["src.open5e"]`.
- **`srd2014:background/sage` fixture finding.** The golden cast references a
  background SRD 5.1 does not contain. Decide in U8 (edit the fixture to Acolyte or
  freeze it as text) when `FreezeReference` exists; until then it is the pinned
  `Ok(None)` case.
