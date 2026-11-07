# RFC: coin trading -- exact purse transfers between statblocks

| | |
|---|---|
| **Status** | Accepted (2026-11-07). The engine command, executor pre-check and wire DTO are implemented; the target test is green. The browser/TUI forms remain deferred client work. This RFC is the separate coin RFC promised by `trading.md` OQ-8. |
| **Scope** | `localtt-engine::commands` (one `Command` variant, the `CoinTransfer` config and the `commands/trading.rs` handler), `localtt-engine::executor` (one `classify` arm), `localtt-protocol::command` (one `CommandDto` variant and its conversion), and the browser/TUI command forms. No core data-model change: coins already live in `StatblockBody::coins`. No new store table, error variant, feed payload or stream event. Target test: `crates/localtt-integration/tests/coin_trading_target.rs` (section 3). |
| **Read with** | `docs/rfcs/trading.md` (the item/entity transfer this mirrors, especially 4.2, 5.3, 6.2 and OQ-8); `docs/rfcs/authz.md` 5.2 and 5.7 (write, DM and the pre-check); `docs/rfcs/engine-executor.md` 5.1-5.5 (commit protocol, revisions, `Transition`); `docs/rfcs/server.md` (generic command route and M4d browser forms); `docs/decisions.md` ENG-36..41 and ARCH-26..27; `crates/localtt-core/DESIGN.md` 3.5 and 4 (the statblock body and the serde contract); `RFC-0001-localtt.md` 4.3 and 5.7. |
| **Pins** | One-way transfer, not a bilateral swap. One purse per command, not a batch. Exact denominations, no conversion or change-making. Every amount field is non-negative and at least one is positive. The source must hold each moved amount; the destination addition is checked for `i32` overflow. Write on the source is enough. Recipient and destination rules mirror `TransferItems`. The transfer is atomic across two statblocks, stamps one revision per sheet, emits two `EntityChanged` events and adds no feed entry. No core change. |

## 1. Goals

1. Let any party member hand coins from a statblock they can write to another
   party member, with the DM able to target a specific statblock of the
   recipient.
2. Mirror `TransferItems` exactly where the asset allows: the same
   recipient/destination resolution, the same non-DM restrictions, the same
   two-revision compare-and-swap, the same atomic two-sheet transition and the
   same `EntityChanged` audit.
3. Keep the money model lossless: a transfer moves the exact denominations the
   actor names; it never invents an exchange rate, rounds a value or changes an
   item's `value` metadata.
4. Reject bad purses before touching either sheet: negative amounts, an all-zero
   amount, insufficient funds and destination overflow are all deterministic
   `Malformed` errors.
5. Pin the engine and wire shapes so the browser and TUI can submit the same
   command through the existing `POST /api/v1/commands` route.
6. Keep the slice small: one command, no new core type, no new store shape and
   no pending-offer model.

## 2. Non-goals

- **No currency conversion or change-making.** A transfer moves the exact `cp`,
  `sp`, `ep`, `gp` and `pp` named. The 5e exchange rates are not applied, and a
  purse is never rebalanced (OQ-1).
- **No coin creation or destruction.** The command only moves existing coins;
  the sum of each denomination across the two sheets is conserved. A DM
  `SetCoins`/`AdjustCoins` command is future work (OQ-9).
- **No item values.** An `InlineItem::value` is item metadata, not a wallet.
  Moving an item through `TransferItems` does not move its value, and
  `TransferCoins` does not touch inventory (OQ-9).
- **No gems, art objects or trade goods.** Those are `InventoryItem`s and move
  through `TransferItems`; only the five coin fields are in scope (OQ-9).
- **No party treasury or shared wallet.** Coins stay on statblocks. A shared
  chest is an NPC/container statblock with write grants; a first-class party
  purse is deferred.
- **No bilateral trade handshake.** There is no offer, acceptance, expiry or
  notification. The transfer is immediate and unilateral; the recipient can
  transfer the coins back (OQ-8).
- **No feed entry.** `EntityChanged` events and revisions are the audit trail
  (OQ-8).
- **No new REST route, WebSocket event or error code.** The generic command
  route, the existing `EntityChanged` event and the existing `EngineError`
  taxonomy carry everything.
- **No core edit.** `Coins` already exists in `StatblockBody`; the arithmetic
  and validation live in the engine.

## 3. Client usage: the target test

