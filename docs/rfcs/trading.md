# RFC: trading -- item transfers and ownership handovers

| | |
|---|---|
| **Status** | Spec phase (2026-11-03). This RFC and `crates/localtt-integration/tests/trading_target.rs` are the first trading artifacts. No library code exists: the target test intentionally does not compile until `Command::TransferItems`, `Command::TransferEntity`, `ItemMove` and `ItemMoveDto` land, then fails with `not yet implemented` until the implement phase. |
| **Scope** | `localtt-engine::commands` (two command variants, `ItemMove`, the `commands/trading.rs` handlers and the `Owner` pre-check), `localtt-engine::executor` (the new requirement), `localtt-protocol::command` (the two DTOs and conversions), and the browser/TUI command forms that submit them. No core data-model change: an item move is expressed with the existing `InventoryEntry`, and an ownership move only touches the existing `owner` header field. Target test: `crates/localtt-integration/tests/trading_target.rs`. |
| **Read with** | `RFC-0001-localtt.md` sections 4.9 (handouts), 5.7 (authorization), 5.12 (notes, handouts, summons) and 9 (open product questions); `docs/rfcs/authz.md` sections 5.2 and 5.7 (write, owner, DM and the pre-check); `docs/rfcs/engine-executor.md` sections 5.1-5.5 (commit protocol, revisions, `Transition`); `docs/rfcs/server.md` (M3 inventory/handout slice and M4d browser forms); `docs/decisions.md` ENG-24, ENG-33, ARCH-22; `crates/localtt-core/DESIGN.md` sections 6 and 7 (statblock/inventory shapes). |
| **Pins** | One-way transfer, not a bilateral swap. A batch of item moves is atomic. Destination entries are fresh and reset equipment/attunement. Entity ownership transfer is owner-or-DM and does not rewrite ACLs or cascade to summons. No feed entry in the first slice. The target test is the fixed point for every one of these. |

## 1. Goals

1. Let any party member hand inventory items to another party member, with the
   DM able to target a specific statblock of the recipient.
2. Let any party member hand an owned entity (character, companion, NPC,
   handout, note, summon, ...) to another party member, with the owner-or-DM
   rule rather than a mere write grant.
3. Make an item transfer atomic: either every move in the batch lands on both
   sheets, or neither sheet changes.
4. Keep optimistic concurrency honest across two entities: the source and the
   destination each carry an optional expected revision, and a stale revision
   aborts the whole command.
5. Pin the engine and wire shapes so the browser and TUI can submit the same
   command through the existing `POST /api/v1/commands` route.
6. Preserve the existing authorization model: write access is enough to move
   items off a sheet; ownership is required to move the entity itself.
7. Keep the first slice small enough to ship without a new core payload or a
   pending-transaction model.

## 2. Non-goals

- **No bilateral trade handshake.** There is no `OfferTrade`, no pending
  acceptance, no expiry and no notification. A transfer is immediate and
  unilateral; the recipient can transfer the asset back.
- **No coin transfers.** `Coins` is five integers, not an inventory entry with
  an id; moving money needs its own command and validation (OQ-8).
- **No ACL editing.** `TransferEntity` changes `owner` only. Explicit `AclEntry`
  grants stay exactly as they were (OQ-4).
- **No cascading ownership changes.** Transferring a PC does not move its
  familiar, summon or companion; each is a separate `TransferEntity` (OQ-5).
- **No feed entry.** The existing `EntityChanged` events and revisions are the
  audit trail for this slice (OQ-9).
- **No handout-copy change.** `GiveHandout` stays DM-only and keeps copying an
  item without consuming the handout. A handout *entity* moves through
  `TransferEntity` (OQ-7).
- **No new REST route or WebSocket event type.** The generic command route and
  the existing `EntityChanged` event already carry everything.
- **No core edits.** A moved item is still an `InventoryEntry`; a transferred
  entity only changes `owner`, `rev` and `updated_at`.

## 3. Client usage: the target test

`crates/localtt-integration/tests/trading_target.rs` is ordinary client code.
It builds a campaign with one DM and three players (one deliberately has no
character), seeds a `MemoryStore` or a real `SqliteStore`, creates characters,
companions, summons and handouts through `CreateEntity`, and drives a real
`Executor` through `execute_as`. Thirteen tests:

