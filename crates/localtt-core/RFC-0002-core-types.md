# RFC-0002: localtt-core types - the serializable domain data model

| | |
|---|---|
| **Status** | Draft (spec phase; no library code written yet) |
| **Audience** | The implementing agent(s) and the maintainer |
| **Scope** | `crates/localtt-core/src/types.rs` (single file for now; layout may split later). No engine, adapter or server code. |
| **Depends on** | RFC-0001 sections 4, 5.9, 6, 7, 9, 12 (binding shape); AGENTS.md (workflow, style) |
| **Target test** | `crates/localtt-core/tests/types.rs` (does not compile until the skeleton phase) |

This RFC fixes the data layout, the public API and the serde contract of the core
type module. It is deliberately a *data* document: no rules logic, no I/O, no
dependencies beyond `serde`, `serde_json` (for `ext` payloads), `uuid` (v7) and
`thiserror`.

## 0. Decisions carried in from review questions

The maintainer answered five API-shaping questions before this draft:

| # | Question | Decision |
|---|---|---|
| Q1 | Entity/body pairing | `Entity<Body>` generic type-state with a sealed body trait. Compile-time pairing, methods exist only where the data exists, no dead variants in memory, no invalid `Entity<Note>`-with-statblock states. |
| Q2 | `StatKey` | Hybrid: `StatKey::Known(KnownStat)` for the stats the engine understands, `StatKey::Custom(CustomStatKey)` as a validated escape hatch. Canonical wire strings are pinned by tests. |
| Q3 | `FieldMask` | `BTreeSet<ViewField>` over a closed section enum, default-deny when a mask is present (`None` mask means full read access). |
| Q4 | ID backend | `uuid` v7. |
| Q5 | ID newtypes | One generic `Id<T>` with `PhantomData<fn() -> T>`, aliased per domain type; plugins may mint `Id<TheirType>`. |

## 1. Goals / Non-goals

### 1.1 Goals

- Provide the serialization contract for everything the campaign file stores:
  campaign, players, ACLs, entities and their bodies, statblocks, items,
  resources, statuses, spellcasting, dice AST, rolls, feed, clock, versions.
- Make invalid states unrepresentable where it is cheap: kind/body pairing
  (type-state), ID domains, campaign time vs wall time, validated keys and refs.
- Make every "reference to external content" degrade to text through a stored
  name snapshot (RFC-0001 4.8).
- Stay rule-agnostic: the engine decides what is *computed*, core stores what is
  *typed in*. Warnings, never errors, is an engine property.
- Keep the public API flat enough to be consumed by the engine, the SQLite
  adapter, a future server and third-party plugins: owned data only, no
  lifetimes, no trait objects, no interior mutability.

### 1.2 Non-goals

- Rules logic (modifier evaluation, derived values, rest handling). Engine.
- Storage layout, SQL DDL, migrations procedure. Adapter + engine.
- Views/DTOs, redaction *logic* (`view_of` lives in the engine; core only
  provides `FieldMask`/`ViewField` as data), translations.
- Custom formulas / scripting extension points (reserved, not built).
- Frontend-specific shapes (snake_case JSON is the single contract).

## 2. Client usage

The target test `crates/localtt-core/tests/types.rs` is the fixed point of the
skeleton and implement phases. It is written as a client would use the crate:

- build a realistic level-5 character via the entity builder (abilities, levels,
  skills with expertise, HP, coins, resource with a formula max, spell block with
  a referenced and a homebrew spell, inventory with a referenced and an inline
  item, attack entries, features with modifiers, statuses including
  concentration, overrides, `ext`);
- round-trip that entity through `serde_json` and assert equality;
- pin the JSON shape by exact asserts on selected paths (`kind`, `body.hp`,
  override keys, modifier keys, canonical stat strings, enum spellings);
- build and dispatch the runtime-tagged `AnyEntity` for all three body kinds;
- the failure cases: `kind`/body mismatch at construction and at
  deserialization, invalid `RefId`, invalid custom `StatKey`, and
  `SchemaVersion::CURRENT` refusing a newer major version;
- compile-time `Send + Sync` assertions for the public types. Core types are
  plain data; there is no concurrent subsystem here, so the concurrency
  contract is exactly "`Send + Sync`, no interior mutability" rather than a
  contention test.

The test is authored against the API sketched in section 3 only; it is not
expected to compile before the skeleton phase produces the signatures with
`todo!()` bodies.

## 3. Public API sketch

### 3.1 Module layout

For now everything lives in `src/types.rs` with section comments in this order;
when the file is split later, the sections become modules and everything is
re-exported from `types` so that no public path changes:

1. ids, 2. primitives (time, version, namespace, refs, stat keys), 3. campaign
and players, 4. ACL and view fields, 5. entities and bodies, 6. statblock
sections, 7. items, 8. modifiers and formulas, 9. resources and statuses,
10. spellcasting, 11. dice, rolls and feed, 12. errors.

`lib.rs` gains exactly one line: `pub mod types;`.

Conventions used throughout:

- Every public type derives `Clone`, `Debug`, `PartialEq`, `Eq` and implements
  `Serialize`/`Deserialize`; `Hash`/`Ord` only where used as keys.
- No `f32`/`f64` anywhere: deterministic serialization, `Eq`, no NaN questions.
- No `#[non_exhaustive]` yet (see Open Questions 6.6); additions are minor
  version bumps during the pre-1.0 phase.
- Plain data structs have public fields; only invariant types (ids, refs, keys,
  versions, ratings, times) keep fields private with accessors.
- Option fields are skipped when serializing (`skip_serializing_if`), but
  deserialization accepts both absent and `null`.

### 3.2 Identifiers

```rust
/// Opaque, time-sortable (v7) identifier with a type-level domain tag.
pub struct Id<T>(/* uuid::Uuid, PhantomData<fn() -> T> */);

impl<T> Id<T> {
    /// Generates a fresh v7 id.
    pub fn new() -> Self;
    /// Wraps an existing UUID (import, migration, tests).
    pub fn from_uuid(uuid: uuid::Uuid) -> Self;
    pub fn as_uuid(&self) -> uuid::Uuid;
}
// Clone/Copy/Debug/PartialEq/Eq/Hash/PartialOrd/Ord/Display implemented by hand
// without a `T: Trait` bound; serde(transparent) over the UUID string, with
// `bound = ""` so `T` needs no serde impls.

pub type CampaignId = Id<Campaign>;
pub type PlayerId   = Id<Player>;
pub type EntityId   = Id<AnyEntity>;      // an id of an entity of any body kind
pub type StatusId   = Id<Status>;
pub type ResourceId = Id<Resource>;
pub type SpellcastingBlockId = Id<SpellcastingBlock>;
pub type SpellEntryId        = Id<SpellEntry>;
pub type ClassLevelId        = Id<ClassLevel>;
pub type InventoryEntryId    = Id<InventoryEntry>;
pub type AttackEntryId       = Id<AttackEntry>;
pub type FeatureEntryId      = Id<FeatureEntry>;
pub type RollId              = Id<Roll>;

/// Monotonic feed position. Not a UUID: it is a per-campaign counter.
pub struct FeedEntryId(/* u64 */);
```

`EntityId = Id<AnyEntity>` avoids inventing a marker type: an entity id is
"the id of an entity, whichever body it has", and `AnyEntity` (3.5) is exactly
that. IDs are opaque: no ordering guarantees are documented beyond uuid v7
generation order.

### 3.3 Primitives: time, versions, namespaces, refs, stat keys