`crates/localtt-integration/tests/coin_trading_target.rs` is ordinary client
code. It builds a campaign with one DM and three players (one deliberately has
no character), seeds a `MemoryStore` or a real `SqliteStore`, creates
characters, companions and handouts through `CreateEntity`, seeds purses
through the builder, and drives a real `Executor` through `execute_as`. Twelve
tests:

| Test | What it pins |
|---|---|
| `a_player_transfers_part_of_a_purse_to_another_players_primary_character` | a non-DM actor moves 3 gp and 2 sp from their own sheet to another player; the implicit destination is the recipient's primary PC; every other denomination is untouched; both revisions advance; two `EntityChanged` events arrive in ascending id order; no feed entry |
| `a_whole_denomination_transfer_leaves_zero_and_other_coins_untouched` | moving all of a denomination stores zero in the source field (the field is not removed), leaves the other four fields alone and adds the same amount to the destination |
| `a_dm_can_pick_a_companions_sheet_explicitly` | the DM can name a companion owned by the recipient; a destination owned by someone else is `Malformed` |
| `a_write_grantee_can_transfer_coins_off_a_shared_sheet` | a write grantee, not the owner, can move coins off the shared sheet to their own primary PC; both revisions advance |
| `a_non_dm_cannot_send_to_the_dm_or_choose_a_destination` | a non-DM cannot address the DM, cannot pass an explicit destination, cannot address a player with no character, cannot address a non-member (`PlayerNotFound`) and cannot send to the same sheet it came from; every refusal leaves both purses untouched |
| `stale_revisions_and_bad_amounts_are_refused_atomically` | stale source revision, stale destination revision, an all-zero amount, each negative denomination, and an amount above the source's stored field all fail; the failure kinds are pinned; both sheets keep their revisions and purses |
| `missing_and_non_statblock_endpoints_are_refused` | a missing source or destination is `NotFound { element: None }`; a handout or note as source or explicit destination is `Malformed`; nothing changes |
| `a_destination_overflow_is_refused_atomically` | a destination near `i32::MAX` plus a positive amount is `Malformed`, and the source is not debited |
| `negative_stored_coins_are_not_repaired` | a negative stored denomination is not normalized; a positive move of that denomination is refused as insufficient, while a zero move of it alongside a positive other denomination succeeds |
| `coin_transfers_survive_a_store_reopen` | a real SQLite close/reopen keeps both purses and both revisions on disk |
| `the_wire_dto_round_trips_and_converts_to_the_command` | the `transfer_coins` tag, the `amount` object with all five fields, omitted optional fields, and `CommandDto -> Command` |
| `the_wire_dto_preserves_an_explicit_destination_and_revisions` | `destination`, `expected_source_rev` and `expected_destination_rev` survive serialize/deserialize and conversion |

The target test is the contract; it does not pre-judge the UI. The browser and
TUI surfaces below submit exactly this command.

A representative call:

```rust
let executed = executor.execute_as(
    aria,
    &Command::TransferCoins {
        source: aria_pc,
        amount: Coins {
            gp: 3,
            sp: 2,
            ..Coins::default()
        },
        recipient: bram,
        destination: None,
        expected_source_rev: Some(aria_rev),
        expected_destination_rev: Some(bram_rev),
    },
)?;
```

The test helper `create_purse` is the same `StatblockBuilder` used by
`trading_target.rs`, with `.coins(coins)` set before `.build_entity(...)`.

### 3.1 Browser usage

The web client does not render a coin section yet, so the form lands with that
panel as client work. The shape:

- **Coins section.** A writable sheet gets a *Transfer* action on the purse.
  The form has five numeric fields (`cp`, `sp`, `ep`, `gp`, `pp`), each
  defaulting to 0, bounded below by 0 and above by the source's current value;
  a recipient select built from `CampaignView::roster` (players only for a
  non-DM; all seats for the DM); and a destination select shown only to the DM.
  Submitting sends `TransferCoins`.
- The DM's destination list is the recipient's readable statblocks; leaving it
  blank uses the engine's primary-PC rule.
- The roster already exposes `id`, `display_name` and `role`; players cannot
  read another player's entities, which is why the non-DM destination is
  implicit.

### 3.2 TUI usage

The TUI coins panel gains a `coins` prompt with five denomination fields, a
recipient and an optional destination for the DM. It builds the same engine
`Command`; the wire DTO is irrelevant to the TUI.

## 4. Data model and command surface

### 4.1 Why a separate command

