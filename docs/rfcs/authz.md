# RFC: U6 -- authorization, entity views and read services

| | |
|---|---|
| **Status** | Spec phase (2026-10-18, branch `u4-u6-spec`). This RFC and `crates/localtt-integration/tests/authz_target.rs` are the first U6 artifacts. No library code exists: the target test intentionally does not compile until the U5 and U6 skeletons land (it consumes U5's `Executor::execute_as`, `Command::{Roll, SpendHitDie, CastSpell}`, `RollSpec`, `EngineError::Environment` and `ElementRef::Spell`), then fails with `not yet implemented` until the implement phase. U6 fills the authorization body of the `execute_as` seam that `dice.md` section 4.4 declares and settles `engine-executor.md` section 10's "`Unauthorized` producers and feed visibility" item. |
| **Scope** | New `localtt-engine::authz` (access predicates, view types, feed visibility) and `localtt-engine::services` (list/get/feed reads); the authorization pre-check inside `Executor::execute_as`; the first `EngineError::Unauthorized` producers. Target test: `crates/localtt-integration/tests/authz_target.rs`. The core data model is not touched; every missing wire field is an Open Question, not an edit. |
| **Read with** | `docs/rfcs/dice.md` (the `execute_as` seam, `RollSpec`, `Environment`, the audiences consumed here); `docs/rfcs/rules.md` (deferred derived values, OQ-1); `docs/rfcs/engine-executor.md` (OQ-1 A-revisit, OQ-5, deferred `Unauthorized` producers); `docs/engine-architecture.md` sections 7.1, 7.2, 7.3, 7.7, 7.8, 7.9, 9, 10 (U6); `docs/decisions.md` DATA-9/DATA-10, ENG-7/ENG-8; `RFC-0001-localtt.md` sections 2.11 and 5.7; `crates/localtt-core/DESIGN.md` sections 3.4, 3.8; this batch's `rules.md`, `dice.md` and `docs/plans/u4-u6.md`. |
| **Pins** | The engine-owned view types replace the arch 7.7 sketch (`body: Option<EntityViewBody>` instead of `statblock: Option<StatblockView>`, no derived values, `ext` included); every difference is a decision (section 8). U6 has **no skeleton-blocking Open Question**; the U6 skeleton depends on the U5 skeleton only for `execute_as` and the command variants it dispatches. |

## 1. Goals

1. Pin the access model: roster membership, DM and owner, grant levels, masks,
   and how multiple grants combine.
2. Pin `authz::{can_read, can_write, view_of, entry_visible}`.
3. Pin the engine-owned view types and the exhaustive `ViewField` -> view
   section mapping, including `hidden`, `writable`, the deny stub and the
   serialization contract.
4. Pin the read services `services::{list_entities, get_entity, feed_after}`
   and their error order.
5. Pin feed visibility derivation from `Audience` (the full viewer x audience
   matrix) and the feed view shape.
6. Pin the `execute_as` authorization pre-check: which commands are DM-only,
   which need `can_write`, which need a read, which need membership, and where
   authorization sits relative to every other failure.
7. Pin the first `EngineError::Unauthorized` producers and their payloads.

## 2. Non-goals

- **No ACL editing.** `SetAcl` and sharing commands stay future work; U6 reads
  the frozen core `AclEntry`/`FieldMask` shapes.
- **No authorization inside commands.** Commands stay authz-free; one
  pre-check in `execute_as` (decision D1). `apply`/`apply_as` never reject
  for access.
- **No derived values in views.** `view_of` is a pure field mirror; the
  `derived(id, viewer)` service, derived AC and every other computed stat are
  a later unit (rules.md section 10, deferred here).
- **No field-level write rules.** Write grants ignore their masks (pinned);
  read masks are the only field masks the engine applies.
- **No campaign-level authorization.** Clock commands are DM-only by role;
  campaign CAS returns with U7 (engine-executor OQ-1 A-revisit).
- **No token or join authentication.** The engine speaks `PlayerId`; tokens,
  invites and sessions are U7.
- **No event/feed fan-out to recipients.** Events stay id/rev delivery hints
  (ENG-7); the per-recipient payload is U7's concern, and it re-reads through
  `view_of`.
- **No core edits.** `ViewField`, `FieldMask`, `AclEntry`, `Audience`,
  `FeedEntry` and the body types are consumed as they are; gaps are Open
  Questions (section 9).

## 3. Client usage: the target test

`crates/localtt-integration/tests/authz_target.rs` is ordinary client code: it
imports `localtt_engine::authz`, `localtt_engine::services`, the U5 command
surface, the golden `fixtures` party and a `MemoryStore`; it adds a roster of
extra members and three test entities (the party's pc/note/handout with
test-local ACLs), drives a real `Executor` through `execute_as`, and reads
through the read services. Eighteen tests:

| Test | What it pins |
|---|---|
| `roster_membership_gates_every_egress` | a non-member is denied even when named owner (the DM still reads); a member with no grant sees nothing; a `Public` feed entry is visible to members and to nobody else |
| `full_readers_see_every_section_and_cannot_write` | owner and DM see every section, `writable`, headers mirrored (`id`, `kind`, `owner`, `parent`, `rev`); `Read(None)` is a full read with `writable: false`; no grant yields the stub (`hidden` all, no name/ext/body) |
| `write_grants_ignore_their_masks` | `Read{Name}` + `Write(empty)` reads and writes everything; a lone `Write(empty)` on a note does the same |
| `read_masks_default_deny` | `Read{Name, Hp}` exposes exactly those; `Read{Ac}` authorizes the read but exposes no body (`Ac` has no carrier) |
| `an_empty_mask_denies_like_no_grant` | `Read{}` denies read and write and produces exactly the deny stub |
| `grants_combine_permissively` | two `Read` masks union (`{Name}+{Hp}`); `{Notes}+{}` keeps `Notes` (an empty mask never subtracts); `Read` + `Write` resolves to full read |
| `sections_map_into_the_statblock_view` | a mask of everything but `{Name, Ext, Ac}` carries every statblock section; hiding `Identity` hides all seven identity sections together |
| `notes_gate_note_and_handout_bodies` | a note's only carrier is `Notes`; a handout's is `Notes`; a statblock with only `Notes` visible keeps the statblock body with notes and nothing else |
| `serialized_views_never_carry_hidden_sections` | across eight masks (name only, hp only, identity, abilities+proficiencies, ac, notes, saves+skills, all): `hidden` is the exact complement, absent sections have no JSON pointer at all, no marker string of a hidden field appears anywhere in the serialized JSON, and the `exhaustion`/`inspiration` riders survive every mask while a body is present |
| `list_entities_filters_by_read_and_sorts_by_id` | DM sees all ascending; the owner sees their pc `writable`; a reader sees the note; a masked reader sees both shares masked; no grant and non-members see nothing; the list view equals the get view |
| `get_entity_reports_missing_before_unauthorized` | readable -> view; existing but unreadable -> `Unauthorized{ actor, entity: Some }`; missing -> `NotFound{ entity, element: None }`, even for a stranger |
| `feed_visibility_follows_the_audience_matrix` | the 6 entries x 4 viewers matrix: `Public` any member; `RollerAndDm` roller + DM (no actor -> DM only); `RollerOnly` roller only, DM excluded (no actor -> nobody) |
| `feed_after_pages_visible_entries_ascending` | ascending ids, invisible entries skipped, exclusive cursor (including a cursor on an invisible position), the limit applies to the page, `0` is an empty page; the view mirrors the entry and never carries `audience` (exact key set) |
| `clock_commands_are_dm_only` | `AdvanceClock`/`SetClock` from a non-DM: `Unauthorized{ actor, entity: None }` with the state unchanged; the DM succeeds with exactly `[CampaignChanged]` and no warnings |
| `write_commands_require_write_access` | `ApplyDamage` denied for no-grant and read-mask viewers (HP untouched), allowed for the writer and the owner; on a note, a write grant passes the pre-check and the command's own `Malformed` (kind) answers next, while a reader is still `Unauthorized` |
| `rolls_require_read_and_hit_dice_require_write` | `Roll{entity: Some}` needs `can_read` (an empty mask denies, a grant elsewhere denies, a one-field mask passes to `Environment`); `Roll{entity: None}` needs roster membership; `SpendHitDie` needs `can_write` |
| `the_trusted_path_performs_no_authorization` | `execute` (no actor) applies damage and moves the clock: the system path is unchecked |
| `authorization_precedes_validation_and_cas` | an unauthorized actor gets `Unauthorized` ahead of command `Malformed`, stale-rev `Conflict` and element `NotFound`; the same requests from an owner reach `Conflict`/`Malformed`/`NotFound{ Spell }`; a missing target is `NotFound` for anyone, never `Unauthorized` |

Notes:

- **It does not compile yet, on purpose.** `localtt_engine::authz`,
  `localtt_engine::services` and `localtt_engine::commands::RollSpec` are
  unresolved, and `Executor::execute_as`, `Command::{Roll, SpendHitDie,
  CastSpell}`, `EngineError::Environment` and `ElementRef::Spell` do not
  exist yet. The U5 skeleton declares the command surface, the U6 skeleton
  the authz types; the spec-phase contract is exactly this.
- **Fixtures are consumed unchanged.** Helpers clone the golden party before
  mutating; test-local members are appended to the roster by a local
  `add_player`; the entity ACLs are replaced per test.
- **Deterministic inputs only:** fixed `FIXED_TIME`, campaign second 0, no
  wall clock, feed ids literal, and no assertions on ids the test did not
  mint.
- **The fail-closed environment is the probe.** `FixedEnv` implements only
  `now()`; the defaulted `roll`/`reference` ports fail closed, so a command
  that passes the pre-check surfaces `Environment` instead of a dice result.
  That is how the tests prove "authorized but stopped deeper".
- **One marker creature covers the string-carrying sections.** `marked_creature`
  writes a distinctive string into every section that can carry one (seventeen
  markers over eleven gated fields; numeric-only sections are covered by the
  pointer sweep, which asserts key presence); the serialized sweep checks that
  a hidden section's marker cannot appear anywhere in the JSON, not merely in
  its own field.

## 4. Public API sketch

Sketches, not compilable code; signatures are the contract. Core types
(`Campaign`, `AnyEntity`, `EntityKind`, `EntityId`, `PlayerId`, `Role`,
`AclEntry`, `AclLevel`, `FieldMask`, `ViewField`, `Profile`, `Identity`,
`Abilities`, `Hp`, `Speed`, `DeathSaves`, `Coins`, `ChallengeRating`,
`StatKey`, `OverrideValue`, `Resource`, `SpellcastingBlock`,
`InventoryEntry`, `AttackEntry`, `FeatureEntry`, `Status`, `Ext`, `Audience`,
`FeedEntry`, `FeedEntryId`, `FeedPayload`, `CampaignTime`, `UnixTime`,
`HandoutContent`, `Ability`, `Skill`, `SkillProficiency`, `Size`, `ClassLevel`)
and the U3 `WorkingSet` trait are imported and elided; `BTreeMap`/`BTreeSet`
come from `std::collections`. Every public item lands documented (both crates
deny `missing_docs`).

### 4.1 `localtt_engine::authz`

```rust
// localtt_engine::authz

/// True when `viewer` may read at least one section of `entity`: roster
/// membership is required first, then DM, owner, any `Write` grant, or at
/// least one visible field from the `Read` grants (section 5.2).
pub fn can_read(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> bool;

/// True when `viewer` may write `entity`: roster membership, then DM, owner
/// or any `Write` grant; masks are ignored (section 5.2).
pub fn can_write(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> bool;

/// The viewer's mirror of one entity: mirrored headers, the visible sections,
/// the hidden set and the write flag. Never a redacted clone (ENG-8).
pub fn view_of(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> EntityView;

/// True when `viewer` may see the feed entry (section 5.5).
pub fn entry_visible(campaign: &Campaign, entry: &FeedEntry, viewer: PlayerId) -> bool;

/// An engine-owned mirror of one entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntityView {
    /// Mirrored entity id.
    pub id: EntityId,
    /// Mirrored entity kind.
    pub kind: EntityKind,
    /// Mirrored owner.
    pub owner: PlayerId,
    /// Mirrored handout/companion parent.
    pub parent: Option<EntityId>,
    /// Mirrored document revision.
    pub rev: u64,
    /// Whether this viewer may write; `can_write` for the same arguments.
    pub writable: bool,
    /// Every section this viewer may not see; `ALL \ visible`.
    pub hidden: BTreeSet<ViewField>,
    /// Visible name; absent when `Name` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Visible plugin bucket; absent when `Ext` is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<Ext>,
    /// The visible body family; absent when no carrier section is visible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<EntityViewBody>,
}

/// The three visible body families (core `BodyKind`), tagged like the core
/// document bodies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntityViewBody {
    /// A statblock family entity.
    Statblock(StatblockView),
    /// A session note.
    Note(NoteView),
    /// A handout.
    Handout(HandoutView),
}

/// One statblock as a viewer sees it: one `Option` per carrier field.
///
/// Where the core field is already an `Option` (`cr`, `creature_type`), the
/// view keeps the core type; the visibility signal lives in `hidden`, so an
/// absent value and a hidden section stay distinguishable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StatblockView {
    /// Profile; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Identity box; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
    /// Class levels; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<ClassLevel>>,
    /// Challenge rating; gated by `ViewField::Identity`, mirrors the core
    /// `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cr: Option<ChallengeRating>,
    /// Creature type; gated by `ViewField::Identity`, mirrors the core
    /// `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creature_type: Option<String>,
    /// Size; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Senses; gated by `ViewField::Identity`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub senses: Option<String>,
    /// Ability scores; gated by `ViewField::Abilities`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abilities: Option<Abilities>,
    /// Saves, skills, tools and languages; one gate each (section 5.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proficiencies: Option<ProficienciesView>,
    /// Hit points; gated by `ViewField::Hp`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp: Option<Hp>,
    /// Movement speeds; gated by `ViewField::Speed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<Speed>,
    /// Death saves; gated by `ViewField::DeathSaves`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_saves: Option<DeathSaves>,
    /// Coins; gated by `ViewField::Coins`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coins: Option<Coins>,
    /// Derived-value overrides; gated by `ViewField::Overrides`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<BTreeMap<StatKey, OverrideValue>>,
    /// Resources; gated by `ViewField::Resources`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Vec<Resource>>,
    /// Spellcasting blocks; gated by `ViewField::Spells`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spellcasting: Option<Vec<SpellcastingBlock>>,
    /// Inventory; gated by `ViewField::Inventory`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Vec<InventoryEntry>>,
    /// Attack lines; gated by `ViewField::Attacks`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attacks: Option<Vec<AttackEntry>>,
    /// Features; gated by `ViewField::Features`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<FeatureEntry>>,
    /// Statuses; gated by `ViewField::Statuses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<Status>>,
    /// Notes; gated by `ViewField::Notes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Exhaustion level; an always-visible rider (section 9, OQ-1).
    pub exhaustion: u8,
    /// Inspiration flag; an always-visible rider (section 9, OQ-1).
    pub inspiration: bool,
}

