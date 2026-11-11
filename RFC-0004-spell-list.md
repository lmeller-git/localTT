# RFC-0004: Paper-sheet spell list

| | |
|---|---|
| **Status** | In progress |
| **Scope** | Core spell-entry level, engine spell/resource CRUD, cast fallback, protocol DTOs, browser spell panel |
| **Temporary** | Delete this file once the feature is implemented and folded into the permanent docs |

## 1. Problem

The character sheet has no paper-style spell list. `SpellcastingBlock` and
`SpellEntry` carry a name and an optional reference but no spell level, so the
UI cannot group spells by level without resolving content. Slot capacity is
derived, while the actual trackers are `Resource`s; the browser only shows the
derived capacity and dumps every resource into a generic list. There are no
commands for adding, editing or removing spells or resources, and the browser
never renders `StatblockView.spellcasting`.

The goal is a list that looks and behaves like the paper sheet: spells grouped
by level, slot trackers in the level headers, and each spell linked to its
reference entry through the existing reference API.

## 2. Goals and non-goals

Goals:

- Store an optional spell level so grouping works for resolved and homebrew
  spells.
- Add spell and resource CRUD commands.
- Let `CastSpell` use the stored level when a reference is missing or does not
  resolve to spell content.
- Render a paper-style spell list with per-level slot trackers.
- Link referenced spells to the existing content drawer.
- Reuse the existing reference search (`kind=spell`) and entry endpoints.

Non-goals for this feature:

- Auto-provisioning slot resources from class tables.
- A full cast dialog (slot picker, upcast, concentration) in the browser.
- Moving spells between spellcasting blocks.
- Changing a resource's kind.
- Cantrip casting without a slot.
- Prepared-count validation.

## 3. Data model

`SpellEntry` gains an optional level snapshot:

```rust
pub struct SpellEntry {
    pub id: SpellEntryId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RefId>,
    /// 0 = cantrip, 1..=9 = leveled, None = unknown or legacy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    pub prepared: bool,
}
```

`None` is distinct from `Some(0)`: a legacy or homebrew entry must not silently
become a cantrip. Missing JSON fields deserialize to `None`.

`SchemaVersion::CURRENT` moves to `1.2`. This is an additive minor change; old
`1.1` documents load and are re-stamped per the version policy in
`crates/localtt-core/DESIGN.md`.

Adding a public field to `SpellEntry` is source-breaking for struct literals;
all in-workspace constructors are updated in phase 1.

## 4. Engine commands

New `Command` variants, all authorized as `Requirement::Write(entity)`:

```rust
AddSpell {
    entity: EntityId,
    placement: SpellPlacement,
    spell: NewSpell,
    expected_rev: Option<u64>,
}
EditSpell {
    entity: EntityId,
    spell: SpellEntryId,
    name: String,
    reference: Option<RefId>,
    level: Option<u8>,
    prepared: bool,
    expected_rev: Option<u64>,
}
RemoveSpell { entity, spell, expected_rev }
AddResource {
    entity, name, kind: ResourceKind, max: Formula,
    current: Option<i32>, recharge: Recharge, expected_rev,
}
EditResource {
    entity, resource,
    name: Option<String>, max: Option<Formula>,
    current: Option<i32>, recharge: Option<Recharge>,
    expected_rev,
}
RemoveResource { entity, resource, expected_rev }
```

`SpellPlacement` is either `Existing(SpellcastingBlockId)` or
`New { source: RefOrText, ability: Ability }`.

`EditSpell` replaces all client-editable fields. This avoids a tri-state
`Option<Option<T>>` in the public API; clients send the current values back and
`expected_rev` protects against lost updates.

`EditResource` is partial. A resource's kind is immutable for now. When the
maximum changes to a constant and `current` is not also supplied, `current` is
clamped to the new maximum.

Validation:

- Spell name trimmed and non-empty.
- Spell level at most 9.
- A spell reference must have kind `spell`.
- A missing block is `NotFound` with `ElementRef::SpellcastingBlock`.
- Resource name non-empty.
- `SpellSlot.level` in `1..=9`; `HitDie.sides` in `4..=12`.
- Constant maxima are non-negative.
- `AddResource.current` defaults to the constant maximum, or `0` for an
  expression maximum.

## 5. Cast fallback

`CastSpell` resolves the reference as today. The effective spell level is the
resolved content level when available, otherwise the stored `SpellEntry.level`:

```text
spell_level = resolved_content_level.or(known.level)
```

- `Some(0)` rejects casting with a slot.
- `Some(n)` validates `upcast_slots` against the slot level.
- Concentration still comes from resolved content.
- An unresolved reference still emits `UnresolvedReference`.

This makes homebrew and unresolved spells usable with slot trackers.

## 6. Protocol

- Add `CommandDto` variants and `TryFrom<CommandDto> for Command` mappings for
  the six commands.
- Add `SpellPlacementDto`, `NewSpellDto`, and
  `ElementRefDto::SpellcastingBlock`.
- Add `ContentEntryDto::spell()` returning a protocol-level
  `SpellDataDto { level, concentration, school, ritual }`, so the web client can
  prefill and display spell data without importing engine types.
- No new HTTP routes. Commands use the existing command endpoint; reference
  search already accepts `kind=spell`.
- Add `web/src/url.rs::spell_search_path`.

## 7. Browser UI

Replace the current "Spell slots" section with a `SpellPanel`:

- Header: spellcasting ability, spell save DC (`spell.dc`), spell attack
  (`attack.spell`).
- Slot strip grouped by `ResourceKind::SpellSlot` plus `PactSlot`, with pips,
  `-`/`+` buttons calling `SpendResource`/`RestoreResource`, and `current/max`.
- Muted derived capacity plus an "Add tracker" action when a level has capacity
  but no resource.
- Slot and pact resources are filtered out of the generic Resources section.
- One section per spellcasting block, grouped into Cantrips, 1st through 9th,
  and Unleveled (`None`).
- Each row: prepared checkbox, name, source badge, edit/remove controls.
- Referenced names open the existing content drawer; unreferenced names are
  plain text.
- Add-spell form: search with `kind=spell`, prefill name/reference/level from the
  full entry, manual fallback for homebrew, block selection, prepared.
- Slot tracker form: add, edit and remove slot/pact resources.

Casting from the list is a follow-up.

## 8. Tests

Core: old-document level deserialization, omission of `None`, cantrip round
trip, schema `1.2` acceptance, builders.

Engine: add/edit/remove spell and resource commands, validation, scoped
`NotFound`, authz classification, cast fallback for stored levels.

Protocol: command golden JSON and mapping for every new variant,
`ElementRefDto::SpellcastingBlock`, `ContentEntryDto::spell()`.

Integration: caster fixture with leveled spells and slots; add-from-search,
spend/restore, long-rest refill, cast spends a slot.

Browser: E2E spell-list journey and UI snapshots.

## 9. Phasing

1. **Data, engine and protocol.** Level field, six commands, cast fallback,
   tests, docs. No UI change.
2. **Read-only UI.** Grouped list, source links, slot pips with spend/restore,
   generic-resource filtering.
3. **Write UI.** Add/edit/remove spells with search prefill; add/edit/remove
   slot trackers.
4. **Polish.** Paper-sheet styling, E2E, snapshots, permanent docs, delete this
   RFC.

Each phase compiles and tests independently.