```rust
/// Seconds on the DM-controlled campaign clock (RFC-0001 5.3).
pub struct CampaignTime(/* i64 */);

/// Unix seconds. Wall clock, for audit/display only, never for durations.
pub struct UnixTime(/* i64 */);

impl CampaignTime { pub fn from_seconds(s: i64) -> Self; pub fn seconds(self) -> i64; }
impl UnixTime     { pub fn from_seconds(s: i64) -> Self; pub fn seconds(self) -> i64; }
// Both: transparent serde as plain numbers, Display, Ord, arithmetic-free.

/// Document schema version, major.minor (RFC-0001 5.9).
pub struct SchemaVersion { pub major: u16, pub minor: u16 }
impl SchemaVersion {
    /// The version this build writes.
    pub const CURRENT: Self;
    /// True when this build may load `file`: same or older major.
    pub const fn accepts(self, file: Self) -> bool;
}

/// Ruleset identifier, e.g. "5e-2014". Lowercase [a-z0-9-], <= 32 chars.
pub struct RulesetId(/* String */);
impl RulesetId {
    pub const DND5E_2014: &'static str; // the default ruleset id string
    pub fn parse(raw: &str) -> Result<Self, RulesetIdError>;
}

/// Plugin data namespace, e.g. "com.example.foo". Lowercase dotted segments.
pub struct Namespace(/* String */);
impl Namespace { pub fn parse(raw: &str) -> Result<Self, NamespaceError>; }

/// Namespaced content reference: "<source>:<kind>/<slug>",
/// e.g. "srd2014:spell/fireball" or "pack.mytome:item/foo".
/// Internals: the canonical string plus two u32 segment boundaries (one
/// allocation; see review pass 8).
pub struct RefId(/* namespace, kind, slug */);
impl RefId {
    pub fn parse(raw: &str) -> Result<Self, RefIdError>;
    pub fn namespace(&self) -> &str; // "srd2014", "pack.mytome"
    pub fn kind(&self) -> &str;      // "spell", "item", "condition", ...
    pub fn slug(&self) -> &str;      // "fireball"
    pub fn as_str(&self) -> &str;
}
// Display prints the canonical "<source>:<kind>/<slug>" form; serde transparent.

/// A reference to shipped/pack content plus its name snapshot, so a missing
/// pack degrades to text instead of a broken sheet (RFC-0001 4.8).
pub struct ContentRef { pub id: RefId, pub name: String }

/// Either a reference or inline text (identity boxes, subclasses, spell sources).
#[serde(untagged)]
pub enum RefOrText { Ref(ContentRef), Text(String) }

/// Plugin payload slot: namespace -> arbitrary JSON.
pub type Ext = std::collections::BTreeMap<Namespace, Json>;
pub use serde_json::Value as Json; // re-export: ext payloads are JSON by design
```

```rust
/// The six 5e abilities.
pub enum Ability { Strength, Dexterity, Constitution, Intelligence, Wisdom, Charisma }
// serde: "str" | "dex" | "con" | "int" | "wis" | "cha";
// Display prints the abbreviation, FromStr parses it (exact, case-sensitive).

/// The eighteen 5e skills, serde slugs such as "sleight_of_hand".
pub enum Skill { Acrobatics, AnimalHandling, Arcana, Athletics, /* ... 18 total */ }

/// Stats the engine knows how to compute (RFC-0001 4.3/4.4/5.1).
pub enum KnownStat {
    Ac,                                    // "ac"
    Initiative,                            // "initiative"
    Speed,                                 // "speed"
    ProficiencyBonus,                      // "proficiency_bonus"
    AbilityScore(Ability),                 // "ability.str"
    AbilityModifier(Ability),              // "ability_mod.str"
    Save(Ability),                         // "save.str"
    Skill(Skill),                          // "skill.stealth"
    PassiveScore(Skill),                   // "passive.perception"
    Attack(AttackKind),                    // "attack.melee" | "attack.ranged" | "attack.spell"
    SpellDc,                               // "spell.dc"
}
pub enum AttackKind { Melee, Ranged, Spell }

/// Validated custom stat path, lowercase [a-z0-9_.-], 1..=64 chars,
/// non-empty dot segments, e.g. "house.luck".
pub struct CustomStatKey(/* String */);

/// A stat path: known to the engine, or a validated extension key.
pub enum StatKey { Known(KnownStat), Custom(CustomStatKey) }
impl StatKey {
    /// Parses canonical known strings first, then validates as custom.
    pub fn parse(raw: &str) -> Result<Self, StatKeyError>;
}
impl std::str::FromStr for StatKey { /* same as parse */ }
// Display: canonical string. Serialize: the string for both variants.
// Deserialize: canonical string -> Known, otherwise validated Custom. Exact
// (case-sensitive) matching; unknown spellings stay Custom and round-trip
// byte-identically.
```

`StatKey::Known` uses the canonical strings above. Note: RFC-0001 4.3 writes
`"skills.stealth"` once; 4.4 writes `"skill.stealth"`. We pin the *singular*
form from 4.4/5.1 (modifier targets are the normative list) and record it as a
decision, not an accident.

### 3.4 Campaign, players, clock

```rust
/// Singleton campaign document (RFC-0001 4.1).
pub struct Campaign {
    pub id: CampaignId,
    pub name: String,
    pub schema_version: SchemaVersion,
    pub default_ruleset: RulesetId,
    pub clock: Clock,
    pub settings: CampaignSettings,
    pub ext: Ext,
}

/// The DM-controlled campaign clock. Rounds are derived (6 s), not stored.
pub struct Clock { pub now_s: CampaignTime }

pub struct CampaignSettings {
    /// Default audience for player rolls (RFC-0001 5.6); default RollerAndDm.
    pub player_roll_audience_default: Audience,
    /// Ring buffer capacity of the feed (RFC-0001 4.7).
    pub feed_capacity: u32,
    /// Hour of day (0..=23) at which Dawn recharges happen (RFC-0001 5.3).
    pub dawn_hour: u8,
}

pub struct Player {
    pub id: PlayerId,
    pub display_name: String,
    pub role: Role,                       // Dm | Player ("dm" | "player")
    /// Hex-encoded digest of the join token; never the token itself.
    pub token_hash: Option<TokenHash>,
    pub created_at: UnixTime,
    pub last_seen: Option<UnixTime>,
}

pub struct TokenHash(/* String, lowercase hex */);

pub enum Role { Dm, Player }
```

Exactly one DM slot is a campaign invariant checked by the engine, not by the
type (`Vec<Player>` on the campaign file). `dawn_hour` is included because
RFC-0001 5.3 requires a configurable dawn; its display formatting is not
stored here.

### 3.5 Entities, bodies, ACL