/// The four maskable parts of a statblock's proficiencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProficienciesView {
    /// Save proficiencies; gated by `ViewField::Saves`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saves: Option<BTreeSet<Ability>>,
    /// Skill proficiencies; gated by `ViewField::Skills`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<BTreeMap<Skill, SkillProficiency>>,
    /// Tool lines; gated by `ViewField::Proficiencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<String>,
    /// Language lines; gated by `ViewField::Proficiencies`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<String>,
}

/// A session note as a viewer sees it; present iff `Notes` is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteView {
    /// The note text.
    pub text: String,
}

/// A handout as a viewer sees it; present iff `Notes` is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HandoutView {
    /// The handout payload.
    pub content: HandoutContent,
}

/// One feed entry as a viewer sees it: the entry minus its audience.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FeedEntryView {
    /// Feed position.
    pub id: FeedEntryId,
    /// Wall-clock stamp.
    pub ts: UnixTime,
    /// Campaign-clock stamp.
    pub clock_s: CampaignTime,
    /// Acting player, when the entry has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<PlayerId>,
    /// Related entity, when the entry has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// What happened.
    pub payload: FeedPayload,
}
```

### 4.2 `localtt_engine::services`

```rust
// localtt_engine::services

/// Every entity the viewer may read, ascending id.
///
/// Infallible: ids come from `WorkingSet::headers`, and an id its own working
/// set cannot resolve (absent or a store fault) is skipped (it cannot happen;
/// engine unit tests pin the behavior).
pub fn list_entities(state: &dyn WorkingSet, viewer: PlayerId) -> Vec<EntityView>;

