# RFC-0003: localtt-integration - the end-to-end scenario suite and API playground

| | |
|---|---|
| **Status** | Draft (spec phase; no crate code written yet) |
| **Audience** | The implementing agent(s) and the maintainer |
| **Scope** | A new leaf crate `crates/localtt-integration/`. It consumes `localtt-core` now and `localtt-engine`/`localtt-adapter` later. It is not a domain crate; nothing depends on it in production. |
| **Depends on** | RFC-0001 sections 2, 4, 5.1-5.7, 5.9, 5.12, 7 (binding shape and testing strategy); RFC-0002 (the core type contract); AGENTS.md (workflow, style) |
| **Target test** | `crates/localtt-integration/tests/scenarios.rs` (does not compile until the skeleton phase) |

This RFC specs a **consumer crate**: it exercises the core (and, later, the
engine) the way a real client would, doubles as the integration suite RFC-0001
section 7 asks for, and is the workbench where the client-facing API is tried
out before it is frozen. It is deliberately *not* a domain crate: it adds no
rules, it only assembles base data, transforms it, and serializes it.

## 0. Decisions carried in from review questions

| # | Question | Decision |
|---|---|---|
| Q1 | Crate name | `localtt-integration` (terse form of `localtt-engine-integration`; the crate consumes core now and engine later, so the shorter, broader name fits). |
| Q2 | Where builders live | Tiny, data-only constructors belong in `core` (section 4). Scenario/statblock builders and command stand-ins start here; they are expected to **graduate to `engine`** once the engine has a builder layer, because some of them will want rules logic. Recorded as Open Question 8.2. |
| Q3 | Target-test scope now | Core-only. The engine crate is an `add()` placeholder; the rules assertions are catalogued in section 3 and activated by a follow-up RFC when the engine lands. |

## 1. Goals / Non-goals

### 1.1 Goals

- Be the **executable reading** of RFC-0001: a place where "create a PC, level
  it up, give it a handout, summon its familiar, serialize the campaign and load
  it back" is written as ordinary client code.
- Be the **integration suite**: end-to-end JSON round trips over whole
  campaigns (multiple entities, players, feed), not just per-type round trips.
  RFC-0002's `tests/types.rs` pins the *shape* of one entity; this suite pins
  that a *campaign* of entities survives storage-like transformations.
- Be the **API playground**: the first thing that discovers where the core API
  is awkward (they are collected in section 4 and Open Questions), and the place
  where the eventual engine command surface is prototyped before it exists.
- Encode a **golden-character catalog** (section 3) that exhausts the 5e rules
  core is expected to store and the engine is expected to derive, so the rules
  work has a target from day one.

### 1.2 Non-goals

- No rules logic that belongs to the engine (modifier evaluation, derived
  values, rests, warnings). Where a scenario needs a rule the engine does not
  have yet, the crate uses a clearly-marked **stand-in** (section 5.4) that is
  deleted when the real engine command lands.
- No I/O: no SQLite, no HTTP, no files. JSON strings only. The adapter's real
  store gets contract tests of its own.
- No production use: not a dependency of `core`, `engine`, `adapter`, `server`.
- No new domain types. Every type the suite builds is a `localtt-core` type.

## 2. Client usage

The target test `crates/localtt-integration/tests/scenarios.rs` is the fixed
point. It is written exactly as a client would use the crate and the core API:

- build a whole party (`fixtures::party()`) plus hand-built characters through
  `builders::StatblockBuilder` and `builders::CampaignBuilder`;
- round-trip the party through `serde_json` and assert equality;
- drive the paper-sheet lifecycle: level up, equip, spend a resource, take
  damage, mark death saves, begin concentration, summon, give a handout;
- assert the **stored** side of the golden characters (what core owns) and pin
  the JSON paths that matter (schema version, kind tags, resource kinds,
  `parent` links, inventory `state`, override keys);
- the failure cases: a newer schema major is refused; a hand-edited document
  whose `kind` and `body` disagree is rejected; an invalid reference is
  rejected.

It does not assert derived values (proficiency bonus, AC, save DC, slots); those
need the engine and are listed in section 3 for the follow-up RFC. The suite is
single-threaded plain data, so there is no contention case; the concurrency
contract is the `Send + Sync` assertion already made by RFC-0002's test.

