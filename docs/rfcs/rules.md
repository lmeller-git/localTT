# RFC: U4 -- derived sheets and the static rules layer

| | |
|---|---|
| **Status** | Spec phase (2026-10-18, branch `u4-u6-spec`). This RFC and `crates/localtt-integration/tests/rules_target.rs` are the first U4 artifacts. No library code exists: the target test intentionally does not compile until the U4 skeleton lands, then fails with `not yet implemented` until the implement phase. This RFC settles the U3 RFC's OQ-2 (static `Formula` evaluation) and splits its OQ-5 (rest recovery); see section 8. |
| **Scope** | New `localtt-engine::rules` module, additions to `localtt-engine::error` (`WarningCode`, `ElementRef`), and the long-rest recovery arithmetic in `commands::rests` (OQ-5 split; no frozen test covers it). Target test: `crates/localtt-integration/tests/rules_target.rs`. U5 consumes `RollParts`/`roll_parts` (cross-linked in `docs/rfcs/dice.md`); U6 views do not include derived values yet (`docs/rfcs/authz.md`). The core data model is not touched; every missing wire field is an Open Question, not an edit. |
| **Read with** | `docs/rfcs/engine-executor.md` sections 3, 5.3, 9 (OQ-2, OQ-5); `docs/engine-architecture.md` sections 7.4-7.5, 7.8, 9 (Rules row), 10 (U4); `docs/decisions.md` ENG-1/9/13-27, DATA-9/10; `RFC-0001-localtt.md` sections 2.2, 2.3, 5.1, 5.3, 8.2; `crates/localtt-core/DESIGN.md` sections 3.6, 3.9; `crates/localtt-integration/DESIGN.md` section 3 (golden catalog); this batch's `dice.md`, `authz.md` and `docs/plans/u4-u6.md`. |
| **Pins** | `docs/engine-architecture.md` section 7.4 sketches the rules layer without `resources`, `slots`, `hit_dice`, `hint` or the minimal class table; every such difference is recorded in section 8, every unresolved fork in section 9. |

## 1. Goals

1. Pin static derivation: `rules::derive(&StatblockEntity, &RulesContext) ->
   DerivedSheet`, the context, and the derived output shapes (`DerivedValue`,
   `Contribution`, `DerivedResource`, `SlotMaxima`, `RollHint`).
2. Settle **OQ-2** from `engine-executor.md`: full static `Formula`
   evaluation (`Const`, `ProficiencyBonus`, `AbilityMod`, `Level`,
   `ClassLevel`, `Add`/`Sub`/`Mul`/`Min`/`Max`/`FloorDiv`), so rest, dawn and
   `RestoreResource` arithmetic has a number when rules call for one.
3. Pin modifier collection: sources (items, features, statuses), condition
   gating (`Equipped`, `Attuned`, `StatusActive`, `Custom`), group expansion
   (`Save`, `Check`), the fixed application order, and warning scoping.
4. Pin AC composition: body-armor selection, shield selection, over-armor and
   over-attunement warnings, strength requirements, zero-quantity entries.
5. Pin the class tables the golden fixtures prove: proficiency bonus from
   level/CR, multiclass spell slots, Pact Magic, hit-dice pools.
6. Pin the U4/U5 seam: `RollParts`/`DiceTerm` and `rules::roll_parts`, which
   extracts attributed dice terms and the flat part for `dice::compose`.
7. Split **OQ-5**: long-rest recovery (HP to maximum, death saves reset,
   exhaustion reduced) lands here; hit-die spend/regain is U5.
8. Extend the shared warning taxonomy: eight new `WarningCode` variants and
   three new `ElementRef` variants (section 4.2).

## 2. Non-goals

- **No dice parsing or rolling.** `dice::parse`/`validate`/`compose`/`roll` and
  the `Rng` port are U5; `roll_parts` only extracts terms, never rolls.
- **No authorization.** `can_read`/`can_write`/views are U6; `derive` has no
  viewer parameter and never hides anything.
- **No U3 command changes for formula maxima.** The frozen test
  `restoring_with_a_formula_maximum_is_unclamped_and_warns` pins the current
  behavior; activating evaluation there is OQ-1, decided outside this batch.
- **No attack-line derivation.** `attack.melee`/`attack.ranged` need weapon
  proficiency and property data the model does not carry; only `attack.spell`
  is derived.
- **No half/third casters, subclasses, or unarmored defense features.**
  `rules::tables` covers the four classes the golden catalog models; anything
  else warns (`UnmodeledClass`); a content-driven ruleset bundle is a later
  unit (RFC-0001 5.1).
