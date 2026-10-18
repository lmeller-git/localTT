# RFC-0001: localtt — a local-first tabletop companion for D&D 5e

| | |
|---|---|
| **Status** | Draft |
| **Audience** | The implementing agent(s) and the maintainer |
| **Scope** | Backend only (domain, engine, persistence, server). Frontend is explicitly out of scope for now. |
| **Rules target** | D&D 5e, 2014 edition (hard assumption for now; core should stay edition-agnostic where this is cheap) |

---

## 0. How to read this document

- This RFC is the source of truth for **intent and design invariants**. It is **not** an implementation plan. The implementing agent should derive a phased plan from it (§11 gives ordering guidance, §10 gives technology options).
- "**MUST**" marks invariants that came out of the design discussion and should not be violated without raising it with the maintainer. "**SHOULD**" marks strong defaults. "**Option**" marks things that are genuinely open.
- Pseudo-schemas in §4 are **illustrative**. Names, exact types and byte-level layout are decided during implementation. The *shape* (which concepts exist, who owns what, what is stored vs. derived) is binding.
- `AGENTS.md` and `TODO.md` in the repo govern workflow conventions. If something here conflicts with them, flag the conflict instead of silently picking one.
- Items in §9 are open or deferred. Do not invent answers to them quietly; either pick the recommended default and record it, or ask.

---

## 1. What we are building

### 1.1 Summary

A **locally hosted web app that digitizes the D&D character sheet and adds light quality-of-life automation** for a local, in-person or voice-call party. The DM runs a server on their own machine; players join from a browser on the same network. It is a **note-taking and bookkeeping aid for social play**, not a virtual tabletop and not a rules referee.

What it provides:

- Beginner-friendly character sheets that mirror the paper sheet, with **suggested** computed values (ability modifiers, saves, skills, AC, attack bonuses, spell DCs, proficiency bonus) and a visible breakdown of where each number comes from.
- Lightweight automation of bookkeeping: spell slots, hit dice, temp HP, limited-use features, rests, equip state, status effects, concentration.
- Server-side dice rolling with **audience control** (public, DM-only, private) and a short table feed.
- Hyperlinks from sheet entries (spells, features, conditions, items) to descriptions, backed by user-supplied reference sources (e.g. a local SRD 2014 snapshot) plus content packs.
- A **DM/player permission model**: the DM sees and edits everything; players see and edit their own entities and whatever is explicitly shared with them (statblocks, notes, handouts).
- **Persistence per campaign** in a single local file, restorable on the next session.

### 1.2 Setting and users

- **One campaign per server run.** The server is started against one campaign file. No lobby, no multi-campaign switching.
- **Network:** the DM's machine is the single source of truth. Players connect over the LAN (or a VPN such as Tailscale, which should work unchanged) using a browser, likely on phones. No player-side install, no cloud, no accounts.
- **Users are beginners as well as veterans.** The DM is technical enough to run a binary. Players only need a browser and an invite link.
- **Content:** the group follows published campaigns, so homebrew is minor now but **will grow later**. It must be possible from the start without redesign.

### 1.3 Non-goals

- Battlemaps, tokens, fog of war, line of sight (solved by other tools or in person).
- A full combat tracker: no initiative automation, no action/bonus action/movement tokens, no turn engine. (Explicitly considered and rejected for now, see §5.3. It must remain *addable* later.)
- Authoritative rules enforcement. The app **suggests**, it never blocks (§2, principle 2).
- Accounts, cloud sync, internet-facing deployment, DRM or content locking.
- Hosting non-SRD copyrighted content. We do not scrape sites and do not redistribute WotC material beyond what the SRD licenses permit.
- PDF import/export, level-up wizards, and scripting are **future** features (§6), though the design must not make them hard.

---

## 2. Key ideas to keep in mind

These came out of the discussion and should guide every trade-off.

1. **Paper sheet first.** The stored model mirrors the boxes on a paper sheet. Automation is a layer of *suggestions* on top. This also makes PDF import/export nearly trivial later.
2. **Nothing is authoritative.** Computed values are suggestions. Players MUST be able to overwrite any derived value. Commands and the rules engine MUST NOT reject input for rules reasons; they return **warnings**, not errors (e.g. spending a slot at 0 warns but succeeds).
3. **Store base data and sparse overrides; derive everything else.** Derived values are never persisted. A displayed value is `override ?? computed`. The UI should be able to show both ("AC 18, computed 16") and offer a reset.
4. **Content is data, homebrew is just more data.** Items and features can be pure text (name + description) with zero mechanics. Mechanics (modifiers, resources) are optional additions. Official and homebrew content share the same shape.
5. **Edition lives in data plus a few hooks.** 2014 is the only ruleset implemented, but every entity carries a ruleset ID and edition-specific behavior sits behind a small, explicit hooks interface.
6. **One entity model, many kinds.** PCs, companions, summons, familiars, NPCs, monsters, notes and handouts are all *entities* with an owner and an ACL. A player owns any number of entities.
7. **Permissions are enforced at exactly one egress point.** A single function, `view_of(entity, viewer)`, decides what any viewer sees. Every REST response and every WebSocket event passes through it.
8. **The DM host is the single source of truth.** No distributed state, no player-side copies to reconcile. Players can export their own sheet as a backup.
9. **Server-authoritative rolls and commands, intent-based writes.** Clients send intents (`spend_slot`, `apply_damage`), not whole documents, so simultaneous edits (DM damages a PC while the player spends a slot) do not clobber each other.
10. **Time is a DM-controlled clock.** Status durations are absolute expiry times on one campaign clock. Rounds are just 6 seconds. There is no turn engine.
11. **Secret rolls are social, not cryptographic.** The DM hosts the file and can read it. "Secret" means hidden from other players.
12. **Hexagonal architecture.** Domain and engine are pure and testable without I/O. Storage, reference data, PDF and the network are adapters.
13. **Extensibility without a redesign.** Data types are versioned, serializable and namespaced for extension, so plugins, new frontends and new rulesets can be added later.

