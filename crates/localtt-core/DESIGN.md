# localtt-core: the domain data model

| | |
|---|---|
| **Status** | Implemented. This document supersedes the former `RFC-0002-core-types.md` (removed). |
| **Scope** | `crates/localtt-core/`: `src/types.rs` (the versioned wire contract) and `src/builders.rs` (ergonomic constructors). |
| **Read with** | `RFC-0001-localtt.md` (product intent and invariants, still current), `docs/architecture.md` (crate map), `docs/decisions.md` (decision log). |

## 1. What core is

Core is the I/O-free domain layer. It owns the serialized shape of everything the
campaign file stores -- campaign, players, ACLs, entities and bodies, statblocks,
items, resources, statuses, spellcasting, the dice AST, rolls, feed, clock and
schema versions -- plus the validation that keeps those documents well-formed.

It stores what is *typed in*; it never computes derived values, enforces rules, or
touches I/O. "Warn, never block" is an engine property: core just makes the states
representable (over-spent resources, negative HP, over-attunement, more than one
concentration status).

Dependencies are `serde`, `serde_json` (for `ext` payloads), `uuid` (v7) and
`thiserror`. Nothing else, so the crate compiles fast and can later be extracted
as a plugin-facing crate.

## 2. Module layout

- `src/types.rs` -- the versioned wire contract. Single file; the sections below
  are named in source, and a later split into `types/` submodules must re-export
  so no public path changes.
- `src/builders.rs` -- `CampaignBuilder`, `StatblockBuilder`, literal helpers and
  constructor sugar (section 8). Kept out of `types.rs` so the wire contract stays
  one file.

`types.rs` section order: 1 ids, 2 primitives (time, versions, namespaces, refs,
stat keys), 3 campaign and players, 4 ACL and view fields, 5 entities and bodies,
6 statblock sections, 7 items, 8 modifiers and formulas, 9 resources and statuses,
10 spellcasting, 11 dice/rolls/feed, 12 errors.

## 3. Data model

Field-level detail lives in the doc comments; this is the shape and the reasoning.

### 3.1 Identifiers

- `Id<T>`: opaque UUIDv7 with a `PhantomData<fn() -> T>` domain tag. One generic
  type aliased per domain (`CampaignId`, `PlayerId`, `EntityId`, `StatusId`,
  `ResourceId`, `SpellcastingBlockId`, `SpellEntryId`, `ClassLevelId`,
  `InventoryEntryId`, `AttackEntryId`, `FeatureEntryId`, `RollId`, ...).
  `EntityId = Id<AnyEntity>` (an id of an entity of whichever body) avoids a
  marker type.
- `FeedEntryId`: a per-campaign `u64` counter, not a UUID.
- All trait impls (`Clone`/`Copy`/`Hash`/`Ord`/`Display`/serde) are written by
  hand without a `T: Trait` bound; `Id<T>` is always `Copy`, `Send + Sync`.

### 3.2 Primitives

- `CampaignTime` (i64 seconds on the DM clock) and `UnixTime` (i64 wall seconds,
  audit/display only). Two newtypes so campaign and wall time cannot be mixed.
- `SchemaVersion { major: u16, minor: u16 }` with `const CURRENT` and
  `const fn accepts(self, file)` (same or older major). See section 5.
- Validated string newtypes: `RulesetId` (`[a-z0-9-]`, <= 32), `Namespace`
  (lowercase dotted segments), `RefId`, `CustomStatKey`.
- `RefId`: canonical `<namespace>:<kind>/<slug>`, e.g. `srd2014:spell/fireball`,
  `pack.mytome:item/foo`. Segments are non-empty `[a-z0-9_-]` runs; the namespace
  is dot-separated segments. Stored as one `String` plus two `u32` segment
  boundaries (one allocation, no re-parse).
- `ContentRef { id: RefId, name: String }`; `RefOrText::{Ref(ContentRef), Text}`
  (untagged). Every external reference stores a name snapshot so a missing pack
  degrades to text instead of a broken sheet.
- `Ext = BTreeMap<Namespace, Json>`; `Json` is re-exported `serde_json::Value`.
- `Ability`, `Skill` (18), `KnownStat`, `StatGroup { Check, Save, Attack }`,
  `AttackKind`.
- `StatKey::Known(KnownStat) | Custom(CustomStatKey)`: the hybrid. Canonical wire
  strings are pinned by tests; anything else that is validly shaped stays `Custom`
  with its original spelling and round-trips byte-identically.

