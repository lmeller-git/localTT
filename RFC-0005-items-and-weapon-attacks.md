# RFC-0005: Item links and derived weapon attacks

| | |
|---|---|
| **Status** | Proposed |
| **Scope** | Open5e item-property import, engine weapon-attack derivation, protocol sheet DTO, browser inventory search/link and attack panel |
| **Temporary** | Delete this file once the feature is implemented and folded into the permanent docs |

## 1. Problem

The inventory add form only accepts a free-text name and always stores an
`InventoryItem::Inline`. A character cannot search the reference catalog for a
weapon or a common item and link the inventory entry to it, although the spell
panel already offers exactly that flow.

The imported SRD items do not carry usable structured properties either. The
Open5e converter maps `weapons.json` and `armor.json` into `ContentData::Item`,
but leaves `InlineItem::properties` as `None`. The raw fields are present under
`ext["src.open5e"]` (`damage_dice`, `damage_type`, `properties`, `base_ac`,
`plus_dex_mod`, `plus_max`, `stealth_disadvantage`, `strength_requirement`), so
the data is imported but the rules layer cannot see it. Equipped imported armor
therefore never changes AC, and imported weapons cannot produce attack lines.

The sheet also has no paper-style attack block. `attack.melee` and
`attack.ranged` exist as derived keys, but only when a contribution explicitly
targets them, and `AttackEntry` is a manual line with no command or UI. There is
no per-weapon attack bonus or damage expression derived from the equipment the
character actually has.

The goal is to make item linking behave like spell linking and to derive one
read-only attack line per equipped weapon using the existing rules pipeline.

## 2. Goals and non-goals

Goals:

- Search reference content with `kind=item` from the inventory add form and
  store an `InventoryItem::Ref` with a name snapshot.
- Open the existing content drawer from a referenced inventory row.
- Keep item linking generic: weapons, armor, gear and wondrous items all use
  the same reference path.
- Map Open5e weapon and armor/shield fields into `WeaponProperties`,
  `ArmorProperties` and `ShieldProperties` in the converter.
- Regenerate the committed `srd2014` snapshot so the shipped data carries those
  properties.
- Derive per-weapon melee/ranged attack bonus, damage dice, ability modifier,
  versatile damage, damage type and range on the server.
- Expose the derived attacks through the existing derived-sheet endpoint and
  render them in the browser.
- Reuse the existing contribution pipeline, including global attack modifiers,
  advantage/disadvantage hints and the existing warning behavior.

Non-goals for this feature:

- Class, race, feat or fighting-style weapon proficiency (Archery, Dueling,
  Great Weapon Fighting, and similar).
- A structured proficiency model or per-weapon proficiency flags.
- Extra Attack, two-weapon fighting, dual wielding or multiple attack lines for
  one entry.
- Magic weapon attack/damage bonuses inferred from description text.
- A ranged and thrown dual line; see section 4.5 for the current simplification.
- Editing an item's reference or inline definition after it is added.
- Hand-authoring weapon properties for homebrew items in the browser.
- Manual `AttackEntry` CRUD or merging manual attacks into the derived list.
- Rolling an attack or damage expression from the sheet.
- Ammunition tracking, critical hits, resistance or immunity.
- Encumbrance, weight totals or a new equipment source/pack.

Bundling the armor/shield importer mapping is deliberate. The converter path and
the snapshot regeneration are shared, and structured armor properties make the
same imported items useful to the existing AC derivation instead of leaving a
second dead payload.

## 3. Data model and content mapping

### 3.1 No persisted schema change

The persisted item model already has everything the feature needs:

- `InventoryItem::Ref(ContentRef)` stores the reference plus a display-name
  snapshot.
- `ContentData::Item(InlineItem)` carries the resolved item, including
  `ItemProperties`.
- `WeaponProperties` has `damage`, `damage_type`, `range_ft`,
  `versatile_damage` and `tags`.
- `ArmorProperties` and `ShieldProperties` cover the open5e armor fields.
- `ContentKind::Item` and the existing reference search already support item
  lookups.

Therefore this RFC does not change the campaign document schema,
`SchemaVersion`, `InlineItem`, `InventoryItem`, `WeaponProperties` or
`AttackEntry`. No migration is required. Existing inline items and references
keep working.

