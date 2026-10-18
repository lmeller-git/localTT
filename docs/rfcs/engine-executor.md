# RFC: U3 -- the engine executor and the first command slice

| | |
|---|---|
| **Status** | Spec phase (2026-10-18, `u3-engine`): this RFC and the target test are the first U3 artifacts. No library code exists yet: the target test intentionally does not compile until the U3 skeleton lands (then it fails with `not yet implemented` until the implement phase). OQ-1 resolved 2026-10-18: option A (no campaign-level CAS in U3; the clock is DM-only and the executor is single-writer). |
| **Scope** | `localtt-engine::{error, state, executor, commands, clock}` plus the `lib.rs` module additions; the target test `crates/localtt-integration/tests/engine_target.rs`. The adapter is not touched; `engine::ports::store` (U1) is consumed as-is. |
| **Read with** | `docs/engine-architecture.md` sections 5.3-5.5 (commit protocol, working set, rev), 7.1-7.3 (modules, command model, errors), 7.6 (clock and the expiry index), 7.9 (events and feed), 9 (testing), 10 U3; `docs/decisions.md` ENG-2/3/4/6/9, STORE-11, ARCH-3; `RFC-0001-localtt.md` 5.2 (warn never block), 5.3 (clock, statuses, dawn), 5.6-5.7 (feed audiences); `crates/localtt-core/DESIGN.md` (the frozen wire types). |
| **Pins** | The design sketches are explicitly "not compiling Rust" (design section 0); this RFC pins the U3 subset; section 8 lists every deviation and every pin the sketch left open, section 9 the open questions. |

## 1. Goals

1. Pin the executor: `executor::Executor`, `Executed`, and the commit protocol
   (`apply` -> `store.apply` -> `state.apply`, durable before visible, ENG-2).
2. Pin the working set and its invariants: `state::{WorkingSet, WorkingSetMut,
   LoadedCampaign, EntityHeader}`, the lazily-rebuilt expiry index and the feed
   id source (design sections 5.4 and 7.6).
3. Pin the error and warning surface: `error::{EngineError, Warning,
   WarningCode, ElementRef}` (design 7.3; the design names `ElementRef` without
   a shape).
4. Pin the U3 command slice (clock, statuses, hp, resources, rests) as one
   `commands::Command` enum, with `commands::{StatusSpec, CommandEnv, Transition,
   apply}` and the `Event` vocabulary the executor returns.
5. Pin clock semantics: the expiry window `(old, new]` over soft statuses, the
   `StatusExpired` feed entries, dawn refills, the rewind warning, and the feed
   ring trim (design 7.6, RFC-0001 5.3).
6. Pin `rev` stamping and the optimistic-concurrency check for U3 entity
   commands, and the honest U3 answer for the design's campaign-level
   `expected_rev` (section 5.5; OQ-1, resolved A).
7. Keep the provisional free functions `localtt_engine::commands::*` resolving:
   the frozen `tests/scenarios.rs` imports them by name, and the `commands/`
   module split must carry them over unchanged (section 6.4).

The target test is written first and lives at
`crates/localtt-integration/tests/engine_target.rs`; section 3 is its map.

## 2. Non-goals

- **No dice, rolls, casting or concentration commands.** `Roll`,
  `CastSpell`, `BeginConcentration`/`EndConcentration` and the `Rng` port are
  U5; `FreezeReference` and the content-consuming paths are U4/U8.
- **No entity lifecycle.** `CreateEntity`, `DeleteEntity`, `SetField`,
  owner/ACL commands and inventory commands are later units; the target test
  seeds the store directly through the U1 port.
- **No authorization.** `EngineError::Unauthorized` is declared for the
  taxonomy but has no U3 producer; authz and `view_of` are U6. U3 commands
  carry no actor field (the server maps actors into commands in U7).
- **No rules derivation.** The U3 slice never evaluates a `Formula` beyond
  `Formula::Const` (OQ-2); the pipeline, modifiers and derived values are U4.
- **No lazy working set.** Load-all is the U3 implementation (ENG-3); the
  trait shape keeps the door open.
- **No campaign-level rev.** Core is frozen and `Campaign` has no `rev`;
  U3 clock commands take no `expected_rev` (OQ-1, resolved A: the clock is
  DM-only and the executor serializes writers).