- **No content loading.** The caller materializes `&BTreeMap<RefId,
  ContentEntry>`; adapter sources stay U2's business.
- **No hit dice spend/regain.** U5 (`SpendHitDie`, long-rest regain).
- **No cross-entity derivation** (RFC-0001 4.3), no caching/recomputation
  strategy, and no derived values inside U6 views.
- **No core edits.** Missing wire fields (e.g. a `ClassLevel` element
  reference for warnings) are Open Questions.

## 3. Client usage: the target test

`crates/localtt-integration/tests/rules_target.rs` is ordinary client code: it
consumes the immutable `fixtures` cast, adds test-local items and statuses,
builds a literal `BTreeMap<RefId, ContentEntry>` where a reference must
resolve, and reads derived values, contributions, slots, warnings and roll
parts. Ten tests:

| Test | What it pins |
|---|---|
| `the_wizard_derives_the_printed_sheet_values` | the golden Aria values: `proficiency_bonus` 3, `ability.int` 18, `ability_mod.int` 4, `save.int` 7, `save.wis` 4, `skill.arcana` 10 (expertise), `skill.investigation` 7, `skill.athletics` -1, `passive.perception` 11, `passive.arcana` 20, `spell.dc` 15, `attack.spell` 7, `ac` 12, `initiative` 2; no warnings |
| `an_override_wins_and_the_computed_value_stays_visible` | Mira's `OverrideValue::Int(15)` AC: `computed` 12, `override_value` 15, `effective` 15, breakdown retained; an unoverridden stat keeps `override_value: None` |
| `spell_slot_maxima_and_hit_dice_follow_the_class_tables` | Aria slots `{1:4, 2:3, 3:2}`, `hit_dice {6:5}`, tracked slot-3 max 2; Borin no slots, `hit_dice {10:3}`, Second Wind max 1; Lyra `pact {level:1, count:2}`, `hit_dice {8:2}`; Mira wizard 3/cleric 2 slots `{1:4, 2:3, 3:2}` and `hit_dice {6:3, 8:2}`; Goblin prof 2 from CR 1/4, no slots/hit dice |
| `equipped_body_armor_and_shields_compose_ac_with_warnings` | best body armor wins (chain 16 + capped dex 0), both armors tolerate the clash with one `OverArmored`; best shield adds (2); unmet `strength_requirement` warns scoped to the item; a second shield adds its own `OverArmored` |
| `conditions_gate_item_feature_and_status_contributions` | `Equipped` needs `ItemState::Equipped`; `Attuned` needs the flag; feature modifiers apply; `StatusActive` matches case-insensitively and stops applying at `now >= expires_at` (soft expiry: the status stays on the sheet); `Custom` warns `UnmodeledCondition` and contributes nothing |
| `zero_quantity_and_over_attunement_warn_and_do_not_contribute` | qty 0 excludes the entry and warns `ZeroQuantity` scoped to it; four attuned entries warn `OverAttunement` once with no element |
| `dice_modifiers_warn_in_static_contexts_and_compose_for_rolls` | a `Save`-group `1d4` contributes flat 0 to every save with one `DiceInStaticContext` warning per contribution (not per expanded save), scoped to the status; `roll_parts` returns flat 4 plus one attributed `1d4` term |
| `advantage_contributions_cancel_pairwise_into_roll_hints` | `Check`-group advantage reaches every skill; two advantage and one disadvantage cancel to a net `Advantage` hint; `computed` ignores hints; an unhinted stat rolls `Normal` |
| `unresolved_item_references_warn_and_contribute_nothing` | a referenced item missing from the context warns `UnresolvedReference` scoped to the entry and contributes nothing; materializing the entry (attuned ring) applies it and clears the warning |
| `unmodeled_classes_warn_and_unevaluable_maxima_stay_none` | a Bard level warns `UnmodeledClass`, derives no slots and no hit dice; `ClassLevel("Wizard")` evaluates (Arcane Recovery max 5); a dice maximum stays `max: None` with `FormulaUnevaluated` scoped to the resource |

Notes:

- **It does not compile yet, on purpose.** `localtt_engine::rules`,
  `WarningCode::{DiceInStaticContext, ...}` and `ElementRef::{Item, Feature}`
  do not exist. The skeleton phase adds the declarations; the spec-phase
  contract is exactly that.