| Test | What it pins |
|---|---|
| `a_player_splits_a_stack_to_another_players_primary_character` | a non-DM actor moves 2 of 3 from their own sheet to another player; the implicit destination is the recipient's primary PC; the source keeps its equipment state and attunement; the destination gets a fresh entry id, `Carried`, `attuned = false`, copied notes; both revisions advance; two `EntityChanged` events arrive in ascending id order; no feed entry |
| `a_whole_stack_moves_and_the_source_entry_disappears` | a full-quantity move removes the source entry rather than leaving a zero row; the destination is still a fresh carried, unattuned entry |
| `a_dm_moves_a_character_and_the_old_owner_loses_write` | `TransferEntity` by the DM; owner and revision move; the former owner is `Unauthorized`; the new owner writes; an explicit ACL grant on the entity survives unchanged |
| `a_write_grantee_cannot_transfer_ownership` | write access is not ownership: a grantee can add an item but cannot `TransferEntity`; the owner can |
| `a_non_dm_cannot_send_to_the_dm_or_choose_a_destination` | a non-DM cannot address the DM, cannot pass an explicit destination, cannot address a player with no character, cannot address a non-member (`PlayerNotFound`), and cannot send an item to the same sheet it came from; every refusal leaves both sheets untouched |
| `stale_revisions_and_bad_moves_are_refused_atomically` | stale source revision, stale destination revision, empty moves, zero/negative/oversized quantities, duplicate entry ids and a missing entry all fail; the failure kinds and payloads are pinned; both sheets keep their revisions and inventories |
| `the_dm_can_pick_a_companions_sheet_explicitly` | the DM can name a companion owned by the recipient; a destination owned by someone else is `Malformed` |
| `transfers_to_self_and_to_the_current_owner_are_malformed` | an item move whose source and destination resolve to the same statblock is `Malformed`; an entity transfer to its current owner is `Malformed`, including a repeat after a successful move |
| `a_split_charged_item_gets_a_fresh_resource_id_on_the_destination` | splitting an inline item with a nested `Resource` regenerates the destination's `ResourceId`; the source keeps the original; charges and maximum are copied |
| `a_handout_moves_but_its_content_is_unchanged` | a handout entity moves from its owner to another player; the body is untouched and the revision advances |
| `entity_transfers_do_not_cascade_to_a_summon` | transferring a PC leaves the summon with the old owner and keeps the `parent` link |
| `transfers_survive_a_store_reopen` | a split item move and an ownership move both survive a real SQLite close/reopen: quantities, states, attunement and owner are on disk |
| `the_wire_dto_round_trips_and_converts_to_the_command` | `transfer_items` and `transfer_entity` tags, the `moves` array shape, omitted optional fields, the `ItemMoveDto -> ItemMove` conversion, and `CommandDto -> Command` |

The target test is the contract; it does not pre-judge the UI. The browser and
TUI surfaces below submit exactly these commands.

### 3.1 Browser usage