/// One entity view for one viewer: missing before unauthorized.
///
/// `NotFound { entity, element: None }` when the id is absent;
/// `Unauthorized { actor: viewer, entity: Some(id) }` when present but
/// unreadable; otherwise the same view `list_entities` would return.
pub fn get_entity(
    state: &dyn WorkingSet,
    id: EntityId,
    viewer: PlayerId,
) -> Result<EntityView, EngineError>;

/// One ascending page of feed entries the viewer may see.
///
/// `cursor` is exclusive (`id > cursor`), the cursor may name an invisible
/// entry, `limit == 0` is an empty page, and the returned views drop the
/// audience (section 5.5).
pub fn feed_after(
    state: &dyn WorkingSet,
    viewer: PlayerId,
    cursor: Option<FeedEntryId>,
    limit: usize,
) -> Vec<FeedEntryView>;
```

### 4.3 The `execute_as` pre-check

`Executor::execute_as` is declared by U5 (`dice.md` section 4.4); U6 fills
its authorization body. No new public surface beyond the private classifier
inside `localtt-engine::executor`:

```rust
// localtt_engine::executor (private)

/// What a command needs from the actor before any command work runs.
enum Requirement {
    /// The actor must be this campaign's DM (`AdvanceClock`, `SetClock`).
    Dm,
    /// The actor must be a roster member; no entity is involved.
    Member,
    /// The actor must be able to read the entity.
    Read(EntityId),
    /// The actor must be able to write the entity.
    Write(EntityId),
}
```

### 4.4 `localtt_engine::error` producers

No new variants. `EngineError::Unauthorized { actor, entity }` (arch 7.3,
already declared) gains its first producers:

```rust
// existing; first produced by U6
/// The actor may not read/write the target entity or perform the command.
Unauthorized {
    /// The acting player.
    actor: PlayerId,
    /// The target entity; `None` for campaign-level denials and free rolls.
    entity: Option<EntityId>,
},
```

`can_read`/`can_write`/`entry_visible` answer `bool`; only the executor
pre-check and `services::get_entity` construct `Unauthorized`.

## 5. Guarantees

### 5.1 Membership and roles

- `member(campaign, viewer)` is `campaign.players` containing `viewer`, any
  role. Membership is the first gate of every predicate: a non-member is
  denied even when the document names them owner, and a non-member's `view_of`
  is the deny stub (section 5.4). The DM, an owner or a grant on another
  entity never leak into a non-member's egress.
- `dm(campaign, viewer)` is `member && role == Role::Dm`. The DM reads and
  writes every entity and sees every feed entry except a `RollerOnly` entry
  the DM did not roll (5.5); that is social policy, not a cryptographic
  boundary (RFC-0001 2.11).
- Ownership is `entity.owner == viewer`, subject to membership.

### 5.2 Grants, levels and masks

For one entity and one member viewer, let the viewer's entries be
`acl.iter().filter(entry.player == viewer)`:

- `write` iff `dm || owner || any entry.level == AclLevel::Write`. A `Write`
  grant implies full read; its `field_mask` is ignored entirely, including an
  empty mask (`write_grants_ignore_their_masks`).
- Otherwise the visible set `V` is the union over the viewer's `Read` entries
  of `mask == None ? ALL : mask`. An entry with `Some(empty)` contributes
  nothing; an empty union denies.
- `can_read` iff `write || V != {}`; `can_write` iff `write`. The deny stub
  and the empty-mask stub are the same view.
- Grants combine permissively: duplicates, several `Read`s and a `Write` all
  resolve as above; nothing subtracts (`grants_combine_permissively`).
- Masks are honored on every entity, not only DM-owned shares; the core
  `AclEntry.field_mask` doc names DM-owned shares as the intent (section 9,
  OQ-4).
- A mask listing only fields the entity kind cannot carry still authorizes the
  read (`Read{Ac}` on a pc) and exposes whatever it can; "authorized but
  empty" is not an error.
- `hidden` is exactly `ALL \ V`; `writable` is `can_write`. `ALL` is the 20
  core `ViewField` variants, exhaustively matched in the engine (the enum is
  not `#[non_exhaustive]`).