The only engine type change is additive and non-persisted: `DerivedSheet` gains
an `attacks` list (section 4.6). `DerivedSheet` is public, so adding a field is
source-breaking for direct struct literals; the in-workspace constructors and
protocol tests are updated in the same phase. The RFC's acceptance covers that
small public-API break.

### 3.2 Item references

The add path already accepts `InventoryItem::Ref`; only the browser prevents it.
This RFC keeps that command surface and adds one validation rule:

- A reference item's `RefId::kind()` must be `item`; anything else is
  `Malformed`.
- The name snapshot is validated non-empty and bounded by the existing
  item-name validation.
- A reference with kind `item` that is missing from the loaded sources is still
  accepted. It degrades to text, the existing `UnresolvedReference` warning
  fires during derivation, and the item can resolve later when the source is
  installed.

The edit command does not gain an item/reference field. Relinking is
remove-and-add, matching the spell panel's immutable reference.

### 3.3 Open5e weapon mapping

`weapons.json` records map into `InlineItem { kind: Weapon, properties:
Some(ItemProperties::Weapon(..)) }`.

| Open5e field | Target |
|---|---|
| `damage_dice` | `WeaponProperties.damage` via the engine dice parser |
| `damage_type` | `WeaponProperties.damage_type` |
| `properties` entries | `WeaponProperties.tags`, plus range or versatile dice |
| `category` | used only to decide that the record is a weapon; not persisted |

Tag mapping is case-insensitive and ignores the parenthetical suffix:

| Open5e text | Tag / extracted field |
|---|---|
| `finesse` | `WeaponTag::Finesse` |
| `light` | `WeaponTag::Light` |
| `heavy` | `WeaponTag::Heavy` |
| `two-handed` | `WeaponTag::TwoHanded` |
| `versatile (1d10)` | `WeaponTag::Versatile`, `versatile_damage = 1d10` |
| `thrown (range 20/60)` | `WeaponTag::Thrown`, `range_ft = (20, 60)` |
| `reach` | `WeaponTag::Reach`; no range is invented |
| `loading` | `WeaponTag::Loading` |
| `ammunition (range 25/100)` | `WeaponTag::Ammunition`, `range_ft = (25, 100)` |
| `special` | `WeaponTag::Special` |
| unknown text | ignored, review note added |

`damage_dice` accepts the pinned shapes `1d8`, `2d6`, `1` and `0`. `0` maps to
`damage: None` because the item has no usable damage line (the net is the only
pinned case). A missing or unparsable value becomes `damage: None` with a review
note; it does not fail the import. An unknown damage type becomes `None` with a
review note. A malformed range or versatile suffix keeps the base tag, leaves
the extracted field empty and adds a review note. The raw record remains under
`ext["src.open5e"]` as today.

### 3.4 Open5e armor and shield mapping

Only records whose category is a real armor category map to armor properties;
`Class Feature`, `Spell` and `No Armor` records keep `properties: None`. This
avoids turning entries such as Draconic Resilience or Unarmored into wearable
armor.

| Open5e field | `ArmorProperties` |
|---|---|
| `base_ac` | `base_ac` |
| `plus_dex_mod == true` | `max_dex_bonus = plus_max` (`None` when uncapped) |
| `plus_dex_mod == false` | `max_dex_bonus = Some(0)` |
| `stealth_disadvantage` | `stealth_disadvantage` |
| `strength_requirement` | `strength_requirement` |

A shield record (`category == "Shield"`) maps to
`ItemProperties::Shield(ShieldProperties { ac_bonus: plus_flat_mod })`. Its
`base_ac` of zero is ignored. Missing required integers and unparsable values
produce `properties: None` or the partial mapping plus review notes, never a
corrupt import.

Mapping heavy armor exposes a small existing AC bug: `rules::armor` currently
computes `min(dex, cap)`, so a cap of `Some(0)` still adds a negative Dexterity
modifier. Heavy armor must ignore Dexterity entirely. The RFC treats
`max_dex_bonus: Some(0)` as "no Dexterity contribution" and leaves `None` as
uncapped and positive caps as clamps. Light armor with a negative Dexterity
modifier keeps that modifier; heavy armor does not.