`Coins` is five `i32` fields inside `StatblockBody`, not an `InventoryEntry`
with an id. It has no element id, no quantity field and no per-line state.
Folding coins into `TransferItems` would either require a fake inventory entry
or an optional `coins` payload on an item batch, mixing two error models and
two application paths. A dedicated `TransferCoins` command is smaller,
validates the whole purse in one place, and leaves the item batch untouched.
This is the option `trading.md` OQ-8 deferred; this RFC is that separate coin
RFC.

### 4.2 Coin amounts

One `TransferCoins` command carries one `Coins` value. There is no batch
because a purse is a single value object: moving two denominations is already
one atomic line. The rules:

- Every field (`cp`, `sp`, `ep`, `gp`, `pp`) must be non-negative.
- At least one field must be positive; an all-zero amount is a no-op that would
  only bump revisions and is `Malformed`.
- Zero fields alongside positive fields are allowed and are no-ops for those
  denominations.
- Each positive field must be at most the source's stored field for that
  denomination.
- The destination's stored field plus the amount must fit in `i32`; overflow is
  `Malformed`.
- The command moves the exact denominations named. It does not convert, round,
  clamp, warn or repair a negative stored value.
- The source and destination are each stamped once (`rev + 1`,
  `updated_at = now`) regardless of how many denominations move.
- The source must be a statblock; a note or handout source is `Malformed`. The
  destination must be a statblock too.

`Coins` is signed in the core model. This command treats negative stored values
as data the table already has: it never rewrites them, and a positive move of a
negative denomination is refused because the source cannot cover it. A zero
move of that denomination is harmless. The alternative (clamping or
normalizing) would silently change a sheet during an unrelated transfer (OQ-5).

### 4.3 Recipient and destination

The recipient/destination model is deliberately identical to `TransferItems`:

- `recipient` must be a roster member; otherwise `PlayerNotFound`.
- A non-DM actor may address only a `Role::Player` recipient and may not set
  `destination`.
- The DM may address any roster member, including another DM, and may set a
  destination statblock owned by the recipient.
- With no explicit destination, the recipient's primary character is the `Pc`
  entity owned by the recipient with the earliest `created_at`, then the lowest
  `EntityId`; no such entity is `Malformed`.
- The resolved destination must differ from the source; otherwise `Malformed`.
- The destination does not need to be readable by the actor. The recipient
  receives the coins; the DM may name a sheet the actor cannot read, and a
  non-DM cannot name one.

This keeps the two transfer commands interchangeable in the UI and avoids a
second authorization story for money (OQ-4).

### 4.4 Wire DTO

The engine command type stays free of serde, as always. `Coins` is already a
core serde type, so the protocol variant can carry it directly; no
`CoinAmountDto` is needed. All five fields are present on the wire, including
zeros. This matches `SetDeathSaves`, which carries the core `DeathSaves` value
directly. Optional revisions and `destination` use
`#[serde(default, skip_serializing_if = "Option::is_none")]`, so a hand-written
command can omit them.

A future defaulted `CoinAmountDto` could omit zero denominations, but it would
add a redundant type and conversion for no semantic gain; it is deferred
(OQ-6).

## 5. Public API sketch

### 5.1 Engine command

```rust
// localtt-engine/src/commands/mod.rs

#[non_exhaustive]
pub enum Command {
    // ... existing variants ...

    /// Moves coins from one statblock to a recipient's statblock.
    ///
    /// The actor must be able to write `source`. A non-DM may address only a
    /// `Role::Player` recipient and may not set a destination. A DM may set a
    /// destination statblock owned by the recipient; otherwise the recipient's
    /// primary character receives the coins. Every amount field is
    /// non-negative and at least one is positive. The transfer is one atomic
    /// transition and moves the exact denominations named.
    TransferCoins {
        /// The statblock the coins leave.
        source: EntityId,
        /// The exact per-denomination amounts to move.
        amount: Coins,
        /// The player receiving the coins.
        recipient: PlayerId,
        /// The recipient's statblock, when the DM wants a specific one.
        destination: Option<EntityId>,
        /// Optional compare-and-swap on the source's revision.
        expected_source_rev: Option<u64>,
        /// Optional compare-and-swap on the destination's revision.
        expected_destination_rev: Option<u64>,
    },
}
```

`Coins` is imported from `localtt_core::types`; no new engine type is
re-exported.

### 5.2 Protocol DTO

```rust
// localtt-protocol/src/command.rs

#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandDto {
    // ... existing variants ...

    /// Moves coins to a recipient's statblock.
    TransferCoins {
        source: EntityId,
        amount: Coins,
        recipient: PlayerId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<EntityId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_source_rev: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_destination_rev: Option<u64>,
    },
}
```