### 3.3 Campaign and players

- `Campaign`: `id`, `name`, `schema_version`, `default_ruleset`, `clock`,
  `settings`, `players: Vec<Player>`, `ext`. The player roster lives on the
  campaign document.
- `Clock { now_s: CampaignTime }`. Rounds are derived (6 s), never stored.
- `CampaignSettings { player_roll_audience_default, feed_capacity, dawn_hour }`.
- `Player { id, display_name, role, token_hash, created_at, last_seen }`;
  `Role::{Dm, Player}`; `TokenHash` is a hex digest, never the token.

### 3.4 Entities, bodies, ACL

- `EntityKind`: Pc, Companion, Summon, Familiar, Npc, Monster, Note, Handout.
  `BodyKind`: Statblock, Note, Handout. Six kinds share the statblock body.
- `Entity<B: Body>` is type-stated: the header (`kind`, `id`, `ruleset`, `name`,
  `owner`, `parent`, `acl`, `rev`, `schema_version`, `ext`, `created_at`,
  `updated_at`) plus a statically known `body: B`. Aliases `StatblockEntity`,
  `NoteEntity`, `HandoutEntity`.
- `Body` is sealed (only the three body types implement it) and carries
  `const BODY_KIND`.
- `EntityBuilder<B>` is the consuming header builder; `build()` checks that
  `kind.body_kind() == B::BODY_KIND` and returns `EntityError::KindBodyMismatch`
  otherwise.
- `AnyEntity` is the runtime-tagged enum (`Statblock`/`Note`/`Handout`) with
  header accessors (`id`, `kind`, `owner`, `parent`, `acl`, `rev`, ...) and
  `as_statblock`/`as_note`/`as_handout`, plus three concrete `From` impls. It
  exists so the type-state does not leak into storage, iteration and redaction
  code.
- ACL: `AclLevel::{Read, Write}`; `AclEntry { player, level, field_mask }`;
  `FieldMask(BTreeSet<ViewField>)` with `contains`/`insert`/`is_empty`/`iter`;
  `ViewField` is the closed set of redactable sheet sections plus a single `Ext`
  bucket. Semantics (core stores, engine enforces): a mask on the effective
  `Read` hides everything not listed and defaults to deny; `None` means full read;
  `Write`, owners and the DM ignore masks.

### 3.5 Statblock body

- `StatblockBody`: `profile` (Character/Creature), `identity`, `abilities`,
  `levels`, `cr`, `creature_type`, `size`, `senses`, `proficiencies`, `hp`,
  `speed`, `death_saves`, `exhaustion`, `inspiration`, `coins`, `overrides`,
  `resources`, `spellcasting`, `inventory`, `attacks`, `features`, `statuses`,
  `notes`. Character-only and creature-only sections are optional data, not
  separate types.
- `Abilities` has named `i32` fields serialized as `str`/`dex`/... and no
  `Default` (section 9); `Identity { race, background, alignment }` (class
  identity lives in `levels`); `ClassLevel { id, class: RefOrText, level,
  subclass }`; `Profile`, `Size`.
- `ChallengeRating` is stored in eighths (`i32`) so every official value is exact
  with no floats; the wire form stays the conventional string (`"1/8"`, `"5"`).
- `Proficiencies`: `saves: BTreeSet<Ability>`, `skills: BTreeMap<Skill,
  SkillProficiency>` (sparse: absent = none), free-text `tools` and `languages`.
- `Hp { max, current, temp }` (all inputs, `current` may go negative);
  `Speed { walk_ft, fly_ft, ..., hover }`; `DeathSaves { successes, failures }`;
  `Coins { cp, sp, ep, gp, pp }`; `OverrideValue::{Int, Bool, Text}` (untagged).
- `NoteBody { text }` and `HandoutBody { content }` with
  `HandoutContent::{Text { text }, Item { item }}` (internally tagged `type`).
  "Give" means the item lands in the recipient's inventory as their own inline
  copy; read-only sharing stays an ACL concern.

### 3.6 Items and inventory

- `InventoryEntry { id, item, qty, state, attuned, notes }`;
  `ItemState::{Carried, Equipped, Stowed}`.
- `InventoryItem::{Ref(ContentRef), Inline(InlineItem)}` (tagged `type`).
- `InlineItem { name, description, kind, requires_attunement, weight_oz, value,
  properties, modifiers, resource, ext }`; `ItemKind`.