Magic items from `magicitems.json` keep `properties: None`. A magic weapon or
armor description is not parsed into rules data.

### 3.5 Snapshot regeneration

The converter changes are not complete until the committed snapshot is
regenerated with the documented tool at the pinned revision
`b22294a4ebd8f5652135ad784eaa20070c4e62ff`:

```text
cargo run -p localtt-adapter --bin localtt-content-convert -- \
  --input <pinned checkout>/data/WOTC_5e_SRD_v5.1 \
  --output crates/localtt-adapter/data \
  --namespace srd2014 \
  --source-revision b22294a4ebd8f5652135ad784eaa20070c4e62ff
```

The snapshot format version is unchanged because only payload values widen.
`report.json` is regenerated with the rest. The fixture catalog gets a small,
hand-written weapon and gear entry for browser tests; it is not derived from the
SRD snapshot.

## 4. Engine contract

### 4.1 Candidates

A new `rules::weapons` module derives attacks from the one collection pass. An
inventory entry yields an attack line when all of these hold:

- the entry is `ItemState::Equipped`;
- the entry is usable (`qty > 0`, reusing `collect::ItemSource::usable`);
- the item resolves to an `InlineItem`;
- `properties` is `Some(ItemProperties::Weapon(weapon))`;
- `weapon.damage` is `Some`.

The list preserves inventory document order. Each entry yields at most one
line; `qty` is a gate, not a repeat count. Unresolved references, carried
weapons and stowed weapons are absent. The existing `UnresolvedReference`
warning still explains an unresolved entry.

`quantity_usable` currently treats only modifiers and armor/shield properties as
a contribution. It must also treat weapon properties as one, so an equipped
weapon at `qty <= 0` warns `ZeroQuantity` and is excluded from the attack list.
A non-positive weapon with no other contribution is no longer silent.

### 4.2 Attack kind and ability

The attack kind selects the existing `attack.melee` or `attack.ranged` key for
contribution matching:

```text
kind = if weapon.tags contains Ammunition { Ranged } else { Melee }

ability =
  if weapon.tags contains Ammunition { Dexterity }
  else if weapon.tags contains Finesse { higher of Strength and Dexterity }
  else { Strength }
```

A Finesse tie chooses Strength, so the result is deterministic. The ability
choice compares the final computed ability modifiers, not the stored scores.
Spell attacks are not part of this list.

### 4.3 Attack bonus

The base is the final ability modifier plus the final proficiency bonus:

```text
base = Set(ability_modifier) + Add(proficiency_bonus)
```

It is staged with the existing `stage_key` against the collected pendings and
pre-resolved contributions for `attack.<kind>`. The pass runs after the score
stage and the one-time pending resolution, so it reuses the settled ability
scores and the existing resolved contributions. This means:

- a modifier targeting `attack.melee`, `attack.ranged` or the `attack` group
  applies to the matching weapon lines;
- the normal `Set`/`Add`/`Min`/`Max` staging rules apply;
- advantage and disadvantage contributions produce the line's `hint`, which is
  `Normal` when none applied;
- dice-valued modifiers contribute zero in the static sheet and keep the
  existing `DiceInStaticContext` warning behavior;
- warnings are still emitted once per contribution, not once per weapon.

The per-weapon base deliberately does not consult an override on the generic
`attack.melee` or `attack.ranged` key. Those keys remain the generic sheet
stats; a per-weapon override would need a per-weapon stat key, which is out of
scope. The generic keys also keep their current "appears only when targeted"
behavior.

### 4.4 Damage

```text
damage_dice    = weapon.damage
damage_bonus   = final ability modifier
versatile_dice = weapon.versatile_damage
damage_type    = weapon.damage_type
range_ft       = weapon.range_ft
```

The ability modifier is added once to the normal damage line and once to the
versatile alternative. The weapon's dice are not rewritten; the browser renders
`dice + bonus`. A zero modifier renders as bare dice (`1d8`), and a negative
modifier renders as `1d8-1`, not `1d8+-1`. The damage type and range are copied
for display.

### 4.5 Known simplifications