```rust
/// The eight kinds of RFC-0001 4.2.
pub enum EntityKind { Pc, Companion, Summon, Familiar, Npc, Monster, Note, Handout }

/// The three body families. Note that six kinds share the statblock body.
pub enum BodyKind { Statblock, Note, Handout }
impl EntityKind { pub const fn body_kind(self) -> BodyKind; }

/// Sealed marker trait implemented only by the three body types.
pub trait Body: sealed + Clone + Debug + PartialEq + Eq + Send + Sync
    + serde::Serialize + serde::de::DeserializeOwned
{
    const BODY_KIND: BodyKind;
}

/// Common header plus a statically known body.
pub struct Entity<B: Body> {
    pub kind: EntityKind,
    pub id: EntityId,
    pub ruleset: RulesetId,
    pub name: String,
    pub owner: PlayerId,
    pub parent: Option<EntityId>,
    pub acl: Vec<AclEntry>,
    pub rev: u64,
    pub schema_version: SchemaVersion,
    pub ext: Ext,
    pub created_at: UnixTime,
    pub updated_at: UnixTime,
    pub body: B,
}

pub type StatblockEntity = Entity<StatblockBody>;
pub type NoteEntity      = Entity<NoteBody>;
pub type HandoutEntity   = Entity<HandoutBody>;

impl<B: Body> Entity<B> {
    /// Creates a builder with defaults: rev 0, schema_version CURRENT,
    /// empty ACL/ext, kind/body pairing checked in `build()`.
    pub fn builder(kind: EntityKind, id: EntityId, name: impl Into<String>,
                   owner: PlayerId, body: B) -> EntityBuilder<B>;
}
pub struct EntityBuilder<B: Body> { /* ... */ }
impl<B: Body> EntityBuilder<B> {
    pub fn ruleset(self, r: RulesetId) -> Self;
    pub fn parent(self, p: EntityId) -> Self;
    pub fn acl(self, acl: Vec<AclEntry>) -> Self;
    pub fn rev(self, rev: u64) -> Self;
    pub fn schema_version(self, v: SchemaVersion) -> Self;
    pub fn ext(self, ext: Ext) -> Self;
    pub fn created_at(self, t: UnixTime) -> Self;
    pub fn updated_at(self, t: UnixTime) -> Self;
    /// Fails with `EntityError::KindBodyMismatch` unless
    /// `kind.body_kind() == B::BODY_KIND` (e.g. Kind::Note with a statblock).
    pub fn build(self) -> Result<Entity<B>, EntityError>;
}

/// Runtime-tagged entity for storage, iteration and `view_of`: the body kind
/// is only known at runtime in those contexts.
pub enum AnyEntity {
    Statblock(Entity<StatblockBody>),
    Note(Entity<NoteBody>),
    Handout(Entity<HandoutBody>),
}
impl AnyEntity {
    pub fn id(&self) -> EntityId;
    pub fn kind(&self) -> EntityKind;
    pub fn name(&self) -> &str;
    pub fn owner(&self) -> PlayerId;
    pub fn parent(&self) -> Option<EntityId>;
    pub fn acl(&self) -> &[AclEntry];
    pub fn rev(&self) -> u64;
    pub fn schema_version(&self) -> SchemaVersion;
    pub fn ruleset(&self) -> &RulesetId;
    pub fn ext(&self) -> &Ext;
    pub fn created_at(&self) -> UnixTime;
    pub fn updated_at(&self) -> UnixTime;
    pub fn as_statblock(&self) -> Option<&Entity<StatblockBody>>;
    pub fn as_note(&self) -> Option<&Entity<NoteBody>>;
    pub fn as_handout(&self) -> Option<&Entity<HandoutBody>>;
}
impl From<Entity<StatblockBody>> for AnyEntity;
impl From<Entity<NoteBody>> for AnyEntity;
impl From<Entity<HandoutBody>> for AnyEntity;
```

`AnyEntity` exists so the type-state does not leak into storage, iteration and
redaction code: adapters load JSON, dispatch on the top-level `"kind"` tag, and
work with `AnyEntity`. Engine code that needs statblock data matches once and
stays in `Entity<StatblockBody>` afterwards.

ACL and field masks (RFC-0001 4.2, 5.7):

```rust
pub enum AclLevel { Read, Write } // "read" | "write"

pub struct AclEntry {
    pub player: PlayerId,
    pub level: AclLevel,
    /// Present and default-deny only for read-shares of DM-owned (or shared)
    /// entities: only the listed sections are exposed, everything else is
    /// redacted. `None` means the grant is not field-masked.
    pub field_mask: Option<FieldMask>,
}

/// A union of sheet sections. `Default` is empty = nothing visible.
pub struct FieldMask(/* BTreeSet<ViewField> */);
impl FieldMask {
    pub fn contains(&self, f: ViewField) -> bool;
    pub fn insert(&mut self, f: ViewField);
    pub fn is_empty(&self) -> bool;
    pub fn iter(&self) -> impl Iterator<Item = ViewField> + '_;
}

/// Redactable sheet sections, serde lowercase ("ac", "hp", "spells", ...).
/// `Ext` is a single bucket for all plugin data.
pub enum ViewField {
    Name, Identity, Abilities, Saves, Skills, Proficiencies, Hp, Ac, Speed,
    DeathSaves, Coins, Overrides, Resources, Spells, Inventory, Attacks,
    Features, Statuses, Notes, Ext,
}
```

Semantics fixed here (core stores, engine enforces): a `FieldMask` present on
the *effective* `Read` access hides everything not listed; `None` means full
read; `Write` access (owner, DM, co-control) ignores masks. Owners and the DM
always see their own entities unmasked.

Errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum EntityError {
    /// kind and body belong to different families (Q1 failure case).
    #[error("kind {kind:?} cannot carry a {body_kind:?} body")]
    KindBodyMismatch { kind: EntityKind, body_kind: BodyKind },
}
// Further parse errors: RefIdError, RulesetIdError, NamespaceError,
// StatKeyError, ChallengeRatingError - all plain enums, Display + Error.
```

Serde for entities. Entity documents are flat:

```json
{
  "kind": "pc",
  "id": "01924f...",
  "ruleset": "5e-2014",
  "name": "Aria",
  "owner": "...", "parent": null, "acl": [], "rev": 3,
  "schema_version": { "major": 1, "minor": 0 },
  "ext": {},
  "created_at": 1750000000, "updated_at": 1750000000,
  "body": { }
}
```

`Entity<B>` serializes through an internal `EntityFields<B>` proxy and
deserializes through the same proxy followed by a kind/body family check, so a
hand-edited file with `"kind": "note"` and a statblock body is rejected with
the same `EntityError` semantics (surfaced as a serde error). `AnyEntity`
dispatches on the same top-level `"kind"` tag. The `"body"` object carries no
second discriminator; the entity-level `kind` is the single source of truth and
`StatblockBody` does not repeat it.

### 3.6 Statblock body

One body shape serves characters and creatures (RFC-0001 4.3); character- and
creature-only groups are optional data, not different types.

```rust
pub struct StatblockBody {
    pub profile: Profile,                 // Character | Creature ("character" | "creature")
    pub identity: Identity,
    pub abilities: Abilities,
    pub levels: Vec<ClassLevel>,          // characters; empty for creatures
    pub cr: Option<ChallengeRating>,      // creatures
    pub creature_type: Option<String>,    // creatures, free text ("fiend (demon)")
    pub size: Size,
    pub senses: String,                   // free text ("darkvision 60 ft."), display only
    pub proficiencies: Proficiencies,
    pub hp: Hp,
    pub speed: Speed,
    pub death_saves: DeathSaves,
    pub exhaustion: u8,                   // 0..=6 in 2014; engine warns beyond
    pub inspiration: bool,
    pub coins: Coins,
    pub overrides: BTreeMap<StatKey, OverrideValue>,
    pub resources: Vec<Resource>,
    pub spellcasting: Vec<SpellcastingBlock>,
    pub inventory: Vec<InventoryEntry>,
    pub attacks: Vec<AttackEntry>,
    pub features: Vec<FeatureEntry>,
    pub statuses: Vec<Status>,
    pub notes: String,                    // free text, markdown, opaque to core
}
impl Body for StatblockBody { const BODY_KIND: BodyKind = BodyKind::Statblock; }

