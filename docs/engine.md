# localtt engine

As-built contract for `localtt-engine`: commands and the executor, rules
derivation, dice, authorization and the read services. Product intent is in
[`RFC-0001-localtt.md`](../RFC-0001-localtt.md); the corresponding decision
rationale is in [`decisions.md`](decisions.md). The code and tests are the
authority where this document and the implementation disagree.

## 1. Module map

- `executor` - synchronous `Executor` and asynchronous `AsyncExecutor`; both run
  `apply -> persist -> publish`.
- `commands/` - the `Command` enum, command handlers, `CommandEnv`, `Transition`
  and the `apply`/`apply_as` entry points.
- `state` - `WorkingSet`, `WorkingSetMut`, `LoadedCampaign` and the expiry index.
- `clock` - exact campaign-clock arithmetic, including dawn crossings.
- `rules/` - `derive` and `roll_parts`; pure, no I/O.
- `dice/` - notation parser, cap validation, composition and roller.
- `authz/` - access predicates, entity/campaign views and feed visibility.
- `services` - list/get/campaign/roster/sheet/feed reads over a `WorkingSet`.
- `content` - the engine-owned content model and `ReferenceSource` port.
- `export` - the versioned `ExportBundle`.
- `error` - `EngineError`, `Warning`, `WarningCode` and `ElementRef`.
- `ports/` - `CampaignStore`, `AsyncCampaignStore`, `StoreBackup` and `Rng`.

The engine depends on `localtt-core` only. Ports are object-safe traits owned by
the engine and implemented by `localtt-adapter`.

## 2. Commands and the executor

A command is a typed intent, not a whole-document replacement. Handlers are
pure over the working set: they return a `Transition` containing the upserts,
feed appends, events and warnings. The executor then:

1. asserts that every upsert advances `rev` by exactly one, or starts a created
   entity at `rev = 1`;
2. asserts the patch shape (upserts strictly ascending by id, feed appends
   strictly ascending and not below the trim cutoff);
3. applies the patch to the store;
4. applies it to the in-memory working set;
5. returns `Executed { warnings, events }` to the caller.

This is durable-before-visible (`ENG-2`): a failed store write leaves the
working set untouched, and no event is published for an uncommitted command.
`Executor` is `Send` but deliberately not `Sync`; the server serializes access.
`into_parts` returns the owned working set and environment so a caller can run
`StoreBackup::sync` while no executor is alive.

`Executor::execute` is the trusted system path and performs no authorization.
`Executor::execute_as(actor, command)` runs the authorization pre-check first,
then records the actor for attribution and the audit trail. Every client
mutation must use `execute_as`.

Clock, roster and handout commands are DM-only. Commands that mutate an
existing document take an optional `expected_rev: Option<u64>` compare-and-swap;
a mismatch is `Conflict`. Transfers carry one revision per side
(`expected_source_rev`, `expected_destination_rev`). `CreateEntity` has no
revision to compare, and `Roll` is append-only. The campaign document itself
has no revision in the current wire contract, so campaign-level CAS remains
deferred.

## 3. Rules derivation

### 3.1 Inputs and output

`rules::derive(&StatblockEntity, &RulesContext)` is pure. `RulesContext` carries
the campaign instant and a materialized `BTreeMap<RefId, ContentEntry>`; the
caller resolves references and missing entries degrade with warnings.
`DerivedSheet` contains:

- `values: BTreeMap<StatKey, DerivedValue>`;
- `resources: BTreeMap<ResourceId, DerivedResource>`;
- `slots: SlotMaxima` with regular spell slots and optional pact slots;
- `hit_dice: BTreeMap<u8, i32>`;
- `warnings: Vec<Warning>`.

`DerivedValue` keeps `computed`, `override_value` and `effective` separate, plus
the contribution `breakdown` and an optional advantage/disadvantage `hint`.
`effective` is the override when present, otherwise the computed integer. The
computed value is never replaced, so override drift stays visible. Group keys
expand into their members and are never stored as values themselves.

### 3.2 Formula evaluation

