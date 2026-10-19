# RFC: U5 -- dice, rolls, casting and hit dice

| | |
|---|---|
| **Status** | Spec phase (2026-10-18, branch `u4-u6-spec`). This RFC and `crates/localtt-integration/tests/dice_target.rs` are the first U5 artifacts. No library code exists: the target test intentionally does not compile until the U5 skeleton lands, then fails with `not yet implemented` until the implement phase. U5 consumes the U4 seam (`rules::RollParts`) and settles engine-executor OQ-5's second half (hit-dice spend/regain); see section 5.9. |
| **Scope** | New `localtt-engine::dice` and `localtt-engine::ports::rng`; command variants `Roll`, `CastSpell`, `BeginConcentration`, `EndConcentration`, `SpendHitDie`; `CommandEnv::{reference, roll}`; `Executor::execute_as`; `EngineError::{Environment, Content}`; `WarningCode::ConcentrationUnsupported`; the concentration-save hint in `commands::hp`; hit-dice regain in `commands::rests`; `localtt_adapter::rng::ChaChaRng`. Target test: `crates/localtt-integration/tests/dice_target.rs`. U6 authorizes the `execute_as` seam and consumes the audiences pinned here (`docs/rfcs/authz.md`); the batch plan is `docs/plans/u4-u6.md`. The core data model is not touched; every missing wire field is an Open Question, not an edit. |
| **Read with** | `docs/rfcs/engine-executor.md` sections 3, 5, 9 (OQ-5); `docs/rfcs/rules.md` (U4's `RollParts`/`DiceTerm`/`RollHint`, section 5.8); `docs/engine-architecture.md` sections 4.4, 7.2, 7.5, 7.9, 9, 10 (U5); `docs/decisions.md` ENG-13/14/18/21/23; `RFC-0001-localtt.md` sections 2.9, 5.6, 8; `crates/localtt-core/DESIGN.md` sections 3.9, 3.10; this batch's `authz.md` and `docs/plans/u4-u6.md`. |
| **Pins** | `docs/engine-architecture.md` section 7.2 sketches the command/environment shape and section 7.5 the dice shape; every difference is a decision (section 8) and every unresolved fork an Open Question (section 9). OQ-1 (the environment-port placement) is resolved to recommendation A (section 9). |

## 1. Goals

1. Pin the dice notation language: the grammar, the `DiceExpr` AST mapping,
   and the four caps (count, sides, depth, terms) with exact `TooLarge`
   payloads, for both parsed notation and ASTs arriving from the wire.
2. Pin the roller: `dice::roll(&RollPlan, &mut dyn Rng) -> DiceResult`, its
   per-term and per-die semantics, keep rules with roll-order tie-breaks, and
   advantage/disadvantage preservation of nat-1/nat-20 detection.
3. Pin composition: `dice::compose(base, &RollParts) -> RollPlan`, the U4/U5
   seam that turns derived roll parts into attributed terms.
4. Pin the `Rng` port and its contract, the shipped `ChaChaRng`
   (OS-seeded and seed-reproducible), and the test double contract.
5. Pin the `Roll` command: expression and check rolls, the actor requirement,
   context and audience, and the feed entry.
6. Pin casting: slot spend, resolved and unresolved spells, cantrip refusal,
   upcast arithmetic, pact slots, and the concentration replacement rule.
7. Pin `BeginConcentration`/`EndConcentration` and the concentration-save hint
   on damage.
8. Pin `SpendHitDie`: the CON modifier, the heal clamp, overspend warning and
   the resulting roll feed entry; plus the long-rest hit-dice regain
   arithmetic (engine-executor OQ-5's second half).
9. Pin the environment seam: `CommandEnv::{reference, roll}` with defaults,
   `Executor::execute_as(actor, &Command)`, and the two new error variants.

## 2. Non-goals

- **No authorization.** `execute_as` records the actor for attribution only;
  `Unauthorized` has no U5 producer and U6 adds the pre-check at this seam.
  `can_read`/`can_write` and feed visibility are U6.
- **No attack or damage automation.** `CastSpell` never rolls; spell attacks
  and damage stay client-built `RollSpec::Expression`s until weapon
  proficiency and property data exist (U4 deferral).
- **No content loading.** `ReferenceSource` is U2's port; commands consume
  `env.reference().get` and never touch a snapshot directory.
- **No recharge-by-roll.** Monster recharge resources stay `Recharge::Manual`
  (U2 mapping).
- **No long-rest regain of spell slots beyond U3's rest semantics.** U4 owns
  LongRest's HP/death-save/exhaustion recovery; U5 adds only hit dice.
- **No statistical randomness tests in this target suite.** The contract test
  pins range, the degenerate die and seed replay; a bounded distribution check
  is engine unit-test work (section 9, OQ-8).
- **No server DTO mapping.** `RollSpec` and the command variants are the
  engine surface U7 maps onto; REST shapes are out of scope.
- **No core edits.** `Roll`, `DiceResult`, `TermResult`, `RollContext`,
  `RollKind`, `FeedPayload` already carry everything the target test uses; any
  later gap is a versioned proposal, recorded as an Open Question.

## 3. Client usage: the target test

`crates/localtt-integration/tests/dice_target.rs` is ordinary client code: it
imports `localtt_engine::dice`, `localtt_engine::ports::rng`,
`localtt_adapter::rng::ChaChaRng`, the golden `fixtures` cast and a `MemoryStore`,
adds test-local statuses/spells/entries, and drives a real `Executor` through
`execute_as`. Twenty tests:

| Test | What it pins |
|---|---|
| `the_parser_builds_the_core_ast` | precedence and associativity (`2d6+3`, `4d6kh3+1d4-1`, `1d20*2+3`), `adv(...)`/`dis(...)` wrapping, whitespace tolerance between tokens, and the refusals: uppercase `D`, bare `d20`, `adv` without parens, empty input, trailing `+` |
| `caps_gate_parsing_and_asts_from_the_wire` | `101d6`/`1d1001` are `TooLarge` with exact `what`/`limit`; `0d6`/`1d1`/`1d20kh0`/`1d20kh2` are `Malformed`; `validate` re-checks depth 16, terms 32, count and sides on ASTs that never went through `parse` |
| `keep_rules_drop_the_rest_and_prefer_earliest_ties` | `4d6kh3` drops the lowest and keeps the rest with `kept` flags; `3d6kl1` keeps the lowest; equal values keep the earliest die in roll order for both rules |
| `advantage_and_disadvantage_keep_one_of_two_evaluations` | the inner expression is evaluated twice; the higher/lower total wins; loser dice stay visible with `kept: false`; a bonus applies inside each evaluation; kept nat-1/nat-20 values stay readable |
| `compose_builds_the_plan_from_roll_parts` | the d20 base, a non-zero flat as its own `Const` term, attributed dice as their own terms, the left-fold `expression`, a zero flat adding no term, and a net hint wrapping the base term |
| `a_check_roll_derives_its_flat_part_and_attributed_terms` | `RollSpec::Check` derives +5 WIS save flat and a Bless `1d4` term, rolls `4+14+3`, writes one feed entry (id 1, fixed `ts`, campaign `clock_s`, actor, entity, audience `RollerAndDm`), leaves the entity at `rev` 0, and fills `Roll.{roller, entity, ts, audience, expr, context, result}` including `target_dc` |
| `expression_rolls_take_context_and_audience` | a targeted expression yields `RollKind::Damage` from `damage.fire`, the audience override is honored, an untargeted expression has `context: None`; checks map `skill.*` to `Check` and `attack.*` to `Attack`; a custom target is `Other`; an oversized wire AST is refused with `TooLarge`, not rolled |
| `a_roll_requires_an_actor_and_a_real_entity` | plain `execute` is `Malformed` for a roll (no attribution); a check needs an entity; an addressed missing entity is `NotFound { element: None }` |
| `a_broken_environment_or_content_source_fails_the_roll` | the `CommandEnv` defaults: `roll` fails `Environment`, `reference` is an empty source; a default env fails a check with `Environment`; a failing `ContentFault` surfaces as `EngineError::Content` while the check materializes its content map |
| `the_rng_contract_holds_for_every_implementation` | every `Rng` draws in `1..=sides` for 256 d20s, `roll_die(1) == 1`; equal `ChaChaRng` seeds replay equal sequences; `new()` is OS-seeded but contract-clean |
| `casting_spends_the_slot_and_replaces_concentration` | Hex (resolved, concentration) spends one slot, replaces Hunter's Mark with one new `Indefinite` status (name, reference, `started_at` from the campaign clock, no source, no modifiers), appends exactly one `ConcentrationEnded` entry (audience `RollerAndDm`, actor = owner) and emits `[EntityChanged, FeedAppended]` |
| `cast_validation_rejects_bad_spells_and_slots` | cantrip with a slot, slot below the spell level, wrong `upcast_slots`, and a non-slot resource are all `Malformed`; a missing spell or slot is `NotFound` with the pinned element; a non-concentration cast leaves concentration and the feed untouched |
| `pact_slots_skip_level_validation_but_not_upcast_nonsense` | a pact slot casts a known spell without a slot-level comparison; `upcast_slots != 0` on a pact slot is `Malformed` |
| `an_unresolved_spell_casts_with_a_warning` | a spell entry with no reference still casts with exactly one `UnresolvedReference` warning (entity + `ElementRef::Spell`), spends the slot and starts the concentration the client asked for |
| `requesting_concentration_on_a_non_concentration_spell_warns` | `concentrate: true` on a resolved non-concentration spell warns `ConcentrationUnsupported` (entity + `ElementRef::Spell`) and concentrates anyway |
| `begin_and_end_concentration_replace_and_clear` | `BeginConcentration` refuses a non-concentration `StatusSpec`; a real one replaces the old concentration, appends the new status and announces the ending; `EndConcentration` clears and announces; ending nothing is `NotFound { element: None }` |
| `damage_to_a_concentrating_entity_suggests_the_save` | 24 damage appends `Hint { "Concentration: CON save DC 12" }`, 5 damage floors at 10, 0 damage appends nothing, and a non-concentrating target gets no hint |
| `spending_a_hit_die_rolls_heals_and_deducts` | one d6 + 2 CON heals 7 (clamped by max HP), deducts one die, emits `[EntityChanged, FeedAppended]`, and the roll record carries `1d6+2`, term attributions, `RollKind::Other` context, the default audience and the acting roller |
| `hit_die_overspending_warns_and_bad_requests_fail` | spending at 0 warns `ResourceOverspent` (entity + `ElementRef::Resource`) and goes to -1; a non-hit-die resource is `Malformed`; an unknown resource is `NotFound`; the trusted path is `Malformed` |
| `equal_scripts_replay_equal_rolls` | two identical worlds and scripts produce equal roll records, and `dice::roll` is a pure function of plan + rng |

Notes:

- **It does not compile yet, on purpose.** `localtt_engine::dice`,
  `localtt_engine::ports::rng`, `localtt_adapter::rng`, `RollSpec`,
  `Executor::execute_as` and the new `CommandEnv` methods do not exist. The
  skeleton phase adds the declarations; the spec-phase contract is exactly
  that.
- **Fixtures are consumed unchanged.** Helpers clone the golden cast before
  mutating; test-local statuses/spells/resources are pushed onto clones. Where
  the catalog overstates a fixture (Aria carries no Hex spell, no first-level
  slot, no inventory), the test adds the missing pieces locally.
- **Content maps are literal.** `MapSource` implements `ReferenceSource` over
  a `BTreeMap`; `TestEnv` implements `CommandEnv` over a `ScriptedRng` and
  that map; `DefaultEnv` exercises the trait defaults. No adapter snapshot is
  imported.
- **Deterministic inputs only:** fixed `FIXED_TIME`, campaign second 0, no
  wall clock, no UUID comparisons; element ids the test did not create are
  read back from the store (`FeedEntryId::new(1)` for a fresh feed).
- **Warning order is not contractual.** The tests check counts, codes,
  elements and bodies, never the order of `warnings`.
- **`RollId` is not asserted.** Only feed ids and record fields with client
  meaning are compared.

## 4. Public API sketch

Sketches, not compilable code; signatures are the contract. Core types
(`DiceExpr`, `Keep`, `DiceResult`, `TermResult`, `DieResult`, `Roll`,
`RollContext`, `RollKind`, `Audience`, `SpellEntryId`, `ResourceId`,
`StatusSpec`, `EntityId`, `PlayerId`, `StatKey`, ...) and the U4 types
(`RollParts`, `DiceTerm`, `RollHint`) are imported and elided. Every public
item lands documented (both crates deny `missing_docs`). `RollPlan`,
`TermPlan` and `RollSpec` derive `Clone`, `Debug`, `PartialEq`, `Eq` (the
first two are compared in tests; `Command`'s derives require the third).

### 4.1 `localtt_engine::ports::rng`

```rust
// localtt_engine::ports::rng

/// Uniform die source (engine-architecture 4.4).
///
/// Contract: `roll_die(sides)` returns a value in `1..=sides` with equal
/// probability for every `sides >= 2`; `roll_die(1) == 1`. Callers cap
/// `sides` and counts (section 5.2); `sides == 0` is a caller bug.
pub trait Rng: Send {
    /// Draws one die.
    fn roll_die(&mut self, sides: u32) -> u32;
}
```

### 4.2 `localtt_engine::dice`

```rust
// localtt_engine::dice

/// Parses dice notation and validates the result against the caps.
///
/// Errors with `Malformed` for a syntax error or a below-range literal, and
/// with `TooLarge` for an exceeded cap.
pub fn parse(notation: &str) -> Result<DiceExpr, EngineError>;

/// Validates an arbitrary AST (a wire payload that never went through
/// `parse`) against the caps: count, sides, keep count, depth and terms.
pub fn validate(expr: &DiceExpr) -> Result<(), EngineError>;

/// Composes one roll from a base expression and derived roll parts (U4,
/// section 5.8): wraps the base in the hint, adds the flat part when it is
/// non-zero, then appends one attributed term per `DiceTerm`.
#[must_use]
pub fn compose(base: DiceExpr, parts: &RollParts) -> RollPlan;

/// Rolls a plan. Infallible: `parse`/`validate` guarantee term limits, and
/// all arithmetic saturates.
pub fn roll(plan: &RollPlan, rng: &mut dyn Rng) -> DiceResult;

/// One composed roll: the attributed terms and the expression they fold
/// into. Exactly one `TermResult` per `TermPlan` comes back from `roll`.
pub struct RollPlan {
    /// The expression actually rolled: a left fold of `Add` over `terms`.
    pub expression: DiceExpr,
    /// The terms, in roll order.
    pub terms: Vec<TermPlan>,
}

/// One planned term.
pub struct TermPlan {
    /// The term expression.
    pub expr: DiceExpr,
    /// Attribution label for the composed term, e.g. `"Bless"`; `None` for
    /// the base and the flat part.
    pub source: Option<String>,
}
```

### 4.3 `localtt_engine::commands` additions

```rust
// localtt_engine::commands

#[non_exhaustive]
pub enum Command {
    // ... U3 variants unchanged ...
    /// Rolls dice; the actor comes from `Executor::execute_as`.
    Roll {
        /// The entity the roll is for; may be absent for a free expression.
        entity: Option<EntityId>,
        /// What to roll.
        spec: RollSpec,
        /// Audience override; `None` means the campaign default.
        audience: Option<Audience>,
    },
    /// Casts a known spell from a tracked slot.
    CastSpell {
        /// The caster.
        entity: EntityId,
        /// The known spell entry inside one of the entity's blocks.
        spell: SpellEntryId,
        /// The slot resource to spend.
        slot: ResourceId,
        /// The declared upcast in slot levels above the spell's level.
        upcast_slots: u32,
        /// The client's concentration request; the resolved spell may
        /// concentrate on its own.
        concentrate: bool,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Starts (or replaces) concentration from a client-supplied status.
    BeginConcentration {
        /// The concentrating entity.
        entity: EntityId,
        /// The new status; `kind` must be `Concentration`.
        status: StatusSpec,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Ends every concentration status on the entity.
    EndConcentration {
        /// The entity.
        entity: EntityId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Spends one hit die and rolls its healing; the actor comes from
    /// `Executor::execute_as`.
    SpendHitDie {
        /// The creature spending the die.
        entity: EntityId,
        /// The `ResourceKind::HitDie` resource to spend.
        resource: ResourceId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
}

/// What a `Roll` command rolls.
pub enum RollSpec {
    /// Rolls a client-supplied expression.
    Expression {
        /// The expression; validated before rolling.
        expr: DiceExpr,
        /// The stat the roll is for; drives `RollContext.kind` and the
        /// context's `stat`, and has no mechanical effect on the dice.
        target: Option<StatKey>,
    },
    /// Rolls a check for one stat: the engine derives the flat part, the
    /// attributed dice and the hint, then rolls `1d20` composed with them.
    Check {
        /// The stat, e.g. `"save.wis"`.
        stat: StatKey,
        /// The DC, when the client compares one.
        target_dc: Option<i32>,
    },
}

/// Everything a command may consult that is not the working set.
///
/// `reference` and `roll` are defaulted (section 8, D4; OQ-1): a test
/// environment over the defaults fails closed on dice and sees an empty
/// content source, so the frozen U3 envs compile unchanged.
pub trait CommandEnv: Send {
    /// Wall-clock time for `updated_at` and feed `ts` stamps.
    fn now(&self) -> UnixTime;

    /// Read-only content access.
    ///
    /// The default is an empty source: `get` answers `Ok(None)`, search and
    /// autocomplete answer no hits, `sources` is empty.
    fn reference(&self) -> &dyn ReferenceSource { /* &EMPTY_REFERENCE */ }

    /// Rolls one plan with the environment's die source.
    ///
    /// The default fails with `Environment`: a command cannot conjure an
    /// RNG, and a missing port is an environment fault, not bad input.
    fn roll(&mut self, plan: &RollPlan) -> Result<DiceResult, EngineError> {
        Err(EngineError::Environment {
            reason: "no dice source is configured".to_owned(),
        })
    }
}
```

### 4.4 `Executor::execute_as`

```rust
// localtt_engine::executor

impl<'a> Executor<'a> {
    /// Executes one command attributed to `actor`.
    ///
    /// U5 records the actor for roll attribution and the audit trail; it does
    /// not authorize. U6 adds the authorization pre-check here, before any
    /// command work (RFC-0001 5.7).
    pub fn execute_as(
        &mut self,
        actor: PlayerId,
        command: &Command,
    ) -> Result<Executed, EngineError>;
}
```

### 4.5 `localtt_engine::error` additions

```rust
// localtt_engine::error

#[non_exhaustive]
pub enum EngineError {
    // ... existing variants unchanged (Unauthorized, NotFound, Conflict,
    // Malformed, TooLarge, Store) ...
    /// The command environment could not provide a service it promises.
    #[error("the command environment is unavailable: {reason}")]
    Environment {
        /// Diagnostic detail.
        reason: String,
    },
    /// A content source failed while a command resolved references.
    #[error(transparent)]
    Content(ContentFault),
}

#[non_exhaustive]
pub enum WarningCode {
    // ... existing ...
    /// A cast asked for concentration on a resolved spell that does not
    /// concentrate.
    ConcentrationUnsupported,
}
```

`WarningCode::UnresolvedReference` and `ElementRef::Spell` are declared by U4
(`rules.md` section 4.2); the cast path is their second producer.

### 4.6 `localtt_adapter::rng`

```rust
// localtt_adapter::rng

/// The shipped ChaCha20-based `Rng` (rand_chacha; OS seeding via
/// getrandom's `SysRng`).
pub struct ChaChaRng { /* ... */ }

impl ChaChaRng {
    /// OS-seeded: fresh entropy per process.
    #[must_use]
    pub fn new() -> Self;

    /// Seed-reproducible: equal seeds replay equal sequences, so a campaign
    /// can be replayed from a seed.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self;
}

impl Rng for ChaChaRng {
    fn roll_die(&mut self, sides: u32) -> u32;
}
```

## 5. Guarantees

### 5.1 Grammar and AST

- Tokens: non-negative integer literals, the pool token, `adv`, `dis`, `+`,
  `-`, `*`, `(`, `)`. ASCII whitespace between tokens is ignored; `adv`/`dis`
  may be separated from their `(`. A pool token is a single contiguous
  `N d M` with an optional attached keep suffix (`khN`/`klN`); whitespace
  inside a pool token, or a `d` outside one, is a syntax error. Keywords are
  lowercase only; `D`, `KH`/`KL` and a bare `d20` shorthand are refused.
- Grammar (standard precedence, left-associative `+`/`-`/`*`; parentheses
  transparent to the AST):

  ```
  expr   = term (("+" | "-") term)*
  term   = factor ("*" factor)*
  factor = "adv" "(" expr ")" | "dis" "(" expr ")" | pool | number | "(" expr ")"
  pool   = number "d" number keep?
  keep   = ("kh" | "kl") number
  number = DIGIT+
  ```

- AST mapping: `+` -> `DiceExpr::Add`, `-` -> `Sub`, `*` -> `Mul`, a pool ->
  `Roll { count, sides, keep: Option<Keep> }`, a literal -> `Const`, `adv` ->
  `Advantage { inner }`, `dis` -> `Disadvantage { inner }`. No unary minus
  and no constant folding.
- `parse` is tokenize + build + `validate`; it never returns an AST that
  `validate` would reject. A numeric literal that does not fit `i32` is
  `Malformed`.
- `validate` accepts any AST over the same shape, including one a client
  deserialized; it re-checks every `Roll` leaf (`count`, `sides`, keep
  count) and the two structural caps. Passing `validate` is what makes
  `roll` infallible.

### 5.2 Caps

Exact payloads, because clients switch on them:

| Cap | Limit | Exceeded | Illegal below/inside |
|---|---|---|---|
| `Roll.count` | 100 | `TooLarge { what: "dice count", limit: 100 }` | `Malformed` |
| `Roll.sides` | 1000 | `TooLarge { what: "dice sides", limit: 1000 }` | `Malformed` |
| `Keep` count | pool `count` | — | `Malformed` |
| Expression depth | 16 | `TooLarge { what: "expression depth", limit: 16 }` | — |
| Expression terms | 32 | `TooLarge { what: "expression terms", limit: 32 }` | — |

- Count range is `1..=100`, sides range `2..=1000`, keep count `1..=count`.
- Depth is the node count on the longest root-to-leaf path; a bare die is 1,
  each wrapper or operator on the path adds one. `validate` walks with an
  explicit depth limit.
- Terms are all AST nodes: leaves, operators and wrappers, parentheses
  transparent.
- A free expression is one plan term; the expression caps bound the whole
  rollout, so `count * sides * terms` needs no separate budget.

### 5.3 Composition (the U4 seam)

- `compose` never rolls and never inspects die values:
  1. The base term: `Advantage { inner: base }` for `RollHint::Advantage`,
     `Disadvantage { inner: base }` for `Disadvantage`, else `base`; `source`
     is `None`.
  2. `parts.flat != 0` adds `TermPlan { expr: Const { value: flat },
     source: None }`; a zero flat adds no term.
  3. Each `parts.dice` entry adds `TermPlan { expr, source }` in order.
- `plan.expression` is the left fold of `Add` over `plan.terms`; a one-term
  plan's expression equals that term. `compose` produces at least one term
  (the base), so the fold is total.
- The plan is a value: rolling it twice with equal scripts yields equal
  `DiceResult`s, and the hint lives in the plan, not in the roller.

### 5.4 Roller semantics

- `roll` is a pure function of the plan and the caller's die source. The
  result has exactly one `TermResult` per `TermPlan`, in order, with
  `source` copied; `DiceResult.total` is the sum of the term totals.
- Term evaluation:
  - `Const { value }` -> no dice, total `value`.
  - `Roll { count, sides, keep: None }` -> `count` draws in order, all
    `kept: true`, total the sum.
  - `Roll { keep: Some(Highest { count: k }) }` -> draws in order; the `k`
    highest values are kept and the rest stamped `kept: false`; ties keep
    the earliest die in roll order. `Lowest` is symmetric. The term total is
    the sum of the kept dice.
  - `Add`/`Sub`/`Mul` -> the respective operation on the two term totals,
    dice concatenated left-then-right; `Sub` subtracts the right total.
  - `Advantage { inner }` -> evaluate `inner` twice, first draw first; the
    evaluation with the higher total is kept (equal totals keep the first).
    The result concatenates both evaluations' dice; the winner's dice keep
    their own flags, every die of the loser is stamped `kept: false`; the
    term total is the winner's. `Disadvantage` is symmetric with the lower
    total.
- Nat-1/nat-20 detection survives: a kept d20's `value` is its own face, and
  the losing evaluation's dice stay visible as unkept.
- All arithmetic saturates: a per-term total at `i32` bounds saturates, and
  the outer sum saturates (section 9, OQ-2). `roll` cannot panic on
  arithmetic.
- `roll` draws exactly as many dice as the plan's structure requires and
  never draws ahead; a plan over `n` pool leaves draws the sum of their
  counts (twice per advantage/disadvantage evaluation).

### 5.5 The `Rng` contract and `ChaChaRng`

- `Rng` is object-safe and `Send`; commands consume it through
  `CommandEnv::roll`, tests through the same seam.
- Contract: every draw for `sides >= 2` is in `1..=sides`; `roll_die(1)`
  returns 1; callers never pass 0.
- `ChaChaRng::new()` seeds from the OS: fresh entropy per process, no
  campaign state; `ChaChaRng::from_seed(seed)` replays: equal seeds produce
  equal sequences, so a campaign's rolls are reproducible once a seed is
  recorded. OS seeding goes through `getrandom`'s `SysRng` (feature
  `sys_rng`): `rand_core` 0.10 removed OS entropy from `SeedableRng`, so the
  adapter carries `rand_chacha` plus `getrandom`; `new()` panics when the OS
  source is unavailable, as the signature has no error channel and a
  fallback seed would silently break the entropy contract. Draws use
  rejection sampling over `next_u32`, so no face is favoured.
- The contract suite (the target test) pins range, the degenerate die and
  seed replay for both `ScriptedRng` and `ChaChaRng`; distribution sanity is
  a seeded, bounded adapter unit test (OQ-8: the engine cannot host it).
- `ScriptedRng` (test-local) returns scripted values in order and refuses
  impossible ones; it is the determinism tool for every other test.

### 5.6 The `Roll` command

- **Actor.** A roll is attributed to a player: the actor is required. Plain
  `execute`/`apply` (the trusted system path) makes `Roll` `Malformed`; this
  is what keeps `Roll.roller` honest. Everything else about the trusted path
  is U3's contract, unchanged.
- **Entity.** `Some(id)` must exist (`NotFound { entity, element: None }`),
  for both specs; read authorization is U6. `Check` additionally requires
  `entity: Some(_)` (`Malformed` without one).
- **Expression.** `dice::validate(&expr)` runs first (`TooLarge`/`Malformed`);
  the plan is exactly one term over the whole expression, source `None`,
  `expression = expr.clone()`. Context: `None` without a target;
  `Some(RollContext { kind, stat: Some(target), target_dc: None })` with one,
  where `kind` follows the stat-family map in section 5.6.1.
- **Check.** The command materializes a `BTreeMap<RefId, ContentEntry>` from
  the entity's inventory `InventoryItem::Ref` entries through
  `env.reference().get`: a `ContentFault` fails the command with
  `EngineError::Content`; `Ok(None)` omits the entry (derivation warns
  `UnresolvedReference` itself, and that warning is not surfaced, section
  5.6.2); `Ok(Some)` inserts it. It then calls `rules::roll_parts(entity,
  stat, &RulesContext { now: campaign clock, content: &map })` and composes
  `1d20` with the parts. `context` is always
  `Some(RollContext { kind, stat: Some(stat), target_dc })`.
- **Rolling.** `env.roll(&plan)` is fallible: the error propagates
  (`Environment` for the default port). On success the `Roll` record is
  built: `id` minted, `roller` = actor, `entity` = the command's entity,
  `ts` = `env.now()`, `expr` = `plan.expression`, `result`, `context`,
  `audience` = the override else `settings.player_roll_audience_default`.
- **Feed.** Exactly one entry: id from `state.next_feed_id()`, `ts` =
  `env.now()`, `clock_s` = the campaign clock, `actor` = actor, `entity`,
  `audience` as above, `payload = FeedPayload::Roll { roll }`. No entity
  mutation: `rev`, `updated_at` and the store are untouched; events are
  `[FeedAppended { ids }]`; warnings are empty.

#### 5.6.1 The stat-family map

Shared by both specs (`Expression.target` and `Check.stat`):
`save.*` -> `RollKind::Save`; `skill.*` and `ability.*` -> `Check`;
`attack.*` -> `Attack`; `damage.*` -> `Damage`; anything else -> `Other`.

#### 5.6.2 No derivation warnings on rolls

`roll_parts` returns no warnings and the command does not surface any: a
sheet's derivation warnings are a read-side concern (`derived` service), and
attaching them to every roll would duplicate and reorder them. Command-level
warnings only (section 5.7-5.9) reach clients.

### 5.7 Casting

`CastSpell` is bookkeeping: it validates, spends the slot, manages
concentration, and never rolls. Validation order, so errors stay
distinguishable:

1. Load the target statblock (`NotFound`/`Malformed`, CAS `Conflict`).
2. Find the `SpellEntryId` in any of the entity's spellcasting blocks ->
   `NotFound { entity, element: Some(ElementRef::Spell(id)) }`.
3. Find the slot resource -> `NotFound { entity, element:
   Some(ElementRef::Resource(id)) }`.
4. The resource kind must be `SpellSlot { level }` or `PactSlot`; anything
   else is `Malformed`.
5. Resolve the spell's reference, when it has one, through
   `env.reference().get`: a `ContentFault` is `EngineError::Content`. A
   resolved `ContentData::Spell` yields the level and concentration flag; a
   missing entry, a non-spell payload, or no reference makes the spell
   **unresolved**: warn `UnresolvedReference` (entity + `ElementRef::Spell`,
   section 9 OQ-4) and continue.
6. A resolved cantrip (level 0) is `Malformed`: cantrips do not spend slots.
7. Upcast: `upcast_slots` must equal `slot_level - spell_level` for a
   resolved spell in a `SpellSlot`. With no known spell level (unresolved),
   or with a `PactSlot` whose level comes from the class table, any
   `upcast_slots != 0` is `Malformed`.
8. Spend one slot, exactly like `SpendResource`: a spend that goes below
   zero warns `ResourceOverspent` (entity + `ElementRef::Resource`) and
   still commits (section 9, OQ-5).
9. Concentration: `should_concentrate` is `concentrate || resolved
   concentration`. When true, remove every `StatusKind::Concentration`
   status from the entity, in document order, appending one
   `FeedPayload::ConcentrationEnded { name }` feed entry per removed entry
   (audience `RollerAndDm`, actor = the entity's owner, `clock_s` = campaign
   clock); then append the new status. When false, nothing about statuses
   changes. A resolved spell that does not concentrate while the client
   asked (`concentrate: true`) warns `ConcentrationUnsupported` (entity +
   `ElementRef::Spell`) and still starts concentration.
10. Stamp the entity (one upsert, `rev + 1`): events are
    `[EntityChanged, FeedAppended ..]`, the feed event present exactly when
    at least one entry was appended; warnings are the accumulated
    step-5/7/8/9 outcomes in production order.

The new concentration status: kind `Concentration`; name = the
`SpellEntry.name` snapshot (the client-visible name, resolved or not);
reference = the `SpellEntry.reference`; `text`/`source` `None`; duration
`Indefinite`; `started_at` = the campaign clock; empty modifiers; freshly
minted `StatusId`. The old statuses are removed, never edited.

### 5.8 `BeginConcentration` and `EndConcentration`

- `BeginConcentration` requires `status.kind == StatusKind::Concentration`
  (`Malformed` otherwise), then replaces: every existing concentration
  status is removed and announced (`ConcentrationEnded` per removed), the
  new status is appended from the `StatusSpec` (minted id, `started_at` =
  campaign clock), the entity is stamped. No `MultipleConcentration`
  warning: replacement is the point. No actor requirement: the feed actor
  for the ending is the entity's owner.
- `EndConcentration` removes every concentration status, announcing each
  one. With none present it is `NotFound { entity, element: None }` (after
  the CAS check), and nothing changes. No actor requirement.
- Both are statblock-only (`Malformed` for other kinds) and honor
  `expected_rev`.

### 5.9 Spending hit dice and long-rest regain

- `SpendHitDie` requires an actor (it produces a roll; plain `execute` is
  `Malformed`). Then: load the statblock, find the resource (`NotFound` with
  `ElementRef::Resource`), require `ResourceKind::HitDie { sides }`
  (`Malformed` otherwise).
- The roll: the CON modifier comes from `rules::derive` (item-ref content
  map materialized like a check, campaign-clock context; a `Content` fault
  fails the command) as the `ability_mod.con` integer effective value, 0
  when absent or not an integer. The plan is `compose(Roll { count: 1,
  sides }, &RollParts { flat: con_mod, dice: vec![], hint: Normal })`; no
  hint, no attributed dice. `env.roll` failures propagate; derivation
  warnings are not surfaced.
- Spend one: `current -= 1`, `ResourceOverspent` warning when it goes below
  zero (entity + resource element).
- Heal: the healed amount is `max(0, result.total)`, clamped to `hp.max`
  (`hp.current = min(hp.max, hp.current + healed)`); temp HP untouched.
- Feed: one `Roll` entry, exactly like section 5.6 (`roller` = actor,
  `entity` = the statblock, audience = the campaign default, `context =
  Some(RollContext { kind: Other, stat: None, target_dc: None })`,
  `expr`/`result` from the plan). Events `[EntityChanged, FeedAppended]`;
  warnings the overspend outcome only.
- Long-rest regain (OQ-5's second half, engine-executor section 9): a
  `LongRest` gives each `ResourceKind::HitDie` resource
  `max(1, max / 2)` dice back, clamped to `max`, where `max` follows U3's
  arithmetic: `Formula::Const` evaluates, a non-const maximum is skipped
  with `FormulaUnevaluated` like every other refill. U4 owns the rest of
  long-rest recovery (HP, death saves, exhaustion). No target-test coverage
  (the integration suite's rests are U3's); engine unit tests cover the
  arithmetic, including the minimum-one-die rule (OQ-3).

### 5.10 The concentration-save hint

- `ApplyDamage` additionally appends `FeedPayload::Hint { text:
  "Concentration: CON save DC {dc}" }` when `amount > 0` and the target has
  an unexpired `StatusKind::Concentration` status, where
  `dc = max(10, amount / 2)` (integer division). The entry: audience
  `RollerAndDm`, actor = the entity's owner, entity, `clock_s` = campaign
  clock (section 9, OQ-6/OQ-7).
- `amount <= 0` and non-concentrating targets append nothing. The hint
  changes no warning and no HP math; with a hint the damage transition's
  events gain the `FeedAppended` after `EntityChanged`, without it they are
  U3's shape unchanged.

### 5.11 Determinism and replay

- The only randomness in any path is `CommandEnv::roll`, and the only
  entropy behind a shipped implementation is `ChaChaRng`. Two worlds with
  equal documents and equal scripts produce equal `Roll` records: id
  minting is deterministic (feed ids from the working set, `RollId` freshly
  minted per roll and never compared), and no wall clock is read (the
  environment's `now` is injected).
- `dice::roll` itself is pure: same plan, same die sequence, equal result.

### 5.12 Errors and the environment

- `Environment` marks a broken or missing command environment (the default
  `roll`, an RNG that cannot serve); it is not input and not a rules
  outcome. `Content` wraps a `ContentFault` from resolution, so the server
  can map it like store faults; neither carries rules semantics.
- Every other failure uses the U3 taxonomy: `Malformed` for client input
  (bad notation, missing actor, illegal upcast, non-slot resource),
  `TooLarge` for caps, `NotFound` for missing entities/elements, `Conflict`
  for CAS. Rules never block: overspends, unresolved references, unsupported
  concentration and the damage hint all commit.
- `execute_as` in U5 performs no authorization; every actor-requiring check
  is attribution. U6 slots `can_read`/`can_write` checks into this seam
  before any command work (`authz.md`).

## 6. Internal structure

```
dice/
  mod.rs        public API: parse, validate, compose, roll, RollPlan, TermPlan
  lexer.rs      tokens: numbers, pool tokens, keywords, punctuation
  parser.rs     recursive descent -> DiceExpr
  validate.rs   the four caps over an arbitrary AST
  roller.rs     plan + Rng -> DiceResult
commands/
  dice_cmd.rs   the Roll command: specs, context, audience, feed entry
  casting.rs    CastSpell, BeginConcentration, EndConcentration
  resources.rs  SpendHitDie joins the spend/restore slice
  hp.rs         the concentration-save hint
  rests.rs      long-rest hit-dice regain
ports/
  rng.rs        Rng
adapter (localtt-adapter):
  rng.rs        ChaChaRng (rand_chacha, added via cargo add during implement)
```

- `dice` imports `rules::{RollParts, RollHint}` (plain data from U4) for
  `compose`; `rules` never imports `dice`, so the module graph stays acyclic.
- `commands::apply` keeps its U3 signature for the frozen unit and
  integration callers; an engine-internal `apply_as(state, command, env,
  actor: Option<PlayerId>)` carries the actor, and `apply` delegates with
  `None`. Actor-requiring handlers (`Roll`, `SpendHitDie`) reject `None`;
  all other handlers ignore the actor. U6's pre-check wraps this seam.
- The empty reference source is a private zero-sized type behind
  `CommandEnv::reference`'s default; the default `roll` returns
  `Environment` without allocating entropy.
- Engine unit tests at implement cover what the target test cannot: the full
  parser/precedence matrix, every cap boundary (101/100, 1001/1000, 17/16,
  33/32, keep edges), per-die sequences through nested advantage including
  equal-total ties (the first evaluation is kept), saturation arithmetic, the
  stat-family map (`ability.*` -> `Check`), the cast validation matrix
  including zero-slot overspend and every unresolved variant (missing entry,
  non-spell payload, no reference), the expired-concentration hint, the
  hit-die heal floor (`max(0, total)`), the `SpendHitDie` content-fault path,
  a compile-time `Send` assertion for the shipped `Rng`/`ChaChaRng`, "never
  draws ahead" exact-draw counts, regain arithmetic, and a seeded distribution
  sanity check for `ChaChaRng`.

## 7. API cross-check against the target test

| Target-test need | API element |
|---|---|
| parse notation / refuse syntax | `dice::parse`, `EngineError::Malformed` |
| gate wire ASTs | `dice::validate`, `EngineError::TooLarge` |
| compose derived parts | `dice::compose`, `RollPlan`, `TermPlan`, `rules::{RollParts, DiceTerm, RollHint}` |
| roll a plan | `dice::roll`, `Rng` |
| deterministic test double | `ScriptedRng` (test-local `impl Rng`) |
| shipped, replayable RNG | `localtt_adapter::rng::ChaChaRng::{new, from_seed}` |
| roll command | `Command::Roll`, `RollSpec::{Expression, Check}` |
| actor attribution | `Executor::execute_as` |
| environment ports | `CommandEnv::{now, reference, roll}` |
| content resolution | `ReferenceSource::get`, `EngineError::Content`, `ContentFault` |
| missing entities/elements | `NotFound`, `ElementRef::{Spell, Resource}` |
| casting | `Command::CastSpell`, `Command::{BeginConcentration, EndConcentration}` |
| hit dice | `Command::SpendHitDie`, `ResourceKind::HitDie` |
| warnings | `WarningCode::{UnresolvedReference, ConcentrationUnsupported, ResourceOverspent}` |
| feed | `FeedPayload::{Roll, ConcentrationEnded, Hint}`, `Roll`, `RollContext`, `RollKind`, `Audience` |

Every element in the sketch is either used above or justified:
`CommandEnv::roll`'s `Result` carries `Environment`; `conflict`/CAS fields
are the U3 convention the other mutating commands share; `Keep` and
`DiceExpr` variants are core types the parser must produce; `RollPlan::expression`
exists so `Roll.expr` (the persisted composed expression) is exactly what
the roller evaluated.

## 8. Decisions and deviations (to fold after review)

| # | Deviation | Why |
|---|---|---|
| D1 | `CommandEnv::roll(&mut self, plan: &RollPlan) -> Result<DiceResult, EngineError>` (arch 7.2: `roll(&mut self, expr: &DiceExpr) -> DiceResult`) | Per-term attribution cannot round-trip through a bare `DiceExpr`; environment failure is fallible; the plan is the composed artifact the feed records. |
| D2 | `Command::Roll { entity, spec, audience }` + `Executor::execute_as(actor, _)` (arch 7.2: `Roll { actor, entity, expr, context, audience }`) | One actor plumbing for every command; U6 authorizes at the same seam; command variants stay document-shaped. |
| D3 | `dice::compose(base, &RollParts) -> RollPlan` (arch 7.5: `compose(base, &[Contribution]) -> DiceExpr`) | U4 already extracts attributed `DiceTerm`s and the hint; `RollParts` is the frozen U4 seam; the plan preserves attribution instead of re-inlining it. |
| D4 | `CommandEnv::{reference, roll}` are defaulted trait methods (arch 7.2: `Executor` owns `Arc<dyn ReferenceSource>`, `Box<dyn Rng>`, `ClockSource`) | Keeps `Executor::new` and every frozen env double compiling; fail-closed defaults. **Blocking question OQ-1.** |
| D5 | `EngineError::{Environment, Content}` added (arch 7.3 taxonomy has neither) | A broken content source and a missing dice service are neither input errors nor rules outcomes; the server maps them distinctly. |
| D6 | `RollSpec::{Expression, Check}` (arch 7.2: a bare `expr` + caller-supplied `context`) | Check rolls need engine-side derivation (flat, attributed dice, hint); the spec keeps that inside the engine and lets U7 map one DTO. |
| D7 | Casting semantics pinned here: unresolved references cast with a warning, cantrips refuse slots, upcast arithmetic, pact slots skip level comparison | The arch lists the command without semantics; the target test defines them. |
| D8 | `SpendHitDie` semantics: actor required, CON modifier via `rules::derive`, `max(0, total)` heal, overspend warning, roll feed entry | Same; the arch lists the command only. |
| D9 | `ApplyDamage` gains the concentration-save `Hint` entry | The core payload exists and the table needs the reminder; U3's damage shape is otherwise unchanged. |
| D10 | Long-rest hit-dice regain lands in `commands::rests` (engine-executor OQ-5 split: U4 took recovery arithmetic, U5 takes hit dice) | No signature change; engine unit tests cover it. |

## 9. Open questions

Every question states options, tradeoffs and a recommendation; the sketch
assumes the recommendation. Numbering is RFC-local: `engine-executor.md` has
its own OQ-5 (rest recovery), cited here as "engine-executor OQ-5".

### OQ-1: environment ports on `CommandEnv` vs `Executor` (**resolved in review: A**)

The target test, `engine_target.rs`'s `FixedEnv`, `properties.rs`'s doubles
and the engine unit tests all construct environments with only `now()`. Arch
7.2 intends the executor to own `Arc<dyn ReferenceSource>`, `Box<dyn Rng>`
and a clock, with `CommandEnv` narrowed to `now()`.

- **A (recommended; what the target test pins):** keep the ports as defaulted
  `CommandEnv` methods. The executor stays `Box<dyn CommandEnv>`; defaults
  fail closed (empty content, `Environment` on dice). Smallest diff, frozen
  test envs compile unchanged, and one object supplies both.
- **B:** move the ports to `Executor` fields and change `Executor::new` to
  take them. Matches the arch sketch and makes "the executor owns the RNG"
  literal (replay lives in one place), but edits `Executor::new` call sites
  in `engine_target.rs`, `dice_target.rs`, `properties.rs`, the engine's own
  tests, and `scenarios.rs` if it builds executors; `CommandEnv` shrinks to
  `now()`.
- **C:** both: defaulted `CommandEnv` methods plus optional executor
  injection with precedence. Middle ground, but two ways to supply one
  dependency and an ambiguous precedence rule.

Recommendation: **A** (settled in the 2026-10-18 adversarial review: ports stay
on `CommandEnv`, the target test and every frozen env compile unchanged). B
remains a post-batch route; it needs the target test and this RFC's D4
rewritten first, because it changes `Executor::new`'s signature and every test
environment.

### OQ-2: roller arithmetic overflow (non-blocking)

Nested multiplication of capped pools can exceed `i32` (32 terms of up to
100d1000).

- **A (recommended):** saturating `i32` arithmetic throughout, keeping
  `roll` infallible; values beyond bounds are physically meaningless at a
  table.
- **B:** checked arithmetic returning `TooLarge`/`Malformed` at roll time:
  turns a legal plan into a runtime error the client cannot predict.
- **C:** `i64` internally: defers the same question and changes nothing
  client-visible; not worth the churn.

Recommendation: **A**, matching U4's saturating formula evaluation.

### OQ-3: hit-dice regain minimum (non-blocking)

- **A (recommended):** `max(1, max / 2)` per resource, per 5e's "minimum of
  one die"; a level-1 character regains its die.
- **B:** plain `max / 2`: a 1-die character regains nothing, a surprising
  table result.

Recommendation: **A**; engine unit tests pin it.

### OQ-4: a spell reference that resolves to a non-spell entry (non-blocking)

- **A (recommended):** treat it as unresolved: warn `UnresolvedReference`
  and cast (the level and concentration flag are unknowable, and the
  campaign data, not the request, is at fault).
- **B:** `Content` error: a broken catalog entry fails the cast; harsher
  than the warn-never-block stance.
- **C:** `Malformed`: blames the client for a document defect.

Recommendation: **A**.

### OQ-5: casting at zero slots (non-blocking)

- **A (recommended):** the same arithmetic as `SpendResource`: go to -1,
  warn `ResourceOverspent`, commit. One spend rule for the whole engine.
- **B:** `Malformed`: blocks a legal table override (the DM may allow a
  cast the sheet shows as spent).
- **C:** clamp at zero: silently drops the spend; rejected.

Recommendation: **A**.

### OQ-6: the damage-hint DC basis (non-blocking)

- **A (recommended):** the `amount` operand, floor-divided by two, minimum
  10, per the standard concentration rule as tables apply it; the target
  test pins 24 -> 12 and 5 -> 10.
- **B:** damage that reached `hp.current` (after temp HP): arguably "damage
  taken", but makes the DC depend on absorption bookkeeping the DM may not
  have announced.

Recommendation: **A**; B is a later refinement if a table asks.

### OQ-7: hint on a soft-expired concentration (non-blocking)

- **A (recommended):** the hint requires an unexpired concentration, using
  U4's expiry predicate; an expired status is not concentrating.
- **B:** any `Concentration` status on the sheet: simpler, but nags about a
  status the rules already ignore.

Recommendation: **A**.

### OQ-8: where distribution sanity lives (**resolved in implement: A'**)

- **A' (resolved):** a seeded, bounded `ChaChaRng` unit test inside the
  adapter, where the type lives (bucket counts within generous bands). The
  engine cannot host it: the adapter depends on the engine, and the reverse
  would invert the crate graph (arch 4.4). The integration target suite
  keeps the exact, offline-fast contract.
- **A:** the original engine-side placement; impossible as written, for
  that dependency-direction reason.
- **B:** a statistical check in the integration contract test: slow, and a
  flake surface in CI.
- **C:** drop it: the arch 4.4 contract suite then only covers
  range/replay/degenerate.

Resolution: **A'**; `localtt_adapter::rng` covers seeded face coverage, the
target test stays exact.

## 10. Deferred items (not blocking the skeleton)

- Attack lines, spell attacks and spell damage automation; client-built
  expressions cover the table meanwhile.
- Content-driven spell data beyond level/school/concentration/ritual (upcast
  text, damage formulas) — U2 payload widening, versioned.
- Recharge-by-roll resources and monster feature plumbing.
- `FreezeReference` and the packs loader (U8).
- Server DTO mapping for `RollSpec`, the new commands, warnings and feed
  entries (U7).
- Campaign-level CAS for the clock/settings (U3 OQ-1 A-revisit) — untouched
  here.
- Initiative order, surprise and action economy (no unit owns them yet).
- Distribution telemetry, roll history queries and per-campaign seeding
  policy beyond `from_seed` (U7+ decisions).