pub struct Identity {
    pub race: Option<RefOrText>,
    pub background: Option<RefOrText>,
    pub alignment: String,
    // class identity lives in `levels`; no duplicated text box.
}

pub struct Abilities {
    pub strength: i32, pub dexterity: i32, pub constitution: i32,
    pub intelligence: i32, pub wisdom: i32, pub charisma: i32,
}
// serde field names: "str", "dex", "con", "int", "wis", "cha".

pub struct ClassLevel {
    pub id: ClassLevelId,
    pub class: RefOrText,                 // e.g. ref srd2014:class/wizard or "Homebrew"
    pub level: i32,
    pub subclass: Option<RefOrText>,
}

pub enum Profile { Character, Creature }
pub enum Size { Tiny, Small, Medium, Large, Huge, Gargantuan }

/// Challenge rating stored in eighths: 0, 1/8, 1/4, 1/2, 1..=30 all fit.
pub struct ChallengeRating(/* i32, eighths */);
impl ChallengeRating {
    pub fn from_eighths(e: i32) -> Self;
    pub fn as_eighths(self) -> i32;
    /// Parses "0", "1/8", "1/4", "1/2", "5", "30".
    pub fn parse(raw: &str) -> Result<Self, ChallengeRatingError>;
}
// Display prints "1/8" style; serde transparent string.

pub struct Proficiencies {
    pub saves: BTreeSet<Ability>,                  // proficient save abilities
    /// Sparse: absent = none. BTreeMap keeps serialization deterministic.
    pub skills: BTreeMap<Skill, SkillProficiency>,
    pub tools: String,                             // free text lines
    pub languages: String,
}
pub enum SkillProficiency { Proficient, Expertise }

pub struct Hp { pub max: i32, pub current: i32, pub temp: i32 }
// All plain inputs. Temp HP stacking/depletion is engine behavior.
// current may go negative (overkill, rules-debt); engine warns, never blocks.

pub struct Speed {
    pub walk_ft: i32,
    pub fly_ft: Option<i32>,
    pub swim_ft: Option<i32>,
    pub climb_ft: Option<i32>,
    pub burrow_ft: Option<i32>,
    pub hover: bool,                              // applies to fly_ft
}

pub struct DeathSaves { pub successes: u8, pub failures: u8 } // 0..=3, engine warns

pub struct Coins { pub cp: i32, pub sp: i32, pub ep: i32, pub gp: i32, pub pp: i32 }

/// Sparse override of a derived value. `override ?? computed` (RFC-0001 2.3).
#[serde(untagged)]
pub enum OverrideValue { Int(i32), Bool(bool), Text(String) }
```

Decisions inside this section, recorded rather than implied:

- `senses` is free text: nothing computes it, and the paper sheet box is text.
  `speed` is structured because modifiers target it (`StatKey::Speed`) and
  distance arithmetic will want numbers.
- `tools`/`languages` are single free-text fields, not id-carrying lists: they
  are edited as one text box, so path-level LWW on the whole field is the
  correct granularity; the stable-id rule (RFC-0001 8.13) applies to lists that
  are edited element-wise.
- All sheet numerics are `i32` except tiny counters (`u8` exhaustion/death
  saves) and time (`i64`): one uniform wire number type, no float, signed so
  "warn but succeed" states (over-spent resources, negative HP) are
  representable. See review pass 8 for the memory trade-off.
- `cr` uses eighths internally so every official value is exact without floats;
  its wire form stays the conventional string (`"1/8"`, `"5"`).

Note and handout bodies:

```rust
/// Markdown text, opaque to core, size-capped by the server (RFC-0001 5.12).
pub struct NoteBody { pub text: String }
impl Body for NoteBody { const BODY_KIND: BodyKind = BodyKind::Note; }

pub struct HandoutBody { pub content: HandoutContent }
impl Body for HandoutBody { const BODY_KIND: BodyKind = BodyKind::Handout; }

/// A handout is either free text or an item definition (RFC-0001 4.9).
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HandoutContent {
    Text { text: String },
    /// "Give": the item lands in the recipient's inventory as their own
    /// `InlineItem` copy; sharing read-only stays an ACL concern.
    Item { item: InlineItem },
}
```

### 3.7 Items and inventory

```rust
pub struct InventoryEntry {
    pub id: InventoryEntryId,
    pub item: InventoryItem,
    pub qty: i32,                         // signed; 0 allowed, engine warns
    pub state: ItemState,                 // Carried | Equipped | Stowed
    pub attuned: bool,
    pub notes: String,
}

/// Either a reference to pack/SRD content or a full inline definition
/// (RFC-0001 4.3 `ItemRef | InlineItem`). Tagged "type": "ref" | "inline".
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InventoryItem {
    /// Name snapshot + reference; description is resolved via ReferenceSource.
    Ref(ContentRef),
    Inline(InlineItem),
}

pub enum ItemState { Carried, Equipped, Stowed }

pub struct InlineItem {
    pub name: String,
    pub description: String,              // markdown, opaque, size-capped by server
    pub kind: ItemKind,                   // weapon|armor|shield|wondrous|gear|other
    pub requires_attunement: bool,
    pub weight_oz: Option<i32>,           // ounces; 1 lb = 16 oz
    pub value: Option<Coins>,
    pub properties: Option<ItemProperties>,
    pub modifiers: Vec<Modifier>,
    /// Charge definition for the item (RFC-0001 4.5, 4.4 "resource?"). A
    /// *definition*: `current` is ignored until the engine instantiates a
    /// tracked copy in `StatblockBody::resources`.
    pub resource: Option<Resource>,
    pub ext: Ext,
}

pub enum ItemKind { Weapon, Armor, Shield, Wondrous, Gear, Other }

#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemProperties {
    Weapon(WeaponProperties),
    Armor(ArmorProperties),
    Shield(ShieldProperties),
}

pub struct WeaponProperties {
    pub damage: Option<DiceExpr>,          // e.g. 1d8; None for utility weapons
    pub damage_type: Option<DamageType>,
    pub range_ft: Option<(i32, i32)>,      // (normal, long); reach for melee
    pub versatile_damage: Option<DiceExpr>,
    pub tags: Vec<WeaponTag>,
}
pub struct ArmorProperties {
    pub base_ac: i32,
    pub max_dex_bonus: Option<i32>,
    pub stealth_disadvantage: bool,
    pub strength_requirement: Option<i32>,
}
pub struct ShieldProperties { pub ac_bonus: i32 }

pub enum WeaponTag { Finesse, Light, Heavy, TwoHanded, Versatile, Thrown, Reach,
                     Loading, Ammunition, Special }
pub enum DamageType { Acid, Bludgeoning, Cold, Fire, Force, Lightning, Necrotic,
                      Piercing, Poison, Psychic, Radiant, Slashing, Thunder }
```

Weight is stored in ounces as an integer (no floats, exact for the SRD's
1/4 lb and 1/2 lb values); display converts to the user's unit. Magic bonuses
(`+1` weapons/armor) are *not* a dedicated field: they belong to the modifier
pipeline, and per-weapon attack math is engine territory.

### 3.8 Attacks, features, spellcasting

```rust
/// Manual attack line on the sheet; may link to an inventory entry for
/// equip state. `bonus` is the displayed base (computed suggestions are UI).
pub struct AttackEntry {
    pub id: AttackEntryId,
    pub name: String,
    pub bonus: Option<i32>,
    pub damage_text: String,              // "1d8+3 slashing" - free text
    pub from_inventory: Option<InventoryEntryId>,
}