The full `Formula` language is supported: `Const`, `ProficiencyBonus`,
`AbilityMod`, `Level`, `ClassLevel`, `Add`, `Sub`, `Mul`, `Min`, `Max` and
`FloorDiv`. Arithmetic is saturating `i32`. `FloorDiv` by zero is unevaluable:
a resource maximum becomes `None` with `FormulaUnevaluated`, and a modifier
contributes nothing with a warning scoped to its source.

`AbilityMod` reads the final computed score, except in the two bootstrap stages:
a proficiency-bonus contribution sees the stored scores and the pre-contribution
bonus; an ability contribution sees the staged proficiency bonus but still the
stored scores. Nothing after the score stage reads a stored score.

A `Dice` leaf in a static context evaluates to zero. A resource maximum
containing one is unevaluable; a modifier value containing one contributes zero
and warns `DiceInStaticContext`.

### 3.3 Contribution pipeline

Sources are collected in this order: the rules base, features in document
order, inventory item modifiers in document order, then status modifiers in
document order. Within one source, modifier order is preserved. Staging then
groups by operation, so the ordering below decides the arithmetic.

For one value, `Set` operations apply first (last wins), then `Add`, then
`Min`/`Max` clamps. `Min` raises the running value to its operand; `Max` lowers
it. An `Add` with no running value starts at zero. A clamp with no running
value is recorded in the breakdown and changes nothing. The breakdown records
every applied contribution in application order, including replaced `Set`s.

Advantage and disadvantage never change `computed`; they cancel pairwise and
the remainder becomes `hint`. `None` means no hint contribution applied;
`Some(Normal)` means equal counts cancelled. Overrides win for `effective` but
never replace `computed`, and downstream bases use `computed`.

Evaluation stages, in order: proficiency bonus; ability scores; ability
modifiers; saves; skills; passive scores; AC; initiative; speed; spell DC and
spell attack; custom keys; resources; slots; hit dice.

### 3.4 Derived keys

Every statblock derives `proficiency_bonus`, the six `ability.*`, the six
`ability_mod.*`, the six `save.*`, all `skill.*`, all `passive.*`, `ac`,
`initiative` and `speed`. `spell.dc` and `attack.spell` exist only when the
entity has a spellcasting block; the first block's ability drives both.
`attack.melee` and `attack.ranged` appear only when a contribution targets them,
directly or through the `Attack` group.

Structural bases:

- ability score = the stored score;
- ability modifier = `floor((final score - 10) / 2)`;
- save = ability modifier plus proficiency when the save is proficient;
- skill = ability modifier plus proficiency, doubled with expertise;
- passive = 10 plus the skill;
- initiative = dexterity modifier;
- speed = `speed.walk_ft`;
- spell DC = 8 + proficiency + casting ability modifier;
- spell attack = proficiency + casting ability modifier.

These bases read the score-derived modifier, not the `ability_mod.*` value. A
contribution to `ability_mod.*` moves that key only.

Proficiency bonus uses total character level clamped to 1..=20, or challenge
rating for creatures (CR 0-4: 2, 5-8: 3, 9-12: 4, 13-16: 5, 17-20: 6, 21-24: 7,
25-28: 8, 29-30: 9). A sheet with neither levels nor CR gets 2.

Group contributions are resolved once against the settled environment and the
result is copied to every member; they are not re-evaluated per member. A
warning is emitted once per contribution, not once per member. The `Attack`
group reaches the deferred melee/ranged keys too; `attack.spell` stays gated on
a spellcasting block.

### 3.5 Armor and AC

Equipped, usable body armor candidates contribute `base_ac + min(dex, cap)`;
the highest wins and ties keep the earliest entry. Without body armor, AC starts
from base 10 plus the dexterity modifier. The highest equipped shield bonus wins,
ties keeping the earliest. More than one body armor warns `OverArmored` once;
likewise more than one shield. Both warnings are element-less. An equipped body
armor whose strength requirement exceeds the final Strength score warns
`StrengthRequirement` but still applies.

### 3.6 Attunement, quantity and conditions

Every entry with `attuned == true` counts, whether or not the item requires
attunement. More than three warns `OverAttunement` once per derivation; the
over-limit entries still contribute. An entry with `qty <= 0` that would
contribute is excluded and warns `ZeroQuantity`; an entry that could not
contribute anyway is silent.