### 5.3 View construction and the section map

`view_of` mirrors the headers (`id`, `kind`, `owner`, `parent`, `rev`) and
materializes sections from `V`:

| `ViewField` | Carrier in the view | Notes |
|---|---|---|
| `Name` | `view.name` | no body carrier |
| `Ext` | `view.ext` | one bucket for all plugin data |
| `Identity` | `profile`, `identity`, `levels`, `cr`, `creature_type`, `size`, `senses` | the seven travel together |
| `Abilities` | `abilities` | |
| `Saves` | `proficiencies.saves` | |
| `Skills` | `proficiencies.skills` | |
| `Proficiencies` | `proficiencies.tools`, `proficiencies.languages` | |
| `Hp` | `hp` | |
| `Ac` | (none) | maskable; carrier waits for the derived service (OQ-7) |
| `Speed` | `speed` | |
| `DeathSaves` | `death_saves` | |
| `Coins` | `coins` | |
| `Overrides` | `overrides` | |
| `Resources` | `resources` | |
| `Spells` | `spellcasting` | |
| `Inventory` | `inventory` | |
| `Attacks` | `attacks` | |
| `Features` | `features` | |
| `Statuses` | `statuses` | |
| `Notes` | `notes` (statblock), `NoteView.text`, `HandoutView.content` | per body family |