pub struct FeatureEntry {
    pub id: FeatureEntryId,
    pub category: FeatureCategory,
    pub name: String,
    pub reference: Option<RefId>,
    pub text: Option<String>,             // inline description for homebrew
    pub resource_id: Option<ResourceId>,  // uses are a resource on this statblock
    pub modifiers: Vec<Modifier>,
    pub ext: Ext,
}
pub enum FeatureCategory { Class, Race, Feat, Trait, Action, Reaction,
                           Legendary, Other }

pub struct SpellcastingBlock {
    pub id: SpellcastingBlockId,
    pub source: RefOrText,                // class or other ("Pact Magic")
    pub ability: Ability,
    pub spells: Vec<SpellEntry>,
}

pub struct SpellEntry {
    pub id: SpellEntryId,
    pub name: String,                     // snapshot for homebrew/referred spells
    pub reference: Option<RefId>,         // e.g. srd2014:spell/fireball
    pub prepared: bool,
}
```

### 3.9 Resources and statuses

```rust
pub struct Resource {
    pub id: ResourceId,
    pub name: String,
    pub kind: ResourceKind,
    pub max: Formula,                     // bare int allowed by Formula's shorthand
    pub current: i32,                     // signed; engine warns on overspend
    pub recharge: Recharge,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceKind {
    SpellSlot { level: i32 },             // 1..=9
    PactSlot,
    HitDie { sides: i32 },                // 4..=12
    Feature,
    Item,
    Other,
}
pub enum Recharge { Short, Long, Dawn, Manual, Never }

pub struct Status {
    pub id: StatusId,
    pub kind: StatusKind,                 // Condition | Effect | Concentration | Custom
    pub name: String,
    pub reference: Option<RefId>,         // e.g. srd2014:condition/prone
    pub text: Option<String>,             // custom status description
    pub started_at: CampaignTime,
    pub duration: StatusDuration,
    pub source: Option<StatusSource>,
    pub modifiers: Vec<Modifier>,         // informational by default
}

pub enum StatusKind { Condition, Effect, Concentration, Custom }

#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusDuration {
    /// Absolute expiry on the campaign clock (RFC-0001 5.3): `expired = now >= expires_at`.
    Timed { expires_at: CampaignTime },
    UntilRest { rest: RestKind },
    Indefinite,
    Manual { text: String },              // "until the vault is opened"
}
pub enum RestKind { Short, Long }

/// Who applied the status and/or the concentration status it hangs off
/// (RFC-0001 4.6, 5.4). Both optional; absent = unknown/table-provided.
pub struct StatusSource {
    pub entity: Option<EntityId>,
    pub status: Option<StatusId>,
}
```

`StatusDuration::{Indefinite, Manual}` are never auto-expired; `UntilRest` is
cleared by the rest commands; soft-vs-hard expiry (RFC-0001 9.2 L2) is engine
policy over this data and does not change the types.

### 3.10 Modifiers, formulas, conditions

```rust
pub struct Modifier {
    pub target: StatKey,
    #[serde(flatten)]
    pub op: ModifierOp,
    pub when: Option<Condition>,
    pub note: Option<String>,
}

/// The operation carries its operand, so "advantage with a value" is not
/// representable. Internally tagged; flattened into `Modifier`, producing
/// {"target":"ac","op":"add","value":2} on the wire.
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ModifierOp {
    Add { value: Formula },
    Set { value: Formula },
    Min { value: Formula },
    Max { value: Formula },
    Advantage,
    Disadvantage,
}

/// Small closed numeric language, not a scripting language (RFC-0001 4.4).
/// Untagged so the common case is a bare number: 2 | {"type":"proficiency_bonus"}.
#[serde(untagged)]
pub enum Formula { Const(i32), Expr(FormulaExpr) }

#[serde(tag = "type", rename_all = "snake_case")]
pub enum FormulaExpr {
    ProficiencyBonus,
    AbilityMod { ability: Ability },
    Level,                                  // total character level
    ClassLevel { class: String },           // matches ClassLevel::class by name snapshot
    Add { lhs: Box<Formula>, rhs: Box<Formula> },
    Sub { lhs: Box<Formula>, rhs: Box<Formula> },
    Mul { lhs: Box<Formula>, rhs: Box<Formula> },
    Min { lhs: Box<Formula>, rhs: Box<Formula> },
    Max { lhs: Box<Formula>, rhs: Box<Formula> },
    FloorDiv { lhs: Box<Formula>, rhs: Box<Formula> },
}

/// Closed condition set (RFC-0001 4.4). StatusActive matches a status by its
/// name snapshot; engine matching is case-insensitive.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    Equipped,
    Attuned,
    StatusActive { name: String },
}
```

Source attribution stays implicit in the container (item, feature, status),
exactly as RFC-0001 4.4 requires; core adds no `source` field to `Modifier`.
Dice-bearing modifiers (Bless: "+1d4 to saves") are *not* representable in
`Formula` by design; see Open Question 6.4.

### 3.11 Dice, rolls, feed

```rust
/// Structured dice AST (RFC-0001 4.7, 5.6). Lives in core because it is part
/// of the serialized `Roll` contract; parser and roller live in engine::dice.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiceExpr {
    Const { value: i32 },
    Roll { count: i32, sides: i32, keep: Option<Keep> },   // 4d6kh3
    Advantage { inner: Box<DiceExpr> },                    // usually the d20 term
    Disadvantage { inner: Box<DiceExpr> },
    Add { lhs: Box<DiceExpr>, rhs: Box<DiceExpr> },        // multi-term bags
    Sub { lhs: Box<DiceExpr>, rhs: Box<DiceExpr> },
    Mul { lhs: Box<DiceExpr>, rhs: Box<DiceExpr> },
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Keep { Highest { count: i32 }, Lowest { count: i32 } }

pub struct Roll {
    pub id: RollId,
    pub roller: PlayerId,
    pub entity: Option<EntityId>,
    pub ts: UnixTime,
    pub expr: DiceExpr,
    pub result: DiceResult,
    pub context: Option<RollContext>,
    pub audience: Audience,
}

/// Per-term and per-die breakdown so frontends can render dice and detect
/// nat 1 / nat 20 under advantage (RFC-0001 5.6).
pub struct DiceResult { pub terms: Vec<TermResult>, pub total: i32 }
pub struct TermResult { pub dice: Vec<DieResult>, pub total: i32 }
pub struct DieResult { pub value: i32, pub kept: bool }

pub struct RollContext {
    pub kind: RollKind,                 // Check | Save | Attack | Damage | Other
    pub stat: Option<StatKey>,          // "skill.stealth" when composed from a sheet
    pub target_dc: Option<i32>,
}

/// Role-agnostic audiences (RFC-0001 5.6): "public", "roller_and_dm",
/// "roller_only". A general audience set is a later, breaking extension.
pub enum Audience { Public, RollerAndDm, RollerOnly }

/// Monotonic per-campaign feed position; the ring buffer is persisted.
pub struct FeedEntryId(/* u64 */);
impl FeedEntryId {
    pub fn new(value: u64) -> Self;
    pub fn get(self) -> u64;
}