`when: None` applies unconditionally. `Equipped` and `Attuned` gate inventory
entries only and are never satisfied by features or statuses.
`StatusActive { name }` matches an unexpired status by name, case-insensitively;
a timed status is expired when `now >= expires_at` but stays on the sheet (soft
expiry). `Custom` is unmodeled, is skipped, and warns `UnmodeledCondition`.

Warning messages are human text, not a contract. Warnings caused by a source's
contribution carry that source's element; `OverArmored`, `OverAttunement` and
`UnmodeledClass` carry none. Every derive warning carries the entity id.

### 3.7 Resources, slots and hit dice

Every stored resource gets a `DerivedResource` whose `max` is the evaluated
formula, or `None` with `FormulaUnevaluated`. `current` is not clamped during
derivation; command spend/restore semantics still apply.

Regular spell slots come from the 2014 multiclass table at combined full-caster
level. Wizard and Cleric levels sum; Warlock levels do not. Pact slots come from
the Pact Magic table at warlock level. Levels above 20 clamp to the table end.

Hit-dice pools sum class levels by die: Wizard 1d6, Cleric 1d8, Fighter 1d10,
Warlock 1d8. A class name outside those four warns `UnmodeledClass` and
contributes no slots or hit dice, but its levels still count toward total level
and proficiency bonus. Half/third casters, subclasses, unarmored defense and a
content-driven ruleset bundle are deferred.

### 3.8 Rest recovery

`ShortRest` refills `Recharge::Short` resources and clears
`UntilRest { rest: Short }` statuses. `LongRest` does the same for long
recharges and additionally:

- sets `hp.current = hp.max`, leaving temporary HP untouched;
- resets death saves to 0/0;
- reduces exhaustion by one (saturating);
- regains `max(1, max / 2)` dice per hit-die resource, clamped to its maximum;
  a non-constant maximum is skipped with `FormulaUnevaluated`.

An optional clock advance is applied as part of the same transition. A rest
target that is also dawn-refilled stays one revision.

### 3.9 `roll_parts`

`roll_parts` derives the sheet, reads the stat's effective integer as `flat`,
and collects dice terms from the same collection pipeline. A dice leaf is
representable when it sits in an `Add`/`Sub` chain over otherwise
integer-evaluable operands. A dice leaf under `Mul`, `FloorDiv`, `Min` or `Max`
makes `roll_parts` return `Malformed`, because the roller cannot express a
scaled die as terms. `Malformed` also when the stat has no value or its
effective value is not an integer override. The flat value and dice terms agree:
every dice leaf contributes zero to `computed`.

## 4. Dice

### 4.1 Grammar

Tokens are non-negative integer literals, the pool token, `adv`, `dis`, `+`,
`-`, `*`, `(`, `)`. ASCII whitespace between tokens is ignored; `adv`/`dis` may
be separated from their `(`. A pool token is a contiguous `N d M` with an
optional `khN`/`klN` suffix. Whitespace inside a pool token is a syntax error.
Keywords are lowercase only; `D`, `KH`/`KL` and a bare `d20` are refused.

```
expr   = term (("+" | "-") term)*
term   = factor ("*" factor)*
factor = "adv" "(" expr ")" | "dis" "(" expr ")" | pool | number | "(" expr ")"
pool   = number "d" number keep?
keep   = ("kh" | "kl") number
number = DIGIT+
```

Parentheses are transparent to the AST. There is no unary minus and no constant
folding. A numeric literal that does not fit `i32` is `Malformed`.

### 4.2 Caps

| Cap | Limit | Exceeded | Illegal below/inside |
|---|---|---|---|
| `Roll.count` | 100 | `TooLarge { what: "dice count", limit: 100 }` | `Malformed` |
| `Roll.sides` | 1000 | `TooLarge { what: "dice sides", limit: 1000 }` | `Malformed` |
| `Keep` count | pool `count` | - | `Malformed` |
| Expression depth | 16 | `TooLarge { what: "expression depth", limit: 16 }` | - |
| Expression terms | 32 | `TooLarge { what: "expression terms", limit: 32 }` | - |

Count is `1..=100`, sides `2..=1000`, keep count `1..=count`. Depth is the node
count on the longest root-to-leaf path; a bare die is 1. Terms count every AST
node, with parentheses transparent. `parse` never returns an AST that
`validate` would reject. Composed plans are validated before rolling, so
sheet-derived dice meet the same caps as client input.