- The body gate: `body.is_some()` iff at least one carrier field (all `20`
  minus `Name`, `Ac`, `Ext`) is visible. The body variant follows the entity
  kind: the six statblock kinds (`Pc`, `Companion`, `Summon`, `Familiar`,
  `Npc`, `Monster`) -> `Statblock`; `Note` -> `Note`; `Handout` ->
  `Handout`.
- Within `StatblockView`, each carrier is present iff its `ViewField` is
  visible; `cr` and `creature_type` additionally mirror the core `Option`
  (visible but absent reads as `None`; `hidden` distinguishes it). The
  `exhaustion` and `inspiration` riders are always serialized when the body
  exists.
- `NoteView.text` and `HandoutView.content` are whole-section payloads: the
  section is either fully visible or absent.

### 5.4 The deny stub and the serialization contract

- The deny stub: `hidden == ALL`, `name`/`ext`/`body` absent, `writable:
  false`, headers mirrored. It is byte-identical for "no grant" and for an
  empty mask.
- Hidden sections are **absent keys**, not `null`s
  (`skip_serializing_if = "Option::is_none"` on every optional field). The
  serialized sweep asserts exact JSON pointers, so a null would fail.
- No value of a hidden section may reach the wire: the marker sweep checks the
  whole serialized string, so a leak anywhere (including inside `ext` or a
  body that should not exist) fails the suite.
- `EntityViewBody` is internally tagged `type` (`statblock`, `note`,
  `handout`), following the core payload-enum convention (`tag = "type"`,
  `rename_all = "snake_case"`). The serialized sweep and the note/handout tests
  assert the tag value, not just the body's presence.
- `FeedEntryView` serializes exactly `id`, `ts`, `clock_s`, `actor`,
  `entity`, `payload`; it never carries `audience` (section 5.5).

### 5.5 Feed visibility and paging

- `entry_visible`: membership first, then
  - `Audience::Public` -> any member;
  - `Audience::RollerAndDm` -> `entry.actor == Some(viewer) || dm(viewer)`;
    with no actor, DM only;
  - `Audience::RollerOnly` -> `entry.actor == Some(viewer)`; with no actor,
    nobody (the DM is deliberately excluded; `RollerOnly` is the player's
    private note).