`TryFrom<CommandDto> for Command` gains one arm. `Coins` is imported from
`localtt_core::types`; `localtt-protocol/src/lib.rs` needs no new re-export
because `Coins` is a core type and `CommandDto` is already re-exported.

### 5.3 Executor pre-check

`TransferCoins` classifies as `Requirement::Write(source)`, exactly like
`TransferItems`:

```rust
// localtt-engine/src/executor.rs

fn classify(command: &Command) -> Requirement {
    match command {
        // ... existing arms ...
        Command::TransferCoins { source, .. } => Requirement::Write(*source),
    }
}
```

No new `Requirement` variant and no new authz predicate are needed. The
pre-check defers an absent source to the command's `NotFound`; the command
still validates the recipient and the role rules.

### 5.4 Internal command handler

```rust
// localtt-engine/src/commands/trading.rs (private to the commands module)

/// The inputs of a `TransferCoins` command.
#[derive(Debug)]
pub(super) struct CoinTransfer {
    /// The statblock the coins leave.
    pub(super) source: EntityId,
    /// The exact per-denomination amounts to move.
    pub(super) amount: Coins,
    /// The player receiving the coins.
    pub(super) recipient: PlayerId,
    /// The recipient's statblock, when the DM pinned one.
    pub(super) destination: Option<EntityId>,
    /// Optional compare-and-swap on the source's revision.
    pub(super) expected_source_rev: Option<u64>,
    /// Optional compare-and-swap on the destination's revision.
    pub(super) expected_destination_rev: Option<u64>,
}

pub(super) fn transfer_coins_command(
    state: &dyn WorkingSet,
    transfer: CoinTransfer,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError>;
```

The handler lives beside `transfer_items_command` so it can reuse
`recipient_seat`, `resolve_destination` and `primary_character`. `CoinTransfer`
has no lifetime parameter because `Coins` is `Copy`. The actor is required;
`apply` without an actor is `Malformed`, exactly like `TransferItems`. The
handler is pure over `WorkingSet` and returns one `Transition` with two
upserts.

## 6. Semantics

### 6.1 Authorization matrix

| Command | Pre-check | Command-level checks |
|---|---|---|
| `TransferCoins` | actor may write `source` | recipient is on the roster; a non-DM recipient must be `Role::Player`; a non-DM may not set `destination`; an explicit destination must be a statblock owned by `recipient`; the resolved destination must differ from `source`; the amount is non-negative, non-zero and covered by the source; the destination addition does not overflow |

The pre-check runs before any command work and produces the familiar
`Unauthorized { actor, entity: Some(id) }`. The command-level checks reject bad
requests with `Malformed` or `PlayerNotFound`.

`TransferCoins` deliberately does **not** require write access to the
destination. The destination is the recipient's own sheet; the actor is handing
money over, not editing the sheet. A player with a write grant on a shared
chest can distribute its coins, which is no more power than removing the coins
and handing them over by hand. This mirrors `TransferItems` (OQ-2).

### 6.2 Recipient and destination resolution

1. `recipient` must be a roster member; otherwise `PlayerNotFound`.
2. `actor_is_dm = authz::dm(campaign, actor)`.
3. If the actor is not the DM:
   - `recipient` must be `Role::Player`, else `Malformed`;
   - `destination` must be `None`, else `Malformed`.
4. Resolve the destination id without applying revision checks yet:
   - `Some(id)`: use `id`;
   - `None`: the recipient's primary character is the `Pc` entity owned by
     `recipient` with the earliest `created_at`, then the lowest `EntityId` as a
     tie-break; no such entity is `Malformed`.
5. Reject `source == destination` as `Malformed`. This check precedes the
   revision checks, so a self-transfer is a bad request rather than a conflict.
6. Load the destination as a statblock with `expected_destination_rev`; a
   non-statblock is `Malformed`, a missing id is `NotFound`, a stale revision is
   `Conflict`.
7. An explicit destination must have `owner == recipient`; otherwise
   `Malformed`.
8. Load the source as a statblock with `expected_source_rev`.
9. Validate the amount (6.3) before mutating either document.

The revision order is source after destination, matching `TransferItems`.
Either order produces the same single `Conflict` when only one side is stale;
the target test isolates the two cases. If both are stale, the destination wins
because it loads first; the RFC pins that deterministic order.

### 6.3 Amount validation

Validation after both documents load:

1. Every field of `amount` is non-negative; a negative field is `Malformed`.
2. At least one field is positive; an all-zero amount is `Malformed`.
3. For each denomination, `amount <= source.body.coins`; an excess is
   `Malformed`. This also refuses a positive move of a negative stored field,
   because a negative stored field cannot cover a positive amount.
4. For each denomination, `destination.body.coins + amount` does not overflow
   `i32`; an overflow is `Malformed`.
5. No other field of either document is consulted.

The validation is per denomination and independent: a purse may have plenty of
gp but no sp, and a transfer of sp fails even if the gp value would cover it.
That is the exact-denomination rule (OQ-1).

### 6.4 Application

After validation, in one pass:

- For each denomination, subtract `amount` from `source.body.coins`.
- For each denomination, add `amount` to `destination.body.coins`.
- Stamp both documents once (`rev + 1`, `updated_at = now`).
- Return two upserts sorted by entity id.

Because validation proved `amount <= source` and
`destination + amount <= i32::MAX`, the arithmetic cannot underflow or overflow.
The handler does not touch `owner`, `parent`, `acl`, the inventory or any other
body field. `Coins` always has all five fields, so a drained denomination is
stored as zero; there is no "remove" step.

### 6.5 Revision and CAS

- `TransferCoins` exposes two independent optional revisions. A stale source or
  destination revision aborts before any mutation.
- The command stamps exactly one revision per touched statblock. Moving five
  denominations is still one revision on each sheet.
- Failed validation never creates a transition, so failed commands never bump a
  revision and never emit an event.
- The command is not idempotent. A client that retries a committed but
  unacknowledged transfer can move the coins twice; the optional revisions do
  not prevent this because the first commit advanced them. Clients should
  re-read after an ambiguous failure.

### 6.6 Atomicity, events and visibility

- The executor builds one `StatePatch` with both upserts. `MemoryStore`
  validates and swaps a cloned state only on success; `SqliteStore` applies the
  patch in one `BEGIN IMMEDIATE` transaction. Either both sheets change or
  neither does.
- The event list contains one `EntityChanged` per touched statblock, at the new
  revision, sorted by entity id. `TransferCoins` emits two.
- There is no `FeedAppended` event and no campaign change.
- Events are returned only after the store commit (ENG-2), so a client that
  receives them can re-read the new revisions.
- The server broadcasts the same `EntityChanged` events it already broadcasts
  for every other entity command; no new stream event type is needed.
- `EntityRemoved` is not produced. Coins are a body field, not an entity.
- No view field changes: `ViewField::Coins` already gates the purse, and a
  writer sees the source's coins. The destination's coins are not required for
  authorization.

### 6.7 Error model

| Condition | Error | HTTP |
|---|---|---|
| command run without an actor | `Malformed` | 400 |
| an amount field is negative | `Malformed` | 400 |
| the amount is all zero | `Malformed` | 400 |
| an amount exceeds the source's stored field | `Malformed` | 400 |
| the destination addition overflows `i32` | `Malformed` | 400 |
| source resolves to destination | `Malformed` | 400 |
| recipient has no `Pc` and no destination was given | `Malformed` | 400 |
| non-DM passes a destination | `Malformed` | 400 |
| non-DM addresses the DM | `Malformed` | 400 |
| explicit destination is not owned by the recipient | `Malformed` | 400 |
| source or destination is not a statblock | `Malformed` | 400 |
| missing source or destination | `NotFound { element: None }` | 404 |
| recipient not on the roster | `PlayerNotFound` | 404 |
| actor lacks write access to the source | `Unauthorized { entity: Some(id) }` | 403 |
| stale source or destination revision | `Conflict { entity, expected, actual }` | 409 |

The HTTP mapping is the existing `protocol::error` mapping; no new error code is
introduced. There is no `ElementRef::Coins`; an insufficient purse is a
`Malformed` with a reason string, not a `NotFound`.

## 7. Guarantees

- **Safety.** No partial coin move. The command validates both documents and
  the whole amount before mutating, and the store applies one patch atomically.
  A command that fails for any reason leaves both revisions and both purses
  unchanged.
- **Conservation.** Every successful transfer subtracts exactly what it adds,
  per denomination. The command cannot create, destroy or convert coins.
- **Ordering and visibility.** Store first, working set second, events last
  (ENG-2). Upserts and `EntityChanged` events are strictly ascending by entity
  id, satisfying `assert_patch_shape`. No feed entry is appended.
- **Progress.** The command is bounded by the roster size and five field
  operations; there is no retry loop and no client callback. The executor
  remains the single writer, and the store transaction is the only blocking
  step. The command is not lock-free (the executor and SQLite serialize
  writers), but it has no unbounded wait.