- `ItemProperties::{Weapon, Armor, Shield}` with `WeaponProperties`,
  `ArmorProperties`, `ShieldProperties`; `WeaponTag`, `DamageType`.
- Decisions: weight is integer ounces (exact, no floats; display converts);
  magic `+1` bonuses are not a field -- they belong to the modifier pipeline.

### 3.7 Attacks, features, spellcasting

- `AttackEntry { id, name, bonus: Option<i32>, damage_text, from_inventory }`;
  `damage_text` is free text, `from_inventory` links equip state.
- `FeatureEntry { id, category, name, reference, text, resource_id, modifiers,
  ext }`; `FeatureCategory::{Class, Race, Feat, Trait, Action, Reaction,
  Legendary, Other}`. Monster actions use the same shape as class features.
- `SpellcastingBlock { id, source: RefOrText, ability, spells }`;
  `SpellEntry { id, name, reference, prepared }`.

### 3.8 Resources and statuses

- `Resource { id, name, kind, max: Formula, current, recharge }`;
  `ResourceKind::{SpellSlot { level }, PactSlot, HitDie { sides }, Feature, Item,
  Other}`; `Recharge::{Short, Long, Dawn, Manual, Never}`. Spell slots, pact
  slots, hit dice, Channel Divinity, item charges and legendary actions are all
  resources; recharge-by-roll is `Manual` for now.
- `Status { id, kind, name, reference, text, started_at, duration, source,
  modifiers }`; `StatusKind::{Condition, Effect, Concentration, Custom}`;
  `StatusDuration::{Timed { expires_at }, UntilRest { rest }, Indefinite, Manual
  { text }}`; `StatusSource { entity, status }`. Durations are absolute expiry on
  the campaign clock (`expired = now >= expires_at`), so advancing the clock is
  one write and correcting it backwards correctly un-expires.

### 3.9 Modifiers, formulas, conditions

- `Modifier { target: StatKey, op: ModifierOp (flattened), when: Option<Condition>,
  note: Option<String> }`; `ModifierOp::{Add, Set, Min, Max, Advantage,
  Disadvantage}`, each numeric op carrying its `Formula`.
- `Formula::{Const(i32), Expr(FormulaExpr)}` (untagged, so a bare number is the
  shorthand). `FormulaExpr` is a small closed language: `ProficiencyBonus`,
  `AbilityMod`, `Level`, `ClassLevel`, `Add`/`Sub`/`Mul`/`Min`/`Max`/`FloorDiv`,
  and a `Dice { expr }` leaf so timed effects like Bless ("+1d4 to saves") are one
  modifier value.
- `Condition::{Equipped, Attuned, StatusActive { name }, Custom { name }}`.
- Statuses are the carrier for timed effect modifiers. They are scanned when the
  engine composes a value; a target matches either the concrete stat
  (`save.wis`) or a group (`save`, `check`, `attack`). Two evaluation contexts:
  **static** (derived stats -- `Formula` must evaluate to `i32`; dice leaves warn
  and use the flat part) and **roll** (dice leaves expand into terms attributed
  to the source). Which concrete rolls belong to a group, and arithmetic over
  dice, are engine policy.
- Source attribution stays implicit in the container (item, feature, status); core
  adds no `source` field to `Modifier`.

### 3.10 Dice, rolls, feed

- `DiceExpr` AST: `Const`, `Roll { count, sides, keep }`, `Advantage`,
  `Disadvantage`, `Add`, `Sub`, `Mul`; `Keep::{Highest, Lowest}`. The AST lives in
  core because it is part of the serialized `Roll`; the parser and roller live in
  `engine::dice`.
- `Roll { id, roller, entity, ts, expr, result, context, audience }`;
  `DiceResult { terms, total }`; `TermResult { dice, total, source }`;
  `DieResult { value, kept }`; `RollContext { kind, stat, target_dc }`;
  `Audience::{Public, RollerAndDm, RollerOnly}`.
- `FeedEntry { id, ts, clock_s, actor, entity, audience, payload }`;
  `FeedPayload::{Roll { roll }, StatusExpired { status, name }, ConcentrationEnded
  { name }, Hint { text }}`. The feed is a persisted ring buffer
  (`feed_capacity`), not an audit log.