## 3. Golden-character catalog (the rules target)

This is the "think about the target tests" half of the brief: the scenarios that
together exhaust the 5e core rules the engine must compute. Each row names the
fixture, the rules it exercises, the phase that asserts it, and *what core
stores* versus *what the engine derives*. Phase C = asserted by this crate now
(pure data + serde). Phase E = asserted once the engine lands (follow-up RFC).

| Fixture | Rules exercised | Store (core) | Derive (engine, phase E) |
|---|---|---|---|
| **Aria**, level-5 high-elf evoker | ability modifiers, proficiency bonus, prepared spells | abilities, levels, saves, skills (prof + expertise), hp, resources, spell block, inventory, attack, feature, Bless status, overrides | `ability_mod.*`, `proficiency_bonus`, `save.int/wis`, `skill.arcana`, `passive.perception`, `spell.dc`, `attack.spell`, slot maxima |
| **Borin**, level-3 champion fighter | armor base AC, heavy-armor dex cap, shield bonus, strength requirement, versatile damage, short-rest recharge | armor/shield `ItemProperties`, `Coins`, longsword properties + `WeaponTag::Versatile`, Second Wind / Action Surge `Resource`s with `Recharge::Short` | AC from equipped armor + shield + dex cap, attack/damage bonus, two-body-armor and strength-req **warnings** |
| **Lyra**, level-2 warlock | Pact Magic, short-rest slots, spell attack, concentration damage rider | `ResourceKind::PactSlot`, `Recharge::Short`, Hex as `StatusKind::Concentration` with a `Dice` modifier | pact slot max, spell attack bonus, Hex `+1d6` composed into a damage roll |
| **Mira**, level-5 (wizard 3 / cleric 2) | multiclass spell slot table | two `ClassLevel`s, per-class spell blocks | multiclass slot table from combined caster levels |
| **Goblin**, CR 1/4 | creature profile, CR eighths, size, creature type, senses, monster action | `Profile::Creature`, `ChallengeRating`, `Size`, `creature_type`, `senses`, `FeatureCategory::Action` | AC/attack lines from traits, recharge-by-roll (manual) |
| **Aria's familiar + wolf companion** | `parent` link, copy-on-summon | `EntityKind::Familiar` / `Companion`, `parent`, independent element ids | companion proficiency sharing is deferred (RFC-0001 4.3) |
| **Bless riding a save** | status-borne modifiers, group targets, dice terms, attribution | `Modifier { target: Group(Save), op: Add { value: Dice(1d4) } }` on a `Status` | fold `+1d4` into the save roll, attribute the term to "Bless" |
| **Timed status vs clock** | absolute expiry, correctness under clock rewind | `started_at` + `StatusDuration::Timed { expires_at }` + `Clock` | `expired = now >= expires_at`; `StatusExpired` feed entries |
| **Temp HP + damage + death saves** | temp HP does not stack, depletes first; healing does not restore it; dying at 0 | `Hp { max, current, temp }`, `DeathSaves`, negative `current` allowed | temp-HP stacking/depletion, death-save hints, concentration-drop hint |
| **Overrides** | `override ?? computed`, staleness | `BTreeMap<StatKey, OverrideValue>` | return value **and** breakdown; keep the override when inputs change |
| **Equip/attune** | only equipped+attuned modifiers contribute | `InventoryEntry { state, attuned }`, `Modifier.when` | contribution filtering; over-attunement warning |
| **Handout give** | a copy lands in the recipient's inventory and becomes theirs | `HandoutContent::Item`, fresh `InventoryEntry` | (none; ACL/`view_of` governs read-sharing) |
| **Redaction** | single egress point | `AclEntry`, `FieldMask`, `ViewField` | `view_of(entity, viewer)` matrix (RFC-0001 5.7) |
| **Roll + feed** | structured dice, audience, ring buffer | `DiceExpr`, `Roll`, `DiceResult`, `FeedEntry` | parser, roller behind the `Rng` port, audience fan-out |

Phase C covers every row's **store** column plus the serde contract. Phase E
covers the **derive** column. Splitting the catalog this way is what lets this
crate land before the engine.