- **Concurrency.** Two clients that read the same revision cannot both commit
  when both pin revisions; the second gets `Conflict` and must re-read. The
  single-writer executor serializes the actual commits, so there is no lost
  update between validation and persistence.
- **Error model.** All failures use the existing `EngineError` taxonomy, and
  every condition has exactly one error in section 6.7. Validation order is
  pinned so a request with several problems gets a deterministic error.
- **`Send`/`Sync`.** `Command`, `Coins` and `CoinTransfer` are plain
  `Send + Sync` data. `Executor` stays `Send` and not `Sync`, as before.
- **Additive API.** `Command` and `CommandDto` are `#[non_exhaustive]`; adding
  one variant is additive. No existing public item changes shape.

## 8. Internal structure

### 8.1 Engine

- `commands/mod.rs`: the `TransferCoins` variant and the dispatch arm in
  `apply_as`.
- `commands/trading.rs`: `transfer_coins_command`, the `CoinTransfer` config,
  `validate_amount` and `apply_amount`. Reuse `recipient_seat`,
  `resolve_destination`, `primary_character`, `two_upserts` and
  `load_statblock`. Update the module doc to say "item, entity and coin
  transfers".
- `executor.rs`: the `classify` arm and one more case in the
  `classify_names_a_requirement_for_every_variant` test (the case count becomes
  31).
- No new `Requirement`, no new `authz` predicate and no core edit.

### 8.2 Protocol

- `command.rs`: the `CommandDto::TransferCoins` variant and the `TryFrom` arm.
  No new DTO and no new conversion.
- `tests.rs`: a golden JSON case for `transfer_coins` with all five amount
  fields, omitted optional fields, and a second case with an explicit
  destination and both revisions. The existing `assert_command_json` helper
  already asserts the conversion.
- `lib.rs`: no new export.

### 8.3 Server

- No new route. `POST /api/v1/commands` accepts the new `CommandDto` variant
  through the existing conversion, which is exhaustive and therefore fails to
  compile when a `CommandDto` variant lacks a `Command` arm.
- The actor thread already returns `EntityChanged` events; the browser and TUI
  re-read the statblocks after a transfer.
- `app.rs` and the TUI command parser are hand-written match/parse sites, not
  exhaustive matches over `CommandDto`; the compiler does **not** flag a
  missing UI arm. Adding the command to the browser builders and the TUI parser
  is a deliberate follow-up in the client work.

### 8.4 Browser

- `web/src/commands.rs`: a builder for `TransferCoins`.
- `web/src/app.rs`: the coins-section transfer form described in section 3.1.
  The recipient select uses `CampaignView::roster`; the DM's destination select
  uses the readable statblocks of the selected recipient.
- `web/src/format.rs`: a coin formatter (`5 gp 2 sp`) when the coins panel
  lands; no engine change.
- The web client does not render a coin section yet; the panel and the form
  land together.

### 8.5 TUI

- `localtt-server/src/tui/command.rs`: `TransferCoins` parsing and prompts,
  following the inventory transfer prompt style.
- The TUI uses the same recipient list as the web; the DM-only destination
  prompt is shown only when the actor is the DM.

### 8.6 Tests

- The target test is the fixed point and is not edited after the skeleton.
- Engine unit tests in `commands/trading.rs` cover each handler branch: actor
  required, recipient seat, non-DM role and destination rules, primary-character
  resolution, each amount refusal, destination overflow, conservation, one
  revision per sheet and no feed. The target test covers the non-statblock
  endpoints and the explicit-destination ownership check end to end.
- Protocol unit tests cover the DTO shape, the required amount fields and the
  conversion.
- Browser E2E covers the coins form: a player-to-player transfer, a
  DM-to-companion transfer, a negative/oversized amount surfaced as an error
  and a stale-revision conflict.
- Run `just test`, `just lint` and `just check` after every Rust change.

## 9. Decision log entries added

These were added to `docs/decisions.md` when this RFC was accepted.

- **ENG-39: coin trading is a one-way, exact-denomination purse transfer, not a
  currency exchange.** -- `TransferCoins` moves non-negative per-denomination
  amounts from one statblock to another; no conversion, change-making,
  rounding, minting or destruction. The recipient can transfer the coins back.
  -- Accepted (coin trading slice, 2026-11-07).
