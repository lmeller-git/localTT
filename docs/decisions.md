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
- **ARCH-3: dependency direction is one-way** (`server -> engine -> core`;
  `adapter -> core`, never `engine`). -- Prevents the engine from depending on
  storage and vice versa. -- Accepted, reviewed.
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
- **Composition root location** (server `[[bin]]` vs a thin binary crate) and port
  placement (RFC-0001 9.3, item 1); **reference vendoring** -- copy used pack
  entries into the campaign or keep them external (RFC-0001 9.3, item 2).
- **Phase E**: engine rules, the `view_of` authorization matrix, a deterministic
  `Rng` fixture, and roll/feed tests.