- **No settings, player, invite or content commands.**
- **No rest recovery arithmetic** beyond RFC-0001 5.3 ("reset resources by
  recharge type, clear rest-typed statuses"): HP, death saves, exhaustion and
  hit-dice recovery are OQ-5.
- **No server, no binary, no migrations.** Those are U7 and the store unit
  respectively.

## 3. Client usage: the target test

`crates/localtt-integration/tests/engine_target.rs` is ordinary client code: it
seeds a `MemoryStore` through the U1 port, wraps the loaded snapshot in a
`LoadedCampaign`, drives `Executor::execute` with the U3 command slice, and
checks both the working set and the reloaded store. Ten tests:

| Test | What it pins |
|---|---|
| `advancing_the_clock_expires_a_status_and_appends_a_feed_entry` | the window `(old, new]`; soft expiry (status stays, sheet rev unchanged); the `StatusExpired` entry's full shape (`RollerAndDm`, actor = owner, `clock_s` = expiry, `ts` = env); event order `CampaignChanged` + `FeedAppended`; reload parity |
| `the_store_mirrors_the_working_set_after_a_command_sequence` | hp/status/resource commands through the commit protocol; rev stamps 1, 2; `started_at` from the campaign clock and `updated_at` from the env clock; reload parity |
| `spending_a_resource_at_zero_warns_and_succeeds` | overspend warns, never blocks (`WarningCode::ResourceOverspent` + `Warning` fields); `EntityChanged { rev }` |
| `a_stale_revision_conflicts_and_writes_nothing` | optional `expected_rev` CAS; the `Conflict { entity, expected, actual }` payload; working set and store untouched |
| `unknown_elements_and_entities_are_not_found` | `NotFound` for a missing status element and a missing entity; `Malformed` for a negative amount; refused commands write nothing |
| `crossing_dawn_refills_dawn_resources_and_rewinding_warns` | signed dawn crossings; forward refill is scoped to `Recharge::Dawn`; ascending `EntityChanged` order across two refilled entities; rewind warns (`DawnRewind`) and never reverses a refill |
| `a_short_rest_refills_short_resources_and_clears_rest_statuses` | rest bookkeeping + optional clock advance; campaign-before-entity event order |
| `one_transition_emits_campaign_entity_and_feed_events` | all three event kinds from one command, in order; a timed status expiring exactly on the rest advance; the entry's `clock_s`, actor, audience and payload |
| `a_store_fault_publishes_nothing` | `EngineError::Store` transparency; a failed write leaves the working set and the store untouched |
| `the_executor_is_send` | the single-writer executor's `Send` expectation |

Notes:

- **It does not compile yet, on purpose.** `localtt_engine::{error, state,
  executor, clock}` and the `commands` rewrite do not exist. The skeleton phase
  adds the declarations and every test fails with `not yet implemented` until
  the implement phase; the spec-phase contract is exactly that.
- **The client seeds through the store.** A fresh campaign arrives via
  `MemoryStore::apply` with a `CampaignSnapshot` made from `fixtures::party()`
  (the same path `store_target.rs` pins); the first U3 command is then a
  mutation, not a campaign creation.
- **`LoadedCampaign::from_snapshot` is the load seam.** The client loads
  through the port and wraps the snapshot; there is no executor constructor
  that reads the store, so the composition root (U7) stays in charge of
  startup.
- **`FixedEnv` freezes wall time.** `CommandEnv::now` is the only environment
  seam in U3, so `updated_at` and feed `ts` stamps are deterministic.
- **The test does not inspect raw JSON**; byte comparisons go through
  `roundtrip::to_json`, as in `store_target.rs`. Expiry-index recomputation,
  feed trimming under capacity and command-sequence properties are
  implement-phase unit/property tests (design 9), not target-test cases.
- **The store-fault case wraps `MemoryStore`** in a test-local failing
  `CampaignStore`; the port's error transparency and the no-publish guarantee
  are pinned there.

## 4. Public API sketch

Sketches, not compilable code; signatures are the contract. `#[non_exhaustive]`
enums are matched by consumers with a wildcard arm. Per-item docs are elided
for brevity; the skeleton lands every public item documented (both crates deny
`missing_docs`). Core types (`EntityId`, `StatusId`, `ResourceId`,
`CampaignTime`, `UnixTime`, `Campaign`, `AnyEntity`, ...) are imported from
`localtt_core::types` and elided here.

### 4.1 `localtt_engine::error`

```rust
/// Failure of a command or of the executor.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The actor may not read/write the target entity or perform the command.
    /// No U3 producer: commands carry no actor yet (U6).
    #[error("player {actor} may not perform the command on {entity:?}")]
    Unauthorized { actor: PlayerId, entity: Option<EntityId> },
    /// No such entity, or no such element inside the entity.
    #[error("no entity {entity} or element {element:?}")]
    NotFound { entity: EntityId, element: Option<ElementRef> },
    /// Optimistic concurrency mismatch (server: 409).
    #[error("entity {entity} is at revision {actual}, not {expected}")]
    Conflict { entity: EntityId, expected: u64, actual: u64 },
    /// Request-level invalid input (server: 400).
    #[error("malformed command: {reason}")]
    Malformed { reason: String },
    /// A dice expression exceeded configured caps (U5); declared for the
    /// taxonomy, no U3 producer.
    #[error("{what} exceeds the limit of {limit}")]
    TooLarge { what: &'static str, limit: u32 },
    /// Persistence failure, transparent.
    #[error(transparent)]
    Store(#[from] StoreFault),
}

/// A non-blocking rule outcome (RFC-0001 2.2). `message` is a short
/// human-readable sentence (U7 may render it); `code` is the machine-readable
/// key clients switch on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub code: WarningCode,
    pub message: String,
    /// The affected entity, when the warning is entity-scoped.
    pub entity: Option<EntityId>,
    /// The affected element, when the warning is element-scoped.
    pub element: Option<ElementRef>,
}

/// Machine-readable warning kinds; a closed set with additive additions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WarningCode {
    /// A spend left the resource below zero.
    ResourceOverspent,
    /// Damage left hit points below zero.
    NegativeHp,
    /// A concentration status was added while one was already active.
    MultipleConcentration,
    /// A clock move crossed at least one dawn instant backwards.
    DawnRewind,
    /// A refill was skipped, or a restore left unclamped, because the maximum
    /// is a `Formula` only the rules unit (U4) can evaluate (OQ-2).
    FormulaUnevaluated,
}

/// One element inside an entity document, for scoped errors and warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ElementRef {
    Status(StatusId),
    Resource(ResourceId),
}
```

### 4.2 `localtt_engine::state`

```rust
/// Common header of an entity, without its body (design 5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityHeader {
    pub id: EntityId,
    pub kind: EntityKind,
    pub name: String,
    pub owner: PlayerId,
    pub parent: Option<EntityId>,
    pub acl: Vec<AclEntry>,
    pub rev: u64,
    pub updated_at: UnixTime,
}

/// Read view commands and services use. Owned return values keep a future
/// lazy implementation possible (ENG-3); the load-all one never fails.
pub trait WorkingSet: Send {
    fn campaign(&self) -> &Campaign;
    fn entity(&self, id: EntityId) -> Result<Option<AnyEntity>, EngineError>;
    fn headers(&self) -> Vec<EntityHeader>;
    fn feed(&self) -> Vec<FeedEntry>;
    /// The id the next feed append receives.
    fn next_feed_id(&self) -> u64;
    /// Every indexed `(entity, status)` whose `expires_at <= until`, in an
    /// unspecified order; the caller filters and sorts.
    fn expiry_candidates_until(&self, until: CampaignTime) -> Vec<(EntityId, StatusId)>;
}

/// Mutated only by the executor, after the store commit.
pub trait WorkingSetMut: WorkingSet {
    /// Applies one durable transition: upserts, removals, feed appends and
    /// trimming; keeps the expiry index and `next_feed_id` in sync.
    fn apply(&mut self, patch: &StatePatch<'_>);
}

/// The load-all working set (ENG-3); fields are private.
#[derive(Debug)]
pub struct LoadedCampaign { /* campaign, entities, feed, next_feed_id, expiries */ }

impl LoadedCampaign {
    /// Wraps a loaded snapshot: builds the entity map, recomputes the expiry
    /// index from the documents and `next_feed_id` from the feed tail.
    #[must_use]
    pub fn from_snapshot(snapshot: CampaignSnapshot) -> Self;
}
```

`LoadedCampaign` implements `WorkingSet` and `WorkingSetMut` (6.2); the executor
takes it boxed as `Box<dyn WorkingSetMut>` (4.4), which is what the target
test's `Box::new(LoadedCampaign::from_snapshot(..))` coerces to.

### 4.3 `localtt_engine::commands`

```rust
/// The U3 command slice: clock, statuses, hp, resources, rests. Later units
/// add variants (dice, casting, inventory, entities, seats).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    // clock (DM commands; the actor is folded into the server DTO in U7)
    AdvanceClock { by_seconds: i64 },
    SetClock { to: CampaignTime },
    // statuses
    AddStatus { entity: EntityId, status: StatusSpec, expected_rev: Option<u64> },
    RemoveStatus { entity: EntityId, status: StatusId, expected_rev: Option<u64> },
    EditStatusDuration {
        entity: EntityId,
        status: StatusId,
        duration: StatusDuration,
        expected_rev: Option<u64>,
    },
    // hp
    ApplyDamage { entity: EntityId, amount: i32, expected_rev: Option<u64> },
    Heal { entity: EntityId, amount: i32, expected_rev: Option<u64> },
    SetTempHp { entity: EntityId, amount: i32, expected_rev: Option<u64> },
    SetDeathSaves { entity: EntityId, value: DeathSaves, expected_rev: Option<u64> },
    // resources
    SpendResource { entity: EntityId, resource: ResourceId, amount: i32, expected_rev: Option<u64> },
    RestoreResource { entity: EntityId, resource: ResourceId, amount: i32, expected_rev: Option<u64> },
    // rests
    ShortRest { entity: EntityId, advance_seconds: Option<i64>, expected_rev: Option<u64> },
    LongRest { entity: EntityId, advance_seconds: Option<i64>, expected_rev: Option<u64> },
}

/// The client-supplied part of a new status; the command layer mints the id
/// and stamps `started_at` from the campaign clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSpec {
    pub kind: StatusKind,
    pub name: String,
    pub reference: Option<RefId>,
    pub text: Option<String>,
    pub duration: StatusDuration,
    pub source: Option<StatusSource>,
    pub modifiers: Vec<Modifier>,
}

/// Everything a command may consult that is not the working set. U3-minimal:
/// `reference()` and `roll()` land with U4/U5.
pub trait CommandEnv: Send {
    /// Wall-clock time for `updated_at` and feed `ts` stamps.
    fn now(&self) -> UnixTime;
}

/// Pure with respect to `state`: computes what should happen and what it
/// costs. State is not modified; the executor persists and then applies.
pub fn apply(
    state: &dyn WorkingSet,
    command: &Command,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError>;

/// One command's owned effect. Fields are crate-private; clients see warnings
/// and events through `Executed` only.
pub struct Transition { /* campaign, upserts, removals, feed_appends, feed_keep_from, warnings, events */ }

impl Transition {
    /// Borrows the owned documents into the patch the store takes.
    #[must_use]
    pub fn patch(&self) -> StatePatch<'_>;
}

/// Delivery hints for the server (U7); events carry ids, not documents.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// One entity's document was replaced; `rev` is the new revision.
    EntityChanged { id: EntityId, rev: u64, kind: EntityKind },
    /// An entity was removed. No U3 producer; declared for the later
    /// lifecycle commands.
    EntityRemoved { id: EntityId },
    /// The campaign document changed (clock, settings, players).
    CampaignChanged,
    /// Feed entries were appended; ascending ids, never empty.
    FeedAppended { ids: Vec<FeedEntryId> },
}
```

`StatusSpec` mirrors the client-supplied subset of core `Status`: the command
layer mints `id` and stamps `started_at`, and the concentration/`source`
semantics arrive with the U5 casting path.

Provisional helper compatibility (section 6.4): the module split re-exports
`{apply_damage, heal, set_temp_hp, spend_resource, begin_concentration,
level_up, summon_from_template, give_handout}` at the same paths, with
unchanged signatures and behavior.

### 4.4 `localtt_engine::executor`

```rust
/// The single-writer command loop. `Send`, deliberately not `Sync`: the
/// server serializes access (mutex or actor thread, U7).
pub struct Executor<'a> {
    state: Box<dyn WorkingSetMut>,
    store: &'a mut dyn CampaignStore,
    env: Box<dyn CommandEnv>,
}

impl<'a> Executor<'a> {
    #[must_use]
    pub fn new(
        state: Box<dyn WorkingSetMut>,
        store: &'a mut dyn CampaignStore,
        env: Box<dyn CommandEnv>,
    ) -> Self;

    /// apply -> persist -> publish. Durable before visible (ENG-2): a failed
    /// store write leaves the working set untouched, and events are returned
    /// only after the commit.
    pub fn execute(&mut self, command: &Command) -> Result<Executed, EngineError>;

    /// The visible state, for services (7.8) and tests.
    #[must_use]
    pub fn state(&self) -> &dyn WorkingSet;
}

/// A committed command's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executed {
    pub warnings: Vec<Warning>,
    pub events: Vec<Event>,
}
```

### 4.5 `localtt_engine::clock`

```rust
/// Whole dawn instants crossed moving the clock from `old` to `new`:
/// positive forwards, negative backwards (a rewind crossing). Exact integer
/// arithmetic over `SECONDS_PER_DAY` and `dawn_hour` (ENG-9); `div_euclid`
/// floors, so pre-origin times behave.
#[must_use]
pub fn crossed_dawns(old: CampaignTime, new: CampaignTime, dawn_hour: u8) -> i64;
```

The expiry window itself lives in `commands::apply` (it needs entity
documents); `clock` stays pure arithmetic.

## 5. Guarantees

### 5.1 Commit protocol, safety and progress

`Executor::execute` runs exactly this protocol:

```
commands::apply(state, command, env) -> Transition   // pure, state untouched
    |
    v
assert rev monotonicity per upsert                   // engine bug check, panics
    |
    v
store.apply(&transition.patch())                     // one durable transaction
    |
    v
state.apply(&transition.patch())                     // in-memory publish
    |
    v
Ok(Executed { warnings, events })                    // only after durability
```

- **Durable before visible.** A command error or a `StoreFault` returns before
  any publish; the working set is unchanged and no events are emitted. There is
  no rollback code and no dirty-state repair (ENG-2, design 5.3).
- **`state.apply` cannot fail.** It is a pure in-memory update (pinned
  infallible in 4.2); a fallible apply after the store commit would be a
  divergence path, and the load-all implementation has nothing that can fail.
- **Rev monotonicity is asserted before the write.** For every upsert the
  executor reads the replaced document from the working set and requires
  `new.rev == old.rev + 1`; a mismatch is an engine bug, never a client error,
  and fails fast with a diagnostic before anything is persisted (design 5.5,
  store RFC 5.3). The command layer stamps `old.rev + 1` from the document it
  just read, so the assertion cannot fire through the public command surface;
  it guards engine regressions. The first create commands must start at
  `rev = 1` (design 5.5); no U3 command creates.
- **Crash safety.** A crash after commit and before the caller uses the result
  is safe: the next startup loads the committed file.
- **Progress.** Everything is synchronous and blocking; there is no
  lock-free/wait-free claim and no internal concurrency. The executor is
  single-writer (`Send`, not `Sync`); the server adds the serialization point
  in U7. `WorkingSet: Send` and `CommandEnv: Send` exist so `Executor` is
  `Send`; the target test pins it.

### 5.2 Clock moves and expiry

`AdvanceClock` computes `to = old.checked_add(by_seconds)`, overflow is
`Malformed`. `SetClock` takes `to` as given (any seconds; pre-origin times are
legal, design 7.6). If `to == old`, the transition is empty: no documents, no
events, no warnings.

Otherwise the campaign document is updated (`clock.now_s = to`) and:

- **Forward moves (`to > old`)** sweep the expiry index: every status with
  `StatusDuration::Timed { expires_at }` where `old < expires_at <= to` is
  "newly expired". The window excludes `old` and includes `to`, matching the
  predicate `now >= expires_at`.
- Each newly expired pair produces one `FeedEntry`:

  | Field | Value |
  |---|---|
  | `id` | the next feed id, assigned in the deterministic order below |
  | `ts` | `env.now()` |
  | `clock_s` | `expires_at` (the in-game instant it expired) |
  | `actor` | the entity owner (the key the `RollerAndDm` audience hangs on; the expiry is about that player's sheet) |
  | `entity` | the entity id |
  | `audience` | `Audience::RollerAndDm` (owner and DM, RFC-0001 5.3) |
  | `payload` | `FeedPayload::StatusExpired { status, name }` with the live name |

  The assignment order is `(expires_at, entity_id, status_id)` ascending, so
  ids are stable no matter how the index enumerated candidates. A transition
  that mints more entries than the ring holds keeps only the newest (5.6).
- **Soft expiry.** The status stays in the document; no entity is rewritten
  and no `EntityChanged` fires. Un-expiring on a rewind is likewise silent
  (RFC-0001 5.3, L2). The sweep is an event transition, not a cleanup: a
  status that is already expired when the campaign loads never fires
  retroactively (there is no "fired" state to consult; the window is the
  contract), and a rewind followed by a new forward crossing re-fires, which
  the feed ring absorbs like any other event.

### 5.3 Dawn, rewind and rests

- `crossed_dawns(old, to, settings.dawn_hour)` counts the dawn instants in
  `(old, to]` (positive) or in `(to, old]` (negative). A dawn instant is
  `day * SECONDS_PER_DAY + dawn_hour * 3600` for integer `day`.
- **Forward crossing** (`> 0`) refills every `Recharge::Dawn` resource on every
  entity to its maximum; one crossing and ten crossings have the same effect.
  An entity is rewritten (rev + 1, `updated_at = env.now()`) only when at
  least one value actually changes; a resource that is already full causes no
  write. Non-const maxima are OQ-2.
- **Backward crossing** (`< 0`) emits one campaign-scoped
  `WarningCode::DawnRewind` warning; refills are never reversed ("owed to the
  discrete nature of recharge", design 7.6).
- **Rests** are RFC-0001 5.3 bookkeeping plus the optional clock move:
  - `ShortRest` refills `Recharge::Short` resources of the target entity and
    clears its `UntilRest { rest: Short }` statuses.
  - `LongRest` refills `Recharge::Short` and `Recharge::Long` and clears
    `UntilRest { rest: Short | Long }` (a long rest includes a short rest).
  - `Some(advance_seconds)` moves the clock first, through the same machinery
    as 5.2/5.3 (expiry sweep over all entities, dawn refills, rewind warning);
    a negative value is `Malformed`, zero is no move, `None` no move.
  - The rest target is always rewritten (rev + 1), even when nothing changed:
    a rest is an explicit checkpoint. HP, death saves, exhaustion and hit dice
    are untouched (OQ-5).
  - One command produces at most one upsert per entity: a rest target that is
    also dawn-refilled is merged into a single new revision.

### 5.4 Command semantics

Common prelude for every entity command, in order: (1) the target entity must
exist, else `NotFound { entity, element: None }`; (2) when `expected_rev` is
`Some(rev)`, it must equal the in-memory `entity.rev`, else `Conflict`;
(3) element-addressing commands (`RemoveStatus`, `EditStatusDuration`,
`SpendResource`, `RestoreResource`) must find their element, else
`NotFound { entity, element: Some(...) }`. Then the effect applies to a copy of
the document; the copy is stamped `rev = old.rev + 1`,
`updated_at = env.now()` and becomes the single upsert.

| Command | Effect | Warnings | Errors (beyond the prelude) |
|---|---|---|---|
| `AddStatus` | mints a `StatusId`; `started_at = campaign.clock.now_s`; appends the status | `MultipleConcentration` when `kind == Concentration` and one is active | — |
| `RemoveStatus` | retains the status out | — | — |
| `EditStatusDuration` | replaces the status's `duration` | — | — |
| `ApplyDamage` | temp HP absorbs first, then current, saturating (the provisional helper's arithmetic) | `NegativeHp` when `current < 0` | `amount < 0` -> `Malformed` |
| `Heal` | `current = (current + amount).min(max)`, saturating | — | `amount < 0` -> `Malformed` |
| `SetTempHp` | `temp = temp.max(amount)` (non-stacking) | — | `amount < 0` -> `Malformed` |
| `SetDeathSaves` | replaces the value | — | either field `> 3` -> `Malformed` |
| `SpendResource` | `current = current.saturating_sub(amount)` (may go negative) | `ResourceOverspent` when the result is `< 0` | `amount < 0` -> `Malformed` |
| `RestoreResource` | `current = current.saturating_add(amount)`, clamped to `max` when it is `Formula::Const`; unclamped with `FormulaUnevaluated` otherwise | `FormulaUnevaluated` when unclamped | `amount < 0` -> `Malformed` |
| `ShortRest` / `LongRest` | 5.3 | `FormulaUnevaluated` per skipped refill | `advance_seconds < 0` -> `Malformed` |
| `AdvanceClock` / `SetClock` | 5.2/5.3 | `DawnRewind`, `FormulaUnevaluated` | clock add overflow -> `Malformed` |

Zero amounts are legal and apply the arithmetic as-is (an idempotent write
still stamps a revision). The U3 command layer reuses the provisional helpers
for `ApplyDamage`, `Heal`, `SetTempHp` and `SpendResource`, so command
semantics and the frozen scenario suite cannot drift apart.

### 5.5 Revisions and conflicts

- Entities load with their stored `rev` (fixtures start at 0). Every command
  that upserts an entity sets `rev = old.rev + 1` and `updated_at = env.now()`;
  clock commands touch entities only through dawn refills.
- `expected_rev` is an optional client-side compare-and-swap: `None` skips the
  check (intent commands read-modify-write inside the single writer), `Some(r)`
  requires `r == entity.rev` in the authoritative in-memory state (ENG-4).
- U3 entity commands all take `expected_rev`; U3 clock commands take none. The
  design's campaign-level `expected_rev` has no vehicle on the frozen core
  `Campaign`; OQ-1 (resolved A) defers campaign-level CAS to U7.

### 5.6 Feed ids and the ring

- Feed positions are monotonic per campaign: `next_feed_id` starts at
  `feed.back().id + 1` after a load (1 for an empty feed) and advances by one
  per appended entry. Ids are never reused; trimming drops old entries but
  never rewinds the counter.
- A transition caps what it appends before the patch is built: entries are
  minted in the 5.2 assignment order, and when one transition mints more than
  the ring holds, only the newest `c = settings.feed_capacity.max(1)` survive;
  the older ones of the same transition are dropped at the source as ordinary
  ring overflow. The trim cutoff comes from the newest kept append:
  `feed_keep_from = last_appended + 1 - c` when `last_appended + 1 > c`, else
  `None`. Because the patch carries at most `c` appends, every kept id is
  `>= feed_keep_from` (store precondition 6, store RFC 5.3); store and working
  set end with exactly the newest `c` entries. Trimming runs before the
  appends (STORE-13); the engine recomputes `next_feed_id` from the surviving
  feed on `state.apply`.
- `feed_capacity == 0` keeps exactly the latest entry, including within one
  transition (OQ-3).

### 5.7 Events

`Executed::events` is the ordered delivery hint for U7: `CampaignChanged`
(when the campaign document changed), then one `EntityChanged` per upsert in
ascending entity id order, then `FeedAppended { ids }` (ascending; the kept
appends, 5.6) when entries were appended. Events never carry documents; the
server re-reads through `view_of` (design 7.9). Warnings and events are
returned only for commands that committed.

### 5.8 Warnings

Warnings never block and never fail a command (RFC-0001 2.2); each warning
carries the machine code, a human-readable message, and entity/element scoping
where it applies. The U3 catalogue is closed in 4.1: `ResourceOverspent`,
`NegativeHp`, `MultipleConcentration`, `DawnRewind`, `FormulaUnevaluated`.

### 5.9 Error model

`EngineError` is `#[non_exhaustive]`; `NotFound`, `Conflict`, `Malformed` (and
later `Unauthorized`, `TooLarge`) are client-facing classes the server maps to
status codes (U7), while `Store` wraps the port fault transparently. `StoreFault`
messages stay diagnostic and are never forwarded; warning messages are
user-visible.

## 6. Internal structure

### 6.1 Module layout

```
crates/localtt-engine/src/
  lib.rs             + pub mod error; pub mod state; pub mod executor; pub mod clock;
                       commands.rs becomes the commands/ directory
  error.rs           EngineError, Warning, WarningCode, ElementRef
  state.rs           EntityHeader, WorkingSet, WorkingSetMut, LoadedCampaign
  executor.rs        Executor, Executed
  commands/
    mod.rs           Command, StatusSpec, CommandEnv, apply(), Transition, Event, re-exports
    hp.rs            apply_damage, heal, set_temp_hp (moved) + the hp commands
    resources.rs     spend_resource (moved) + spend/restore commands
    statuses.rs      begin_concentration (moved) + status commands
    rests.rs         short/long rest commands (pure 5.3 effects)
    entities.rs      level_up, summon_from_template (moved; commands arrive later)
    inventory.rs     give_handout (moved; commands arrive later)
  clock.rs           crossed_dawns (pure arithmetic; design 7.6)
```

### 6.2 `LoadedCampaign`

Fields (private): `campaign: Campaign`, `entities: BTreeMap<EntityId,
AnyEntity>`, `feed: VecDeque<FeedEntry>`, `next_feed_id: u64`, `expiries:
BTreeMap<CampaignTime, Vec<(EntityId, StatusId)>>`.

- `from_snapshot` walks the entities once and indexes every `Timed` status;
  `next_feed_id` comes from the feed tail.
- `apply` maintains the index from the patch's own documents: it removes the
  old document's timed entries and inserts the new ones per upsert, removes
  the entries of a removed entity, pops front entries below `feed_keep_from`,
  appends the new entries, and recomputes `next_feed_id`.
- Two invariants, property-tested in the implement phase (design 5.4): headers
  reflect the entity map, and `expiries` equals a recomputation from the
  documents. The index is a candidate enumerator only; the truth is the
  per-status `now >= expires_at` predicate.

### 6.3 `Transition` construction

`commands::apply` builds one owned `Transition`: the campaign delta, one
merged upsert per touched entity (a rest target that also dawn-refills appears
once), feed entries with assigned ids capped to the newest `c` (5.6), the trim
cutoff, warnings and events.
`Transition::patch` borrows those documents into `StatePatch<'_>`; the
executor persists, applies the same patch to the state, then destructures the
transition into `Executed`.

### 6.4 Provisional helper compatibility

The frozen `crates/localtt-integration/tests/scenarios.rs` imports eight free
functions from `localtt_engine::commands`. The U3 split moves them into the
submodules above and re-exports them at the `commands` root with unchanged
signatures and bodies; the skeleton phase must not stub them (the scenario
suite is green today). The four helpers used by the U3 slice keep their
arithmetic exactly; the others (`begin_concentration`, `level_up`,
`summon_from_template`, `give_handout`) stay pure helpers until their command
variants land in later units.

## 7. API cross-check against the target test

Used by the test, where pinned:

- `state::{LoadedCampaign::from_snapshot, WorkingSet::{campaign, entity,
  headers, feed}}` (4.2); `EntityHeader::id` (4.2);
- `executor::Executor::{new, execute, state}` (4.4);
- `commands::{Command, CommandEnv::now, Event, StatusSpec}` (4.3):
  `Command::{AdvanceClock, SetClock, AddStatus, RemoveStatus, ApplyDamage,
  SpendResource, ShortRest}`, `Event::{CampaignChanged, EntityChanged,
  FeedAppended}`;
- `error::{EngineError::{NotFound, Conflict, Malformed, Store}, Warning,
  WarningCode::{ResourceOverspent, DawnRewind}, ElementRef::Resource/Status}`
  (4.1);
- `working set fields`: the `StatusExpired` feed entry's shape, and the
  soft-expiry document keeping its `rev` (5.2);
- core types: `AnyEntity`, `Audience`, `Campaign`, `CampaignTime`, `EntityId`,
  `EntityKind`, `FeedEntryId`, `FeedPayload::StatusExpired`, `Formula::Const`,
  `Recharge`, `Resource`, `ResourceId`, `ResourceKind`, `RestKind`,
  `StatblockEntity`, `StatusDuration`, `StatusId`, `StatusKind`, `UnixTime`;
  `localtt_core::builders::ref_id`; U1's `MemoryStore`, `CampaignStore`,
  `StoreFault`, `CampaignSnapshot`, `StatePatch`; the suite's own
  `fixtures::*`, `roundtrip::to_json` and the `serde` dependency; the target
  test's local `FlakyStore` wrapper for the store-fault case.

Sketch elements the test does not use, and their justification:

- `EngineError::{Unauthorized, TooLarge}`: declared for the taxonomy (design
  7.3); producers are U6/U5.
- `Command::{Heal, SetTempHp, SetDeathSaves, EditStatusDuration,
  RestoreResource, LongRest}`: slice completeness (design 10 U3); exercised by
  implement-phase unit tests and later by the scenario suite.
- `WorkingSetMut`, `WorkingSet::next_feed_id`,
  `WorkingSet::expiry_candidates_until`: the executor's and the clock sweep's
  seams (4.2), used internally and by implement-phase property tests.
- `commands::apply` and `Transition`: the pure command seam (design 7.2),
  unit-tested directly in the implement phase; `Transition` fields stay
  crate-private.
- `clock::crossed_dawns`: pure arithmetic behind 5.3 (ENG-9).
- `WarningCode::{NegativeHp, MultipleConcentration, FormulaUnevaluated}` and
  `Event::EntityRemoved`: implemented semantics without a target-test case
  (OQ-2/OQ-5, later units).
- `Executed`: `execute`'s return type; the test reads its fields.

## 8. Decisions and deviations (to fold after review)

The design sketches are not compiling Rust, so most of this section pins what
they left open; items 1-3, 10 and 13-14 amend approved behavior or the design
sketch. On review acceptance these fold into `docs/decisions.md` (as
`ENG-10..`) and the design-doc edits land with that fold, not in this commit.

1. **`Executed` drops `reverted`.** Design 7.2 sketched
   `Executed { warnings, events, reverted: false }`; there is no rollback path
   and the field would always be false. Deviation, recorded.
2. **U3 rev semantics:** clock commands lose `expected_rev` (no campaign
   `rev` on the frozen core document); all U3 entity commands take
   `expected_rev: Option<u64>` as a uniform CAS. Deviation from design 5.5
   (which reserved `expected_rev` for whole-document commands); OQ-1 resolved
   A on 2026-10-18.
3. **`crossed_dawns` returns `i64`** (design 7.6 sketched `u32`), so one call
   represents forward and backward crossings. Deviation, recorded.
4. **Warning surface pinned** (`Warning { code, message, entity, element }`,
   five codes) and `ElementRef` given its two-variant shape; design 7.3/7.4
   named both without shapes. `WarningCode` and `ElementRef` are
   `#[non_exhaustive]`, additive.
5. **`CommandEnv` is U3-minimal and `Send`** (`now()` only); `reference()` and
   `roll()` arrive with U4/U5. Pin of design 7.2.
6. **`WorkingSet` gains `next_feed_id()`;** the design's transition sketch
   assigned feed ids without a source for them. Pin.
7. **`WorkingSet`/`WorkingSetMut` are `Send`; `WorkingSetMut::apply` is
   infallible.** The design's `Send` executor claim needs the first; the
   second removes a divergence path after the store commit (5.1). Pin.
8. **`LoadedCampaign::from_snapshot` and `Executor::state()` pinned;** the
   design sketched the struct and loop without the load seam or a read-back
   accessor. Pin.
9. **Feed and event shapes pinned:** expiry entries (5.2), event order (5.7),
   ring trim and the append cap (5.6), id allocation order, capacity-0 stance
   (OQ-3). Pins of design 7.6/7.9.
10. **Rest scope pinned** to RFC-0001 5.3 bookkeeping plus the optional clock
    move; recovery arithmetic is OQ-5. Pin (with a possible scope change).
11. **Non-const `Formula` maxima** degrade loudly (`FormulaUnevaluated`, no
    silent skip), not silently. Pin; OQ-2 confirms.
12. **Provisional helper paths are a compatibility contract** after the
    `commands/` split (6.4). Pin; design 7.1 already predicted the mechanical
    carry-over.
13. **`clock::transitions` is absorbed by `commands::apply`; `clock` keeps
    only `crossed_dawns`.** The design 7.6 sketch placed the expiry sweep in
    the clock module, but the sweep produces feed entries and document deltas
    the transition owns; keeping `clock` pure arithmetic avoids a second owner
    of transition data. Deviation, recorded.
14. **`Transition` fields are crate-private.** The design 7.2 sketch marked
    `warnings`/`events` public; clients see outcomes only through `Executed`
    after the commit (5.1), so a transition can be neither read nor built
    outside the engine module. Deviation, recorded.
15. **The rev-monotonicity check is an executor assertion.** Design 5.5
    promised the check without a mechanism: the executor asserts
    `new.rev == old.rev + 1` per upsert before persisting and fails fast with
    a diagnostic on violation (engine bug, never a client error; 5.1). Pin.

## 9. Open questions

Every question below states options, tradeoffs and a recommendation; this RFC's
sketch assumes the recommendation. OQ-1 was resolved on 2026-10-18 (option A,
below); the remaining questions are non-blocking.

### OQ-1: campaign-level `expected_rev` (resolved: option A)

Design 5.5/7.2 gives `SetClock`/`AdvanceClock` `expected_rev: Option<u64>`,
but the frozen core `Campaign` has no `rev` field and design 8.1 promises core
gains nothing in this program.

- **A (recommended):** U3 clock commands take no `expected_rev`; U3 entity
  commands take the optional CAS. Campaign-level CAS returns with the server
  (U7) as a separate versioned core proposal (`Campaign.rev` + DATA entry +
  migration) if real concurrent clients make it worth a wire change.
- **B:** add `Campaign.rev` to core now: correct end state, but a wire change,
  version bump, migration and fixtures, contradicting the frozen-core promise
  for a conflict the single writer cannot have today.
- **C:** keep the fields and never check them: dead semantics, rejected.
- **D:** derive a campaign revision from the entity revisions: meaningless as
  a CAS token, rejected.

**Resolved (2026-10-18): A.** The clock is DM-only (RFC-0001 5.3, D10) and the
executor is single-writer, so there is no cross-user race to arbitrate; a stale
DM client's clock move applies on top of newer state (last intent wins), which
is a UI concern, not a data race. B is rejected for U3: no wire change is
justified for a conflict the architecture cannot produce. If stale-intent
protection is ever wanted, campaign CAS returns with U7 as a separate versioned
core proposal (A-revisit).

### OQ-2: non-const `Resource::max` in U3 (non-blocking)

`max` is a `Formula`; evaluation is U4 work, and the design's refill paths
(rests, dawn) need a number.

- **A (recommended):** U3 evaluates `Formula::Const` only. A rest or dawn
  refill of a non-const resource is skipped with `FormulaUnevaluated`;
  `RestoreResource` (no clamp needed for the arithmetic) applies unclamped and
  warns. Nothing is silent.
- **B:** skip silently: rejected (silent mechanics loss, RFC-0001 spirit).
- **C:** ship a U3 mini-evaluator for `Level`/`ProficiencyBonus`/`AbilityMod`:
  duplicates the U4 pipeline and splits formula semantics across units.
- **D:** `Malformed` for a non-const max: rejected; the document is data, not
  client input.

Recommendation: **A**. The code and message can be revisited when U4 lands.

### OQ-3: `feed_capacity == 0` (non-blocking)

- **A (recommended):** treat the capacity as `max(1)`: the ring always keeps
  the newest entry, so the event that just happened is never invisible (a
  single transition that mints several entries keeps only its newest, 5.6).
- **B:** treat it as truly zero: the feed never stores anything; appends are
  dropped (a valid "no feed" configuration, but the engine silently discards
  durable events).
- **C:** refuse zero at a future settings command and on load: bricks stored
  campaigns until the write path exists; the load gate must not refuse files
  the store accepted.

Recommendation: **A**.

### OQ-4: settings range validation (non-blocking)

`CampaignSettings::dawn_hour` documents 0..=23 and core does not validate it;
U3 reads it as `day * 86400 + dawn_hour * 3600`, which is an exact instant for
any `u8` (values above 23 land in the next day).

- **A (recommended):** U3 does not validate; range checking belongs to the
  future settings-write command (and, if wanted, to core constructors through
  the versioned wire process, not to a load gate that would brick files).
- **B:** refuse at load (`Malformed`): the store accepts what the engine
  refuses, splitting the boundary.
- **C:** add core constructor validation now: wire-process work, out of U3.

Recommendation: **A**.

### OQ-5: rest recovery scope (non-blocking)

RFC-0001 5.3 lists rests as "reset resources by recharge type, clear
rest-typed statuses"; 5e also restores HP, resets death saves, reduces
exhaustion and half-restores hit dice.

- **A (recommended):** U3 stays bookkeeping-only; recovery arithmetic lands
  with the rules unit or explicit commands, where the 5e details (hit dice,
  exhaustion) have a home.
- **B:** `LongRest` also sets `hp.current = hp.max` and resets death saves in
  U3: simple stored-field writes, no rules needed, but the scope grows beyond
  the approved line.
- **C:** full 5e recovery in U3: overlaps U4 and expands the test matrix.

Recommendation: **A**.

## 10. Deferred items

- **Implement-phase unit and property tests** (design 9): expiry index equals
  recomputation, transition windows under rewind, dawn crossing counts, feed
  trimming and id allocation under capacity (including transitions that mint
  more entries than the ring holds), command error and warning matrices.
- **Implement-phase edge cases:** rev monotonicity over sequences and the
  executor's assertion (via a test-only transition), `AdvanceClock` overflow
  and no-op moves (`to == old`), already-expired statuses firing only on a
  crossing, multi-status id assignment order.
- **Content and dice environment**: `CommandEnv::reference`/`roll`, the
  content-consuming commands and `Rng` are U4/U5.
- **Campaign-level CAS** for clock/settings writes: OQ-1 A-revisit (with U7).
- **Rest recovery arithmetic and hit dice**: OQ-5, U4+.
- **`EngineError::Unauthorized` producers** and feed visibility resolution
  over audiences: U6.
- **Server DTO mapping** for commands, events and warnings: U7.