- **ENG-40: a coin transfer is atomic across two statblocks and validates the
  whole purse before mutating.** -- At least one denomination is positive;
  every field is non-negative; the source must hold each moved amount; the
  destination addition is checked for `i32` overflow; source and destination
  each stamp one revision in one transition; optional revisions guard each
  side. -- Accepted (coin trading slice, 2026-11-07).
- **ENG-41: coin transfer authorization is write-on-source with the
  item-transfer recipient rules.** -- A write grantee can move coins off a
  shared sheet; a non-DM may address only a `Role::Player` recipient and may
  not pick a destination; the DM may pick a destination owned by the recipient;
  the destination defaults to the recipient's primary character. -- Accepted
  (coin trading slice, 2026-11-07).
- **ARCH-27: coin trading reuses the generic command route and
  `EntityChanged` events; it adds no feed entry or stream event.** --
  `POST /api/v1/commands` carries the new DTO variant through the existing
  conversion; clients re-read the touched statblocks. -- Accepted (coin trading
  slice, 2026-11-07).

## 10. Open Questions

Every question states options, tradeoffs and a recommendation. OQ-1, OQ-2 and
OQ-4 can change the command surface enough to block the skeleton; the rest are
non-blocking if the recommendation is accepted.

### OQ-1: Exact denominations or automatic conversion?

- **A (recommended):** move the exact denominations named. Every field is
  independent; no exchange rate, no change-making, no rounding. The table can
  exchange coins manually or with a future command. The command conserves each
  denomination across the two sheets.
- **B:** convert to a canonical value (cp) and make change using the 5e rates.
  Convenient for "give 1 gp" when the recipient's purse is in sp, but
  introduces rounding, variant-rate policy and a hidden exchange rate; the
  audit becomes harder.
- **C:** accept a `value_cp` and let the engine choose denominations from the
  source. Same issues as B plus a denomination-selection policy.

Recommendation: **A**. A transfer should be lossless and predictable; exchange
is a separate feature.

### OQ-2: Write or owner authorization?

- **A (recommended):** write on the source, mirroring `TransferItems`. A write
  grantee can already edit items and HP; moving coins off a shared sheet is no
  more power. The recipient/destination rules still apply.
- **B:** owner-or-DM, mirroring `TransferEntity`. Safer for money, but
  inconsistent with items and blocks a write grantee from distributing a
  shared chest's coins.

Recommendation: **A**. Consistency with the existing transfer is worth more
than the marginal extra protection; the DM can repair any mistake.

### OQ-3: Negative and zero amounts?

- **A (recommended):** reject any negative field; require at least one positive
  field; allow zero fields alongside positives. Negative amounts would invert
  the transfer direction and create a theft vector; an all-zero amount is a
  no-op that only bumps revisions.
- **B:** allow negatives as a shorthand for the reverse transfer. Convenient
  for a two-sided trade UI, but ambiguous and unsafe.
- **C:** allow all-zero as a no-op. Wastes a revision and an event.

Recommendation: **A**.

### OQ-4: Recipient and destination rules?

- **A (recommended):** mirror `TransferItems`: recipient on the roster; non-DM
  recipient must be `Role::Player`; non-DM may not set a destination; explicit
  destination must be owned by the recipient; implicit destination is the
  recipient's primary character; source and destination must differ.
- **B:** allow a non-DM to name any statblock they own as destination. Solves
  the intra-owner gap, but diverges from items and needs a new authorization
  branch.
- **C:** allow the recipient to be any roster member and the destination any
  statblock. Too broad; a non-DM could inject coins into a shared chest or
  another player's sheet.

Recommendation: **A** for this slice; B is a paired follow-up for items and
coins together (OQ-10).

### OQ-5: Overflow and negative stored values?

- **A (recommended):** validate `amount <= source` per denomination and
  `destination + amount` with `checked_add`; overflow is `Malformed`. A
  negative stored value is not repaired; a positive move of that denomination
  is refused as insufficient, while a zero move of it is allowed. The command
  never clamps or warns.
- **B:** saturate the destination at `i32::MAX` and warn. Silently changes the
  amount; bad for an audit trail.
- **C:** use `i64` or `u32` in core. A core data-model change, out of scope.

Recommendation: **A**. The command is a transfer, not a data repair.

### OQ-6: Wire shape?

- **A (recommended):** use `Coins` directly in `CommandDto::TransferCoins`; all
  five fields are present on the wire. `Coins` is already serde and
  `SetDeathSaves` uses a core value type directly. No new DTO or conversion.
- **B:** add a `CoinAmountDto` with `#[serde(default)]` and zero-skipping.
  Smaller hand-written JSON, but a redundant type and conversion. It can be
  added later without breaking the engine.