- `feed_after` walks `WorkingSet::feed()` (ascending id), keeps entries with
  `id > cursor` that pass `entry_visible`, takes `limit`, and maps each to a
  `FeedEntryView` (audience dropped, payload cloned). A cursor naming an
  invisible or removed entry still pages by id; `limit == 0` returns nothing.
- Visibility is a read-side derivation only: entries keep their audience in
  the store, and the view is the only egress.

### 5.6 Read services and error order

- `list_entities` iterates `WorkingSet::headers()` (already ascending id),
  resolves each id through `WorkingSet::entity`, builds the same view
  `get_entity` would, and keeps it iff `can_read`. The list and the get views
  are equal for the same (entity, viewer) pair.
- `get_entity`: absent id -> `NotFound { entity: id, element: None }`; present
  but unreadable -> `Unauthorized { actor: viewer, entity: Some(id) }`;
  readable -> the view. Missing beats unauthorized: there is nothing to
  authorize.
- Neither service needs the store, the executor or the write path: read
  paths are pure functions of the loaded working set.

### 5.7 The authorization pre-check

Classification of the U3--U5 command surface:

| Command | Requirement | Denial payload |
|---|---|---|
| `AdvanceClock`, `SetClock` | `Dm` | `entity: None` |
| every other current variant (hp, resources, rests, statuses, concentration, casting, hit dice) | `Write(entity)` | `entity: Some(id)` |
| `Roll { entity: Some(id), .. }` | `Read(id)` | `entity: Some(id)` |
| `Roll { entity: None, .. }` | `Member` | `entity: None` |

- The pre-check runs before all command work: ahead of input validation
  (`Malformed`), CAS (`Conflict`), element lookup inside a present entity
  (`NotFound { element: Some(..) }`) and content resolution. An unauthorized
  request never observes whether the rest of the request was well formed.
- An absent target entity defers: the pre-check does not authorize an entity
  it cannot load, and the command's own `NotFound { entity, element: None }`
  is the answer, for any actor. There is nothing to authorize and nothing to
  leak.
- A denial produces no transition: no upserts, no feed appends, no events, no
  warnings, and the working set and store are untouched.
- The pre-check never mutates: it reads `WorkingSet::campaign` and the target
  entity only.
- The trusted path is untouched: `Executor::execute` (and the engine-internal
  `apply`/`apply_as` entry points) perform no authorization. Serving a client
  through them is a server bug by definition; U7 routes every client mutation
  through `execute_as`.

## 6. Internal structure

```
engine/src/
  authz/
    mod.rs      public surface: can_read, can_write, view_of, entry_visible
    access.rs   membership, roles, grant/mask algebra, feed audience predicate
    view.rs     EntityView and the ViewField section map, serialization
  services.rs   list_entities, get_entity, feed_after over WorkingSet
  executor.rs   execute_as: classify -> pre-check -> the U5 apply seam
```

- `authz` is pure over core documents: campaign and entity are arguments, no
  ports, no working set, no I/O. Unit tests build literal campaigns and
  entities.
- `services` depends on `WorkingSet` (read-only) and `authz`; it never touches
  the store or the executor.
- `executor` grows the private classifier and the pre-check ahead of the U5
  `apply_as` seam; the `commands/` modules stay authz-free and unchanged.
- The engine owns one `[ViewField; 20]` list; the `slot` guard in its tests
  matches every variant exhaustively, so a core addition is a compile error
  (OQ-3, OQ-8).
- Engine unit tests at implement cover what the target test cannot: grant/mask
  algebra properties (union is monotone, the empty mask is neutral, `Write`
  dominates), duplicate ACL entries, a non-member owner, each of the 20
  single-field masks against a fully marked creature, cursor and limit edges,
  and a classifier sweep over every command variant.

## 7. API cross-check against the target test

| Target-test need | API element |
|---|---|
| membership and role gates | `can_read`, `can_write`, `entry_visible`, `Role::Dm` |
| grant levels and masks | `AclLevel`, `AclEntry`, `FieldMask`, `ViewField` (core) |
| hidden sections, write flag | `EntityView::{hidden, writable}` |
| the deny stub | `view_of` + section 5.4 |
| section mirroring | `EntityViewBody`, `StatblockView`, `ProficienciesView`, `NoteView`, `HandoutView` |
| marker/pointer sweep | `Serialize`, `skip_serializing_if`, `EntityViewBody` tagging |
| entity list and get | `services::{list_entities, get_entity}`, `WorkingSet::{headers, entity}` |
| error order | `EngineError::{NotFound, Unauthorized}` |
| feed pages | `services::feed_after`, `FeedEntryView`, `WorkingSet::feed` |
| audience matrix | `entry_visible`, `Audience`, `FeedEntry::{audience, actor}` |
| DM-only commands | `Executor::execute_as` + `Requirement::Dm` |
| entity commands | `Requirement::Write` |
| rolls | `Command::Roll`, `RollSpec` (U5), `Requirement::{Read, Member}` |
| denial payloads | `EngineError::Unauthorized { actor, entity }` |
| trusted path | `Executor::execute` unchanged |