- `TermResult::source` is a label snapshot (not an id): the source may be gone by
  the time the entry is read. `Roll.expr` stores the composed expression actually
  rolled, so a persisted roll is self-contained.

### 3.11 Errors

Construction/parse errors (`RefIdError`, `StatKeyError`, `RulesetIdError`,
`NamespaceError`, `ChallengeRatingError`) are plain enums with `Display` +
`Error`. `EntityError::KindBodyMismatch { kind, body_kind }` is the document
validation error, surfaced through serde. Rules-like conditions are not errors at
any layer (section 1).

## 4. Serde contract

Tagging is part of the public contract:

| Shape | Representation |
|---|---|
| Structs | flat object, `snake_case` fields |
| Plain enums | `snake_case` string |
| Payload enums | internally tagged, key `"type"` (`"op"` for `ModifierOp`) |
| `Formula`, `RefOrText`, `OverrideValue` | untagged (bare number/string/bool is the shorthand; objects are unambiguous) |
| `Id<T>`, `RefId`, `RulesetId`, `Namespace`, `TokenHash`, `CampaignTime`, `UnixTime`, `FeedEntryId` | transparent (string or number) |
| `SchemaVersion` | object `{"major","minor"}` |
| `FieldMask` | transparent, sorted array of section strings in `ViewField` order |
| Entity documents | flat header + `"body"` object, top-level `"kind"` |

Rules, all tested:

1. Unknown object fields are ignored on deserialize and not re-emitted (forward
   tolerance inside a major version).
2. `Option` fields are omitted when `None`; `null` from older or hand-edited files
   still deserializes as `None`.
3. No floats anywhere; no `deny_unknown_fields` (incompatible with `flatten`, and
   forward tolerance is wanted).
4. Deserializing `Entity<B>` validates `kind.body_kind() == B::BODY_KIND` and
   fails the parse on mismatch. The same predicate guards the builder
   (`EntityBuilder::build`) and the wire path (`into_entity`, which `AnyEntity`
   also uses), so the concretely-typed and runtime-tagged paths cannot disagree.
5. `StatKey` round-trips byte-identically (known spellings become `Known`,
   anything else valid stays `Custom`).
6. `Formula` nodes serialize as bare numbers when constant.
7. Unknown enum tags (a `"type"`/`"op"` string this build does not know) are a
   deserialization **error**, unlike unknown object fields. Payload-enum additions
   therefore need a major bump or an explicit fallback variant. This is
   deliberate: no silent reinterpretation.

## 5. Version policy

- `SchemaVersion::CURRENT` is the version this build writes.
- A loader accepts a document whose **major** is equal or lower; a newer major is
  refused rather than partially loaded. A newer **minor** within the same major is
  accepted and its unknown fields are dropped.
- A document that loads is **re-stamped at `CURRENT`** on deserialize, for both
  `Entity` and `Campaign` (the latter through a private `CampaignWire`). Keeping
  the file's own value would be a lie once the newer minor's unknown fields are
  dropped.
- The schema gate itself is store logic and lives in `localtt_adapter::load`
  (`load_checked` + `LoadError`), not here.

## 6. Guarantees

- **Safety:** no `unsafe`, no `unwrap`/`expect` outside tests. Every fallible path
  returns a `Result`.
- **Threading:** every public type is `Send + Sync`; no lifetimes, trait objects,
  interior mutability or smart pointers. `Body: Send + Sync` makes `Entity<B>` so
  for every body. The test asserts this statically; there is no concurrency
  behaviour to test in core.
- **Determinism:** no floats, and `BTreeMap`/`BTreeSet` in `ext`, `overrides`
  and `skills`, so serialized bytes are stable for a given value. Explicit `Vec`
  order is meaningful (lists are ordered and addressed by element id).
- **Progress:** not applicable -- synchronous, allocation-only data.

## 7. Invariants that are not type-level

Recorded so nobody assumes the type system enforces them: uniqueness of element
ids inside a list, exactly-one-DM, owner-must-exist, referenced ids resolving, ACL
masks being honoured, formula-context rules (dice leaves are roll-only; other
contexts use the flat part), and group-target expansion.

Type-level invariants are only: kind/body family, id domain separation,
campaign-vs-wall time separation, and validated key/ref/namespace shapes.

## 8. Builders (`builders.rs`)

Pure data assembly; no rules, no I/O. Used by the integration suite and intended
for any consumer (PDF import, content packs) that has to assemble a document.