Recommendation: **A** for this slice.

### OQ-7: Should coins and items move in one command?

- **A (recommended):** no. A coin transfer is a separate command; a "trade
  bundle" (items + coins + maybe an entity) is a later command family with its
  own atomicity and UI. Keeping them separate makes each command's validation
  and error model small.
- **B:** extend `TransferItems` with an optional `coins: Option<Coins>` field.
  One round trip for a loot handover, but mixes two payloads and complicates
  the item batch's error precedence.

Recommendation: **A**.

### OQ-8: Feed entry, notification or recipient acceptance?

- **A (recommended):** no. `EntityChanged` events and revisions are the audit;
  the recipient can transfer the coins back. Same as items.
- **B:** append a `FeedPayload::Transfer` entry. Better table awareness, but a
  core version bump and audience/rendering work.
- **C:** a pending offer/accept flow. Much larger; same arguments as
  `trading.md` OQ-3.

Recommendation: **A**.

### OQ-9: Coin minting/destruction and non-coin valuables?

- **A (recommended):** no. `TransferCoins` only moves existing coins between
  statblocks; it never creates or destroys them. Gems, art objects and trade
  goods are `InventoryItem`s and move through `TransferItems`. A DM
  `SetCoins`/`AdjustCoins` command is separate future work.
- **B:** let the DM use negative amounts to remove coins or add an `AdjustCoins`
  command now. More complete, but another command surface and validation.

Recommendation: **A**.

### OQ-10: Can a non-DM move coins between their own statblocks?

- **A (recommended):** mirror items; no explicit destination for a non-DM. A
  move from their companion to their primary PC works (the implicit
  destination differs); a move from their primary PC to their companion needs
  the DM. A future OQ-4 B change can lift this for both items and coins.
- **B:** allow a non-DM to name a statblock they own. Small, safe and solves
  the gap, but diverges from the current item command.
- **C:** add a separate `MoveCoins` command. More surface for the same
  primitive.

Recommendation: **A** for this slice, with B as a paired follow-up.

## 11. Deferred items

- The browser coins panel and transfer form, and the TUI `coins` prompt
  (section 12, step 6); the engine and wire command already accept them.
- Currency exchange and change-making (OQ-1 B).
- A combined item-and-coin trade bundle (OQ-7 B).
- A DM `SetCoins`/`AdjustCoins` command for minting, destroying or correcting a
  purse (OQ-9 B).
- A transfer feed payload, table announcement or notification (OQ-8 B).
- A bilateral offer/accept flow for trades (OQ-8 C).
- A non-DM explicit destination for a statblock they own, applied to both
  `TransferItems` and `TransferCoins` (OQ-4 B / OQ-10 B).
- A first-class party treasury or shared wallet; today a shared chest is an
  NPC/container statblock with write grants.
- Rate limiting or spam controls for repeated transfers, if play shows a need.
- A server-side transfer history view; the `EntityChanged` revisions are the
  audit for now.

## 12. Implementation plan

1. **Approve this RFC and resolve the open questions.** OQ-1, OQ-2 and OQ-4 are
   the blocking ones.
2. **Write the target test.** `crates/localtt-integration/tests/coin_trading_target.rs`
   is the fixed point. It will not compile until the skeleton lands.
3. **Skeleton.** Add the `TransferCoins` variant, `CoinTransfer`,
   `transfer_coins_command` with a `todo!()` body, the dispatch arm, the
   `classify` arm and the protocol variant/conversion. Confirm the target test
   fails at `todo!()`.
4. **Spike.** Implement the happy path (a player to another player's primary
   character) and the first refusal. Stop for review.
5. **Complete.** Add the amount validation, overflow check, DM destination,
   write-grantee path, persistence and engine unit tests. The target test goes
   green.
6. **Client forms.** Add the browser coins panel and transfer form, and the TUI
   `coins` prompt; add the browser E2E journey.
7. **Verify and record.** Run `just test`, `just lint` and `just check`; apply
   the decision log entries in section 9.

## 13. Implementation status

Engine and protocol implemented (2026-11-07). `Command::TransferCoins`,
`CoinTransfer`, `transfer_coins_command`, the `classify` arm and the
`CommandDto` conversion are in place; the target test passes all twelve cases.
Engine unit tests cover the happy path, actor/recipient rules and the amount
refusals, and protocol golden JSON covers both wire shapes. The browser coins
panel and the TUI `coins` prompt remain deferred client work (section 12, step
6); the decision log entries in section 9 are applied.
