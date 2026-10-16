# Review: localtt-integration and its API (three passes)

| | |
|---|---|
| **Reviews** | `crates/localtt-core/src/builders.rs`, `crates/localtt-integration/` (`src/`, `tests/scenarios.rs`, `RFC-0003-integration.md`) |
| **Baseline** | commit `6c29e32` ("add initial test structure") |
| **Status** | Findings and their resolution. Severity tags: `[H]` high, `[M]` medium, `[L]` low, `[i]` info. The A-P findings were applied in a follow-up fix pass (see [Resolution](#resolution-applying-the-a-p-findings) at the end); T* is the later test pass. |

## Scope and method

Every function body in the reviewed crates is `todo!()` (skeleton phase), so
"the implementation" is the **declared API surface** (signatures, types, module
placement) plus the **behaviour the test suite states it will check**. Pass 2
therefore judges the declared signatures and the *real* core type layouts in
`types.rs` (those fields are concrete, not stubs), not runtime measurements.
Pass 3 judges test intent and coverage, not yet-passing assertions.

Three passes, as requested:

1. API judgement: usability, fit, extensibility, and misplaced components.
2. Performance: allocation/layout cost, acceptability by call frequency, and
   reconciliation with pass 1.
3. Test critique: real logic, coverage, and edge cases.

---

## Pass 1 - API judgement

### 1a. Is it easy to use and sensible for this project?

- **[H] A1 - The builder cannot faithfully build a `StatblockBody`, and the
  infallible `build()` forces silent bad defaults.**
  `StatblockBuilder::character("Rowan")` must return a fully-populated
  `StatblockBody`, but `Abilities` has no `Default` (RFC-0002 section 9.9
  forbids it) and neither does `Size`/`Hp`/`Identity`/`Proficiencies`. So the
  constructor must fabricate `Abilities::all(10)` and `Hp { max: 0, .. }` - the
  exact "silent 10 corrupts a sheet" outcome section 9.9 warns against - and
  `build(self) -> StatblockBody` can never tell whether the caller set them.
  Fix: make `build()` fallible and require `abilities` + `hp` (or take them in
  the constructor, or use `Option` fields plus a typed `BuildError`).

- **[H] A2 - Core is missing the `Default` impls RFC-0002 section 9.9 mandates.**
  Section 9.9 requires `Default` for the "empty-able leaf sections ... so PDF
  import can build a statblock field by field". The skeleton derives `Default`
  for none of `Hp`, `Speed`, `Coins`, `DeathSaves`, `Proficiencies`, `Identity`,
  `CampaignSettings`, `Size`. The builder depends on exactly those blanks, and
  the PDF importer will too. Action: add them in core's implement phase.

- **[H] A3 - The builder cannot build its own fixtures.**
  There is no setter for `identity` (race/background/alignment),
  `proficiencies.tools`/`languages`, `creature_type`, `death_saves`,
  `exhaustion`, `inspiration`, or `cr` on a character. Aria is a "high-elf
  evoker", but `identity.race` is unreachable through the builder, and RFC-0003
  section 5.3 says fixtures are written through the builders. Either the builder
  grows these setters or section 5.3 must allow direct struct literals.

- **[M] A4 - Two builder idioms in one crate.**
  `Entity::builder` is consuming (`fn ruleset(self, ..) -> Self`); the new
  `CampaignBuilder`/`StatblockBuilder` are `&mut self`. The `&mut self` form is
  justified only to return a non-self value mid-chain (`item() ->
  InventoryEntryId`, `dm()/player() -> PlayerId`); `CampaignBuilder` inherits it
  without need. Pick one idiom, or document why the split exists.

- **[M] A5 - `StatblockBuilder` is a god-object mirroring `StatblockBody`.**
  ~20 methods duplicate the field list; every new `StatblockBody` field must be
  added twice and can drift. Compose with `EntityBuilder`, or use a macro.

- **[M] A6 - `build()` returns a body, not an entity.**
  A complete character therefore needs two builders plus manual threading of
  kind/id/name/owner (visible in `tests/scenarios.rs` lines 174-183). A
  `StatblockBuilder` that owns the header and yields `StatblockEntity` would
  make the common client path one call.

- **[M] A7 - Panicking helpers are public API of a production crate.**
  Moving the builders into core put `content_ref`/`stat`/`ref_id` (which
  `expect` on literals) into `localtt-core`. A production consumer can call
  `core::builders::stat("garbage")` and panic. Gate them (`test-support`
  feature) or move them to the integration crate. RFC-0003's own note that
  "AGENTS.md's no-unwrap rule is about production paths" is now self-defeating,
  because core *is* a production path.

- **[L] A8 - `attack` cannot express "no bonus".**
  `attack(name, bonus: i32, ..)` but `AttackEntry.bonus: Option<i32>`. Take
  `Option<i32>`, or split into `attack` / `attack_unbonused`.

- **[L] A9 - Payload boilerplate is not reduced.**
  `item(..)` saves the container, but the common case still hand-writes a
  15-line `InlineItem` (test lines 108-127). Add `inline_item(..)` or a core
  `InlineItem::builder`.

- **[L] A10 - `dm` and `player` differ only by `Role`.** Consider
  `participant(name, Role)`.

- **[L] A11 - `Formula::dice(dice(1, 6))` reads poorly.** Two things named
  "dice"; rename one.

### 1b. Is it extensible / future-compatible?

- **[M] B1 - Level-up matches a class by `&str`, which cannot find a
  referenced class.** `transform::level_up(entity, class: &str, to_level)`
  matches `ClassLevel.class`, a `RefOrText`. A fixture storing
  `RefOrText::Ref(srd2014:class/wizard)` is not searchable by `"Wizard"`, so the
  suite is silently forced to store classes as `Text`. Either `level_up` takes a
  `ClassLevelId`, or `RefOrText` needs a compare-by-name helper.

- **[M] B2 - `Condition` is closed.** Only `Equipped`/`Attuned`/`StatusActive`
  are representable; homebrew conditions ("while raging", "in sunlight") are
  not. Needs a `Custom` variant or payload. (An RFC-0002 decision, surfaced
  here.)

- **[L] B3 - `LoadError` cannot grow or express semantic failure.** It is not
  `#[non_exhaustive]` and has only `Schema`/`Json`, so "parsed but invalid"
  (see T2) is unnameable and any new kind is a breaking change for matchers.
  Add `#[non_exhaustive]` and an `Entity(EntityError)` variant.

- **[L] B4 - `Party` is a fixed 8-field struct.** A new entity role breaks it.
  A `Vec<AnyEntity>` plus accessors would extend without churn (fixture-only).

- **[L] B5 - The public API exists twice** (`localtt_core::builders` and
  `localtt_integration::builders`). Two paths for one item invites drift.
  Prefer importing from core, or re-export the whole module
  (`pub use localtt_core::builders;`).

- **[i] B6 - Freezing builders in core before the engine exists** is the
  accepted risk recorded in RFC-0003 OQ2. `StatblockBody` is still moving, so
  churn is expected. Tracked, not a defect.

### 1c. Components that belong elsewhere

- **[H] C1 - `roundtrip::{load_checked, LoadError}` is adapter/store logic.**
  "Load a persisted document behind a schema gate" is exactly what the sqlite
  adapter (or a store port) must do; RFC-0003 itself says "a later
  adapter-backed test can reuse `load_checked`". Move it to `adapter` and consume
  it here.

- **[H] C2 - `transform::*` are `engine::commands` in disguise.** The moment
  they stop being `todo!()`, they are rules (`apply_damage`, `heal`,
  `set_temp_hp`, `begin_concentration`, `level_up`, `summon_from_template`,
  `give_handout`) that the engine will implement again and can diverge from.
  Keep them skeletal, or land a minimal engine command surface now. This is the
  largest structural risk in the crate.

- **[M] C3 - `fixtures::*` and `ids::*` are a reusable testkit.** Engine tests
  will want the golden characters. Champion the `localtt-testkit` extraction
  (RFC-0003 OQ4), but only once a second consumer exists, to avoid a dev-dep
  cycle.

- **[M] C4 - `content_ref`/`stat`/`ref_id`/`dice` belong to the testkit**, not
  core (see A7).

- **[L] C5 - `roundtrip::to_json`/`from_json`** are one-line `serde_json`
  wrappers; testkit or call site, not a facade.

- **Keep in core:** `Abilities::all`, `RulesetId::dnd5e_2014`, `Formula::*`,
  `Modifier::*` are legitimate, reusable constructors.

---

## Pass 2 - Performance

No hot loop exists yet. Each finding is judged against **call frequency**:
a one-time constructor is fine; something in a per-roll/per-message loop is not.
Reconciliation with pass 1 is called out explicitly.

- **[M] P1 - `AnyEntity` is an unboxed large-variant enum.** It already carries
  `#[expect(clippy::large_enum_variant)]`; a bare `Note`/`Handout` costs the
  size of the statblock variant (~0.8-1 KB). `Party::entities()` returns
  `Vec<AnyEntity>`, i.e. copies that much per entity. **Acceptable:** storage,
  iteration and `view_of` over dozens of entities are not latency-sensitive, and
  RFC-0002 deferred boxing to the implement phase. **Reconciled:** pass 1 did
  not flag this because it is a pre-existing core decision, not part of the
  reviewed API; the action belongs to RFC-0002's implement phase.
  - Sub-note: `Party::entities(&self) -> Vec<AnyEntity>` deep-clones the party
    on every call. Pass 1 did not weigh cost (fixture convenience). Accept, but
    memoize if it ever lands in a loop.

- **[M] P2 - `load_checked` cannot peek the schema version without a full
  parse.** With only `T: DeserializeOwned`, reading `schema_version` before
  materializing the document requires a whole `serde_json::Value` DOM (or a
  parse-then-check that defeats "refuse before partial load"). That is a second
  full pass + allocation over a possibly multi-MB save. **Borderline
  acceptable** (load is a startup/edit path, not a loop). **Reconciled with pass
  1:** C1 already says "move it to the adapter"; doing so also removes this cost
  from this crate, and the streaming reader there can use a tiny `SchemaProbe`
  struct (`{ schema_version }` + `IgnoredAny`) via `serde_json::Deserializer`.
  The fix is relocation, not micro-optimization.

- **[L] P3 - `InventoryItem::Inline(InlineItem)` large variant** (boxing
  deferred). Every `Ref` entry pays for `Inline`. Acceptable (sheet-sized).

- **[L] P4 - `RefId` is `String` + two `u32`** (one allocation per reference); a
  reference-heavy sheet makes many small allocations. Acceptable: references are
  parsed at load and only compared/serialized later; the composition hot path
  uses `StatKey`/`Formula`, not `RefId`. The internal offsets design (no re-parse)
  is a deliberate win.

- **[L] P5 - `Modifier` embeds `Formula` inline (~150 B/modifier).** Sheets hold
  tens, not thousands. `Formula`/`DiceExpr` recursion is boxed correctly.
  Acceptable.

- **[i] P6 - `BTreeMap`/`BTreeSet` throughout cost O(log n) vs O(1).** This is
  the deliberate price of deterministic, diffable bytes (RFC-0003 section 6.2).
  **Reconciled:** the determinism guarantee is *why*; no change.

- **[L] P7 - `Id::new()` (UUID v7, random + clock) is called per element** in
  `summon_from_template`/`give_handout`/`level_up`/`begin_concentration`; a
  summon with N elements does N+1 draws. Per user action, so acceptable; note it
  for monsters with large inventories.

- **[L] P8 - Test-suite cost.** `party()` rebuilds 5+ entities (many
  allocations) on each of six calls; `fixtures_are_deterministic_byte_for_byte`
  calls it twice and serializes twice. Fine for tests; `OnceLock` if it grows.

- **[i] P9 - `StatblockBuilder`'s `&mut self` chain is allocation-free** (returns
  `&mut Self`; `build(self)` moves). So A4's idiom unification should be decided
  on ergonomics, not cost.

**Reconciliation summary.** Only P2 is a genuine cost with a fix, and pass 1
already located that fix (relocate to the adapter, implement as a streaming
probe). P1/P3 are pre-existing core layout choices already deferred with an
`#[expect]`. P4-P9 are acceptable at their call frequency. No finding here
contradicts pass 1.

---

## Pass 3 - Test critique

Structural note: every test panics at `todo!()` today, so none asserts anything
yet. This pass judges the stated intent. (The earlier missing-`golden_warlock`
import was fixed in `6c29e32`.)

### Real logic

- **[H] T1 - `damage_temp_hp_and_death_saves_are_representable` under-asserts
  and never touches death saves.** It only checks `hp.max == 32` and
  `current < max`; it would pass even if temp HP never absorbed, never
  de-stacked, and death saves did not exist. Assert the exact progression
  (temp 5 -> set 3 keeps 5 -> damage 12 -> current 25, temp 0 -> heal 4 -> 29)
  and set/assert `DeathSaves`.

- **[H] T2 - `a_hand_edited_kind_body_mismatch_is_rejected` asserts only
  `.is_err()`.** Any unrelated parse failure passes, and `LoadError` cannot
  name a kind/body mismatch (see B3), so the intended failure is unpinnable.
  Surface a typed semantic error and assert it.

- **[M] T3 - `load_checked` has no positive control.** The only test covers
  rejection of a newer major, so an implementation that rejects *everything*
  passes. Add accept cases (same major/higher minor, older major) and one valid
  load.

- **[M] T4 - `fixtures_are_deterministic_byte_for_byte` is narrow.** It compares
  two fresh `party().campaign` serializations and stops at the campaign; the
  entities/note/handout (which carry most ids) are not covered, and it cannot
  catch a deterministic-but-wrong fixture. Compare entities too, and pin a fixed
  expected string once ids are fixed.

- **[M] T5 - `a_summon_is_a_fresh_copy_linked_to_its_caster` can be vacuous.**
  The "fresh ids" assertion is `.zip(..).all(..)` over the inventory; with an
  empty template inventory it is trivially true. Guard with
  `assert!(!template.body.inventory.is_empty())`.

- **[M] T6 - `level_up_bumps_base_data_and_keeps_overrides` is representation-
  blind.** It passes `"Wizard"` as a `&str` and checks only `levels[0]`, so it
  neither pins which class was bumped nor exercises the `RefOrText::Ref` case
  (B1).

- **[M] T7 - The hand-built character test asserts none of several setters.**
  `skill`, `save_proficiency`, `notes` and `attack`'s bonus/damage are called
  but never checked; only `abilities.dex`, `hp.max`, `attacks[0].from_inventory`,
  `inventory[0].state`, `overrides`, the ring's `when` and the feature formula
  are asserted.

- **[M] T8 - The campaign test never checks `Player.role` or display names.**
  `dm()`/`player()` could both emit `Role::Player` and the test passes.

- **[M] T9 - The creature test skips promised fields.** No assertion on `size`,
  `senses`, the catalog's `FeatureCategory::Action` row, or the entity `kind`.

- **[L] T10 - "Distinct" is never tested.**
  `the_golden_characters_are_distinct_and_complete` never compares the
  characters to each other. Compare their ids/abilities, or rename.

- **[L] T11 - `spending_past_zero_is_allowed_and_round_trips` has no positive
  control.** Add a normal spend that decrements by exactly N, and check `max`.

- **[L] T12 - `giving_a_handout_copies_the_item_into_the_inventory`** checks
  length/kind/state but not that the copy equals the handout's item.

- **[L] T13 - `a_read_share_carries_a_default_deny_mask` pins JSON order equal
  to `ViewField` declaration order.** Legit as a determinism pin, but the wire
  order is now bound to enum order; document that coupling.

### Coverage gaps (versus the RFC-0003 section 3 catalog)

The catalog claims "phase C covers every row's **store** column". Several are
absent:

- **[H] T14 - "Bless riding a save" and "Timed status vs clock" rows have no
  test.** `StatusKind::{Effect, Condition, Custom}`,
  `StatusDuration::{UntilRest, Indefinite, Manual}`, `StatusSource`, and
  status-borne modifiers outside concentration are all unexercised; the local
  `concentration()` helper is the only `Status` construction.

- **[H] T15 - The whole "Roll + feed" row is untested at the storage level.**
  `Roll`, `DiceResult`, `TermResult`/`DieResult`, `RollContext`, `Audience`,
  `FeedEntry`, `FeedPayload`, and `DiceExpr` beyond a single `Roll` leaf.

- **[H] T16 - `ModifierOp` beyond `Add` is untested.** `Set`, `Min`, `Max`,
  `Advantage`, `Disadvantage` are never constructed, nor is any `FormulaExpr`
  arithmetic (`Add`/`Sub`/`Mul`/`Min`/`Max`/`FloorDiv`) or the
  `Level`/`ClassLevel` leaves. "Stuff that cancels out" (Set vs Add, Min/Max
  clamps, advantage vs disadvantage) is therefore unrepresentable in the suite.

- **[M] T17 - Weird multiclass cases are absent.** No three-class character, no
  duplicate class entry, no zero/negative level, no second multiclass entity,
  and Mira's two `spellcasting` blocks are not built or asserted (`levels.len()
  == 2` only).

- **[M] T18 - Custom/homebrew values are absent.** No `StatKey::Custom`
  (`house.luck`) override or modifier, no `OverrideValue::{Bool, Text}`, no
  reference-less custom feature beyond one class feature; "custom bonuses" are
  only `Formula::Const(1)` on the ring.

- **[M] T19 - Boundary values are absent.** `qty` 0/negative (documented as
  allowed and warned), `ChallengeRating` `"0"`/`"1/8"`/`"30"`, `Exhaustion`
  0..6, `DeathSaves` 0..3, `Hp.current` negative via damage, sparse overrides on
  a custom stat.

- **[M] T20 - Kind/body matrix is thin.** Only PC (builder), Summon (transform)
  and the fixture kinds are exercised; no `Npc`/`Monster` through
  `Entity::builder`, no positive `Note`/`Handout` build, and no
  `.parent(...)`/`.acl(...)`/`.rev(...)`/`.schema_version(...)` setter coverage.

- **[M] T21 - The canonical `StatKey` vocabulary is unpinned.** Only `ac`,
  `initiative`, `skill.nature`, `attack` are parsed; `save.wis`,
  `skill.stealth`, `ability_mod.str`, `passive.perception`, `attack.melee`,
  `spell.dc`, and the `save` group are not.

- **[L] T22 - `Condition::{Equipped, StatusActive}` and `AclLevel::Write` are
  untested.** `an_invalid_reference_is_rejected` tests core `RefId::parse`
  directly - that is a core unit test, not an integration boundary.

- **[L] T23 - No threading assertion here.** The suite relies on RFC-0002's
  `Send + Sync` test. Since these types cross the server boundary, a trivial
  static assert for `Party`/entities would make this suite self-contained.

---

## Summary and suggested order of work

| Theme | Findings | Where to fix |
|---|---|---|
| Builder cannot build the fixtures; silent bad defaults | A1, A2, A3 | core implement phase (add `Default`s; make `build` fallible) |
| Engine/adapter logic living in the test crate | C1, C2, A7, C4 | move `load_checked` to adapter, commands to engine, helpers to testkit |
| Tests under-assert the logic they set up | T1, T2, T7, T8, T9 | integration tests |
| Catalog rows promised in phase C are missing | T14, T15, T16 | integration tests |
| Edge cases (multiclass, cancels-out, custom, boundaries) | T16-T21 | integration tests |
| Genuine perf cost with a fix | P2 (via C1) | adapter |
| Acceptable/layout-deferred perf | P1, P3-P8 | RFC-0002 implement phase |

Largest single issue: **A1/A2/A3** - the builder as specced cannot produce the
golden characters it exists to serve, and its infallible `build()` forces the
made-up defaults RFC-0002 section 9.9 explicitly rejects. Pass 2 shows the
fabrication is O(1) (not a performance concern), and pass 3 shows the tests hide
it because every fixture sets abilities and HP explicitly. It is a design
problem to settle before the implement phase, not a performance or coverage
problem.

---

## Resolution: applying the A-P findings

Applied as one unit on branch `datamodel-integration` (baseline below). Every
body is still `todo!()`: this pass changes the **declared API and module
placement**, which is what the findings were about; the bodies land in the
implement phase. `T*` belongs to the later test pass and is out of scope here.
The second pass below re-reads each finding against the code that was written.

### Pass 1a - API

| # | Finding | Fix | Verification |
|---|---|---|---|
| A1 | Infallible `build()` forces silent defaults | `StatblockBuilder::build`/`build_entity` return `Result<_, BuildError>`; `abilities`/`hp` are tracked and refused (`MissingAbilities`/`MissingHp`) | `BuildError` has both variants; the builder can no longer emit a usable-looking sheet with fabricated scores |
| A2 | Missing `Default`s (§9.9) | `#[derive(Default)]` on `Hp`, `Speed`, `Coins`, `DeathSaves`, `Proficiencies`, `Identity`; manual `Default = Medium` for `Size` | All present. **Caveat:** `CampaignSettings` deliberately has no `Default` - its fields are policy (audience default, feed capacity, dawn hour), not empty-able blanks, so a derived `Default` would invent policy. The builder sets them explicitly. Recorded in RFC-0003 5.2.1. |
| A3 | Builder cannot build its fixtures | Added `identity`, `tools`, `languages`, `creature_type`, `death_saves`, `exhaustion`, `inspiration`, `cr`; `character`/`creature` no longer take an unused name | All setters present; `Identity` is now `Default`, so `Identity { race: .., ..Default::default() }` composes |
| A4 | Two builder idioms | Documented the split in `localtt_core::builders`: `&mut self` where a method returns a *product* mid-chain, consuming otherwise (`EntityBuilder`) | Module docs state the rule explicitly; no behaviour change |
| A5 | God-object mirroring `StatblockBody` | Documented in `StatblockBuilder`: it *holds* the body (no re-declared fields), the mirror is one optional setter per section, and a declarative setter macro is the implement-phase remedy | **Partial:** the drift is real but bounded to *optional* setters; documented and carried as a non-blocking implement-phase item, not silently dropped |
| A6 | `build()` returns a body, not an entity | Added `build_entity(kind, id, name, owner) -> Result<StatblockEntity, _>`; the test uses the single call | Present; the two-builder threading the finding cited (test lines 174-183) is gone |
| A7 | Panicking helpers are production `core` API | `content_ref`/`stat`/`ref_id` now return `Result<_, _>`; `dice` -> `dice_pool` (infallible) | No `expect`/panic remains in `localtt-core::builders` |
| A8 | `attack` cannot express "no bonus" | `bonus: Option<i32>` | Matches `AttackEntry::bonus` |
| A9 | Payload boilerplate | Added `item_inline(name, kind, state, attuned) -> InventoryEntryId` | **Partial:** covers the bare "just carried it" case. The finding's ring example carries a description + modifier and is inherently rich, so it still uses `InventoryItem::Inline(..)`; a core `InlineItem::builder` was judged too much surface for now |
| A10 | `dm`/`player` differ only by `Role` | Added `participant(name, Role)`; kept `dm`/`player` as shorthands | Present |
| A11 | `Formula::dice(dice(1, 6))` reads poorly | Renamed the free helper to `dice_pool`; `Formula::dice(DiceExpr)` unchanged | No collision; the test reads `Formula::dice(dice_pool(1, 6))` |

### Pass 1b - extensibility

| # | Finding | Fix | Verification |
|---|---|---|---|
| B1 | `level_up` matches a class by `&str` | Moved to `localtt_engine::commands` and keyed by `ClassLevelId` | The test passes `pc.body.levels[0].id`; a `RefOrText::Ref` class is reachable by construction, not silently forced to `Text` |
| B2 | `Condition` is closed | Added `Condition::Custom { name: String }` in core | Present; serializes as `{"type":"custom","name":..}` under the existing tag |
| B3 | `LoadError` cannot grow / express semantic failure | `#[non_exhaustive]` on the relocated `LoadError` | **Partial:** the requested `Entity(EntityError)` variant is *not* expressible - the kind/body family check runs inside `Entity`'s `Deserialize`, so serde reports it through the erased deserializer error type (`D::Error`), unrecoverable in a generic `load_checked`. Documented on the enum; the real fix is a core `Entity::validate` split, deferred to RFC-0002's implement phase |
| B4 | `Party` is a fixed 8-field struct | Kept the typed named fields; documented that a new role is a small fixture edit | **Accepted, no change:** typed field access (`party().pc`) is more useful than churn-avoidance for a fixture the tests index by field. Low stakes by the finding's own note |
| B5 | Public API exists twice | Deleted `localtt_integration::builders`; the test imports `localtt_core::builders` | One path only; `just lint`/`just check` pass |
| B6 | Freezing builders in core (info) | None | Tracked in RFC-0003 OQ2 |

### Pass 1c - placement

| # | Finding | Fix | Verification |
|---|---|---|---|
| C1 | `load_checked`/`LoadError` is store logic | Moved to `localtt_adapter::load`; `roundtrip` keeps only `to_json`/`from_json` | `localtt-integration` no longer mentions the schema gate; the test imports `localtt_adapter::load` |
| C2 | `transform::*` are `engine::commands` | Moved to `localtt_engine::commands` | `localtt-integration/src/transform.rs` deleted; the test imports `localtt_engine::commands` |
| C3 | `fixtures`/`ids` are a reusable testkit | Deferred until a second consumer exists (the finding's own condition) | No `localtt-testkit` yet; no dev-dep cycle. `engine`/`adapter` are only `dev-dependencies` of this crate |
| C4 | Helpers belong to the testkit | Resolved together with A7: they now return `Result`, so they are safe `core` sugar and stay | See A7 |
| C5 | `to_json`/`from_json` are thin wrappers | Kept in `roundtrip` (this crate *is* the testkit) | Present |
| - | Keep in core (`Abilities::all`, `RulesetId::dnd5e_2014`, `Formula::*`, `Modifier::*`) | Unchanged, still non-panicking | `dice` renamed per A11 |

### Pass 2 - performance

No signature changed for performance; the pass-2 verdicts stand.

| # | Finding | Action | Verification |
|---|---|---|---|
| P1 | `AnyEntity` large variant | Accepted; deferred to RFC-0002's implement phase | `#[expect(clippy::large_enum_variant)]` unchanged |
| P2 | `load_checked` needs a full parse to peek the version | Fixed by the C1 relocation; the adapter documents a small `SchemaProbe` peek | `adapter::load` doc names the probe |
| P3-P8 | Acceptable at call frequency | None | Unchanged |
| P9 | `&mut self` chain is allocation-free | Informs A4 | Unchanged |

### Re-checked

- `just fmt` then `just lint` (fmt check + `clippy -D warnings`) pass.
- `just check` (`cargo doc --workspace` + `cargo hack --feature-powerset check`) passes.
- `cargo test --workspace --all-targets --no-run` compiles everything, including
  RFC-0002's `tests/types.rs` (the `Condition::Custom` variant did not break any
  exhaustive match).
- The target test is red for the **right** reason: all 17 tests panic at
  `todo!()` (the skeleton contract), not at a compile error.
- **`Campaign` placement:** it is defined in `localtt-core/src/types.rs:631`, not
  in the integration crate - the integration crate only *imports* it and, before
  this pass, *re-exported* `CampaignBuilder`. The facade is now gone (B5), so the
  only thing that was ever in the integration crate is removed.

### Still to do (not this pass)

- `T*` (test hardening) and the catalog coverage gaps T14-T21: the later test pass.
- Implement-phase items surfaced here: the `Entity`/`EntityError` `validate` split
  (B3), boxing `AnyEntity` (P1), and the builder setter macro (A5).
- `just test`'s full matrix (miri/loom/shuttle): red by design until the bodies
  land; the `todo!()` panics are the expected skeleton result.

Review baseline: commit `6c29e32`. Fix pass: this working tree on
`datamodel-integration` (uncommitted; the maintainer decides integration).