---

## 3. Architecture

### 3.1 Hexagonal layout mapped to the current workspace

```
                 ┌───────────────────────────────┐
  HTTP / WS ───▶ │ localtt-server  (driving)     │
  clients        │ routing, join flow, DTOs,     │
                 │ event fan-out, composition    │
                 └──────────────┬────────────────┘
                                │ calls
                 ┌──────────────▼────────────────┐
                 │ localtt-engine (application)  │
                 │ rules, commands, dice, clock, │
                 │ authz/view_of, services,      │
                 │ ports/ (traits)               │
                 └──────────────┬────────────────┘
                                │ depends on
                 ┌──────────────▼────────────────┐
                 │ localtt-core (domain)         │
                 │ types/ (data layout, serde),  │
                 │ ids, errors                   │
                 └──────────────▲────────────────┘
                                │ implements engine ports
                 ┌──────────────┴────────────────┐
                 │ localtt-adapter (driven)      │
                 │ sqlite store, reference data, │
                 │ content packs, later: pdf     │
                 └───────────────────────────────┘
```

Dependency rules:

- `core` has **no I/O dependencies**. It depends on little more than `serde` and an ID library, so it stays fast to compile and can be factored out as a plugin-facing crate.
- `engine` depends only on `core`. It is pure logic plus orchestration and **owns the ports** (`ports/`).
- `adapter` depends on `engine` (to implement its ports) and on `core`; never the other way around.
- `server` depends on `engine`, `adapter` and `core`. The composition root (wiring adapters into the engine) lives either in `server` or in a thin binary crate (open question, §9).

### 3.2 Responsibilities per crate

**`localtt-core`**
- `types/`: the serialization and data layout of all persisted and wire-visible domain types: `Entity`, bodies, `Status`, `Resource`, `Clock`, `FeedEntry`, `Roll`, `Acl`, `RefId`, schema versions. Types are data: constructors and invariant helpers only, no rules logic. This module MAY be extracted into its own crate later so plugin authors can depend on it without pulling in the engine. Keep it dependency-light and treat its serde representation as a public, versioned contract.
- Shared ID types, error types, and the `Ruleset` identifier.

**`localtt-engine`**
- `ports/`: traits the engine needs from the outside world (see 3.3); the engine defines them, the adapter implements them.
- `rules/`: modifier pipeline, formula evaluation, derived stats, warnings, ruleset data and hooks (2014).
- `commands/`: domain actions applied to entities (§5.2).
- `dice/`: notation parser, AST, roller (RNG via a port), result structure.
- `clock/` (or part of `commands`): clock arithmetic and status expiry (pure functions of statuses and `now`).
- `authz/`: ACL checks and `view_of` redaction.
- `service/`: application services orchestrating ports: load, authorize, apply command, persist, emit events. Keep pure logic (`rules`, `dice`, `authz`) free of port access so it can be unit-tested directly.

**`localtt-adapter`**
- SQLite store (entities, players, feed, campaign meta, migrations, export/import, backups).
- Reference data: SRD 2014 import and lookup, content pack loader, full-text search.
- Later: PDF import/export adapters.
- In-memory implementations of the ports for tests (here or in a test-support module).

**`localtt-server`**
- Driving adapter: REST for CRUD and commands, WebSocket for live events, invite/join flow, DM key handling, request limits, origin checks.
- DTO mapping, OpenAPI generation, per-recipient event fan-out through `engine::authz::view_of`.
- Serving the embedded frontend later.
- Note: the workspace currently has `lib.rs` only; a binary entry point (CLI args, startup banner with LAN URL and QR code) is needed somewhere.

### 3.3 Ports (illustrative)

| Port | Purpose |
|---|---|
| `CampaignStore` | Load/save campaign meta, players, entities, feed. Transactional unit of work for one command. |
| `ReferenceSource` | `get(RefId)`, `search(query, kind)` over SRD and packs. Read-only. |
| `Rng` | Dice randomness, injectable for deterministic tests. |
| `EventSink` | Engine publishes domain events with audiences; the server subscribes and fans out. |
| `SheetExporter` / `SheetImporter` | Future PDF adapters. |

**Command execution model:** commands are serialized through a single writer (an actor/task or a mutex around the unit of work). At party scale (under ~10 clients) this is far more than sufficient and removes whole classes of races. Each command is one atomic transaction that returns: the new `rev`, any warnings, and the domain events it produced.

### 3.4 Layout is not final

The mapping above is a proposal. Reasonable adjustments, to be decided during implementation (see §9): whether the adapter crate splits (e.g. store vs. reference vs. pdf, since they pull heavy and unrelated dependencies), and where the binary lives. Ports live in the engine (decided 2026-10-17; decision ENG-1), and the adapter may depend on the engine to implement them.

---

## 4. Core domain model

Illustrative pseudo-schema. IDs are opaque and stable. **Every list element has its own stable ID; lists are never addressed by index** (indices shift under concurrent edits).

### 4.1 Campaign and players