Every element in the sketch is either used above or justified:
`can_read` exists as a standalone predicate because both the pre-check and the
list filter need it without building a view; `can_write` because commands and
the write flag need the same ruling; `entry_visible` because `feed_after` and
future WS fan-out share one predicate; `EntityView::parent` and `kind` mirror
headers for client routing; `Clone` on the views is the server's per-recipient
fan-out need (U7). Cut from the sketch: a header-only list variant, a derived
section carrier, `SetAcl` shapes, and any identity sub-field types -- no test
or client needs them yet.

## 8. Decisions and deviations (to fold after review)

| # | Deviation | Why |
|---|---|---|
| D1 | Authorization is one pre-check in `Executor::execute_as`; commands stay authz-free (arch 7.2: authorization inside `apply` via an actor carried in the command) | One ruling, one place, no actor parameter threaded through every handler; U5's seam already carries the actor for attribution, U6 checks before it. The trusted system path stays unchecked, as it must. |
| D2 | `EntityView.body: Option<EntityViewBody>` (arch 7.7: `statblock: Option<StatblockView>`) | Notes and handouts need a view home too; the envelope mirrors the core `AnyEntity` body families and tags `type` like the documents. |
| D3 | `EntityView.ext: Option<Ext>` added (arch 7.7 sketch omits it) | Without a carrier the plugin bucket is unreadable even to the owner; one option gates the whole bucket under `ViewField::Ext`. |
| D4 | `view_of(campaign, entity, viewer)` -- no resolved content, no derived values (arch 7.7: `view_of(entity, viewer, resolved_content)`) | Derived values are a service concern (rules.md section 10); U6 mirrors stored fields only. The signature grows a content argument when that unit lands. |
| D5 | `ViewField::Identity` gates seven sections; `Saves`/`Skills`/`Proficiencies` gate the four proficiency parts (DATA-9: section-level, finer deferred) | Core's mask vocabulary is the frozen wire; the target test pins exactly this mapping. |
| D6 | `exhaustion`/`inspiration` are always visible riders in a visible body | No core `ViewField` covers them; they are small, rules-relevant sheet state (OQ-1). |
| D7 | `ViewField::Ac` is maskable but carrier-less until the derived service | Core stores no AC; a fake field would lie. The override map survives inside `Overrides` (OQ-7). |
| D8 | Module is `localtt_engine::services` (flat) (arch 7.1: `service/mod.rs`) | Matches the crate's plural `commands`; can become a directory without an API change. The test pins the path. |
| D9 | `feed_after` pages ascending with an exclusive cursor; `FeedEntryView` never carries `audience` (arch 7.8 names the function only) | Ascending mirrors the feed's storage order and keeps cursors stable; the audience is a server-side gate, not client data (OQ-6). |
| D10 | Services take `&dyn WorkingSet` and `list_entities` is infallible (arch 7.8: "thin functions over the executor/working set") | Read paths need neither store nor executor; header ids come from the working set itself, so an unresolvable header is skipped as an invariant violation. |
| D11 | `Unauthorized` payload: `entity: None` for DM-only commands and free rolls, `Some(id)` otherwise; first producers | The payload is the arch 7.3 shape, already frozen; the tests pin both branches. |
| D12 | `can_read(campaign, entity, viewer)` / `can_write(campaign, entity, viewer)` take the campaign (arch 7.7: `can_read(entity, viewer)`) | Roster membership is the first gate and lives on the campaign; without it a non-member could read through a stray ACL row (section 5.2). |

## 9. Open questions

Every question states options, tradeoffs and a recommendation; the sketch
assumes the recommendation. **None of these blocks the U6 skeleton** (the
answer changes no signature the target test writes against).

### OQ-1: exhaustion and inspiration are not maskable (non-blocking)

Core has no `ViewField` for the two riders; a visible statblock body always
shows them.

- **A (recommended; what the target test pins):** keep them always visible
  beside a visible body. They are two tiny, rules-relevant fields; masking
  them is not a table need today.
- **B:** add `ViewField::Exhaustion`/`Inspiration` to core: a wire change,
  DATA entry and version bump for fields nothing hides; the engine's
  exhaustive map then covers them.
- **C:** hide them with `Identity`: surprising (a shared hp view would drop
  exhaustion) and still not individuated.

Recommendation: **A**; B is the versioned path if a table asks.

### OQ-2: identity group granularity (non-blocking)

`ViewField::Identity` gates profile, identity, levels, cr, creature_type,
size and senses together; DATA-9 defers finer granularity.

- **A (recommended; pinned):** one section key for the seven, as the frozen
  vocabulary offers.