- **Fixtures are consumed unchanged.** The test never mutates a shared golden
  entity: helpers clone first, and test-local items/statuses are pushed onto
  the clone. The golden numbers come from `crates/localtt-integration/DESIGN.md`
  section 3; where the catalog overstates a fixture (Aria has no Bless status,
  no overrides, no armor), the test adds the missing pieces locally and says
  so.
- **The content map is caller-supplied.** Only one test needs a resolution
  (a referenced ring); it builds a one-entry map. No adapter is imported.
- **Deterministic inputs only:** fixed times (campaign second 0 or 100), no
  wall clock, no UUID comparisons; `StatusId`/`InventoryEntryId` are read back
  from the values the test created.
- **Warning order is not contractual.** The test checks membership and counts,
  never the order of `warnings`.
- **`roll_parts` is exercised from the rules side only.** The `Roll` command,
  `CommandEnv::roll` and the roller are U5; this test fixes the seam
  (`RollParts`, `DiceTerm`, `RollHint`) without touching the executor.

## 4. Public API sketch

Sketches, not compilable code; signatures are the contract. Core types
(`StatKey`, `KnownStat`, `Ability`, `Skill`, `StatblockEntity`,
`InventoryEntryId`, `FeatureEntryId`, `SpellEntryId`, `ResourceId`,
`CampaignTime`, `OverrideValue`, `DiceExpr`, `Modifier`, `Formula`,
`Condition`, `ContentEntry`, `RefId`, ...) are imported from
`localtt_core::types`/`localtt_engine::content` and elided here. Every public
item lands documented (both crates deny `missing_docs`). Output types derive
`Clone`, `Debug`, `PartialEq` and, where the fields allow it, `Eq`
(`ContributionSource`, `ContributionOp` and `RollHint` are also `Copy`) so
clients can compare snapshots as the target test does.

### 4.1 `localtt_engine::rules`

```rust
/// Inputs a derivation needs beyond the entity: the campaign instant and the
/// materialized content entries the entity's references resolve to.
pub struct RulesContext<'a> {
    /// Campaign instant used for status expiry and time-gated rules.
    pub now: CampaignTime,
    /// Materialized entries keyed by reference id; the caller resolves them
    /// (engine architecture 7.4). Missing entries degrade with warnings.
    pub content: &'a BTreeMap<RefId, ContentEntry>,
}

/// Derives the full sheet: values, resources, class tables, warnings.
#[must_use]
pub fn derive(entity: &StatblockEntity, ctx: &RulesContext<'_>) -> DerivedSheet;

/// The static sheet of one entity at one instant.
pub struct DerivedSheet {
    /// Derived stat values by canonical key.
    pub values: BTreeMap<StatKey, DerivedValue>,
    /// Derived maxima for the entity's tracked resources, keyed by element id.
    pub resources: BTreeMap<ResourceId, DerivedResource>,
    /// Class-table spell-slot capacity (not the tracked resources).
    pub slots: SlotMaxima,
    /// Hit-dice pools by die sides: how many dice of that size the entity has.
    pub hit_dice: BTreeMap<u8, i32>,
    /// Non-blocking rule outcomes, entity-scoped.
    pub warnings: Vec<Warning>,
}

/// One derived stat.
pub struct DerivedValue {
    /// The computed value, when the rules know how to compute one.
    pub computed: Option<i32>,
    /// The sheet's sparse override, when present.
    pub override_value: Option<OverrideValue>,
    /// `override_value` when present, else the computed integer; `None` when
    /// neither exists. The only value a client shows as "the" value.
    pub effective: Option<OverrideValue>,
    /// The contributions that produced `computed`, in application order.
    pub breakdown: Vec<Contribution>,
    /// Net advantage/disadvantage from contributions; `None` when no hint
    /// contribution applied, `Some(RollHint::Normal)` when they cancelled.
    pub hint: Option<RollHint>,
}

/// One contribution to a derived value.
pub struct Contribution {
    /// Where the contribution came from.
    pub source: ContributionSource,
    /// Display attribution: `modifier.note`, else the source's name.
    pub label: String,
    /// The operation the source applied.
    pub op: ContributionOp,
    /// The operand; `None` for advantage/disadvantage.
    pub value: Option<i32>,
}

/// The origin of one contribution.
pub enum ContributionSource {
    /// The rules' own base value.
    Base,
    /// One inventory entry.
    Item(InventoryEntryId),
    /// One feature entry.
    Feature(FeatureEntryId),
    /// One status.
    Status(StatusId),
}

/// What a contribution does.
pub enum ContributionOp {
    /// Replace the running value.
    Set,
    /// Add to the running value.
    Add,
    /// Clamp to at least the operand.
    Min,
    /// Clamp to at most the operand.
    Max,
    /// Grant advantage (a roll hint, not a number).
    Advantage,
    /// Grant disadvantage (a roll hint, not a number).
    Disadvantage,
}

/// A tracked resource's derived maximum.
pub struct DerivedResource {
    /// The evaluated maximum; `None` when the formula is unevaluable here.
    pub max: Option<i32>,
    /// The evaluated formula as a single `Set` contribution, or empty when
    /// unevaluable.
    pub breakdown: Vec<Contribution>,
}

/// Spell-slot capacity from the class tables.
pub struct SlotMaxima {
    /// Regular slots by spell level, 1 to 9.
    pub spell: BTreeMap<u8, i32>,
    /// Pact Magic, when the entity has warlock levels.
    pub pact: Option<PactSlots>,
}

/// Pact Magic capacity.
pub struct PactSlots {
    /// Slot level.
    pub level: u8,
    /// Number of slots.
    pub count: i32,
}

/// Net advantage or disadvantage on a roll.
pub enum RollHint {
    /// Neither, or a pairwise cancellation.
    Normal,
    /// Net advantage.
    Advantage,
    /// Net disadvantage.
    Disadvantage,
}

/// The flat part, attributed dice and hint for one roll of a stat (U5 seam).
pub struct RollParts {
    /// The integer the roll starts from.
    pub flat: i32,
    /// Dice terms from applicable modifiers, in application order.
    pub dice: Vec<DiceTerm>,
    /// The net hint.
    pub hint: RollHint,
}

/// One attributed dice term.
pub struct DiceTerm {
    /// Display attribution from the contributing modifier.
    pub source: Option<String>,
    /// The dice expression.
    pub expr: DiceExpr,
}

/// Builds the flat part, attributed dice and hint for one stat roll (U5).
///
/// Errors with `Malformed` when the stat has no integer effective value (no
/// derived value, or an override that is not an integer) or when a dice leaf
/// sits under an operation the roller cannot represent (section 5.8).
pub fn roll_parts(
    entity: &StatblockEntity,
    stat: &StatKey,
    ctx: &RulesContext<'_>,
) -> Result<RollParts, EngineError>;
```