## 4. Proposed additive core changes (found by writing the client)

Writing realistic fixtures against the current API (RFC-0002) is very verbose.
The following additive, non-breaking additions to `core` pay for themselves in
every consumer (engine, PDF import, packs), so they belong in `core` rather than
only here. All are minor-version additions; RFC-0002's "no `Default` for
`Abilities`/`StatblockBody`" rule is respected (no made-up scores).

```rust
impl Abilities {
    /// Every ability set to the same score (RFC-0002 9.9 sanctions this as the
    /// explicit alternative to a made-up `Default`).
    pub fn all(score: i32) -> Self;
}

impl RulesetId {
    /// The default 5e (2014) ruleset, already parsed.
    pub fn dnd5e_2014() -> Self;
}

impl Formula {
    /// `+proficiency_bonus`.
    pub fn proficiency_bonus() -> Self;
    /// `+ability_mod(a)`.
    pub fn ability_mod(ability: Ability) -> Self;
    /// A dice leaf; mirrors [`FormulaExpr::Dice`].
    pub fn dice(expr: DiceExpr) -> Self;
}

impl Modifier {
    /// An unconditional `Add` modifier on `target`.
    pub fn add(target: StatKey, value: Formula) -> Self;
    /// An `Add` modifier that applies only while `when` holds.
    pub fn add_when(target: StatKey, value: Formula, when: Condition) -> Self;
}
```

Everything else that makes fixtures readable - the campaign/statblock builders
and the command stand-ins - stays in this crate for now (section 5). The
maintainer's steer (Q2) is that they may then move to `engine`, because some
will want rules logic; keeping them here avoids freezing a builder API in `core`
before the engine can inform it. That split is Open Question 8.2.

## 5. Public API sketch

### 5.1 Module layout

```
crates/localtt-integration/
  Cargo.toml          # localtt-core (path), serde_json; dev: none beyond test deps
  RFC-0003-integration.md
  src/lib.rs          # re-exports; #![deny(missing_docs)]
  src/ids.rs          # deterministic fixture ids (fixed UUIDs)
  src/builders.rs     # CampaignBuilder, StatblockBuilder, element helpers
  src/fixtures.rs     # the golden characters and `party()`
  src/roundtrip.rs    # JSON round trip + schema gate
  src/transform.rs    # command stand-ins (section 5.4), gated as provisional
  tests/scenarios.rs  # the target test
```

`lib.rs` re-exports `builders`, `fixtures`, `roundtrip`, `transform` and the
handful of free functions below. Adapting the workspace's `crates/*` glob means
the crate joins `just test`/`just check` with no CI change.

### 5.2 Builders