- **B:** split core into finer variants (race/alignment/senses/cr/...): lets a
  share hide senses but keep cr; a wire change with a DATA entry, version bump
  and migration path for existing masks (split semantics must be decided).
- **C:** engine-side synthetic sub-fields beside the core enum: two mask
  vocabularies, the worst of both.

Recommendation: **A** now; **B** as its own versioned core proposal when a
real share needs it.

### OQ-3: the `ALL` set (non-blocking)

`hidden` is complement-based, so the engine needs the full `ViewField` set.

- **A (recommended):** a private engine-side `[ViewField; 20]`, constructed
  exhaustively so adding a core variant fails compilation.
- **B:** a core `ViewField::ALL` constant: convenient for every consumer, but
  a core edit (docs-only, additive) the frozen-core constraint pushes out.
- **C:** per-kind section sets (a note's ALL differs): attractive nowhere yet;
  `hidden` is complement over the section vocabulary, and empty masks still
  deny.

Recommendation: **A**; B is a cheap follow-up core proposal if other
consumers appear.

### OQ-4: masks on non-DM-owned shares (non-blocking)

Core's `AclEntry.field_mask` doc says masks are "for read-shares of DM-owned
entities", but the target test honors masks on any entity.

- **A (recommended; pinned):** honor any mask, wherever it appears. A mask is
  a stricter read in every reading; ignoring it on player-owned entities would
  silently promote the share to a full read.
- **B:** honor masks only on DM-owned entities, matching the doc text
  literally: surprising and less safe.
- **C:** validate at ACL-write time only (when `SetAcl` lands): orthogonal;
  document intent changes need a core DESIGN update through review.

Recommendation: **A**; a core DESIGN wording update is logged for the docs
pass after acceptance.

### OQ-5: multi-grant resolution (non-blocking)

The target test pins permissive combination (level max; masks union; empty
contributes nothing; `Write` and `None` dominate).

- **A (recommended; pinned):** the algebra in section 5.2.
- **B:** resolve the single "best" entry (highest level, then widest mask):
  same results for every case in the suite, but a second rule to specify and
  test for no gain.
- **C:** reject conflicts (`Malformed`) on a future `SetAcl`: the engine
  cannot enumerate every UI's share shapes; keep reads permissive.

Recommendation: **A**, pinned by engine property tests.

### OQ-6: feed paging direction (non-blocking)

- **A (recommended; pinned):** ascending pages with an exclusive cursor;
  stable under appends and the natural complement of "after".
- **B:** newest-first with an inclusive cursor: what many UIs want, but
  reverse iteration over a growing ring complicates the contract.
- **C:** a direction flag: an API surface decided before U7 knows the UI.

Recommendation: **A**; U7 can reverse a fetched page or request B explicitly.

### OQ-7: the `Ac` carrier (non-blocking)

- **A (recommended; pinned):** `Ac` stays maskable but carrier-less; the
  derived service adds the carrier with computed/override values.
- **B:** synthesize AC from `overrides["ac"]` now: computed AC is what almost
  every viewer needs, and a half-answer invites drift.
- **C:** drop `Ac` from `ViewField`: a wire change, and clients already use it
  in masks.

Recommendation: **A**; the derived-values unit owns the shape.

### OQ-8: `ViewField` variant additivity (non-blocking)

OQ-1/OQ-2 propose new core variants; new tags arrive from older/newer
documents.

- **A (recommended):** any new variant is its own versioned core proposal: a
  DATA entry, a minor version bump, tolerant loading policy (VER-2 accepts
  newer minors, re-stamps on load) and fixtures; masks from a newer version
  degrade loudly, never silently reinterpret.
- **B:** batch riders, identity split and any other granularity in one
  proposal now: fewer version bumps, but couples unrelated sharing needs to
  this unit's schedule.

Recommendation: **A**; the U6 deliverable is the list of candidates, not a
wire change.

## 10. Deferred items (not blocking the skeleton)

- ACL editing and sharing commands (`SetAcl`, handout recipients, owner
  changes): the write path for everything section 5.2 reads. Fixtures stand in
  until it lands.
- Derived values in views: the `derived(id, viewer)` service, derived AC, the
  `Ac` carrier and every computed stat (rules.md section 10; arch 7.7/7.8).
  Belongs to a derived-values unit after U5/U6, before or with U7.
- A header-only `list_entities` variant when payload size matters (arch 7.8).
- Reverse feed paging and cursor polish for the UI (OQ-6).
- `export(viewer)` (DM-only) and player sheet export: store/export work.
- `autocomplete` pass-through: U2's port, no authorization (arch 7.8).
- Campaign-level CAS for clock/settings writes: engine-executor OQ-1
  A-revisit, with U7.
- `FreezeReference` and the packs loader (U8).
- Server DTO mapping, per-recipient WS fan-out and join/token auth (U7);
  every client mutation routes through `execute_as` there.
- Content-consuming commands and the content map materialization they need:
  U2 runtime, allocated in `docs/plans/u4-u6.md`.