### 4.2 `localtt_engine::error` additions

```rust
// Existing variants unchanged (ResourceOverspent, NegativeHp,
// MultipleConcentration, DawnRewind, FormulaUnevaluated).

#[non_exhaustive]
pub enum WarningCode {
    // ... existing ...
    /// A dice leaf was flattened to its flat part (0) in a static context.
    DiceInStaticContext,
    /// A content reference did not resolve to a usable entry.
    UnresolvedReference,
    /// More than one applicable body armor, or more than one shield.
    OverArmored,
    /// More than three attuned inventory entries.
    OverAttunement,
    /// An equipped armor's strength requirement is unmet.
    StrengthRequirement,
    /// An inventory entry with non-positive quantity was skipped.
    ZeroQuantity,
    /// A `Condition::Custom` was not modeled and contributed nothing.
    UnmodeledCondition,
    /// A class level named a class the static tables do not model.
    UnmodeledClass,
}

#[non_exhaustive]
pub enum ElementRef {
    // ... existing Status(StatusId), Resource(ResourceId) ...
    /// One inventory entry.
    Item(InventoryEntryId),
    /// One feature entry.
    Feature(FeatureEntryId),
    /// One spell entry (U5's unresolved cast warnings).
    Spell(SpellEntryId),
}
```

`FormulaUnevaluated` keeps its U3 meaning and is reused for unevaluable
resource maxima and unevaluable modifier operands; its doc comment gains the
U4 producers. The `Spell` variant has no U4 producer; it is declared here so
the batch pins the `ElementRef` shape once (U5 uses it).

## 5. Guarantees

### 5.1 Purity and determinism