```rust
// builders.rs

/// Ergonomic assembly of a `Campaign`; fills current schema version and the
/// model defaults, and owns the player roster.
pub struct CampaignBuilder { /* ... */ }
impl CampaignBuilder {
    /// New campaign with a fresh id, `SchemaVersion::CURRENT`, the 5e-2014
    /// ruleset, clock at 0 and default settings.
    pub fn new(name: impl Into<String>) -> Self;
    pub fn ruleset(&mut self, r: RulesetId) -> &mut Self;
    pub fn dawn_hour(&mut self, hour: u8) -> &mut Self;
    pub fn feed_capacity(&mut self, capacity: u32) -> &mut Self;
    /// Adds a DM slot and returns its id.
    pub fn dm(&mut self, display_name: impl Into<String>) -> PlayerId;
    /// Adds a player slot and returns its id.
    pub fn player(&mut self, display_name: impl Into<String>) -> PlayerId;
    pub fn build(self) -> Campaign;
}

/// Ergonomic assembly of a `StatblockBody`. `&mut self` methods so that an
/// entry id can be handed back mid-chain (`item`) and reused by a later
/// `attack` - the consuming style of `core::EntityBuilder` cannot do that.
pub struct StatblockBuilder { /* ... */ }
impl StatblockBuilder {
    pub fn character(name: impl Into<String>) -> Self;
    pub fn creature(name: impl Into<String>, cr: ChallengeRating) -> Self;
    pub fn size(&mut self, size: Size) -> &mut Self;
    pub fn abilities(&mut self, abilities: Abilities) -> &mut Self;
    pub fn class_level(
        &mut self,
        class: RefOrText,
        level: i32,
        subclass: Option<RefOrText>,
    ) -> &mut Self;
    pub fn save_proficiency(&mut self, ability: Ability) -> &mut Self;
    pub fn skill(&mut self, skill: Skill, level: SkillProficiency) -> &mut Self;
    pub fn hp(&mut self, max: i32, current: i32) -> &mut Self;
    pub fn speed(&mut self, speed: Speed) -> &mut Self;
    pub fn senses(&mut self, text: impl Into<String>) -> &mut Self;
    pub fn coins(&mut self, coins: Coins) -> &mut Self;
    /// Adds a sparse override; `override ?? computed` (RFC-0001 2.3).
    pub fn override_stat(&mut self, key: StatKey, value: OverrideValue) -> &mut Self;
    pub fn resource(&mut self, resource: Resource) -> &mut Self;
    pub fn spellcasting_block(
        &mut self,
        source: RefOrText,
        ability: Ability,
        spells: Vec<SpellEntry>,
    ) -> &mut Self;
    /// Adds an inventory entry and returns its id.
    pub fn item(
        &mut self,
        item: InventoryItem,
        qty: i32,
        state: ItemState,
        attuned: bool,
    ) -> InventoryEntryId;
    pub fn attack(
        &mut self,
        name: impl Into<String>,
        bonus: i32,
        damage_text: impl Into<String>,
        from_inventory: Option<InventoryEntryId>,
    ) -> &mut Self;
    pub fn feature(&mut self, feature: FeatureEntry) -> &mut Self;
    pub fn status(&mut self, status: Status) -> &mut Self;
    pub fn notes(&mut self, text: impl Into<String>) -> &mut Self;
    pub fn build(self) -> StatblockBody;
}

/// Small element helpers so fixtures do not repeat `RefId::parse(..).expect`.
pub fn content_ref(id: &str, name: &str) -> ContentRef;   // panics on a bad literal
pub fn stat(raw: &str) -> StatKey;                        // panics on a bad literal
pub fn ref_id(raw: &str) -> RefId;                        // panics on a bad literal
pub fn dice(count: i32, sides: i32) -> DiceExpr;          // plain pool
```