- Thrown-only weapons without the `Ammunition` tag produce one melee line that
  carries their thrown range. This is correct for daggers, handaxes, javelins,
  spears and tridents; it mislabels darts as melee even though their category is
  ranged. The numbers (Dexterity on a Finesse dart, thrown range) stay correct.
  A category field or a ranged/thrown dual line is deferred.
- Reach adds no range because the pinned data does not carry the reach distance.
- Versatile weapons show both the normal and the two-handed damage line; the
  browser does not track which grip is active.
- A magic weapon's descriptive `+1` is not applied. Its item modifiers still
  apply through the normal pipeline if a pack supplies them.

### 4.6 Engine types

`DerivedSheet` gains:

```rust
/// The two kinds of weapon attack; spell attacks are not derived here.
pub enum WeaponAttackKind {
    Melee,
    Ranged,
}

pub struct DerivedAttack {
    /// The inventory entry that produced this line.
    pub entry: InventoryEntryId,
    /// The resolved item's display name.
    pub name: String,
    /// Melee or ranged; selects the contribution key.
    pub kind: WeaponAttackKind,
    /// The ability that adds its modifier to attack and damage.
    pub ability: Ability,
    /// Ability modifier plus proficiency plus applicable attack modifiers.
    pub attack_bonus: i32,
    /// The weapon's damage dice, without the ability modifier.
    pub damage: DiceExpr,
    /// The ability modifier added to each damage line.
    pub damage_bonus: i32,
    /// The versatile damage dice, when the weapon has them.
    pub versatile_damage: Option<DiceExpr>,
    /// The weapon's damage type, when known.
    pub damage_type: Option<DamageType>,
    /// Normal and long range in feet, when known.
    pub range_ft: Option<(i32, i32)>,
    /// Net advantage or disadvantage from attack contributions.
    pub hint: RollHint,
    /// The attack-bonus contributions in application order.
    pub breakdown: Vec<Contribution>,
}

pub struct DerivedSheet {
    // existing fields...
    /// Equipped weapon attacks in inventory order.
    pub attacks: Vec<DerivedAttack>,
}
```

`WeaponAttackKind` maps to `AttackKind::Melee` or `AttackKind::Ranged` only for
selecting the contribution key; the engine's three-valued `AttackKind` is not
used as the public field. The ability and proficiency base contributions are
included in `breakdown` so a client can explain the total. They are labeled
with the ability name (for example `Strength`) and `Proficiency`. The damage
bonus is not represented as a contribution; it is a plain derived addend.

Authorization is unchanged. `services::derived_sheet` already denies a masked
read, so adding weapon names does not expose inventory to a viewer who cannot
read the whole document.

## 5. Protocol

`DerivedSheetDto` gains a defaulted array so a client or fixture written before
the field still deserializes:

```rust
#[serde(default)]
pub attacks: Vec<DerivedAttackDto>,
```

The transport shape does not reuse the engine's attack-kind enum on the wire:

```rust
#[serde(rename_all = "snake_case")]
pub enum WeaponAttackKindDto {
    Melee,
    Ranged,
}

pub struct DerivedAttackDto {
    pub entry: InventoryEntryId,
    pub name: String,
    pub kind: WeaponAttackKindDto,
    pub ability: Ability,
    pub attack_bonus: i32,
    pub damage: DiceExpr,
    pub damage_bonus: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versatile_damage: Option<DiceExpr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_type: Option<DamageType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_ft: Option<(i32, i32)>,
    pub hint: RollHintDto,
    pub breakdown: Vec<ContributionDto>,
}
```

`Ability`, `DiceExpr` and `DamageType` already serialize and are used elsewhere
in the protocol. Optional fields use `skip_serializing_if` so an absent
versatile line, type or range stays out of the JSON. A line looks like:

```json
{
  "entry": "0190a5b4-7c3e-7a1b-8c2d-3e4f5a6b7c8d",
  "name": "Longsword",
  "kind": "melee",
  "ability": "str",
  "attack_bonus": 6,
  "damage": {"type": "roll", "count": 1, "sides": 8},
  "damage_bonus": 3,
  "versatile_damage": {"type": "roll", "count": 1, "sides": 10},
  "damage_type": "slashing",
  "hint": "normal",
  "breakdown": [
    {"source": {"type": "base"}, "label": "Strength", "op": "set", "value": 3},
    {"source": {"type": "base"}, "label": "Proficiency", "op": "add", "value": 3}
  ]
}
```