- `derive` and `roll_parts` are pure: no I/O, no mutation of the entity, no
  wall clock (the context's `now` is the only time source), no interior
  mutability. The same entity, context and instant produce equal output;
  warning order is stable across calls but not contractual.
- Nothing is silently dropped: every skipped contribution, unevaluable
  formula and unresolved reference produces a warning (RFC-0001 2.2).

### 5.2 Static formula evaluation (OQ-2, resolved)

- Full `Formula` language: `Const`, `ProficiencyBonus`, `AbilityMod(ability)`,
  `Level` (sum of class levels), `ClassLevel(class)` (sum of matching class
  levels, matched case-insensitively on the name snapshot: `RefOrText::Text`
  text or the `ContentRef` display name), `Add`, `Sub`, `Mul`, `Min`, `Max`,
  `FloorDiv`.
- Arithmetic is saturating `i32`; `FloorDiv` by zero makes the formula
  unevaluable (resource max `None` + `FormulaUnevaluated`; a modifier
  contributes nothing + `FormulaUnevaluated` scoped to its source).
- `AbilityMod(a)` reads the **final computed** ability score (after its
  contributions), not the raw document value.
- A `Dice` leaf in a static context evaluates to 0; a resource maximum that
  contains one is unevaluable (`None` + `FormulaUnevaluated`); a modifier
  value that contains one contributes 0 with `DiceInStaticContext`.

### 5.3 Contribution application

- Sources: the rules base, then each feature's `modifiers` in `features`
  order, then each inventory entry's item modifiers in `inventory` order,
  then each status's `modifiers` in `statuses` order. Within one source, the
  document order of `modifiers` is kept.
- Within a value, `Set` ops apply first (last one wins), then `Add`s, then
  `Min`/`Max` clamps, matching engine architecture 7.4. `Min` raises the
  running value to the operand; `Max` lowers it. The breakdown records every
  contribution that applied, replaced `Set`s included, in application order;
  it is the audit trail, not just the winners.
- `Advantage`/`Disadvantage` never change `computed`; they cancel pairwise
  and the remainder becomes `hint` (`None` when no hint contribution applied,
  `Some(Normal)` when equal counts cancelled).
- Overrides win: `effective = override_value` when present, else the computed
  integer. `computed` is never replaced by the override, so drift stays
  visible (RFC-0001 8.2). Downstream bases use `computed`, not overrides.
- Evaluation stages, in order: proficiency bonus; ability scores; ability
  modifiers; saves; skills; passive scores; AC; initiative; speed; spell DC
  and spell attack; custom keys; resources; slots; hit dice.

### 5.4 Base values and always-present keys

- Every entity derives `proficiency_bonus`, the six `ability.*`, the six
  `ability_mod.*`, the six `save.*`, all `skill.*`, all `passive.*`, `ac`,
  `initiative` and `speed`. `spell.dc` and `attack.spell` exist only when the
  entity has a spellcasting block (the first block's ability drives both).
  `attack.melee`/`attack.ranged` are deferred.
- Structural bases: ability score = the stored score; ability modifier =
  `floor((final score - 10) / 2)`; save = ability modifier plus the
  proficiency bonus when the ability is in `proficiencies.saves`; skill =
  ability modifier plus the proficiency bonus, doubled with `Expertise`;
  passive = 10 plus the skill; initiative = dexterity modifier; speed =
  `speed.walk_ft`; spell DC = 8 + prof + ability modifier; spell attack =
  prof + ability modifier (each contribution can still add on top).
- `proficiency_bonus`: characters by total level (clamped to 1..=20); the 2014
  table gives 2/3/4/5/6 across the level bands; creatures by challenge rating
  (CR 0-4: 2, 5-8: 3, 9-12: 4, 13-16: 5, 17-20: 6, 21-24: 7, 25-28: 8,
  29-30: 9); a sheet with neither levels nor CR gets 2.
- Group keys (`KnownStat::Group`) expand into their members and are not
  themselves `values` entries: `Save` into the six saves, `Check` into every
  skill. A group modifier is copied to each member; a warning it produces is
  emitted once per contribution, not once per member.
- Custom stat keys appear only when a contribution targets them.

### 5.5 Conditions

- `when: None` applies unconditionally (the source's own state does not gate
  it).
- `Equipped` applies only to inventory entries with `state ==
  ItemState::Equipped`; `Attuned` only when the entry's `attuned` flag is
  true. Both are no-ops for features and statuses (no warning).
- `StatusActive { name }` applies while at least one status matches the name
  case-insensitively and is unexpired: a `StatusDuration::Timed` status is
  expired at `now >= expires_at` (soft expiry: the status stays on the sheet;
  derive just stops counting it). `Indefinite`, `UntilRest` and `Permanent`
  never expire.
- `Custom` is unmodeled: the contribution is skipped and
  `UnmodeledCondition` warns, scoped to the source element.

### 5.6 Items, AC and armor

- Inline items contribute their own `modifiers`. Referenced items resolve
  `ContentData::Item` from the context; a missing entry or one of another
  kind warns `UnresolvedReference` scoped to the entry and contributes
  nothing.
- Body-armor candidates are equipped entries with
  `ItemProperties::Armor(ArmorProperties)` and `qty > 0`. A candidate's value
  is `base_ac + min(dex modifier, max_dex_bonus)` (uncapped when the cap is
  `None`). The highest value wins; ties keep the earliest entry. The winner
  contributes one `Set`, labeled with the entry's item name.
- With no applicable body armor, AC starts from `Base` `Set(10)` plus a
  `Base` `Add(dex modifier)`.
- Shield candidates are equipped entries with `ItemProperties::Shield` and
  `qty > 0`; the highest `ac_bonus` wins (ties: earliest) and contributes one
  `Add`. Other applicable body armors and shields contribute nothing.
- More than one applicable body armor warns `OverArmored` once; more than one
  applicable shield warns `OverArmored` once. Both are element-less.
- An equipped body armor whose `strength_requirement` exceeds the final
  computed Strength score warns `StrengthRequirement` scoped to the entry and
  still applies.

### 5.7 Attunement, quantity and warnings

- Every inventory entry with `attuned == true` counts, whether or not its item
  requires attunement; more than three warns `OverAttunement` once per
  derivation, element-less. Over-limit entries still contribute (warn never
  block).
- An entry with `qty <= 0` that carries modifiers or armor/shield properties
  is excluded from contributions and warns `ZeroQuantity` once, scoped to the
  entry. Entries with `qty <= 0` that could not contribute anyway are silent
  (empty consumables must not spam).
- Warning scoping: warnings triggered by a source's contribution carry that
  source's element (`Item`/`Feature`/`Status`); `OverArmored` and
  `OverAttunement` carry none; every derive warning carries
  `entity: Some(entity.id)`. Messages are human text, not contractual
  (RFC-0001 2.2).
- Label: `modifier.note` when present, else the source's display name (item
  name, feature name, status name).

### 5.8 Roll parts (the U5 seam)

- `roll_parts` derives the sheet internally, reads the stat's `effective`
  integer as `flat`, and collects dice terms from the same collection
  pipeline filter: modifiers whose target is the stat itself or a group
  containing it, that pass their condition, from non-skipped sources.
- Dice leaves are extracted from the modifier's formula in evaluation order
  (left to right). A dice leaf is representable when it sits in an
  `Add`/`Sub` chain (any depth) over otherwise integer-evaluable operands;
  a dice leaf under `Mul`, `FloorDiv`, `Min` or `Max` makes `roll_parts`
  return `Malformed` (the roller cannot express a scaled die as terms).
- `hint` is the derived `hint`; `None` maps to `RollHint::Normal`.
- `Malformed` also when the stat has no derived value or its `effective` is
  not `OverrideValue::Int` — a text or boolean override has nothing to roll.
- The static value and the roll parts agree: every dice leaf contributes 0 in
  `DerivedValue::computed`, and `flat` equals that computed/effective value.

### 5.9 Resources, slots and hit dice

- Every `entity.body.resources` entry gets a `DerivedResource`; its `max` is
  the evaluated formula (`Formula::Const` or a full expression), `None` plus
  `FormulaUnevaluated` when unevaluable. `current` is not clamped here; U3's
  spend/restore semantics stand.
- `slots.spell` comes from the 2014 multiclass table at the combined
  full-caster level (Wizard/Cleric levels sum; Warlock levels do not).
  `slots.pact` comes from the Pact Magic table at the warlock level. Levels
  above 20 clamp to the table end.
- `hit_dice` sums class levels by the class's die: Wizard 1d6, Cleric 1d8,
  Fighter 1d10, Warlock 1d8.
- A class level whose class name matches no table entry warns
  `UnmodeledClass` (element-less) and contributes no slots and no hit dice;
  its levels still count toward total level and proficiency bonus. Half/third
  casters, subclasses and a content-driven ruleset bundle are deferred.

### 5.10 Long-rest recovery (OQ-5 split)

- A `LongRest` command additionally recovers: `hp.current = hp.max` (temp HP
  untouched), `death_saves` back to 0/0, and `exhaustion =
  exhaustion.saturating_sub(1)`. No warnings: these are deterministic rules.
- The arithmetic lives in `commands::rests` (engine-internal; no signature
  change) and is covered by engine unit tests in the implement phase; no
  frozen test exercises long rests, so it is safe to add without touching
  U3's contract. Hit-dice spend and long-rest regain are U5.
- This settles engine-executor OQ-5's first half; the RFC-0001 5.3 promise
  ("reset resources by recharge type, clear rest-typed statuses") stays as
  built in U3.

## 6. Internal structure

```
rules/
  mod.rs        public API: RulesContext, derive, roll_parts, output types
  formula.rs    static Formula evaluation (tables-aware)
  collect.rs    modifier collection: conditions, groups, warnings, labels
  armor.rs      AC bases: body armor, shields, strength, over-armor
  tables.rs     proficiency, multiclass slots, pact magic, class hit dice
```

- `derive` orchestrates: evaluate `proficiency_bonus`; collect applicable
  contributions once (a `Vec<Pending>` carrying source, label, op, evaluated
  operand and the original formula for dice extraction); run the staged
  pipeline per key; apply overrides; evaluate resources; consult `tables.rs`;
  collect warnings.
- `roll_parts` reuses `collect.rs` and `formula.rs` and returns early on the
  `Malformed` cases; it does not roll dice and does not touch `dice`.
- No new dependencies: `core` types plus the existing `error` module. No
  `adapter`/`content` imports; the map arrives by borrow.
- The engine unit tests at implement cover what the target test cannot:
  full formula expression matrix, group/condition tables, AC tie-breaks,
  warning scoping per source kind, and the long-rest recovery arithmetic.

## 7. API cross-check against the target test

| Target-test need | API element |
|---|---|
| construct a derivation input | `RulesContext { now, content }` |
| derive | `rules::derive(&StatblockEntity, &RulesContext) -> DerivedSheet` |
| read values, overrides, breakdowns, hints | `DerivedValue::{computed, override_value, effective, breakdown, hint}`, `Contribution::{source, op, value}` |
| identify contribution sources | `ContributionSource::{Base, Item, Feature, Status}` |
| read resource maxima | `DerivedResource.max` |
| read slot tables | `SlotMaxima`, `PactSlots` |
| read hit-dice pools | `DerivedSheet.hit_dice` |
| match warning codes and elements | `WarningCode::{DiceInStaticContext, UnresolvedReference, OverArmored, OverAttunement, StrengthRequirement, ZeroQuantity, UnmodeledCondition, UnmodeledClass, FormulaUnevaluated}`, `ElementRef::{Item, Feature, Status, Resource}` |
| compose roll parts | `RollParts`, `DiceTerm`, `RollHint`, `rules::roll_parts` |

Every element in the sketch is either used above or justified: `ElementRef::
Spell` is U5's pinned taxonomy (declared once for the batch); `ContributionOp::
Min/Max` are the core modifier ops and are covered by engine unit tests;
`DerivedResource.breakdown` mirrors `DerivedValue.breakdown` for the future
`derived()` read path.

## 8. Decisions and deviations (to fold after review)

| # | Deviation | Why |
|---|---|---|
| D1 | `DerivedValue.effective: Option<OverrideValue>` (arch 7.4: `OverrideValue`) | A text-only or unevaluable stat has no value; `None` keeps "no value" representable instead of a sentinel. |
| D2 | `DerivedSheet` adds `resources`, `slots`, `hit_dice` (arch 7.4: `values`, `warnings` only) | RFC-0001 5.1 MUSTs spell slot maxima (multiclass), hit-dice totals; `restoring_with_a_formula_maximum...` shows resource maxima are needed by U3's read paths. |
| D3 | `DerivedValue.hint: Option<RollHint>` (arch 7.4 has no hint field) | Advantage/disadvantage must survive from `derive` to `roll_parts`; re-deriving rows would duplicate pipeline semantics. |
| D4 | `StaleOverride` warning dropped (arch 7.4 lists it) | It needs a stored previous computed value; nothing in the model or tests tracks one. Deferred until a client needs drift telemetry (drift is already visible: both values are returned). |
| D5 | Class tables cover Wizard/Cleric/Fighter/Warlock only (arch 7.4 defers "multiclass slot tables, pact magic specifics" to "the rules unit(s)") | The golden catalog is the only oracle; a ruleset-as-data bundle (RFC-0001 5.1) replaces static tables later. Unknown classes warn, never mis-derive. See OQ-2. |
| D6 | Group keys are expanded, never stored as values | A group has no single value; storing one would invent a number. `roll_parts` on a group key is `Malformed` for the same reason. |
| D7 | Long-rest recovery lands in `commands::rests` (OQ-5 split) | No frozen test covers long rests; hit-dice regain is U5. Section 5.10. |
| D8 | `RulesContext` takes `&BTreeMap<RefId, ContentEntry>` (arch 7.4: same) | Matches the sketch; U5's `ReferenceSource` port is for command-time lookup, not derivation. See OQ-3. |
| D9 | `roll_parts` returns `Result<RollParts, EngineError>` rather than infallible | Text overrides and unrepresentable dice placements must fail loudly; U5 maps the error to a `Malformed` command failure. |

## 9. Open questions

Every question states options, tradeoffs and a recommendation; the sketch
assumes the recommendation.

### OQ-1: activating formula evaluation in U3 command paths (**blocking**)

OQ-2 is now settled, so U3's rest/dawn refill and `RestoreResource` could
evaluate `Resource::max` instead of only `Formula::Const`. But the frozen
target test `engine_target.rs::restoring_with_a_formula_maximum_is_unclamped_and_warns`
pins the current "unclamped + `FormulaUnevaluated`" behavior.

- **A (recommended for this batch):** leave U3 paths exactly as built. U4
  adds evaluation only in `rules` (and the read-side `DerivedResource`). The
  duplication is small (two evaluation entry points) and disappears when a
  follow-up unit rewrites the clamp behavior.
- **B:** switch U3 to `rules::formula` evaluation now and edit the frozen
  test. Better end state, but it edits a frozen artifact mid-batch and
  changes command semantics beyond U4's scope without a dedicated review.
- **C:** expose a clamps-aware evaluation mode in U3 now: premature, the
  refill behavior spec (RFC-0001 5.3) is not being changed in this batch.

Recommendation: **A**; the plan allocates a follow-up decision item. This
question needs a reviewer call before the U4 skeleton because it decides
whether U3 files are touched.

### OQ-2: class-table scope (non-blocking)

- **A (recommended):** the four classes the golden catalog proves; everything
  else warns `UnmodeledClass`. Honest, test-backed, no unverified rules data.
- **B:** all twelve 2014 classes as static data: more useful immediately, but
  unverified against any fixture and to be replaced by the ruleset bundle.
- **C:** load class data from the content source: correct end state, but the
  U2 payloads have no progression fields yet (a wire/version proposal).

Recommendation: **A** now, **C** as its own unit. The target test pins A.

### OQ-3: `RulesContext` map vs `ReferenceSource` (non-blocking)

U5's command environment exposes `&dyn ReferenceSource`. U4 takes a
materialized `BTreeMap` (arch 7.4's sketch).

- **A (recommended):** keep the map in `rules` (pure, no trait-object
  lifetime in the public API); the executor materializes the map once per
  derivation from its loaded content. U5's casting uses `ReferenceSource`
  where resolution is a command-time concern.
- **B:** `RulesContext` holds `&dyn ReferenceSource`: one resolution story,
  but the engine's rules API grows a port dependency and a lifetime
  parameter.
- **C:** a generic `C: ContentLookup` parameter: type-parameter creep for one
  method.

Recommendation: **A**; the plan cross-checks with U5 before its skeleton.

### OQ-4: over-attunement contributions (non-blocking)

- **A (recommended):** over-limit attuned entries still contribute; the
  warning is social/UI noise ("you are over-attuned"), warn-never-block.
- **B:** entries beyond the third contribute nothing (strict 5e). Needs a
  deterministic "which three" rule (document order) and turns a warning into
  silent mechanics loss.

Recommendation: **A**; the target test only pins the warning.

### OQ-5: multiple spellcasting blocks (non-blocking)

Mira has two blocks (Wizard/Int, Cleric/Wis); `spell.dc`/`attack.spell` can
only hold one ability today.

- **A (recommended):** the first block in document order drives both keys;
  per-block keys arrive with the ruleset-data unit.
- **B:** warn `UnmodeledCondition`-style when blocks disagree: noise for a
  normal multiclass sheet.
- **C:** add per-source keys now: a wire change out of scope.

Recommendation: **A**.

## 10. Deferred items (not blocking the skeleton)

- Attack lines (`attack.melee`/`attack.ranged`): weapon proficiency and
  property data.
- Half/third casters, subclasses, unarmored defense (barbarian/monk).
- Full class data as a ruleset/content bundle; content-driven progression.
- Hit-dice spend/regain (U5); long-rest hit-dice recovery specifically.
- Derived values in `EntityView`/the `derived(id, viewer)` service (U6/U7).
- Caching or incremental derivation; the current contract is a full
  recompute.
- Cross-entity derivation (RFC-0001 4.3).
- `StaleOverride` telemetry (D4) and fly/swim/climb/burrow speed keys.
- A `ClassLevel` naming warning elements: core has no `ElementRef` variant
  for it; today the warning is element-less.