`content_ref`/`stat`/`ref_id` are test-scaffolding conveniences that `expect`
on string literals; that is allowed because they are test support, not library
code (AGENTS.md's no-`unwrap` rule is about production paths).

### 5.3 Fixtures

```rust
// fixtures.rs

/// A whole campaign with its cast; the unit this suite round-trips.
pub struct Party {
    pub campaign: Campaign,
    pub dm: PlayerId,
    pub player: PlayerId,
    pub pc: StatblockEntity,        // Aria
    pub companion: StatblockEntity, // Aria's wolf, parent = pc
    pub familiar: StatblockEntity,  // Aria's owl, parent = pc
    pub note: NoteEntity,           // DM-only session note
    pub handout: HandoutEntity,     // item handout
}
impl Party {
    /// Every entity as the runtime-tagged type storage speaks.
    pub fn entities(&self) -> Vec<AnyEntity>;
}

/// Aria: level-5 high-elf evoker (section 3). Deterministic ids/times.
pub fn golden_wizard(owner: PlayerId) -> StatblockEntity;
/// Borin: level-3 champion fighter.
pub fn golden_fighter(owner: PlayerId) -> StatblockEntity;
/// Lyra: level-2 warlock.
pub fn golden_warlock(owner: PlayerId) -> StatblockEntity;
/// Mira: level-5 wizard 3 / cleric 2.
pub fn golden_multiclass(owner: PlayerId) -> StatblockEntity;
/// Goblin: a CR 1/4 creature.
pub fn goblin(owner: PlayerId) -> StatblockEntity;

/// The assembled party used by most tests.
pub fn party() -> Party;
```

Fixtures use **fixed UUIDs and fixed times** (via `ids.rs` and
`Id::from_uuid`) so serialized bytes are reproducible across runs and machines;
this is what makes deterministic snapshot tests (Open Question 8.6) possible
later. Fresh `Id::new()` calls are only used where determinism does not matter.

### 5.4 Transform stand-ins (provisional)

These mirror the engine command catalogue (RFC-0001 5.2) closely enough that the
same tests keep passing when the real commands land and the calls are swapped.
They are **data-only**: no derived computation, no warnings, no authorization.

```rust
// transform.rs
#![doc = "Provisional stand-ins for engine::commands; deleted as the engine lands."]

/// Mirror of `commands::apply_damage`: temp HP absorbs first, then current;
/// `current` may go negative (engine warns, never blocks - RFC-0001 2.2).
pub fn apply_damage(entity: &mut StatblockEntity, amount: i32);
/// Mirror of `commands::heal`; healing does not restore temp HP.
pub fn heal(entity: &mut StatblockEntity, amount: i32);
/// Mirror of `commands::set_temp_hp`; non-stacking (take the higher).
pub fn set_temp_hp(entity: &mut StatblockEntity, amount: i32);
/// Mirror of `commands::spend_resource`; overspending is allowed.
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32);
/// Mirror of `commands::begin_concentration`: replaces any existing
/// concentration status (RFC-0001 5.4, command-layer semantics).
pub fn begin_concentration(entity: &mut StatblockEntity, status: Status);
/// Mirror of `commands::level_up` for one class: bumps the level, optionally
/// appends a feature/spell and a resource. Overrides are untouched (stale
/// overrides stay; RFC-0001 5.1).
pub fn level_up(entity: &mut StatblockEntity, class: &str, to_level: i32);
/// Mirror of `commands::create_summon_from_template`: a **copy** with fresh
/// element ids, owned by `caster`, linked via `parent` (RFC-0001 5.12).
pub fn summon_from_template(
    template: &StatblockEntity,
    caster: &StatblockEntity,
) -> StatblockEntity;
/// Mirror of `commands::give_handout`: a `HandoutContent::Item` becomes a fresh
/// `InlineItem` inventory entry owned by the recipient.
pub fn give_handout(handout: &HandoutEntity, recipient: &mut StatblockEntity) -> InventoryEntryId;
```

Every stand-in takes `&mut` and mutates in place; the real engine commands will
likely be `(state, command) -> result`, and these tests will be rewritten to that
shape. Keeping the names identical makes that a mechanical edit.

### 5.5 Round trip and the schema gate

```rust
// roundtrip.rs

/// Serializes any core document to a JSON string.
pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error>;
/// Deserializes a core document from JSON.
pub fn from_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, serde_json::Error>;
/// Loads a document after checking `SchemaVersion::CURRENT.accepts` on the
/// document's own version (RFC-0001 5.9): a newer major is refused rather than
/// partially loaded.
pub fn load_checked<T: serde::de::DeserializeOwned>(
    value: &str,
) -> Result<T, LoadError>;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The file's major version is newer than this build supports.
    #[error("file schema {0:?} is newer than this build")]
    Schema(SchemaVersion),
    /// The document did not parse.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

`load_checked` reads `schema_version` out of the raw JSON first (a small
`serde_json::Value` peek) so the gate runs before a full parse; the exact peek
is an implementation detail for the skeleton phase.

## 6. Guarantees

### 6.1 Kind of crate

- Test-only. It has no production consumers; adding it as a `dev-dependency`
  elsewhere is an Open Question (8.4), not a promise.
- No `unsafe`, no `unwrap`/`expect` in library paths; `expect` is confined to
  the literal-parsing helpers (5.2), which are test scaffolding.

### 6.2 Determinism

- Fixtures produce identical JSON bytes for identical inputs: fixed UUIDs,
  fixed wall/campaign times, `BTreeMap`-backed maps only. This keeps room for
  snapshot tests and makes round-trip diffing meaningful.
- No floats, no RNG, no wall-clock reads anywhere in the crate.

### 6.3 Error model

- The crate introduces exactly one error type, `LoadError`, and only for the
  schema gate (5.5). Everything else surfaces `serde_json::Error` or the core
  parse errors unchanged, so tests fail loudly and specifically.

### 6.4 Threading / progress

- Plain data; no concurrency. `Send + Sync` is already asserted for the core
  types by RFC-0002's test. There is no contention case to add here.
- No locks, no blocking, no async. Progress is not applicable.

## 7. Internal structure

- `builders` -> `fixtures`: fixtures are written with the builders, so any
  builder ergonomics problem shows up immediately (that is the point).
- `transform` mutates only core documents and never calls `builders` or
  `fixtures`; it stays swappable for `engine::commands`.
- `roundtrip` is the only module that mentions `serde_json` in its signature; a
  later adapter-backed test can reuse `load_checked` unchanged.
- Phasing:
  - **C (this RFC / now):** `ids`, `builders`, `fixtures`, `roundtrip`,
    `transform`; `tests/scenarios.rs` core-only; the core additions in section 4.
  - **E (follow-up RFC, when `engine` exists):** assert the derive column in
    section 3, add a deterministic `Rng` fixture and roll/feed tests, and add
    the `view_of` authorization matrix (RFC-0001 5.7) against whatever port the
    engine exposes.

## 8. Open questions

None block the skeleton phase. (Separately, the target test depends on the
section 4 core additions; that is a build-order constraint, not an open question:
either land them with `core`'s implement phase, or the skeleton phase adds them
too.)

1. **Crate name.** `localtt-integration` (chosen) vs `localtt-engine-integration`
   (the maintainer's longer form) vs `localtt-scenarios`. Recommended:
   `localtt-integration`; it consumes core today, so a name that says "engine"
   only would undersell it. Rename is mechanical while nothing depends on it.
2. **Builder home (Q2).** Data-only constructors in `core` (recommended, and
   used by the target test), builders here now and possibly `engine` later.
   Tradeoff: freezing a builder API in `core` before the engine is risky; a
   builder in `engine` cannot be used until the engine exists, which would stall
   this crate. Recommended: section 4 into `core`, the rest stays here.
3. **Stand-ins vs waiting for engine commands (5.4).** Recommended: keep the
   stand-ins so the client workflow is executable now; delete each as its real
   command lands. Alternative: mark each `#[ignore]` until the engine exists,
   which loses the "does the workflow make sense" signal.
4. **Reusability.** Should `engine`/`adapter` dev-depend on this crate for
   their own tests (a shared testkit), or should this stay a leaf consumer and
   each crate keep its own helpers? Tradeoff: sharing reduces duplication but
   creates dev-dependency cycles once this crate depends on `engine`.
   Recommended: leaf consumer now; extract a `localtt-testkit` only if a second
   consumer appears.
5. **`transform` shape.** `&mut` in place (chosen, terse) vs value-returning
   `(state, command) -> result` mirrored from the eventual engine. Recommended:
   `&mut` now; revisit when the engine command signature is fixed.
6. **Snapshot tests.** Add `insta` now for the JSON fixtures, or keep exact
   path asserts only? Recommended: path asserts now (no new dependency); add
   `insta` in phase E once the engine makes snapshots worth diffing.
7. **Multiclass fixture rules.** Whether "wizard 3 / cleric 2" slot math is the
   canonical multiclass golden character is an engine concern; this crate only
   stores the two `ClassLevel`s today.

## 9. Cross-check

- Every item the target test uses is in section 5 (builders, fixtures,
  round trip, transforms) or section 4 (the additive core constructors).
- Every item in sections 4 and 5 is used by the target test or justified:
  `CampaignBuilder`/`StatblockBuilder` (creation tests), `party`/`golden_*`
  (round-trip and catalog), `load_checked` (schema-gate failure),
  `apply_damage`/`heal`/`set_temp_hp` (HP test), `spend_resource` (resource
  test), `begin_concentration` (concentration test), `level_up`,
  `summon_from_template`, `give_handout` (transformation tests), the section 4
  constructors (throughout the fixtures and the hand-built character).
- No element of the sketch is speculative: the ones not touched directly by a
  test (`golden_multiclass`, `golden_warlock`, `ref_id`, `dice`) are fixtures
  the section 3 catalog requires and are exercised by the fixture-assembly
  tests.
- The builder methods the target test does not call directly (`creature`,
  `size`, `speed`, `senses`, `coins`, `resource`, `spellcasting_block`,
  `status`) are the ones the section 3 fixtures are written with; section 5.3
  requires fixtures to be built through the builders, so they are exercised by
  every test that calls a `golden_*`/`party` fixture.
