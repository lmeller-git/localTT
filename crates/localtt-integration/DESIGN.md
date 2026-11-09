# localtt-integration: the end-to-end scenario suite

| | |
|---|---|
| **Status** | Implemented. This document supersedes `RFC-0003-integration.md` and `REVIEW-0003-integration.md` (both removed). |
| **Scope** | `crates/localtt-integration/` -- a leaf consumer crate: `src/` plus the native, session and browser test suites. |
| **Read with** | `crates/localtt-core/DESIGN.md` (the data model it builds), `docs/architecture.md`, `docs/engine.md`, `docs/decisions.md`. |

## 1. What this crate is

It is the **executable reading** of the product intent: "create a PC, level it up,
give it a handout, summon its familiar, serialize the campaign and load it back",
written as ordinary client code. It doubles as the integration suite (whole-campaign
JSON round trips, not just per-type ones) and as the **API playground**: the first
consumer to discover where the core/engine/adapter surfaces are awkward.

It is deliberately **not** a domain crate: it adds no rules and no I/O, only
assembles base data, transforms it, and serializes it. Nothing in production
depends on it. `localtt-engine` and `localtt-adapter` are dev-dependencies: only
the tests need them.

## 2. Layout

```
crates/localtt-integration/
  src/lib.rs          # pub mod fixtures, roundtrip; pub(crate) mod ids
  src/ids.rs          # deterministic fixture ids (crate-private)
  src/fixtures.rs     # the golden characters and party()
  src/roundtrip.rs    # thin to_json/from_json
  tests/scenarios.rs    # the target suite
  tests/session_e2e.rs # full-stack session tests
  tests/browser_e2e.rs # ignored Firefox/geckodriver smoke (`just e2e`)
  tests/ui_snapshots.rs # ignored screenshot/report scenario (`just ui-shot`)
  tests/support/       # in-process server and WebDriver harness for browser tests
```

The builders the fixtures use live in `localtt_core::builders`, the command surface
in `localtt_engine::commands`, and the schema gate in `localtt_adapter::load` --
this crate holds no production logic of its own.

## 3. Golden-character catalog

The scenarios that together exhaust the 5e core rules. The **store** column is what
core owns (asserted today); the **derive** column is covered by the engine's rules
tests and `tests/rules_target.rs`.

| Fixture | Rules exercised | Store (core) | Derive (engine) |
|---|---|---|---|
| **Aria**, level-5 high-elf evoker | ability modifiers, proficiency bonus, prepared spells | abilities, levels, saves, skills (prof + expertise), hp, resources, spell block, inventory, attack, feature, Bless status, overrides | `ability_mod.*`, `proficiency_bonus`, `save.*`, `skill.*`, passive scores, `spell.dc`, `attack.spell`, slot maxima |
| **Borin**, level-3 champion fighter | armor base AC, heavy-armor dex cap, shield bonus, strength requirement, versatile damage, short-rest recharge | armor/shield `ItemProperties`, `Coins`, longsword properties + `WeaponTag::Versatile`, Second Wind `Resource` (`Recharge::Short`) | AC from equipped armor + shield + dex cap, attack/damage bonus, two-body-armor and strength-req warnings |
| **Lyra**, level-2 warlock | Pact Magic, short-rest slots, spell attack, concentration damage rider | `ResourceKind::PactSlot`, `Recharge::Short`, Hex as a concentration `Status` with a `Dice` modifier | pact slot max, spell attack bonus, Hex `+1d6` composed into a damage roll |
| **Mira**, level-5 (wizard 3 / cleric 2) | multiclass spell slot table | two `ClassLevel`s, per-class spell blocks | multiclass slot table from combined caster levels |
| **Goblin**, CR 1/4 | creature profile, CR eighths, size, creature type, senses, monster action | `Profile::Creature`, `ChallengeRating`, `Size`, `creature_type`, `senses`, `FeatureCategory::Action` | AC/attack lines from traits, recharge-by-roll (manual) |
| **Aria's familiar + wolf companion** | `parent` link, copy-on-summon | `EntityKind::Familiar` / `Companion`, `parent`, independent element ids | companion proficiency sharing is deferred (RFC-0001 4.3) |
| **Bless riding a save** | status-borne modifiers, group targets, dice terms, attribution | a `Group(Save)` + `Add { Dice(1d4) }` modifier on a `Status` | fold `+1d4` into the save, attribute the term to "Bless" |
| **Timed status vs clock** | absolute expiry, correctness under a clock rewind | `started_at` + `StatusDuration::Timed { expires_at }` + `Clock` | `expired = now >= expires_at`; `StatusExpired` feed entries |
| **Temp HP + damage + death saves** | temp HP does not stack or get healed; dying at 0 | `Hp`, `DeathSaves`, negative `current` allowed | temp-HP stacking/depletion, death-save and concentration-drop hints |
| **Overrides** | `override ?? computed`, staleness | `BTreeMap<StatKey, OverrideValue>` | value **and** breakdown; keep the override when inputs change |
| **Equip/attune** | only equipped+attuned modifiers contribute | `InventoryEntry { state, attuned }`, `Modifier.when` | contribution filtering; over-attunement warning |
| **Handout give** | a copy lands in the recipient's inventory and becomes theirs | `HandoutContent::Item`, a fresh `InventoryEntry` | (none; ACL/`view_of` governs read-sharing) |
| **Redaction** | single egress point | `AclEntry`, `FieldMask`, `ViewField` | `view_of(entity, viewer)` matrix (RFC-0001 5.7) |
| **Roll + feed** | structured dice, audience, ring buffer | `DiceExpr`, `Roll`, `DiceResult`, `FeedEntry` | parser, roller behind the `Rng` port, audience fan-out |