- **Inventory row (`StatblockPanel`).** A writable sheet gets a *Transfer*
  action on each inventory row. The form has a quantity (default 1, bounded by
  the row's quantity), a recipient select built from
  `CampaignView::roster` (players only for a non-DM; all seats for the DM), and
  a destination select shown only to the DM. The DM's destination list is the
  recipient's readable statblocks; leaving it blank uses the engine's primary
  PC rule. Submitting sends `TransferItems`.
- **Entity header.** The header gets a *Transfer ownership* action when the
  viewer is the owner or the DM (`EntityView::writable && (viewer == owner ||
  viewer is DM)`). The form has a recipient select and sends `TransferEntity`.
- **Handout item view.** The existing *Give handout* form stays DM-only. The
  new ownership action is separate.
- The roster already exposes `id`, `display_name` and `role`; players cannot
  read another player's entities, which is why the non-DM destination is
  implicit.

### 3.2 TUI usage

The TUI inventory row gains a `transfer` prompt (quantity, recipient, optional
destination for the DM), and the entity panel gains an `owner` prompt (recipient).
Both build the same engine `Command`; the wire DTO is irrelevant to the TUI.

## 4. Data model and command surface

### 4.1 Why one-way transfer

A bilateral swap needs a pending-offer document, acceptance/decline commands,
expiry, notification, conflict handling when either side changes, and a
decision about what the table sees while an offer is open. None of that is
needed to satisfy "hand an item to another player". The recipient can hand it
back with the same command, and the DM can repair any mistake. The first slice
pins one-way transfer; a handshake is a later feature (OQ-3).

### 4.2 Item moves

One `TransferItems` command carries a batch of `ItemMove` lines. A batch is
atomic and lets the table move a loot bundle in one action. The rules:

- `moves` must be non-empty.
- Every `entry` must exist on the source sheet and name a distinct entry.
- Every `qty` must be positive and at most the stored quantity.
- A full-quantity move removes the source entry; a partial move decrements it.
- Every moved line becomes one new destination entry:
  - fresh `InventoryEntryId`;
  - the same `InventoryItem` (reference or inline clone);
  - `qty` from the move;
  - `state = Carried`;
  - `attuned = false`;
  - `notes` copied from the source entry.
- If an inline item carries a nested `Resource`, the destination copy gets a
  fresh `ResourceId`. The source keeps its own id. This keeps element ids unique
  across entities and makes a split of a charged stack safe.
- The source and destination are each stamped once (`rev + 1`,
  `updated_at = now`) regardless of how many lines the batch has.
- The source must be a statblock; a note or handout source is `Malformed`.

### 4.3 Entity ownership moves

`TransferEntity` changes the entity header only:

- `owner` becomes the recipient;
- `rev` advances by one and `updated_at` is stamped;
- `kind`, body, `parent`, `acl`, `created_at` and every element id stay as they
  were;
- no child entity is visited.

The actor must be the current owner or the DM. A write grantee is not enough.

### 4.4 Wire DTOs

The engine command types stay free of serde, following the existing
`StatusSpec`/`StatusSpecDto` split. The protocol owns `ItemMoveDto` and the two
`CommandDto` variants. Optional revisions and `destination` use
`#[serde(default, skip_serializing_if = "Option::is_none")]`, so a hand-written
command can omit them and the JSON stays small.

## 5. Public API sketch

### 5.1 Engine commands

```rust
// localtt-engine/src/commands/mod.rs

/// One inventory entry, or part of one, moved by `TransferItems`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemMove {
    /// The source entry.
    pub entry: InventoryEntryId,
    /// How many units to move; positive and at most the stored quantity.
    pub qty: i32,
}

#[non_exhaustive]
pub enum Command {
    // ... existing variants ...

    /// Moves inventory lines from one statblock to a recipient's statblock.
    ///
    /// The actor must be able to write `source`. A non-DM may address only a
    /// `Role::Player` recipient and may not set a destination. A DM may set a
    /// destination statblock owned by the recipient; otherwise the recipient's
    /// primary character receives the items. The whole batch is one atomic
    /// transition.
    TransferItems {
        /// The statblock the items leave.
        source: EntityId,
        /// The lines to move; non-empty, distinct entries, positive quantities.
        moves: Vec<ItemMove>,
        /// The player receiving the items.
        recipient: PlayerId,
        /// The recipient's statblock, when the DM wants a specific one.
        destination: Option<EntityId>,
        /// Optional compare-and-swap on the source's revision.
        expected_source_rev: Option<u64>,
        /// Optional compare-and-swap on the destination's revision.
        expected_destination_rev: Option<u64>,
    },

    /// Moves an entity's ownership to another player.
    ///
    /// The actor must be the entity's owner or the DM. A non-DM may address
    /// only a `Role::Player` recipient. ACL grants, the body, the parent link
    /// and child entities are untouched.
    TransferEntity {
        /// The entity whose owner changes.
        entity: EntityId,
        /// The player who becomes the owner.
        recipient: PlayerId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
}
```

`ItemMove` is re-exported from `localtt_engine::commands` alongside the existing
command payload types.

### 5.2 Protocol DTOs

```rust
// localtt-protocol/src/command.rs

/// One line of a `transfer_items` command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemMoveDto {
    /// The source entry.
    pub entry: InventoryEntryId,
    /// How many units to move.
    pub qty: i32,
}

impl From<ItemMoveDto> for ItemMove {
    fn from(dto: ItemMoveDto) -> Self {
        Self {
            entry: dto.entry,
            qty: dto.qty,
        }
    }
}

#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandDto {
    // ... existing variants ...

    /// Moves inventory lines to a recipient's statblock.
    TransferItems {
        source: EntityId,
        moves: Vec<ItemMoveDto>,
        recipient: PlayerId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<EntityId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_source_rev: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_destination_rev: Option<u64>,
    },

    /// Moves an entity's ownership to another player.
    TransferEntity {
        entity: EntityId,
        recipient: PlayerId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_rev: Option<u64>,
    },
}
```

`TryFrom<CommandDto> for Command` gains both arms; `ItemMoveDto` converts with
`From`. `ItemMoveDto` is plain and constructible (no `#[non_exhaustive]`), so
clients and the target test can build it. `localtt-protocol/src/lib.rs` re-exports
`ItemMoveDto` from the crate root next to `CommandDto`; the `command` module
itself stays private. The target test reaches it as
`localtt_server::protocol::ItemMoveDto` through the server's existing
`pub use localtt_protocol as protocol`.

### 5.3 Executor pre-check

`TransferItems` classifies as `Requirement::Write(source)`. `TransferEntity`
needs a new private classification:

```rust
// localtt-engine/src/executor.rs

#[derive(Debug, PartialEq, Eq)]
enum Requirement {
    Dm,
    Member,
    Read(EntityId),
    Write(EntityId),
    /// The actor must be the entity's owner or the campaign DM.
    Owner(EntityId),
}

fn classify(command: &Command) -> Requirement {
    match command {
        // ... existing arms ...
        Command::TransferItems { source, .. } => Requirement::Write(*source),
        Command::TransferEntity { entity, .. } => Requirement::Owner(*entity),
    }
}
```

`pre_check` for `Owner` is the same shape as `Write`: an absent entity defers
to the command's `NotFound`; otherwise the actor passes when the campaign DM
check holds or `entity.owner() == actor`. The command still validates the
recipient and the role rules.

### 5.4 Internal command handlers

```rust
// localtt-engine/src/commands/trading.rs (private to the commands module)

/// The inputs of a `TransferItems` command, grouped to keep the handler small.
pub(super) struct ItemTransfer {
    pub(super) source: EntityId,
    pub(super) moves: Vec<ItemMove>,
    pub(super) recipient: PlayerId,
    pub(super) destination: Option<EntityId>,
    pub(super) expected_source_rev: Option<u64>,
    pub(super) expected_destination_rev: Option<u64>,
}

/// The inputs of a `TransferEntity` command.
pub(super) struct EntityTransfer {
    pub(super) entity: EntityId,
    pub(super) recipient: PlayerId,
    pub(super) expected_rev: Option<u64>,
}

pub(super) fn transfer_items_command(
    state: &dyn WorkingSet,
    transfer: ItemTransfer,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError>;

pub(super) fn transfer_entity_command(
    state: &dyn WorkingSet,
    transfer: EntityTransfer,
    actor: Option<PlayerId>,
    env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError>;
```

The two config structs keep the handlers inside the repository's five-argument
rule; every field is a distinct command input. The actor is required; `apply`
without an actor is `Malformed`, exactly like `CreateEntity`. The handlers are
pure over `WorkingSet` and return one `Transition` with two upserts (item moves)
or one upsert (ownership).

## 6. Semantics

### 6.1 Authorization matrix

| Command | Pre-check | Command-level checks |
|---|---|---|
| `TransferItems` | actor may write `source` | recipient is on the roster; a non-DM recipient must be `Role::Player`; a non-DM may not set `destination`; an explicit destination must be a statblock owned by `recipient`; the resolved destination must differ from `source` |
| `TransferEntity` | actor is the DM or the entity's owner | recipient is on the roster; a non-DM recipient must be `Role::Player`; `recipient` must differ from the current owner |

The pre-check runs before any command work and produces the familiar
`Unauthorized { actor, entity: Some(id) }`. The command-level checks reject bad
requests with `Malformed` or `PlayerNotFound`.

`TransferItems` deliberately does **not** require write access to the
destination. The destination is the recipient's own sheet; the actor is handing
an item over, not editing the sheet. A player with a write grant on a shared
chest can distribute its contents, which is no more power than removing the
items and handing them over by hand.

### 6.2 Recipient and destination resolution

1. `recipient` must be a roster member; otherwise `PlayerNotFound`.
2. `actor_is_dm = authz::dm(campaign, actor)`.
3. If the actor is not the DM:
   - `recipient` must be `Role::Player`, else `Malformed`;
   - `destination` must be `None`, else `Malformed`.
4. Resolve the destination id without applying revision checks yet:
   - `Some(id)`: use `id`;
   - `None`: the recipient's **primary character** is the `Pc` entity owned by
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

The revision order is source after destination, but either order produces the
same single `Conflict` for the target test's cases: each test leaves the other
entity's revision valid.

### 6.3 Item move rules

Validation after both documents load:

1. `moves` is non-empty.
2. Every `qty` is positive.
3. Every `entry` id is distinct within the batch.
4. Every `entry` exists on the source; a miss is
   `NotFound { entity: source, element: Some(ElementRef::Item(entry)) }`.
5. Every `qty` is at most the stored entry's quantity.

Application, in move order:

- If `qty == stored.qty`, remove the source entry.
- Otherwise subtract `qty` from the source entry.
- Push the destination entry with a fresh id, the cloned item, `qty`,
  `ItemState::Carried`, `attuned = false`, and the source notes.
- For an inline item with a nested `Resource`, regenerate `resource.id` on the
  destination copy.

The destination entries are appended in move order. The two documents are
stamped once each and returned as two upserts sorted by entity id.

### 6.4 Entity transfer rules

1. Load the entity with `expected_rev`; missing is `NotFound`, stale is
   `Conflict`.
2. `recipient` must be on the roster; otherwise `PlayerNotFound`.
3. `recipient` must differ from `entity.owner()`; otherwise `Malformed`.
4. If the actor is not the DM, `recipient` must be `Role::Player`;
   otherwise `Malformed`.
5. Set `owner = recipient`, stamp `rev` and `updated_at`, and return one upsert.

The pre-check already guarantees owner-or-DM. A non-DM owner can transfer their
own entity to another player; the DM can transfer any entity to any roster
member, including themselves.

### 6.5 Revision and CAS

- `TransferItems` exposes two independent optional revisions. A stale source or
  destination revision aborts before any mutation.
- `TransferEntity` exposes one optional revision.
- Both commands stamp exactly one revision per touched entity. A batch of ten
  moves is still one revision on each sheet.
- Failed validation never creates a transition, so failed commands never bump a
  revision and never emit an event.

### 6.6 Atomicity, events and visibility

- The executor builds one `StatePatch` with both upserts. `MemoryStore` validates
  and swaps a cloned state only on success; `SqliteStore` applies the patch in
  one `BEGIN IMMEDIATE` transaction. Either both sheets change or neither does.
- The event list contains one `EntityChanged` per touched entity, at the new
  revision, sorted by entity id. `TransferItems` emits two; `TransferEntity`
  emits one. There is no `FeedAppended` event and no campaign change.
- Events are returned only after the store commit (ENG-2), so a client that
  receives them can re-read the new revisions.
- The server broadcasts the same `EntityChanged` events it already broadcasts
  for every other entity command; no new stream event type is needed.
- `EntityRemoved` is not produced. The source inventory entry is an element,
  not an entity.

### 6.7 Error model

| Condition | Error | HTTP |
|---|---|---|
| command run without an actor | `Malformed` | 400 |
| empty `moves` | `Malformed` | 400 |
| `qty <= 0` | `Malformed` | 400 |
| `qty > stored qty` | `Malformed` | 400 |
| duplicate `entry` in `moves` | `Malformed` | 400 |
| source resolves to destination | `Malformed` | 400 |
| recipient has no `Pc` and no destination was given | `Malformed` | 400 |
| non-DM passes a destination | `Malformed` | 400 |
| non-DM addresses the DM | `Malformed` | 400 |
| explicit destination is not owned by the recipient | `Malformed` | 400 |
| entity transfer to the current owner | `Malformed` | 400 |
| source or destination is not a statblock | `Malformed` | 400 |
| missing source, destination or entity | `NotFound { element: None }` | 404 |
| missing inventory entry | `NotFound { element: Item(id) }` | 404 |
| recipient not on the roster | `PlayerNotFound` | 404 |
| actor lacks write or ownership | `Unauthorized { entity: Some(id) }` | 403 |
| stale source or destination revision | `Conflict { entity, expected, actual }` | 409 |

The HTTP mapping is the existing `protocol::error` mapping; no new error code is
introduced.

## 7. Guarantees

- **Safety.** No partial item move and no partial ownership move. The command
  validates both documents before mutating, and the store applies one patch
  atomically. A command that fails for any reason leaves both revisions and
  both inventories unchanged.
- **Ordering and visibility.** Store first, working set second, events last
  (ENG-2). Upserts and `EntityChanged` events are strictly ascending by entity
  id, satisfying `assert_patch_shape`. No feed entry is appended.
- **Progress.** The command is bounded by the roster size, the source inventory
  size and the move count; there is no retry loop and no client callback. The
  executor remains the single writer, and the store transaction is the only
  blocking step. The command is not lock-free (the executor and SQLite
  serialize writers), but it has no unbounded wait.
- **Concurrency.** Two clients that read the same revision cannot both commit
  when both pin revisions; the second gets `Conflict` and must re-read. The
  single-writer executor serializes the actual commits, so there is no lost
  update between validation and persistence.
- **Error model.** All failures use the existing `EngineError` taxonomy, and
  every condition has exactly one error in section 6.7. Validation order is
  pinned so a request with several problems gets a deterministic error.
- **`Send`/`Sync`.** `Command`, `ItemMove`, `ItemMoveDto` and the two DTO
  variants are plain `Send + Sync` data. `Executor` stays `Send` and not
  `Sync`, as before.
- **Additive API.** `Command` and `CommandDto` are `#[non_exhaustive]`; adding
  variants and `ItemMove` is additive. No existing public item changes shape.

## 8. Internal structure

### 8.1 Engine

- `commands/mod.rs`: `mod trading;`, the `ItemMove` type, the two `Command`
  variants, dispatch arms in `apply_as`, and the `pub use` for `ItemMove`.
- `commands/trading.rs`: `transfer_items_command`,
  `transfer_entity_command`, `resolve_destination`, `primary_character`,
  `validate_moves`, `apply_moves`, and the private `TransferredItem` mapping.
  The module is pure over `WorkingSet`, like the other command modules.
- `executor.rs`: `Requirement::Owner`, the two `classify` arms, and the
  owner-or-DM branch in `pre_check`.
- `authz`: no new public predicate is required; the executor can use
  `authz::dm` plus `entity.owner() == actor`. If the check grows, add a
  crate-internal `authz::owns` rather than exposing a new public API.

### 8.2 Protocol

- `command.rs`: `ItemMoveDto`, the two `CommandDto` variants, the
  `From<ItemMoveDto> for ItemMove` conversion and the two `TryFrom` arms.
- No new error DTO: `PlayerNotFound`, `Malformed`, `Conflict` and
  `Unauthorized` already map to HTTP responses.
- `tests.rs`: a serialized shape test for both tags, the optional-field
  omission and the conversion.

### 8.3 Server

- No new route. `POST /api/v1/commands` accepts the new `CommandDto` variants
  through the existing conversion.
- The actor thread already returns `EntityChanged` events; the browser and TUI
  re-read the entities after a transfer.
- `app.rs` and the TUI command parser add the two commands to their match arms;
  a missing arm is a compile error, which is the intended exhaustiveness check.

### 8.4 Browser

- `web/src/commands.rs`: builders for `TransferItems` and `TransferEntity`.
- `web/src/app.rs`: the inventory-row transfer form and the entity-header
  ownership form described in section 3.1. The recipient select uses
  `CampaignView::roster`; the DM's destination select uses the readable
  statblocks of the selected recipient.
- `web/src/format.rs`: no new formatter is required; item names and roster
  names already exist.

### 8.5 TUI

- `localtt-server/src/tui/command.rs`: `TransferItems` and `TransferEntity`
  parsing and prompts, following the inventory and handout prompt style.
- The TUI uses the same recipient list as the web; the DM-only destination
  prompt is shown only when the actor is the DM.

### 8.6 Tests

- The target test is the fixed point and is not edited after the skeleton.
- Engine unit tests in `commands/trading.rs` cover each branch:
  authorization boundaries, destination resolution, each malformed move,
  duplicate entries, missing entries, resource regeneration, ACL preservation
  and no-cascade.
- Protocol unit tests cover the DTO shape and the conversions.
- Browser E2E covers the two forms: a player-to-player split, a DM-to-companion
  transfer, a DM ownership move, and a stale-revision conflict surfaced as an
  error.
- Run `just test`, `just lint` and `just check` after every Rust change.

## 9. Decision log entries to add

These are proposed for `docs/decisions.md` when the slice lands; they are not
applied by this spec.

- **ENG-36: trading is one-way transfer, not a bilateral swap.** -- `TransferItems`
  and `TransferEntity` move an asset immediately; the recipient can move it
  back. A pending-offer model stays future work. -- Proposed (trading slice,
  2026-11-03).
- **ENG-37: an item transfer is an atomic batch of fresh destination entries.**
  -- `moves` must be non-empty, distinct and positive; a full move removes the
  source entry; a partial move decrements it; the destination is always
  `Carried`, unattuned and gets a fresh entry id; a nested item `ResourceId` is
  regenerated. One revision per touched sheet. -- Proposed (trading slice,
  2026-11-03).
- **ENG-38: entity ownership transfer is owner-or-DM and does not rewrite ACLs
  or cascade.** -- A write grantee cannot move the entity; explicit grants
  survive; children keep their own owner and parent. -- Proposed (trading slice,
  2026-11-03).
- **ARCH-26: trading reuses the generic command route and `EntityChanged`
  events; it adds no feed entry or stream event.** -- The browser and TUI
  re-read the transferred entities. -- Proposed (trading slice, 2026-11-03).

## 10. Open Questions

Every question states options, tradeoffs and a recommendation. Only OQ-3 can
change the command surface enough to block the skeleton; the rest are
non-blocking if the recommendation is accepted.

### OQ-1: Which character receives an implicit item transfer?

- **A (recommended):** the recipient's earliest-created `Pc`, with the lowest
  `EntityId` as a tie-break; no `Pc` is `Malformed`; the DM can name a
  destination explicitly. A future explicit "active character" setting replaces
  the heuristic.
- **B:** require exactly one `Pc` and refuse otherwise. Simpler and
  unambiguous, but a player with two characters cannot receive an implicit
  transfer, and a player cannot name another player's entity.
- **C:** add `Player.active_entity: Option<EntityId>` now. Cleanest product
  answer, but a core wire change, a roster command, a defaulting policy and a
  version bump; too large for this slice.

Recommendation: **A**. The target test only exercises the single-PC case; the
heuristic is engine unit-test work.

### OQ-2: Must the destination be owned by the recipient?

- **A (recommended):** yes. The recipient field stays meaningful and an item is
  always handed to the named player's own sheet.
- **B:** allow any destination the actor can write. This turns the command into
  a general sheet-to-sheet move and lets a write grantee inject items into a
  shared chest, but the recipient field becomes advisory and the authorization
  story is murkier.

Recommendation: **A**.

### OQ-3: Does the recipient have to accept?

- **A (recommended):** no. A transfer is immediate; the recipient can transfer
  the asset back. This matches "hand items to other players" and keeps the slice
  small.
- **B:** add a pending `TradeOffer` entity or campaign field, accept/decline
  commands, expiry and notification. Safer against griefing, but a much larger
  slice with a new persistence shape and table state.

Recommendation: **A** for this slice. If the table wants consent, B is a
follow-up feature, not a tweak to these commands.

### OQ-4: What happens to ACLs on an ownership transfer?

- **A (recommended):** leave them untouched. The old owner loses only the
  implicit ownership; explicit grants survive, and the DM can remove them later
  when ACL editing lands.
- **B:** remove every grant naming the old owner. Cleaner "handover", but it
  silently destroys shares the old owner may have set for other reasons.
- **C:** add a `keep_acl: bool` or `strip_old_owner: bool` field. More API and
  UI surface for a case ACL editing will cover.

Recommendation: **A**. The target test pins that an unrelated grant survives.

### OQ-5: Do summons and familiars follow their parent?

- **A (recommended):** no cascade. `TransferEntity` changes exactly one entity;
  a summon keeps its owner and `parent` link. The DM can transfer it separately.
- **B:** cascade to every entity whose `parent` is the transferred entity.
  Convenient for a PC-plus-summon bundle, but it moves entities the actor did
  not name and makes the revision/event set unbounded.

Recommendation: **A**, accepted. The target test pins no cascade. Cascading is
not dropped: a later feature adds dedicated cascade commands (transfer an entity
together with its summons, owned items and, once tradeable, coins) on top of the
single-entity primitive this slice lands (section 11).

### OQ-6: What happens to a nested item resource id on a split?

- **A (recommended):** regenerate the destination copy's `ResourceId`, including
  a whole-entry move. Element ids stay unique per entity and split charged
  stacks do not alias.
- **B:** copy the id verbatim. Simpler, but a split creates two resources with
  the same id on different sheets; cross-entity lookups and future tooling
  become ambiguous.
- **C:** forbid splitting an item with a nested resource; only whole-entry moves
  are allowed. Safe but surprising for stacks.

Recommendation: **A**. The target test pins it.

### OQ-7: Should `GiveHandout` become owner-callable?

- **A (recommended):** no. `GiveHandout` stays a DM-only copy tool. A handout
  entity moves through `TransferEntity`; a player who owns a handout can hand
  the handout itself to another player but cannot copy its item into a sheet.
- **B:** let a handout owner call `GiveHandout` on their own handout, turning it
  into an inventory item. More useful, but changes the existing DM-only
  authorization and can duplicate an item indefinitely.

Recommendation: **A**. Revisit when the handout workflow is exercised in play.

### OQ-8: Are coin transfers part of this slice?

- **A (recommended):** no. `Coins` is five integers, not an inventory entry; a
  coin command needs an amount type, per-denomination validation, change-making
  policy and its own revision semantics.
- **B:** add `TransferCoins { source, recipient, destination, cp, sp, ep, gp, pp,
  expected_source_rev, expected_destination_rev }` now. More complete trading,
  but a second command surface and a lot of edge cases (negative coins,
  overflow, partial stacks).

Recommendation: **A**; a separate coin RFC.

### OQ-9: Should a transfer append a feed entry?

- **A (recommended):** no. `EntityChanged` events and revisions are the audit;
  the feed stays roll-focused and no core payload is added.
- **B:** add `FeedPayload::Transfer { from, to, item, qty }` or an entity-level
  payload. Better table awareness, but a core version bump, audience rules and
  feed rendering for every transfer.

Recommendation: **A** for the first slice; B if the table misses the
announcement.

### OQ-10: May a non-DM transfer an entity to the DM?

- **A (recommended):** no. A non-DM may address only `Role::Player` recipients;
  the DM can already take or create anything through their own commands.
- **B:** allow it. Harmless in a cooperative table, but it makes the DM's seat a
  special destination and complicates the role rule.

Recommendation: **A**.

## 11. Deferred items

- Bilateral trade offers, acceptance, expiry and notifications (OQ-3 B).
- Coin and gem transfers (OQ-8 B).
- A transfer feed payload and table announcement (OQ-9 B).
- ACL editing, so ownership transfer can optionally strip or rewrite grants
  (OQ-4 B/C).
- An explicit active-character setting on the roster, replacing the primary-PC
  heuristic (OQ-1 C).
- Cascading transfer commands (OQ-5 B): transfer an entity together with its
  summons/familiars, owned items and, once coins are tradeable, gold. The
  single-entity primitive in this slice is the building block; the cascade gets
  its own command family and its own RFC once core features are stable.
- Owner-callable handout copying (OQ-7 B).
- Transferring items into a shared chest or an NPC's sheet as a first-class
  destination (OQ-2 B).
- Rate limiting or spam controls for repeated transfers, if play shows a need.
- A dedicated trading RFC for the server-side notifications and the browser
  history view.

## 12. Skeleton readiness

The target test pins the command names, field names, error payloads, revision
semantics, event shape and DTO tags. No target-test API element is missing from
section 5. All open questions are resolved to their recommended option: OQ-3 A
(immediate transfer, no recipient acceptance), OQ-4 A (ACLs untouched), OQ-5 A
(no cascade; a later cascade command family is recorded in section 11), OQ-6 A
(fresh resource ids), OQ-7 A (`GiveHandout` unchanged), OQ-8 A (no coins) and
OQ-9 A (no feed entry). The skeleton can proceed without a design change.