```
Campaign (singleton per file)
  id, name, schema_version
  default_ruleset: RulesetId            // e.g. "5e-2014"
  clock: Clock { now_s: i64 }           // seconds since campaign epoch
  settings: { player_roll_audience_default, feed_capacity, ... }
  ext: Map<Namespace, Json>

Player                                  // a "slot"; the unit of identity
  id, display_name
  role: Dm | Player
  token_hash                            // hash of the join token; never the token
  created_at, last_seen
```

There is exactly one DM slot (co-DM is a possible later extension, §9).

### 4.2 Entity

```
Entity
  id: EntityId
  kind: Pc | Companion | Summon | Familiar | Npc | Monster | Note | Handout
  ruleset: RulesetId                    // present from day one
  name
  owner: PlayerId                       // DM-owned entities are owned by the DM's PlayerId
  parent: Option<EntityId>              // summon/companion/familiar -> its PC
  acl: [AclEntry { player, level: Read | Write, field_mask: Option<FieldMask> }]
  rev: u64                              // increments on every change
  schema_version
  body: Statblock | Note | Handout      // by kind
  ext: Map<Namespace, Json>
  created_at, updated_at
```

- **Ownership implies write.** Owner has full rw on the entity; the DM has rw on everything; ACL entries grant additional `Read` (optionally field-masked) or `Write` (co-control) to other players.
- A player may own any number of entities (PC plus familiar plus summons). The DM can reassign ownership.
- `field_mask` only matters for DM-owned entities shared read-only with players (e.g. show a monster's name, AC and description, hide HP and actions). Players always see their own entities unmasked.

### 4.3 Statblock body

One body shape serves characters and creatures. Character-only and creature-only sections are optional.

```
StatblockBody
  profile: Character | Creature
  identity: free-text boxes (race/class/background/alignment text) + optional refs

  base:                                 // the "boxes on the paper sheet"
    abilities { str, dex, con, int, wis, cha }
    levels: [ClassLevel { id, class (ref|name), level, subclass? }]    // character
    cr, creature_type, size, senses, ...                               // creature
    proficiencies { saves, skills (none|prof|expertise), tools, languages, ...text }
    hp { max, current, temp }
    speed, death_saves { successes, failures }, exhaustion, inspiration
    coins { cp, sp, ep, gp, pp }

  overrides: Map<StatPath, Value>       // sparse; keyed by stat path, e.g. "ac", "skills.stealth"

  resources: [Resource]
  spellcasting: [SpellcastingBlock { id, source (class/other), ability, spells: [{ id, ref|name, prepared }] }]
  inventory: [InventoryEntry { id, item: ItemRef | InlineItem, qty, state, attuned, notes }]
  attacks: [AttackEntry { id, name, bonus?, damage_text, from_inventory?: id }]   // manual entries
  features: [FeatureEntry { id, category, ref? | inline name+text, resource_id?, modifiers? }]
  statuses: [Status]
  notes: free text
```

- `FeatureEntry.category` covers class/race/feat/trait and, for creatures, `action | reaction | legendary | other`, so monster actions use the same shape as character features.
- `HP max` is a plain input with an optional computed hint (average from hit dice); it does not need an override entry.
- Cross-entity derivation (e.g. a Beast Master companion using its PC's proficiency bonus) is **deferred**. For now a companion is an ordinary statblock whose values the player can type or override. The `parent` link keeps the door open.

### 4.4 Items, features, modifiers

```
InlineItem
  name, description (markdown), kind (weapon|armor|shield|wondrous|gear|...)
  requires_attunement, weight?, value?
  properties? (structured, optional), modifiers?, resource?

Modifier
  target: StatKey                       // e.g. "ac", "save.wis", "attack.melee", "skill.stealth"
  op: Add | Set | Min | Max | Advantage | Disadvantage | ...
  value: Formula
  when: Option<Condition>               // closed set: Equipped, Attuned, StatusActive(..), ...
  note: Option<String>

Formula                                 // small closed set, not a scripting language
  Const | ProficiencyBonus | AbilityMod(a) | Level | ClassLevel(c)
  | Add | Sub | Mul | Min | Max | FloorDiv ...
```

Source attribution is implied by the container (item, feature, status), so the breakdown can say *where* each contribution came from. A reserved extension point may allow custom formulas later, but it must not be built now.

### 4.5 Resources

```
Resource
  id, name
  kind: SpellSlot{level} | PactSlot | HitDie{sides} | Feature | Item | Other
  max: Formula | Int                    // computed, overridable
  current: i32
  recharge: Short | Long | Dawn | Manual | Never
```

Spell slots, pact slots, hit dice, Channel Divinity, Second Wind, item charges and legendary actions are all resources. Recharge-by-roll (e.g. monster "Recharge 5–6") is `Manual` for now.

### 4.6 Status (includes concentration)

```
Status
  id
  kind: Condition | Effect | Concentration | Custom
  name, ref?, text?                     // custom statuses are first-class (inline name + text)
  started_at: i64
  duration: Timed{expires_at} | UntilRest(Short|Long) | Indefinite | Manual(text)
  source: { entity?, status? }          // who applied it; link to a concentration source
  modifiers?: [Modifier]                // optional; statuses are informational by default
```

### 4.7 Dice, rolls and the feed

```
Roll
  id, roller: PlayerId, entity?: EntityId, ts
  expr: DiceExpr (AST)
  result: terms [ { dice: [ { value, kept } ], ... } ], total
  context?: RollContext { kind: Check|Save|Attack|Damage|..., stat?, target_dc? }
  audience: Audience

Audience = Public | RollerAndDm | RollerOnly    // presets over a general AudienceSet

FeedEntry
  id (monotonic), ts, clock_s
  kind: Roll | StatusExpired | ConcentrationEnded | Hint | ...
  actor?, entity?, payload
  audience: Audience
```

The feed is a **ring buffer** of the last N entries, persisted so it survives a restart but is not a permanent log.

### 4.8 Reference IDs

References are namespaced strings of the form `<source>:<kind>/<slug>`, e.g. `srd2014:spell/fireball` or `pack.<packname>:item/foo`. Sheets store the reference plus a **name snapshot**, so a missing pack or a renamed entry degrades to plain text rather than a broken sheet.

### 4.9 Notes and handouts

- `Note`: markdown body with an ACL. DM notes are private by default.
- `Handout`: either free text or an item definition. A DM can **share** it (read-only via ACL) or **give** it (a copy lands in a player's inventory and becomes theirs, editable). Both actions exist. Which is the default is open (§9).

---

## 5. Systems

### 5.1 Rules engine (suggestion layer)

**Requirements**
- MUST compute derived values from base data plus modifiers: proficiency bonus, ability modifiers, saves, skills, passive scores, AC, initiative bonus (value only), attack bonuses, spell save DC and spell attack bonus, spell slot maxima (including multiclass tables), hit dice totals, speed.
- MUST return, per derived stat, the value **and a breakdown of contributions with sources** (`+2 prof, +3 DEX, +1 Ring of Protection`). Beginners learn the rules from this.
- MUST return **warnings** instead of failing: two body armors equipped, over-attunement, spending an empty resource, multiple concentration statuses, etc.
- MUST treat `overrides` as winning over computed values, without discarding the computed value.

**Hints**
- Modifier pipeline: base, then `Set` ops (e.g. armor base AC), then `Add` ops, then `Min`/`Max` clamps, with explicit stacking rules where 5e has them. Only modifiers whose `when` condition holds (equipped, attuned, status active) contribute.
- Rulesets are data bundles (classes, spell slot tables, proficiency table, conditions, ...) plus a small hooks interface. Known edition-sensitive spots: rest recovery (e.g. 2014 long rest regains up to half of total hit dice), exhaustion effects, where ability score increases come from (race vs. background), weapon mastery, class features. From what we know, the underlying math (proficiency bonus, modifiers, slots, DCs) is the same across editions, but this has not been verified against the 2024 rules. **Do not build hooks for hypothetical differences.** Add one when a second ruleset needs it.
- Temp HP: does not stack (take the higher), is depleted before HP, and healing does not restore it.
- Formulas are a closed enum, evaluated by the engine. No scripting language yet.
- HP max, hit dice and similar values that groups commonly houserule are inputs with computed hints, not forced calculations.

**Rough edges**
- Multiclass spellcasting (slot table from combined caster levels, separate spell lists per class) and Pact Magic (short-rest recharge, separate slots).
- Features whose uses scale with a stat (e.g. proficiency-bonus uses per long rest).
- Overrides going stale when inputs change. Keep the override (it was the player's intent) and keep the computed value visible so drift is obvious.

### 5.2 Commands

Two kinds of writes:

- **Domain commands** (named intents): atomic, rules-aware, return warnings and feed entries.
- **Field edits** (`set(path, value)`): the paper-sheet edit. Path-level last-write-wins is acceptable for this scale. Whole-document replace requires an `If-Match` style `rev` check. Lists are edited by element ID.

Candidate command catalogue (v1 shape, not exhaustive):

- Entities: create, update (field edit), delete, reassign owner, share/unshare (ACL), create summon from a template (copy on create, optional ephemeral flag), give handout.
- Overrides: set override, clear override.
- HP: apply damage, heal, set temp HP, death save marks.
- Resources: spend, restore, set, spend hit die (rolls via the dice system, heals).
- Casting: cast spell (spends a slot, optional upcast, proposes concentration if the spell has the flag).
- Rests: short rest, long rest (resets resources by recharge type, clears rest-typed statuses). Whether they auto-advance the clock is deferred (§9).
- Inventory: add/remove item, equip/unequip, attune/unattune.
- Statuses: add, remove, dismiss, edit duration.
- Concentration: begin (replaces any existing), end.
- Clock: advance, set (DM only).
- Dice: roll with expression, label, audience.
- Players and invites: create invite, revoke, reissue token.
- Campaign: export, import.

Commands never fail for rules reasons. They fail only for authorization, malformed input, or missing entities.

### 5.3 Clock and status durations

**Decision:** no combat mode, no turn engine. One campaign clock, controlled by the DM, drives everything time related. A full in-battle mode (initiative, action economy tokens) was considered and rejected as contrary to the app's spirit (notes and QoL for social play). It stays addable as a layer later: a combat mode would simply call `advance_clock(6s)` per round and add initiative on top, with no change to the status model.

**Design**
- The clock is an integer `now_s` on the campaign. Only the DM writes it (`advance_clock`, `set_clock`); everyone reads it. One round is 6 seconds, so a duration of "10 rounds" is 60 seconds.
- Statuses store an **absolute `expires_at`**, not a remaining counter. Advancing the clock is then one write. "Remaining" and "expired" are derived: `expired = now >= expires_at`. Correcting the clock backwards correctly un-expires things.
- Statuses with `UntilRest(...)` are cleared by the rest commands. `Indefinite` and `Manual(text)` are never auto-expired ("until the vault is opened").
- When a clock change expires statuses, the engine emits `StatusExpired` feed entries to the owner and the DM.
- The clock also enables `Dawn` recharge: crossing a configurable dawn hour recharges resources of that type. Display formatting of the clock ("Day 12, 14:30") is separate from storage. A named calendar is a later display option.

**Rough edges**
- Turn-relative effects ("until the end of the target's next turn") depend on initiative position, not elapsed time. Treat as "1 round" and leave exact timing to the table.
- The clock is global. A split party shares one clock.

**Deferred details (not needed now):** whether expiry is soft (hidden from the active bar but recoverable until dismissed or the next long rest; recommended, because it keeps clock corrections non-destructive) or hard; whether rests advance the clock by default (short 1h, long 8h, DM-confirmed).

### 5.4 Concentration

- Modeled as a `Status` with `kind: Concentration`. It shows what the entity is concentrating on and for how long, and gets its own symbol and pinned placement in the UI.
- **Replace semantics live in the command layer, not storage:** `begin_concentration` removes any existing concentration status on that entity and emits `ConcentrationEnded`. A manual edit may still add a second one (homebrew exists; nothing is locked), and the engine warns.
- **Linked effects:** an effect applied to others by a concentration spell stores `source: (caster entity, concentration status)`. Ending the concentration can then clear or flag the linked statuses. Store the link now; the clearing behavior may come later.
- **Suggestions, not automation:** casting a spell with the concentration flag proposes beginning concentration. `apply_damage` attaches a hint to a concentrating entity: "CON save DC = max(10, damage/2)". Dropping to 0 HP hints that concentration ends.

### 5.5 Status effects and equip state

- **Status bar:** each status shows its name, a link to the condition or spell description (or inline text for custom ones), and remaining time. Custom statuses are first-class because DMs hand them out routinely.
- Statuses are **informational by default**. Attaching modifiers is opt-in and reuses the modifier pipeline (e.g. Bless adds to attacks and saves).
- **Equip state** is a flag on each inventory entry (`carried | equipped | stowed`, plus `attuned`). It does not model action cost. The flag is enough for the table to track gearing up, and a feed entry can tell the DM. Only modifiers from equipped (and, where required, attuned) items contribute to derived stats.
- Conflicts (two body armors, a fourth attuned item) produce warnings only. AC takes the best applicable base.

### 5.6 Dice and the table feed

**Dice**
- Notation parser producing an **AST**, not just a total: `NdM±K`, advantage/disadvantage, keep highest/lowest (`4d6kh3`), multi-term bag rolls (`2d6+1d4+3`). Reroll and explode modifiers are optional later.
- Results are structured: each die's value and whether it was kept, so frontends can render breakdowns and nat 1 / nat 20 detection works under advantage and disadvantage.
- RNG behind a port. Default is an OS-seeded CSPRNG; use a uniform range sampler to avoid modulo bias. Cap dice counts, sides and expression length to prevent abuse.
- Rolls are composed from the modifier pipeline when requested by a sheet ("roll Stealth"), and carry an optional `RollContext` (kind, stat, target DC).
- Keeping the AST, the individual d20 results and the crit range around makes **expected damage and hit-rate analytics** (including crits) a later, purely additive feature.

**Audience** covers all discussed cases with three role-agnostic values:
- `Public`: whole-party log ("Alex rolled 17 on Perception").
- `RollerAndDm`: the "share with DM only" case.
- `RollerOnly`: a secret roll. For a player this is a private roll; for the DM it is a DM-secret roll.

Internally audiences are sets, so "DM reveals to one player" is a later addition. The default for player rolls is a campaign setting (recommended default: `RollerAndDm`), overridable per roll.

**Feed**
- One ring buffer (last N entries, persisted) carries rolls and system messages (`StatusExpired`, `ConcentrationEnded`, `Hint`, ...). Same audience model, same persistence, same `view_of` delivery path, so there is still exactly one egress point.
- The feed is deliberately temporary. It is not an audit log.

### 5.7 Permissions and `view_of`

**Model**
- Roles: one `Dm`, many `Player`s.
- DM: rw on every entity, note and roll; can reassign ownership.
- Player: rw on entities they own; read (optionally field-masked) or write on entities shared via ACL; sees only feed entries whose audience includes them.
- Players MAY create entities of owner-style kinds (companion, summon, familiar, notes). Whether the DM can restrict this is open.

**The biggest implementation risk is leaking data at an egress point.** Therefore:

- `view_of(entity, viewer)` (and the equivalent for feed entries) is the **only** function that produces viewer-facing representations. REST handlers and WebSocket fan-out MUST NOT serialize domain types directly.
- WebSocket broadcasts are filtered **per recipient**. Never send a "full entity updated" event to everyone.
- Authorization tests are a matrix: every (viewer role, ownership/ACL state, entity kind, egress path) combination.

### 5.8 Identity, joining and network

**Invite-slot model**
1. The DM creates a player slot (optionally pre-assigned to existing entities) and gets a join link plus QR code.
2. The player opens the link; a long random token is stored in the browser (cookie or localStorage). The server stores only its hash.
3. On return, the token reconnects them to the same slot and all entities they own. If it is lost, the DM reissues a link.

This beats storing session state on the player's machine: state lives in one place, so there is nothing to sync or reconcile. Players can additionally export their own sheet as JSON.

**DM identity:** a DM key (URL with token) is printed at startup. Persisting its hash in the campaign file so a bookmark survives restarts is recommended; details are open (§9).

**Network and security**
- Bind `127.0.0.1` by default. Binding `0.0.0.0` for LAN play is an explicit opt-in flag. The startup banner prints the LAN URL and QR code.
- Plain HTTP on a home LAN is acceptable, but tokens travel in the clear. Document this honestly. Self-signed TLS typically causes more browser-warning friction than it prevents.
- Check `Origin` on WebSocket upgrades. Use unguessable tokens (128+ bits). Rate-limit token attempts. Cap request and body sizes.
- mDNS (`name.local`) is an optional convenience and is unreliable on some networks; IP plus QR is the primary path.

### 5.9 Persistence

- **One SQLite file per campaign.** Single file means trivial backup, copying and "restore next session".
- Entity bodies are **JSON documents** inside rows. The structure is still evolving and relational queries over sheet internals are not needed. Extract only what is queried (id, kind, owner, parent, name, rev, ACL).
- Likely tables: campaign meta (schema version, clock, settings), players, entities, ACL entries, feed. Exact DDL is for the implementer.
- **Schema versioning from day one:** `schema_version` on the campaign and on every entity document. Migrations are pure functions over the JSON. The loader MUST refuse a file with a newer major version than it supports instead of silently dropping data. Commit fixture campaign files from each version and test migrations against them.
- Export/import to plain JSON as an escape hatch and for player-side backups.
- Backups: use SQLite's online backup mechanism (not a raw file copy while the server runs, especially in WAL mode) and take an automatic backup on startup.
- Reference data (SRD and packs) lives **outside** the campaign file. Campaign data stores `RefId` plus name snapshots. Whether used pack entries should be vendored into the campaign file for portability is open (§9).
- SQLite is synchronous; run it on a dedicated thread or via `spawn_blocking`, behind the single-writer command executor.

### 5.10 Reference data and hyperlinks

- **Reference data is user-supplied; the server bundles no data of its own.** It loads whatever local directories or live APIs it is pointed at, and a pinned SRD 2014 snapshot is the standard local source (candidate datasets: the community 5e database and Open5e; verify license terms and field names of whichever is used). A local source keeps a LAN game working offline with no live API dependency.
- **Do not scrape** third-party sites. It is brittle, legally murky and not redistributable.
- **Content packs:** TOML/JSON/Markdown-with-frontmatter loaded from a folder, namespaced by pack, layered over the other reference sources. This is how homebrew and non-SRD books the group owns are added. A pack entry is the same shape as an inline item/feature/status.
- Reference entries carry: id, kind (spell, feature, condition, item, monster, class, race, background, rule), name, markdown description, optional structured fields (spell level, school, concentration flag, duration text and parsed seconds when parseable), and source/attribution.
- Full-text search via SQLite FTS5 or equivalent behind the `ReferenceSource` port.
- Attribution: each source's license and required attribution travel with its entries (the SRD is CC-BY); surface them in the app.
- Rough edges: duration strings need parsing for the common patterns ("up to 1 minute", "1 hour"), with manual fallback; items with variable properties (+1 weapons); name drift between editions.

### 5.11 Real-time sync

- WebSocket per client. Server pushes per-recipient filtered events (via `view_of`).
- Version 1: send the full filtered entity on change (documents are small); patches are an optimization for later.
- Reconnect: the client resumes with the last seen feed ID and re-fetches entity `rev`s that changed.
- Concurrency: intent-based commands plus element-ID addressing make conflicts rare. Whole-document writes use `rev` checks. CRDTs are overkill for this scale.

### 5.12 Notes, handouts, summons

- Notes and handouts are ACL-governed entities and come almost free once entities, ACL and `view_of` exist.
- Summons are created by **copying** a template (e.g. a monster reference) into a new entity owned by the summoner and linked via `parent`. Copy rather than reference, since the player or DM will tweak it. An optional ephemeral flag makes dismissal one action.
- Treat player-authored markdown as opaque text on the backend; cap its size. Rendering must sanitize (see §8).

---

## 6. Future-proofing (design now, build later)

| Future feature | What to keep in mind now |
|---|---|
| **PDF export/import** | Stored fields mirror paper sheet boxes with stable, documented keys; free-text boxes exist. Export reads display values (`override ?? computed`). Import is best-effort: unmapped data goes to notes/`ext` and the result lands in a "needs review" state. Implement as `SheetExporter`/`SheetImporter` adapters with data-driven field maps per sheet template. Do not bundle official WotC PDFs; users supply their own. Generating our own layout via HTML print CSS is the lowest-risk first export. |
| **Other frontends** | The server API is frontend-agnostic: REST + WebSocket + generated OpenAPI/JSON Schema, plus generated TypeScript types. A first-party web UI is embedded later. A CLI or TUI client for the DM is conceivable. Keep payloads light for phones. |
| **Plugins / scripting** | Extract `core::types` into its own crate. Use a namespaced `ext: Map<Namespace, Json>` on campaign, entity, item and feature types for plugin data. Reserve (do not build) a custom-formula extension point. Content packs (data only) come first; code plugins (e.g. WASM, or an embedded scripting language) are a much later option. Stable IDs and versioned serialization are the preconditions. |
| **Level-up and multiclass helper** | Possible only because levels, classes and choices are stored as base data and everything else is derived. Implement as a ruleset-data-driven wizard that produces ordinary base-data edits. Not a core feature. |
| **2024 edition** | Ruleset ID on every entity, ruleset data bundles, minimal hooks interface. Review the actual rule differences before adding a second ruleset. |
| **Combat mode** | A layer driving `advance_clock(6s)` per round plus initiative and action tokens. Must not require status model changes. A plain DM-owned ordered initiative list with no automation is a cheap middle path. |
| **Expected damage / hit rate** | Roll AST, individual d20 results, crit range and `RollContext` are stored from the start. |
| **Companion derivation** | `parent` link plus an optional cross-entity formula source. Deferred. |
| **Event log / undo** | Keep writes as discrete commands so an event log can be added without rewriting call sites. Not needed for v1. |
| **Calendars, co-DM** | Clock display formatting and the role model are the extension points. |

---

## 7. Testing strategy

- **Golden characters:** compare computed stats against known-correct published example sheets. The rules engine is where subtle bugs will concentrate.
- **Property tests** (e.g. `proptest`): clock advance/rewind consistency, "exactly one concentration status after `begin_concentration`", dice results within bounds, temp HP never stacking.
- **Authorization matrix tests** for `view_of` and WebSocket fan-out. Write these before any networking exists.
- **Migration tests** against committed fixture campaign files from every schema version.
- **Serde round-trip and snapshot tests** for `core::types`, since they are a public contract.
- **Hexagonal payoff:** engine tests run against in-memory port implementations and a deterministic `Rng`. Adapter contract tests run the same suite against the in-memory and SQLite stores.
- **Dice distribution tests:** sanity checks on uniformity.

---

## 8. Rough edges and risks (consolidated)

1. **Egress leaks** (REST and WebSocket) are the top security/privacy risk. Mitigated by the single `view_of` path and a test matrix.
2. **Stale overrides:** an override can hide a better computed value. Mitigate by always exposing both.
3. **Edition drift:** 2014 and 2024 differ in a few engine-relevant places. Unverified for 2024 here; verify before adding it.
4. **Multiclass spellcasting, Pact Magic, scaling feature uses** are fiddly rules areas.
5. **Turn-relative effects** cannot be represented exactly without initiative; approximated as rounds.
6. **Concentration parsing:** spell duration data is partly free text; manual fallback required.
7. **Stored XSS across players:** the DM views player-authored markdown (notes, item text). Backend treats it as opaque and size-capped; any frontend MUST sanitize on render.
8. **Plain HTTP on LAN:** tokens are sniffable on hostile networks. Document it; recommend VPN for remote play.
9. **Secret rolls are social:** the DM can read the campaign file.
10. **SRD licensing/attribution** and dataset field accuracy must be verified for whichever dataset is used.
11. **SQLite and copying:** copying a live WAL-mode database file can produce an inconsistent copy; use the backup API.
12. **Reference portability:** a campaign file moved to a machine without the same packs degrades to name snapshots.
13. **Index-addressed lists** break under concurrent edits; every list element needs a stable ID from the start.
14. **Schema evolution** is the long-term cost center. Version everything now.
15. **Monster-specific rules** (legendary actions, recharge-by-roll, lair actions) do not fit the character-oriented resource model perfectly. Start with manual resources.

---

## 9. Open questions and decision log

### 9.1 Decided

| # | Decision |
|---|---|
| D1 | Rust workspace, hexagonal architecture (`localtt-core`, `-engine`, `-adapter`, `-server`). |
| D2 | 2014 edition is a hard assumption for now; core stays edition-agnostic where cheap. |
| D3 | One campaign per server run; minimal scope (statblocks, notes, items, QoL). |
| D4 | DM hosts a local server; players join via browser; invite-slot identity model. |
| D5 | One SQLite file per campaign, JSON documents for entity bodies. |
| D6 | Computed values are never authoritative; players can override any derived value; engine warns, never blocks. |
| D7 | Multiple entities per player; unified entity + ACL model. |
| D8 | Status bar with custom statuses; concentration modeled as a status that replaces on new concentration. |
| D9 | Equip state tracked per inventory entry; no action-economy modeling. |
| D10 | DM-controlled campaign clock; rounds derived (6s); no combat mode for now. |
| D11 | Dice with audience control built early; feed generalizes the roll log. |
| D12 | PDF import/export, level-up helper, scripting are later. |

### 9.2 Deferred implementation details (maintainer: "late details")

| # | Question | Recommended default |
|---|---|---|
| L1 | Do rests auto-advance the clock? | Prompt the DM with 1h (short) / 8h (long); skippable. |
| L2 | Soft or hard expiry of statuses? | Soft: hidden from active bar, recoverable until dismissed or next long rest. |

### 9.3 Open

1. **Crate layout:** ports live in `engine` (closed 2026-10-17; decision ENG-1) and the binary is `server`'s `[[bin]]` (closed; decision ENG-5). Does `adapter` split by concern? Does `types` get its own crate now or later?
2. **Reference vendoring:** are used pack entries copied into the campaign file for portability, or kept external with name snapshots only?
3. **DM identity persistence:** hashed DM key stored in the campaign file, regenerated per run, or loopback-is-DM?
4. **Default roll audience for players:** `RollerAndDm` (recommended) vs `Public`.
5. **Handouts:** is "give" (copy) the default action, with read-only sharing explicit?
6. **Player-created entities:** can the DM restrict or approve?
7. **Co-DM / spectator roles:** needed? (Currently no.)
8. **Entity ruleset vs campaign ruleset:** is mixing allowed? (Default: campaign default, entity field present.)
9. **Override display:** show computed beside override always, or only on expand? (Frontend concern, but affects API shape: the engine should always return both.)
10. **HP max:** how much to compute vs. input (rolled vs average hit points)?
11. **Initiative:** is a plain, non-automated initiative list ever wanted?

---

## 10. Technology options (guidance for iteration, not decisions)

Verify current maintenance status and license of every candidate before adopting. Lean entries reflect the discussion and are not binding.

| Concern | Candidates | Notes / lean |
|---|---|---|
| Async runtime | `tokio` | Default. |
| HTTP + WebSocket | `axum`; `actix-web`; `poem` | Lean `axum` (tower ecosystem, WS built in, good OpenAPI crates). |
| Persistence | `rusqlite` (sync, simple); `sqlx` (async, compile-time-checked queries); `sea-orm`/`diesel` | Lean `rusqlite` behind a single-writer thread, since the command model is already serialized. Bundled SQLite feature so users need no system library. |
| Migrations | hand-rolled JSON migrations plus `rusqlite_migration` or similar for DDL | Document-level migrations are pure functions in the engine/adapter. |
| Serialization | `serde`, `serde_json` | Decide enum tagging representations deliberately, since they are part of the public contract. |
| Schema/OpenAPI/TS types | `schemars` (JSON Schema); `utoipa` or `aide` (OpenAPI); `ts-rs` or `specta` (TypeScript types) | Generate from `core::types` so frontends are never hand-synced. |
| IDs | `uuid` (v7), `ulid` | Time-sortable IDs are convenient for feeds. |
| Tokens / hashing | `rand`/`getrandom`, `sha2` or `blake3` | Tokens are high-entropy random, so a fast hash is fine; no password hashing needed. |
| RNG | `rand` with `rand_chacha`, OS-seeded | Behind the `Rng` port; seedable for tests. |
| Dice parsing | hand-rolled recursive descent; `winnow`, `nom`, `chumsky`; existing dice-notation crates | Structured AST and audience requirements likely favor hand-rolled or a parser-combinator library. Evaluate existing crates but do not let them dictate the result type. |
| Reference search | SQLite FTS5; `tantivy` | FTS5 is enough at SRD scale. |
| Content packs | `toml`, `serde_json`, a frontmatter parser, `pulldown-cmark`/`comrak` if server-side Markdown handling is ever needed | Sanitization is a frontend concern, see §8. |
| Embedding the UI | `rust-embed`, `include_dir` | Single binary for the DM. |
| CLI / config | `clap` | Flags: campaign path, `--lan`, port, backup options. |
| QR / discovery | `qrcode`; `mdns-sd` | QR for join links; mDNS optional. |
| Observability / errors | `tracing`, `thiserror` (libraries), `anyhow` (binary) | |
| Testing | `proptest`, `insta` (snapshots), `cargo-nextest` | See §7. |
| PDF (later) | `lopdf` (AcroForm fill/read); `printpdf`; HTML print CSS via the browser | Prefer print CSS first; AcroForm only with user-supplied sheets. |
| Plugins (much later) | WASM (`wasmtime`, `extism`); scripting (`rhai`, `mlua`) | Content packs first; code plugins only after types are stable. |
| Frontend (later) | Out of scope | Anything with a WebSocket client; phone-first layout. |

Alternative stack for completeness: Python (FastAPI + Pydantic + SQLite) would work if collaborators demand it, but distribution to non-technical DMs is harder. The OpenAPI/JSON Schema contract keeps that door ajar.

---

## 11. Suggested implementation ordering (guidance)

Each step should be useful and testable on its own, using in-memory adapters until the real ones exist.

0. **Workspace hygiene:** CI, lints, `justfile` targets, fixtures directory, ADR/decision notes.
1. **`core::types`:** entities, statblock, resources, statuses (including concentration), clock, ACL, feed and roll types, `RefId`, schema versions. Serde round-trip and snapshot tests.
2. **`engine::rules`:** 2014 data bundle, modifier pipeline, formulas, derived values with breakdowns and warnings, overrides. Golden character tests.
3. **`engine::commands`:** resources, HP/temp HP, rests, equip, statuses, concentration, clock, field edits. Property tests.
4. **`engine::dice` and feed:** parser, AST, roller, audiences, ring buffer semantics.
5. **`engine::authz`:** ACL and `view_of` across entities and feed, with the full test matrix. Before any networking.
6. **`adapter`: SQLite store:** schema, migrations, backups, JSON export/import, contract tests against the in-memory store.
7. **`adapter`: reference data:** SRD 2014 import, ID scheme, search, content-pack loader, attribution.
8. **`server`:** REST + commands, WebSocket with per-recipient fan-out, invite/join flow, DM key, startup banner with QR, OpenAPI and TS type generation.
9. **Notes, handouts, summons** and remaining polish.
10. **Later:** PDF export then import, first-party frontend, level-up helper, expected-damage tooling, plugin story, companion derivation, 2024 ruleset.

---

## 12. Glossary

- **Entity:** any owned, ACL-governed record (PC, companion, summon, familiar, NPC, monster, note, handout).
- **Override:** a sparse, player-set value that wins over a computed one.
- **Derived value:** computed from base data and modifiers; never persisted.
- **Modifier:** a data-defined adjustment to a stat, with an optional condition.
- **Resource:** anything with a maximum, a current value and a recharge type.
- **Status:** a tracked effect on an entity (condition, effect, concentration, custom) with a duration.
- **Audience:** who may see a roll or feed entry.
- **Feed:** the persisted ring buffer of rolls and system messages.
- **Clock:** the DM-controlled campaign time in seconds; drives status expiry.
- **Slot:** a player identity bound to a join token.
- **`view_of`:** the single function producing viewer-specific representations.
- **Ruleset:** an edition's data bundle plus hooks (`5e-2014` first).