pub struct FeedEntry {
    pub id: FeedEntryId,
    pub ts: UnixTime,
    pub clock_s: CampaignTime,
    pub actor: Option<PlayerId>,
    pub entity: Option<EntityId>,
    pub audience: Audience,
    pub payload: FeedPayload,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedPayload {
    /// The entry audience mirrors the roll audience; both pass `view_of`.
    Roll { roll: Roll },
    StatusExpired { status: StatusId, name: String },
    ConcentrationEnded { name: String },
    Hint { text: String },
}
```

### 3.12 The serde contract

Tagging rules are part of the public contract (RFC-0001 10: "decide enum
tagging representations deliberately"). Summary:

| Shape | Representation |
|---|---|
| Structs | flat object, `snake_case` field names |
| Plain enums (no payload) | string, `snake_case` |
| Payload enums (except below) | internally tagged, key `"type"` (or `"op"` for `ModifierOp`) |
| `Formula`, `RefOrText`, `OverrideValue` | untagged: bare number / string / bool is the shorthand; objects are unambiguous |
| `Id<T>`, `RefId`, `RulesetId`, `Namespace`, `TokenHash`, `CampaignTime`, `UnixTime`, `FeedEntryId` | transparent: string or number |
| `SchemaVersion` | object `{"major","minor"}` |
| `FieldMask` | transparent: sorted array of section strings (`["name","ac"]` by enum order) |
| Entity documents | flat header + `"body"` object, top-level `"kind"` |

Additional rules, all tested:

1. Unknown fields are ignored on deserialize (forward tolerance inside a major
   version); they are not re-emitted. Version gates protect against real
   incompatibilities (see Guarantees).
2. `Option` fields are omitted when `None`; `null` from older/hand-edited files
   still deserializes as `None`.
3. No floats anywhere; no `deny_unknown_fields` (incompatible with `flatten`,
   and forward tolerance is wanted).
4. Deserialization of `Entity<B>` validates `kind.body_kind() == B::BODY_KIND`
   and fails the parse on mismatch.
5. `StatKey` round-trips byte-identically: canonical known strings parse to
   `Known`, everything else (validly shaped) stays `Custom` with its original
   spelling.
6. `DiceExpr` and `Formula` serialize nested objects; `Formula` nodes appear as
   bare numbers when constant.
7. Unknown enum tag values (a `"type"`/`"op"` string this build does not know)
   are a deserialization **error**, unlike unknown object fields, which are
   ignored. Payload-enum additions therefore require a major version bump or an
   explicit fallback variant; this is intentional (no silent
   reinterpretation).

## 4. Guarantees

### 4.1 Safety

- Core contains **no `unsafe`** and no `unwrap`/`expect` outside tests. Every
  fallible path returns a `Result` (parse errors, builder validation, serde
  validation).
- All public types are fully owned: no lifetimes, no borrows stored, no trait
  objects, no generic smart pointers. `Id<T>` is the only generic wrapper and
  it is `Copy`.

### 4.2 Threading (`Send`/`Sync`)

- Every public type is `Send + Sync`. Types are plain data; there is no
  interior mutability (`Rc`, `RefCell`, cells) anywhere.
- `Body: Send + Sync` is a supertrait bound, so `Entity<B>` is `Send + Sync`
  for every body.
- The test asserts `Send + Sync` statically for the main types. There is no
  concurrency *behavior* to test in this subsystem: the RFC's concurrency story
  is single-writer commands at the engine level; core only guarantees that data
  can be moved between tasks/threads safely.

### 4.3 Determinism and ordering

- No floats, no hash maps in serialized positions: serialization is
  deterministic for a given value except for explicit `Vec` order, which is
  meaningful (lists are ordered, element-addressed).
- `BTreeMap` is used for `ext`, `overrides` and `skills` so document bytes are
  stable across runs and platforms (snapshot tests, diffable exports).

### 4.4 Progress

Not applicable: core is synchronous, allocation-only data. No locks, no
blocking, no waiting. (Stated explicitly so the reviewer does not look for a
progress story.)

### 4.5 Error model

- Two-tier: **construction/parse** errors (`RefIdError`, `StatKeyError`,
  `RulesetIdError`, `NamespaceError`, `ChallengeRatingError`) and **document
  validation** errors (`EntityError::KindBodyMismatch`, surfaced through serde).
- Version policy: a loader built at `SchemaVersion::CURRENT` accepts documents
  whose **major** is equal or lower; a newer major is refused instead of
  partially loaded (RFC-0001 5.9). A newer *minor* within the same major is
  accepted and unknown fields are ignored; adapters should warn because a save
  from the older build can drop those fields.
- Rules-like conditions (0 HP, overspent resources, over-attunement, negative
  quantities) are **not** errors at any layer of core: the types allow the
  states so the engine can warn without blocking (RFC-0001 2.2).

### 4.6 Invariants that are *not* type-level

Recorded so implementers do not assume them: uniqueness of element ids inside a
list, exactly-one-DM, owner-must-exist, referenced ids resolving, ACL masks
being honored. These are engine/adapter checks. Type-level invariants are only:
kind/body family, ID domain separation, campaign-vs-wall time separation,
validated key/ref/namespace shapes.

## 5. Internal structure

- Single file `src/types.rs`, sections 3.1-3.11 of this RFC in order. Splitting
  into `types/` submodules later must not change public paths (re-export).
- `Entity<B>` serde uses a private `EntityFields<B>` proxy struct;
  `AnyEntity` deserializes a private `tagged` enum over the same proxy; both
  funnel `"kind"` through one validation function so there is exactly one place
  that decides kind/body compatibility.
- `Body` is sealed via a private `sealed::Sealed` supertrait; the three body
  types are the only implementors (and `AnyEntity` conversions are the only
  erasure point).
- Canonical `StatKey` strings and `Display`/`FromStr` live next to the enum and
  are the only place that knows the wire spelling; the tests pin every
  canonical string.

## 6. Open questions

Each is a genuine fork with a recommendation; none block the skeleton except
6.1, which only needs a yes/no.

1. **Field visibility vs AGENTS.md.** AGENTS.md says "private fields by
   default". This draft uses public fields for plain data structs (and private
   fields for invariant newtypes). Rationale: these are versioned documents
   whose field set *is* the contract; accessors for ~40 structs would double
   the API and add no invariants. Recommended: keep public fields, record the
   deviation in AGENTS.md ("data crates may use public fields"). If the
   maintainer insists, the alternative is public fields *plus* careful
   `#[doc(hidden)]` accessors - not worth it.
2. **`ext` placement.** RFC-0001 6 names campaign, entity, item and feature;
   the draft implements exactly that. Statuses and resources have no `ext`.
   Recommended: keep it; add per-type `ext` when a real plugin need appears
   (additive, minor bump).
3. **`EntityKind` vs body: five of eight kinds share one body.** `kind` stays a
   runtime field (validated against the body family). A stricter design
   (`Entity<Pc>`, `Entity<Monster>`, ...) was rejected because the method set
   for all six statblock kinds is identical and the marker zoo would leak into
   storage and iteration. Recorded for the record; no action.
4. **Dice-bearing modifiers.** Bless-style "+1d4 to saves" cannot be expressed
   in `Formula`. Options: (a) keep `Formula` arithmetic-only and let the table
   roll the extra die manually (recommended for v1, least machinery);
   (b) add `FormulaExpr::Dice { expr: DiceExpr }` - forces an
   evaluate-to-roll-contribution return type in the engine. Recommended: (a)
   now, revisit the first time the engine composes rolls from modifiers.
5. **`ViewField` granularity.** Section-level masks are coarse: "show AC" or
   "hide HP", not "show HP max but hide current". Recommended: ship coarse;
   split a section into sub-fields only if a real share scenario needs it
   (breaking change, so deliberately delayed).
6. **`#[non_exhaustive]` policy.** Not used. Additions to payload enums
   (`FormulaExpr`, `FeedPayload`, `DiceExpr`, `Condition`, ...) are breaking
   for exhaustive matches. Recommended: stay plain while all consumers are
   in-workspace; before 1.0 / crate extraction, mark payload enums
   `#[non_exhaustive]` in one deliberate pass.
7. **`Audience` as a set.** RFC-0001 5.6 wants a general audience set later
   ("DM reveals to one player"). The enum is a breaking serde change when the
   set arrives; feed entries would need a migration (pure function over JSON).
   Recommended: enum now (small, clear), migrate feed documents when sets land.
8. **`weight_oz`** is unusual for pack authors who think in pounds. Options:
   keep ounces (exact, no floats), or accept `"weight": "3.5 lb"` free text
   (PDF parity, no arithmetic). Recommended: keep ounces; the pack loader can
   accept `3.5` pounds as input sugar later without changing the stored type.
9. **`StatusActive { name }`** matches by name snapshot. A ref-based match
   (`{ name, reference }`) would survive renames better. Recommended: name now;
   widen to name + optional ref when status/conditions get renamed by packs.
10. **Full-text `tools`/`languages`.** The stable-id rule (RFC-0001 8.13) is met
    by making these one text field, not a list. If the UI later wants
    per-language checkboxes (known/unknown), this becomes an id-carrying list -
    a breaking change. Recommended: keep text; convert only when a feature
    needs it.

## 7. Review pass: does it hold under the Rust model?

One pass over the whole sketch, checking the constructs actually compile and
behave as intended. Findings, including the ones that forced changes:

1. **`From<Entity<B>>` was not implementable** (original sketch). `Body` only
   exposes a const family tag, and a generic impl cannot pick one of three
   runtime variants. Fixed: three concrete `From` impls, one per body type.
   Related: `AnyEntity` cannot simply be `Entity<AnyBody>`, because
   `Body::BODY_KIND` is an associated const and a runtime enum cannot provide
   it. The plain enum plus concrete impls is the honest shape.
2. **`Id<T>` derives would impose `T: Trait` bounds** (`#[derive(Clone)]` adds
   `T: Clone` even for `PhantomData<fn() -> T>` fields). Fixed in the sketch:
   `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Hash`, `PartialOrd`, `Ord`,
   `Display`, `Serialize`, `Deserialize` are implemented by hand without a `T`
   bound. `PhantomData<fn() -> T>` keeps `Id<T>` covariant, `Send + Sync` and
   `Copy` regardless of `T`, and creates no drop-check or recursion problems.
3. **Recursive type `EntityId = Id<AnyEntity>`** where `AnyEntity` contains
   `Entity<B>`: legal, because `Id<T>` stores no `T` (function-pointer phantom,
   always sized). This is deliberate: an entity id is domain-agnostic across
   bodies, so `Entity<StatblockBody>` and `Entity<NoteBody>` share the id type.
4. **`serde(transparent)` over a UUID plus `PhantomData`** requires the phantom
   field to be skipped by the derive; serde derives do skip `PhantomData`
   fields. If a future serde version breaks that, fall back to a manual
   `Serialize`/`Deserialize` on `Id<T>` (two small impls, already centralized).
   `bound = ""` is required on the struct's serde attributes.
5. **Sealed trait vs the workspace's `unnameable_types = "warn"` lint.** The
   classic `mod sealed { pub trait Sealed {} }` pattern can trip the lint on
   `pub trait Body: sealed::Sealed`. Plan: verify at skeleton time against
   MSRV 1.85; if it fires, seal with a `#[doc(hidden)] pub trait Sealed` in a
   public hidden module, or carry a documented
   `#[expect(unnameable_types)]` on `Body`. This is the one place in the
   design where a lint interaction is expected; it must not be silenced
   silently.
6. **Entity serde must be a manual `Deserialize` impl**, not
   `#[serde(try_from = "EntityFields<B>")]`: serde's `try_from` attribute takes
   a type path string and generic parameters in that string are brittle.
   Manual impl: deserialize `EntityFields<B>`, convert with the shared
   validation function, map the error with `serde::de::Error::custom`. This
   keeps one single place that decides kind/body compatibility for
   `Entity<B>`, `AnyEntity` and hand-edited files.
7. **Untagged enums are sound here** because the variants occupy disjoint JSON
   shapes: `Formula` = number vs object, `RefOrText` = string vs object,
   `OverrideValue` = number vs bool vs string. Costs: parse-time buffering
   (transient allocations) and generic "did not match any variant" error
   messages. Consequence recorded for tests: assert on *failure*, never on
   serde error *text*.
8. **`flatten` on `Modifier.op`** with internally tagged `ModifierOp` works and
   produces `{"target":"ac","op":"add","value":2}`. It requires `Content`
   buffering during parse and forbids `deny_unknown_fields` on `Modifier` -
   both accepted and consistent with the global no-`deny_unknown_fields`
   policy.
9. **Unknown enum tags fail** (unlike unknown fields): a `"type"` value this
   build does not know is a deserialization error, not an ignored field. This
   is correct behavior for a versioned contract (no silent reinterpretation)
   and is now stated in 3.12; forward additions to payload enums are
   major-version territory or need an explicit fallback variant.
10. **Recursion is bounded.** `Box<Formula>`/`Box<DiceExpr>` are sized and
    allocate one node each; `serde_json`'s default recursion limit (128)
    bounds hostile documents during parse; the engine's expression-length cap
    (RFC-0001 5.6) handles the rest. No stack-overflow exposure at this layer.
11. **`Eq` holds everywhere** because there is no float: good for key types
    (`StatKey`, `Skill`, `Ability`, `Namespace`, `ViewField`, `FeedEntryId`)
    used in `BTreeMap`/`BTreeSet`.
12. **No lifetimes, no `dyn`, no interior mutability** in any public signature:
    the only `impl Trait` return is `FieldMask::iter`, which borrows `self`.
    This keeps the API OpenAPI/TS-generation friendly and plugin-ABI simple.

## 8. Review pass: memory efficiency

Measured on the target toolchain with a scratch binary (rustc, edition 2024):

| Type | size | note |
|---|---|---|
| `String` / `Vec<u8>` / `BTreeMap<u64,u64>` | 24 bytes | inline header, heap payload |
| `Option<String>` | 24 bytes | niche in the pointer, no extra byte |
| `[u8;16]` (≈ `Uuid`) | 16 bytes, align 1 | |
| `Option<[u8;16]>` | 17 bytes | no niche in a plain byte array |
| `Option<i32>` / `Option<u32>` | 8 bytes | padded to alignment |
| `Option<u64>` | 16 bytes | no niche |
| `(String, String, String)` | 72 bytes, 3 allocations | naive ref representation |
| `(String, u32, u32)` | 32 bytes, 1 allocation | chosen ref representation |

Findings:

1. **Type-state pays off as intended** (Q1): `Entity<NoteBody>` contains no
   statblock payload at all (header + one `String`), `Entity<StatblockBody>`
   carries it inline. Without it, every note would pay for an enum
   discriminant plus the largest body variant's inline size.
2. **`RefId` internals**: store the canonical string once plus two `u32`
   segment boundaries (32 bytes inline, one allocation) instead of three
   `String`s (72 bytes, three allocations). Every ref-able entry carries one,
   so this is the single largest per-document saving. Applied to the sketch
   (`RefId` internals; accessors slice the raw string). Cap length at 4 KiB to
   keep the `u32` spans honest.
3. **Empty collections are free**: `BTreeMap`/`Vec` are 24 inline bytes and do
   not allocate when empty. A default note entity is therefore header + one
   body string, no hidden allocations.
4. **Entity header is ~200 bytes inline** (13 fields, mostly 24-byte headers
   and one 17-byte `Option<EntityId>`). Fine: entities are counted in dozens.
5. **`Option<EntityId>` costs 17 bytes** because `Uuid` has no niche. About
   five per statblock (`parent`, `status.source.entity`, `roll.entity`, ...);
   a `NonZeroUuid`-backed id would save ~8 bytes each but buys an invariant
   and unsafe-adjacent conversion - rejected, not worth it at this scale.
6. **Integer uniformity is deliberately not width-optimized.** Narrowing ~60
   `i32` fields to `i16`/`u8` saves on the order of 150 bytes per statblock;
   that is noise next to string payloads, and `i32` keeps one JSON number type
   and no casts. Revisit only with measurement.
7. **`BTreeMap` over `HashMap`** for `ext`, `overrides`, `skills`: 24 inline
   bytes, no hashing overhead, sorted serialization for free, and n is tiny
   (<= 18 skills), where linear scans inside a B-tree's small nodes are not a
   concern. `Vec<(K, V)>` would be marginally leaner but loses keyed access
   ergonomics in the engine.
8. **No interning**: `String` everywhere rather than `Arc<str>`/`CompactString`.
   Name snapshots are intentionally duplicated (degradation requirement,
   RFC-0001 4.8). Interning is a later, purely additive optimization inside
   core; it cannot be done with public field types without a breaking change,
   which is one more reason to keep documents modest in size today.
9. **`Box`ed ASTs** allocate one node per operator; typical expressions are
   under ten nodes. Feed ring: `feed_capacity` bounds the number of entries;
   each entry holds its payload inline (roll AST + result vectors), which is a
   few hundred bytes plus heap - acceptable, and the ring capacity is
   configurable.
10. **Parse-time costs** (untagged enums, flatten, `ext` JSON) are transient
    allocations during load/save, not resident memory; loads happen per
    command in the engine, at party scale, over small documents. No action.

Verdict: no memory blockers. The two decisions worth keeping an eye on if
documents ever grow are string interning and id niches; both are deliberately
deferred.

## 9. Review pass: extensibility

Checked against RFC-0001 6 ("design now, build later") and section 13
("extensibility without a redesign"). Findings:

1. **New content and homebrew are data-only.** `RefId` namespaces handle
   `pack.<name>:` sources; name snapshots degrade to text; `ContentRef` needs
   no pack to exist. New reference *kinds* (race, class, monster, rule, ...)
   are just new `kind` strings - no core change, no validation table to
   update.
2. **New stats need no core change** thanks to the hybrid `StatKey`: custom
   keys are validated and stored, `Known` grows only when the engine learns to
   compute something. Policy recorded: turning a previously-custom string into
   `Known` changes load semantics, so it is a major-version event, and all
   canonical strings are pinned by tests so it cannot happen by accident.
3. **Payload-enum growth is the known weak spot.** Rule 7 of 3.12 means
   additions to `FormulaExpr`, `DiceExpr`, `Condition`, `FeedPayload`,
   `ResourceKind` are breaking for older readers (unknown tag = error). This
   is the deliberate trade for "no silent reinterpretation". Pre-1.0: minors
   may break with migrations. The planned relief valve is one deliberate
   `#[non_exhaustive]` \+ fallback-variant pass before 1.0 / crate extraction
   (Open Question 6.6).
4. **Ruleset evolution is structural, not bolted on.** Every entity and the
   campaign carry a `RulesetId`; core stores no 2014-specific fields beyond
   data (profile, levels, cr). A 2024 ruleset adds engine data/hooks, not
   types. Mixing editions is a policy question (RFC-0001 9.8), not a type
   limitation.
5. **Plugin story holds.** Core depends only on `serde`, `serde_json`,
   `uuid`, `thiserror`; `Id<T>` lets plugins mint their own id domains; `ext`
   maps namespace plugin data. `Body` is intentionally sealed: entity kinds
   are a fixed part of the product, not a plugin extension axis. Extracting
   `core` as a plugin-facing crate keeps working; nothing in core references
   engine concepts.
6. **Frontend generation later.** The types are derived-ready for `schemars` /
   `ts-rs`; those derives should be added as optional, feature-gated
   dependencies in a later pass, not now (they would be the first extra deps
   in core). The serde contract in 3.12 is stable enough to generate from.
7. **Feed/audience migration is bounded.** Feed is a ring buffer; when
   `Audience` becomes a set (Open Question 6.7), only the last
   `feed_capacity` documents need a pure JSON migration, and rolls inside
   them ride along.
8. **Future features check out against the data model**: initiative list
   (campaign-level list, no status-model change), level-up helper (edits
   `ClassLevel` + base data; `FormulaExpr::Level`/`ClassLevel` already
   exist), expected-damage analytics (`DiceExpr` AST + per-die results are
   stored), companion derivation (`parent` exists; a cross-entity formula
   source would be a new `FormulaExpr` variant), PDF import (flat, documented
   keys; free-text boxes exist). None require a redesign.
9. **Importer ergonomics**: provide `Default` for the empty-able leaf sections
   (collections, zeros, empty strings) so PDF import can build a statblock
   field by field. Do **not** give `Abilities` or `StatblockBody` a `Default`
   with made-up values (a silent 10 in every ability is exactly the kind of
   "helpful" default that corrupts a sheet); use explicit constructors such as
   `Abilities::all(10)` instead.
10. **Content-pack statuses**: RFC-0001 5.10 wants pack entries "the same shape
    as an inline item/feature/status". A stored `Status` carries runtime state
    (`id`, `started_at`, `duration`), so a pack condition is loaded into a
    `Status` with generated id, `started_at` = now, `duration` = `Indefinite`,
    `source` = `None`. That is a loader concern, not a core type; if packs ever
    need richer definitions, a `StatusDefinition` type belongs to the adapter
    and can be added without touching core.

Verdict: the model is extensible along the axes RFC-0001 cares about (content,
stats, rulesets, plugins, frontends). The single deliberate limitation is
payload-enum growth, tracked in Open Question 6.6 with a concrete plan.

## 10. Cross-check and verdict

- Every element the target test touches (`crates/localtt-core/tests/types.rs`)
  exists in section 3: entity builder and validation, `AnyEntity` dispatch,
  full statblock construction, `Formula`/`Modifier`/`StatKey` spelling,
  `DiceExpr`/`Roll`/`FeedEntry`, `FieldMask`, `SchemaVersion::accepts`,
  `RefId`/`StatKey` failure paths, `Send + Sync` assertions.
- Every element of section 3 that the test does not touch directly
  (`Campaign`, `Clock`, `CampaignSettings`, `Player`, `TokenHash`, `Role`,
  `ItemProperties`, `WeaponProperties`, `SkillProficiency`, `RestKind`,
  `StatusSource`, `RollContext`, ...) is required by a cited RFC-0001 section
  and is justified by the storage/wire contract, not by speculation.
- The three review passes (7, 8, 9) were run against the full sketch; the
  changes they produced (three concrete `From` impls, `RefId` internals, the
  unknown-tag serde rule, the no-`Default`-for-abilities rule) are already
  reflected in the sections above.

Nothing in this RFC blocks the skeleton phase; Open Question 6.1 is a
yes/no on field visibility (public fields recommended) and Open Questions
6.4-6.10 are explicitly deferred. Open Question 6.2 (ext placement) affects
no skeleton signature.