No HTTP route or command DTO is added. The existing
`GET /api/v1/entities/{id}/sheet` response carries the new array, and the
existing add/edit/remove inventory commands do the writing. Reference search
already accepts `kind=item`.

## 6. Browser UI

### 6.1 Inventory add and search

`ItemDraft` gains an optional `reference: Option<RefId>`. The add form becomes a
smaller copy of the spell add form:

- a search input and button using a new
  `web/src/url.rs::item_search_path` (`kind=item`);
- hits rendered as buttons; picking one sets the name and reference;
- a source line showing the reference with a `Clear` action;
- the existing name, quantity and state controls;
- the submit path builds `InventoryItem::Ref(ContentRef { id, name })` with the
  current name field when a reference is present, otherwise the current inline
  gear item.

Searching only runs on the button or Enter, matching the spell form. Empty
queries clear hits. A stale search response is ignored by the existing
`active(state, token)` guard. Editing the name after picking a hit leaves the
reference in place until `Clear` is pressed, also matching spells.

The form remains the only write path. There is no item-definition editor, so a
homebrew weapon is still an inline `Gear` item with no properties and therefore
no derived attack.

### 6.2 Inventory rows and links

`InventoryRow` resolves the item name as today. When the entry is
`InventoryItem::Ref`, the name renders as a button that calls the existing
`open_content` drawer, with a stable `data-testid="inventory-item-link"`.
Inline names stay plain text. Read-only rows keep the same link behavior without
edit controls.

The row does not gain property editors. Equip state is still changed through the
existing edit form.

### 6.3 Attack panel

The character detail renders an `Attacks` section from
`sheet.attacks`:

- one row per derived attack, keyed by `entry`;
- the weapon name, linked to content when the matching inventory entry is a
  reference;
- the signed attack bonus (`+6`, `+0`, `-1`);
- damage as `1d8+3 slashing`, with a second `1d10+3` versatile line when
  present;
- range as `25/100 ft` when present;
- the `hint` as an advantage/disadvantage marker;
- a muted note on the section: "Attack bonuses assume proficiency."

The attack line joins back to the inventory entry through `entry` to recover the
reference for the link; the DTO does not duplicate the reference. The panel is
read-only for every role. It shows an empty state when no equipped weapon
qualifies, for example "Equip a weapon to see its attacks." A weapon with no
damage, such as a net, produces no row.

There are no roll buttons. The existing dice tools remain the way to roll the
displayed expressions.

### 6.4 Formatting and selectors

Add `format::signed(i32)`, `format::dice_expr(&DiceExpr)` and
`format::damage_type_label(DamageType)` helpers, with unit tests for constants,
pools, sums, subtractions, multiplication, advantage/disadvantage wrappers,
nested terms and every damage type. Stable
selectors: `attack-section`, `attack-row`, `attack-name`, `attack-bonus`,
`attack-damage`, `attack-versatile`, `attack-range`,
`attack-proficiency-note`, and `inventory-item-link`. Reuse the spell search and
hit styling where possible rather than inventing a second visual language.

## 7. Tests

No core schema tests are added because no stored shape changes. The existing
item round-trip covers the reference and property payloads.

Adapter importer:

- longsword maps to `1d8` slashing, versatile `1d10`, tag `Versatile`;
- dagger maps to Finesse, Light, Thrown and `(20, 60)`;
- longbow maps to Ammunition and `(150, 600)`;
- blowgun's `damage_dice: "1"` maps to a constant one;
- net's `"0"` maps to `damage: None` and keeps Thrown and `(5, 15)`;
- chain mail maps to base 16, `max_dex_bonus: Some(0)`, Stealth disadvantage
  and Strength 13;
- breastplate maps to `max_dex_bonus: Some(2)`; leather maps to `None`;
- shield maps to `ShieldProperties { ac_bonus: 2 }`;
- `Class Feature`, `Spell` and `No Armor` records keep `properties: None`;
- malformed damage, range, versatile suffix and unknown damage type produce
  review notes rather than an import failure;
- the regenerated snapshot and report still load through the reference
  contract.

Engine rules:

- a level 5 character with Strength 16 and an equipped longsword gets `+6` to
  attack and `1d8+3` slashing, plus `1d10+3` versatile;
- a finesse weapon uses the higher of Strength and Dexterity, with Strength on
  a tie;
- a ranged weapon uses Dexterity; a thrown non-ammunition weapon uses the
  melee rules and carries its range;
- carried, stowed, non-positive, damage-less and unresolved weapons produce no
  line, with the existing warnings;
- an inline weapon and a referenced weapon behave identically;
- an attack-group or attack-key modifier applies to the matching lines, and an
  advantage contribution sets the hint without changing the bonus;
- multiple equipped weapons keep inventory order;
- a zero-quantity weapon now warns `ZeroQuantity`;
- heavy armor with `max_dex_bonus: Some(0)` ignores a negative Dexterity
  modifier, while light armor still applies one;
- an item reference whose kind is not `item` is rejected as `Malformed`.

Protocol:

- `DerivedAttackDto` maps every engine field, including the optional omissions;
- `WeaponAttackKindDto` maps melee and ranged;
- `DerivedSheetDto` golden JSON includes the attacks array and still round-trips
  the existing sheet fields.

Web native:

- `ItemDraft` with a reference builds `AddInventoryItem` with
  `InventoryItem::Ref`; without one it builds the inline gear item;
- `item_search_path` encodes the query and sets `kind=item`;
- `signed` and `dice_expr` cover positive, zero and negative bonuses and nested
  dice terms.

Integration and browser:

- fixture entries add a longsword with weapon properties and a common gear item
  such as Thieves' Tools; update fixture counts and search expectations;
- one E2E journey searches items, confirms a spell is not in the item hits,
  links and equips a longsword, opens the content drawer from the inventory row,
  sees the derived attack and versatile damage, changes the state to carried and
  sees the attack disappear, then links the gear item and confirms no attack
  line appears;
- the UI snapshot scenario equips the linked weapon, captures the attack panel
  at desktop and narrow widths, and records attack rows in the semantic report;
- `docs/testing.md` journey count and scope are updated with the new journey.

## 8. Phasing

1. **Content mapping.** Implement weapon and armor/shield mapping in the
   Open5e converter, add importer tests, regenerate the `srd2014` snapshot and
   report, and add the fixture weapon/gear entries. No engine or UI change.
2. **Engine and protocol.** Add `DerivedAttack`, the `rules::weapons` pass, the
   weapon quantity warning, the reference-kind validation, `DerivedAttackDto`
   and the sheet DTO field. Cover rules and protocol tests. No browser change.
3. **Read-only UI.** Render the attack panel and make referenced inventory names
   open the content drawer. Add the formatting helpers. No write-form change.
4. **Write UI and browser coverage.** Add item search and reference selection to
   the add form, add the E2E journey, and update the snapshot scenario so it
   links and equips a weapon.
5. **Fold and clean up.** Update `docs/engine.md`, `docs/architecture.md`,
   `docs/decisions.md`, `docs/testing.md` and `TODO.md`; delete this RFC.

Each phase compiles and tests independently. Phase 1 can land without changing
how existing inline items behave. Phase 2 can land while the browser still shows
the old inventory form; the new sheet field is simply ignored until phase 3.

## 9. Deferred work

- Weapon proficiency by class, race and feat, and fighting-style modifiers.
  Until then the attack bonus assumes proficiency and the UI says so.
- Simple versus martial weapon categories and per-weapon proficiency flags.
- A ranged/thrown category distinction and a dual line for thrown weapons.
- Extra Attack, two-weapon fighting and per-entry multiple lines.
- Unarmed strikes and natural weapons.
- Magic weapon attack/damage bonuses from item data or description text.
- Manual `AttackEntry` CRUD and merging manual lines with derived lines.
- Rolling attack and damage from the attack panel.
- A homebrew item editor for kind and weapon/armor properties.
- Ammunition tracking, critical hits, resistance and immunity.
- Encumbrance and an equipment pack/source for common gear that the pinned SRD
  dataset does not carry.

The `TODO.md` attack-line entry shrinks to the remaining proficiency and magic
bonus work; the basic derived weapon line is no longer deferred. The dice entry
keeps spell attacks and spell-damage automation as the remaining work.