## 4. Fixtures and determinism

**Determinism is a contract** (`fixtures_are_deterministic_byte_for_byte`): fixed
UUIDs and fixed times, `BTreeMap`-backed maps only, no floats, no RNG, no
wall-clock reads. Identical inputs serialize to identical bytes, which keeps the
door open for snapshot tests and makes round-trip diffing meaningful.

- `ids::fixture_uuid(seed)` derives a stable v7-shaped UUID from a seed;
  `ids::fixture_id::<T>(seed)` wraps it as an `Id<T>`. Both are crate-private.
- Each fixture owns a seed space; within it, the `fixtures`-private
  `element_id(seed, index)` gives distinct element ids. Element ids are passed to
  the builders explicitly (the `*_with_id` variants and direct struct
  construction), so nothing has to be reseeded or patched after the fact. Fresh
  `Id::new()` is used only where determinism does not matter.
- `party()` builds Aria plus her wolf companion (`parent` = the PC) and owl
  familiar (`parent` = the PC), a DM-only note, an item handout, and the campaign
  with its two player seats. Helper fixtures (`golden_wizard`, `golden_fighter`,
  `golden_warlock`, `golden_multiclass`, `goblin`) are `pub` so other suites can
  reuse the cast.

## 5. Surfaces it consumes

- **Core builders** (`localtt_core::builders`): `CampaignBuilder`,
  `StatblockBuilder`, and the literal helpers. See `crates/localtt-core/DESIGN.md`
  section 8 for their shape and rationale. The fixtures are written through them on
  purpose, so any builder ergonomics problem shows up here first.
- **Engine commands** (`localtt_engine::commands`): the free helpers
  `apply_damage`, `heal`, `set_temp_hp`, `spend_resource`, `begin_concentration`,
  `level_up` (keyed by `ClassLevelId`, not a name -- a class may be stored as a
  `RefOrText::Ref` and would not be findable by text), `summon_from_template` and
  `give_handout` (returns `Option<InventoryEntryId>`: `Some` for an item, `None`
  for text). These helpers mutate `&mut` in place and are data-only. The
  executor's `Command` path is the production surface: it adds authorization,
  warnings and durable-before-visible transitions.
- **Adapter schema gate** (`localtt_adapter::load`): `load_checked` +
  `LoadError`, which peek `schema_version` out of the raw JSON before a full parse
  so a too-new file is refused without materializing it.

## 6. What the suite pins

`tests/scenarios.rs` covers, across the crates:

- **Character creation**: a hand-built character through the core builders, plus
  the golden cast through `fixtures`.
- **Handling**: damage, temp HP, heal, resource spend (overspend allowed and
  warned), concentration replace, level up (by class id, including a
  `RefOrText::Ref` class), summon (a fresh copy linked to its caster), handout.
- **Serialization**: whole-party JSON round trips through `roundtrip`, and the
  schema gate in `localtt_adapter::load` (same-major/higher-minor and older-major
  documents load; a newer major is refused; both re-stamp to `CURRENT`).
- **Catalog coverage** (store column): statuses and every duration, rolls and the
  feed, the full modifier and formula surface (`Set`/`Min`/`Max`/advantage plus
  every arithmetic leaf and `Level`/`ClassLevel`), multiclass and duplicate
  classes, custom/homebrew stats, boundary values, the entity kind/body matrix
  with `rev`/`parent`/`acl`/`schema_version`, and the canonical `StatKey`
  vocabulary.
- **Failure cases**: a hand-edited `kind`/`body` mismatch is rejected; an invalid
  reference is rejected.
- **Concurrency**: a static `Send + Sync` bound over the suite types. The suite is
  plain data, so there is no contention case.

`tests/session_e2e.rs` is the full-stack exploratory suite: it joins the layers
as a real session would (fixtures -> store -> executor with the shipped
`ChaChaRng` and `FixtureSource` -> read services), pins a SQLite close/reopen, a
multi-actor authorization matrix over command-generated feed entries, and the
failure atomicity of refused commands.

`tests/browser_e2e.rs` and `tests/ui_snapshots.rs` are the ignored browser
layers. Both run the real server in-process with `web/dist` and headless
Firefox; the harness lives in `tests/support/`. See
[`docs/testing.md`](../../docs/testing.md) for the journeys, selectors and
artifacts.

`tests/scenarios.rs` does not assert derived values (proficiency bonus, AC, save
DC, slots); those live in `tests/rules_target.rs` and the engine unit tests.

## 7. Review ledger

The three-pass review (API, performance, test critique) is closed. Its outcomes
are folded into the code, the builder contract in
[`crates/localtt-core/DESIGN.md`](../localtt-core/DESIGN.md), and the decision
log. Two items remain deliberately deferred: `AnyEntity` /
`InventoryItem::Inline` boxing, and the core `Entity::validate` split. Both are
tracked in [`TODO.md`](../../TODO.md).

## 8. Deferred work

- **Snapshot tests:** exact path asserts today (no new dependency); add `insta`
  once snapshots are worth diffing.
- **`localtt-testkit`:** extract only when a second consumer appears.
- **Fixed-string determinism pin:** a `party()` serialization asserted against a
  literal, once the fixture bytes are considered frozen.
- **`Party` reuse:** `Party::entities()` deep-clones the party; memoize if it ever
  lands in a loop.
- **Cross-entity derivation** (for example companion proficiency sharing): see
  [`TODO.md`](../../TODO.md).