- **Two idioms, deliberately.** `EntityBuilder` is consuming (`fn ruleset(self) ->
  Self`) and never yields anything but `Self` until `build`.
  `CampaignBuilder`/`StatblockBuilder` take `&mut self` because some methods must
  also hand back a product mid-chain (`CampaignBuilder::dm` returns a `PlayerId`,
  `StatblockBuilder::item` returns an `InventoryEntryId`). A consuming chain
  cannot stay fluent and yield that product without tuple gymnastics.
- **Fallible finish.** `StatblockBuilder::build()` returns `Result<_, BuildError>`
  and requires `abilities` + `hp` (the same `BuildError` covers
  `MissingAbilities`, `MissingHp`, and an inner `Entity` error). This is what
  enforces "no silent 10 in every ability".
  `build_entity(kind, id, name, owner)` finishes body plus header in one call;
  `entity_builder(...) -> Result<EntityBuilder<StatblockBody>, BuildError>` hands
  back the header builder so a caller can set `parent`/`acl`/`rev`/timestamps
  before `.build()`.
- **Deterministic element ids.** Builder-minted elements get fresh v7 ids; the
  `*_with_id` variants (`class_level_with_id`, `spellcasting_block_with_id`,
  `item_with_id`, `attack_with_id`) take a caller-supplied element id so a fixture
  can build the same body byte-for-byte on every run.
- **`item_inline(name, kind, state, attuned, requires_attunement)`** covers the
  bare "just carried it" case; `attuned` (the entry's state) and
  `requires_attunement` (a property of the item) are separate.
- **Literal helpers** return `Result` instead of panicking, so they are safe in
  production core: `content_ref`, `stat`, `ref_id`, and the infallible
  `dice_pool`. Constructor sugar: `Abilities::all(score)`,
  `RulesetId::dnd5e_2014()`, `Formula::{proficiency_bonus, ability_mod, dice}`,
  `Modifier::{add, add_when}`.
- Known cost: `StatblockBuilder` mirrors `StatblockBody` field by field, so a new
  body field is added twice. A declarative setter macro is the escape hatch if the
  list keeps growing (section 9).

## 9. Known limits, deferred work, open questions

These are deliberate, not oversights. Each has a reason and a trigger.

- **`AnyEntity` and `InventoryItem::Inline` large variants** are unboxed, carrying
  an `#[expect(clippy::large_enum_variant)]`. Boxing changes the public shape and
  is deferred; acceptable at dozens of entities per campaign.
- **`Entity`/`EntityError` validate split.** The kind/body family check runs
  inside `Deserialize`, so the loader cannot surface a typed semantic error
  (`LoadError` has no `Entity` variant). The fix is an `Entity::validate` split;
  not needed until an import path wants to name the failure.
- **`#[non_exhaustive]`:** used today only on the two error enums (`BuildError` in
  core, `LoadError` in the adapter). It is *not* yet on the payload data enums
  (`FormulaExpr`, `DiceExpr`, `Condition`, `FeedPayload`, `ResourceKind`), so
  additions there are breaking for exhaustive matches. Plan: one deliberate
  `#[non_exhaustive]` + fallback-variant pass before 1.0 / crate extraction, while
  all consumers are in-workspace.
- **`Audience` as a set:** `Public`/`RollerAndDm`/`RollerOnly` covers the discussed
  cases. "DM reveals to one player" needs an audience set and a feed-document
  migration (pure JSON over the ring).
- **`ViewField` granularity** is section-level ("show AC", not "show HP max but
  hide current"); split only if a real share scenario needs it (breaking).
- **`StatusActive { name }`** matches by name snapshot; widen to name + optional
  ref when packs start renaming conditions.
- **`weight_oz`** is unusual for pack authors; the loader can accept pounds as
  sugar later without changing the stored type.
- **`tools`/`languages`** are single free-text fields, not id-carrying lists; the
  stable-id rule bites only if the UI wants per-language checkboxes.
- **`ext`** is on campaign, entity, item and feature only; add per-type `ext` when
  a real plugin need appears (additive).
- **No string interning:** name snapshots are intentionally duplicated; interning
  is a later, additive optimization (but a breaking change to public field types).
- **`RefId` has no explicit length cap.** It stores the canonical string once plus
  two `u32` boundaries; the spans assume a reference fits in 4 GiB, which any sheet
  does. If a hard input cap is wanted, it belongs with input validation.