### 4.3 Composition and roller

`dice::compose(base, &RollParts)` wraps the base in the hint, adds the flat part
when non-zero, then appends one attributed term per `DiceTerm`. The result is a
`RollPlan`; `roll` is infallible once the plan is valid.

Roller semantics:

- one `TermResult` per `TermPlan`, in order, with `source` copied;
- `Roll` draws `count` dice in order; `kh`/`kl` keep the requested count, ties
  keeping the earliest die in roll order;
- `Add`/`Sub`/`Mul` operate on term totals and concatenate dice left-then-right;
- `adv`/`dis` evaluate the inner expression twice, first draw first; the higher
  (or lower) total wins and equal totals keep the first evaluation; the loser's
  dice stay visible with `kept: false`;
- a kept d20 carries its own face, so a client can still spot a natural 1 or 20;
- all arithmetic saturates, so `roll` cannot panic;
- the roller draws exactly as many dice as the plan requires and never draws
  ahead.

### 4.4 RNG

`ports::rng::Rng` is the die source. Randomness is always injected, never read
from a global source in engine code. `localtt-adapter::rng::ChaChaRng` provides
seeded replay. A seeded bounded test in the adapter checks face coverage; the
integration target suite stays exact and offline-fast.

### 4.5 Roll, casting and concentration commands

`RollSpec::Expression` rolls a client expression and records an optional target
stat for context only. `RollSpec::Check` derives the flat part, attributed dice
and hint, then rolls `1d20` composed with them. The actor comes from
`execute_as`; the feed entry records the composed expression, the result,
context and audience. A `None` audience uses the campaign default.

`CastSpell` resolves the spell entry, spends the named slot, checks the
declared upcast against the slot level minus the spell level, and starts
concentration when the resolved spell requires it or the client requested it. An
unresolved reference casts with `UnresolvedReference`; a cantrip refuses a slot
spend; overspending a slot is allowed and warns `ResourceOverspent`, matching
`SpendResource`.

`BeginConcentration` replaces every existing concentration status and appends
the new one from the `StatusSpec`. `EndConcentration` removes every
concentration status. Each replaced or ended status appends a
`ConcentrationEnded` feed entry.

`SpendHitDie` reads the CON modifier through `rules::derive`, rolls the die,
heals `max(0, total)`, allows overspending with a warning, and appends a roll
feed entry. A long rest regains `max(1, max / 2)` hit dice per hit-die pool.

`ApplyDamage` appends a concentration-save hint when the entity has an
unexpired concentration status: DC `max(10, damage / 2)`, using the damage
operand before temporary-HP absorption.

## 5. Authorization and views

### 5.1 Access algebra

Membership is the first gate of every predicate. A non-member is denied even
when named owner or granted access. The DM reads and writes every entity and
sees every feed entry except a `RollerOnly` entry they did not roll.

For one entity and one member viewer:

- `write` is true when the viewer is the DM, the owner, or holds any `Write`
  grant. A `Write` grant implies full read and ignores its mask entirely.
- Otherwise the visible set `V` is the union over the viewer's `Read` entries of
  `mask == None ? ALL : mask`. An empty mask contributes nothing; an empty union
  denies.
- `can_read` is `write || V != {}`; `can_write` is `write`.
- Grants combine permissively; nothing subtracts. Masks are honored on every
  entity, not only DM-owned shares.
- `hidden` is exactly `ALL \ V`; `writable` is `can_write`. `ALL` is the 20
  core `ViewField` variants, matched exhaustively in the engine.

### 5.2 View construction

`view_of` mirrors the headers (`id`, `kind`, `owner`, `parent`, `rev`) and
materializes sections from `V`:

| `ViewField` | Carrier |
|---|---|
| `Name` | `view.name` |
| `Ext` | `view.ext` |
| `Identity` | `profile`, `identity`, `levels`, `cr`, `creature_type`, `size`, `senses` |
| `Abilities` | `abilities` |
| `Saves` | `proficiencies.saves` |
| `Skills` | `proficiencies.skills` |
| `Proficiencies` | `proficiencies.tools`, `proficiencies.languages` |
| `Hp` | `hp` |
| `Ac` | no carrier yet; maskable only |
| `Speed` | `speed` |
| `DeathSaves` | `death_saves` |
| `Coins` | `coins` |
| `Overrides` | `overrides` |
| `Resources` | `resources` |
| `Spells` | `spellcasting` |
| `Inventory` | `inventory` |
| `Attacks` | `attacks` |
| `Features` | `features` |
| `Statuses` | `statuses` |
| `Notes` | `notes` on a statblock; `NoteView.text`; `HandoutView.content` |

The body is present when at least one carrier field other than `Name`, `Ac` or
`Ext` is visible. The variant follows the entity kind: the six statblock kinds
map to `Statblock`, `Note` to `Note`, `Handout` to `Handout`. Within a statblock
view, each carrier is present only when its field is visible. `exhaustion` and
`inspiration` are always present when the body exists.

`view_of_campaign` requires membership and returns the campaign header plus the
roster. Every member sees the same roster (id, display name, role). The
join-token hash and last-seen stamp never enter the view.

### 5.3 Deny and serialization

The deny stub has `hidden == ALL`, no `name`/`ext`/`body`, `writable: false`,
and mirrored headers. It is byte-identical for "no grant" and "empty mask".
Hidden sections are absent keys, not `null`s. No value from a hidden section may
reach the wire; the serialized marker sweep checks the whole JSON string.

`EntityViewBody` is internally tagged `type` with `statblock`, `note` or
`handout`. `FeedEntryView` serializes exactly `id`, `ts`, `clock_s`, `actor`,
`entity` and `payload`; it never carries `audience`.

### 5.4 Feed visibility

`entry_visible` applies membership first, then:

- `Public` - any member;
- `RollerAndDm` - the actor, or the DM; with no actor, the DM only;
- `RollerOnly` - the actor only; with no actor, nobody. The DM is deliberately
  excluded.

`feed_after` walks the feed in ascending id, keeps visible entries with
`id > cursor`, takes `limit`, and projects each to a `FeedEntryView`.
`limit == 0` returns nothing. Visibility is a read-side derivation only; stored
entries keep their audience.

### 5.5 Pre-check

The authorization pre-check runs before all command work, including input
validation, CAS, element lookup inside a present entity and content resolution.
An unauthorized request never learns whether the rest of the request was well
formed. An absent target entity defers to the command's own `NotFound`, for any
actor. A denial produces no transition, no events, no warnings and no writes.

| Command | Requirement | Denial payload |
|---|---|---|
| `AdvanceClock`, `SetClock`, roster commands, `GiveHandout` | `Dm` | `entity: None` |
| `CreateEntity` | `Member` | `entity: None` |
| `Roll { entity: None, .. }` | `Member` | `entity: None` |
| `Roll { entity: Some(id), .. }` | `Read(id)` | `entity: Some(id)` |
| `TransferItems`, `TransferCoins` | `Write(source)` | `entity: Some(source)` |
| `TransferEntity` | `Owner(id)` | `entity: Some(id)` |
| every other entity command | `Write(entity)` | `entity: Some(id)` |

## 6. Read services

- `list_entities` iterates `WorkingSet::headers()` in ascending id, builds each
  view and keeps it iff `can_read`. It is infallible; an unresolvable header is
  skipped as an invariant violation.
- `get_entity` returns `NotFound` for an absent id and `Unauthorized` for a
  present but unreadable id. Missing beats unauthorized.
- `campaign_view` requires membership.
- `admin_roster` requires the DM and is the only roster read that reports
  `has_invite`.
- `derived_sheet` requires full field visibility: a masked read is denied
  because derivation recomputes from hidden sections. It also refuses non-
  statblocks and materializes inventory references through the `ReferenceSource`.
- `feed_after` is described in section 5.4.

The read services take `&dyn WorkingSet`, not the store or the executor.

## 7. Deferred engine work

The consolidated backlog is [`TODO.md`](../TODO.md). The main deferred areas
are field/ACL/delete commands, campaign-level CAS, derived values inside views,
a header-only entity list, reverse feed paging, the packs loader and
`FreezeReference`, content-driven class progression, and a cached/incremental
derivation path.
