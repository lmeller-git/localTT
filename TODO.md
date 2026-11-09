# TODO

Consolidated backlog. Everything here is deferred work that is not already
implemented; the decision log explains why each area is open where a decision
exists (`docs/decisions.md`). Product intent and the remaining product questions
stay in `RFC-0001-localtt.md` section 9.3.

## Core data model

- `AnyEntity` / `InventoryItem::Inline` boxing; the large enum variants carry an
  `#[expect(clippy::large_enum_variant)]` until the public shape changes.
- `Entity::validate` split so a loader can name a kind/body mismatch as a typed
  error instead of a serde string.
- `#[non_exhaustive]` pass on payload enums before 1.0 or crate extraction.
- `weight_oz` as a pounds input sugar.

## Engine and rules

- Non-const `Resource::max` evaluation in the U3 rest/dawn refill paths, and the
  related clamp-behavior rewrite. U4 evaluates formulas in `rules`; the command
  paths still assume `Formula::Const` and need their own review.
- Attack lines (`attack.melee`, `attack.ranged`): weapon proficiency and
  property data. The keys are derivable today only when a contribution targets
  them.
- Half and third casters, subclasses, unarmored defense (barbarian/monk), and
  the rest of the class progression surface.
- Class progression and the full class data as a content/ruleset bundle
  replacing the four static tables.
- Cross-entity derivation (for example a companion using its PC's proficiency
  bonus). `parent` keeps the door open.
- Incremental or cached derivation; the current contract is a full recompute.
  Profile before designing the cache.
- `StaleOverride` telemetry, fly/swim/climb/burrow speed keys, and a
  `ClassLevel` warning element (needs a core `ElementRef` variant; the warning
  is element-less today).
- Recharge-by-roll resources and monster feature plumbing; the current model
  maps recharge to `Manual`.
- Surprise and action economy (the initiative order itself is implemented).
- Executor revision-assertion violation is unreachable through the public
  command surface; exercising it needs a test-only seam.

## Dice and table flow

- Attack lines, spell attacks and spell-damage automation; client-built
  expressions cover the table meanwhile.
- Content-driven spell data beyond level, school, concentration and ritual
  (upcast text, damage formulas); this is a versioned content-payload widening.
- Distribution telemetry, roll-history queries, and a per-campaign seeding
  policy beyond `from_seed`.

## Authorization, views and entity lifecycle

- ACL editing and sharing commands (`SetAcl`, owner changes, handout
  recipients): the write path for the access predicates that already exist.
- Derived values inside `EntityView`, a derived AC carrier (so `ViewField::Ac`
  has a mask target), and every computed stat exposed through views. The
  separate `services::derived_sheet` read already serves the full sheet to an
  unmasked reader. Adding derived values to views adds a resolved-content
  parameter to the view path.
- A header-only `list_entities` variant when payload size matters.
- Reverse feed paging and cursor polish for the UI.
- `export(viewer)` (DM-only) and player sheet export.
- Campaign-level CAS for clock and settings writes; needs a versioned
  `Campaign.rev` core proposal, a DATA entry and a migration.
- Campaign settings writes and their range validation.
- Entity delete commands and field-edit commands beyond the current narrow
  inventory/handout set.
- `Audience` as a set ("DM reveals to one player"); needs a feed-document
  migration.
- `StatusActive` by reference in addition to name.
- Finer `ViewField` granularity, per-type `ext`, and the `ViewField::ALL` set as
  a versioned core proposal (each needs a DATA entry, minor version, migration
  and fixture policy).

## Content and reference data

- `FreezeReference` and the rest of the packs loader. `FreezeReference` also
  settles the `srd2014:background/sage` fixture finding (the golden cast names a
  background SRD 5.1 does not contain; edit to Acolyte or freeze as text).
- Remote backends and remote search: the async backend dispatcher, a live REST
  backend, and search/autocomplete fan-out. The local blocking `ReferenceSource`
  stays unchanged.
- Skip-and-diagnose search over unreadable bodies, together with the operator
  diagnostics story.
- Description-search bench: a 10,000-entry synthetic fixture and the p99 <= 10 ms
  budget (`CONTENT-5`).
- Body caching in `IndexedSources`; add only when a profile asks for it.
- SHA-256 of the committed snapshot and report for integrity pinning.
- Non-ASCII slug folding (currently deterministic but lossy).
- Item property mapping and monster feature extraction from `ext["src.open5e"]`.
- Installed data packaging: how `crates/localtt-adapter/data/` ships with an
  installed binary. The runtime root itself already resolves through `--data` or
  the platform local data directory; `SHIPPED_DATA_ROOT` stays build-time.

## Persistence and storage

- `i64` bound on feed-append ids and entity `rev`: `MemoryStore` accepts a `u64`
  beyond `i64::MAX` where `SqliteStore` returns `Backend`. Close it with a shared
  `InvalidPatch` precondition or document the SQLite bound.
- `fixtures/campaigns/v1.0/`: the serialized party bundle plus its
  regenerate-and-compare test. It has no consumer until the first document
  migration lands, so the `STORE-2` "fixtures per version" clause is unhonoured.
- Backup file permissions: SQLite creates the destination `0644`, widening a
  deliberately private `0600` campaign; copy the source mode on Unix.
- Backup retention policy: count- or age-based; rotation is best-effort today.

## Server, identity and clients

- Browser coins panel and transfer form, and the TUI `coins` prompt. The engine
  and wire command already accept them.
- Bilateral trade offers, acceptance, expiry and notifications, including the
  browser transfer history view and the dedicated trading/notification RFC.
- A transfer feed payload and table announcement.
- Owner-callable handout copying.
- Transferring items into a shared chest or an NPC sheet as a first-class
  destination; a first-class party treasury or shared wallet.
- A non-DM explicit destination for a statblock they own, for both item and coin
  transfers.
- An explicit active-character setting on the roster, replacing the primary-PC
  heuristic.
- Cascading transfer commands: transfer an entity with its summons/familiars,
  owned items and coins.
- Currency exchange and change-making; a combined item-and-coin trade bundle.
- A DM `SetCoins`/`AdjustCoins` command for minting, destroying or correcting a
  purse; gem and art-object transfers stay `TransferItems`.
- Rate limiting or spam controls for repeated transfers, if play shows a need.
- Co-DM role changes and multiple live DM keys.
- Invite expiry, single-use invites, and the invite "claimed" and `last_seen`
  state.
- QR codes for join links; the copy/paste link is the M5 surface.
- TUI roster/invite UI; the browser is the invite surface today.
- TLS and reverse proxy.
- Hash algorithm tags; identity hashing is SHA-256 only.
- Player-created entity restriction or DM approval.
- Whether an entity ruleset may differ from the campaign ruleset.
- How much of `HP max` is computed versus input (rolled versus average hit
  points).
- Whether rests advance the clock by default (short 1h, long 8h, DM-confirmed);
  the optional clock advance exists but is not a default.

## Testing and tooling

- Browser E2E beyond the current journeys: accessibility and responsive layout.
- Extend the `just ui-shot` scenario set and its failure artifacts.
- Replace the Trunk inline-loader CSP allowance with a hash or nonce.
- Mutation testing and `insta` snapshot tests; neither recipe exists yet.
- `localtt-testkit`, extracted only when a second consumer appears.
- A fixed-string determinism pin for `party()` serialization, once the fixture
  bytes are considered frozen.
- Memoize `Party::entities()` if it ever lands in a loop.
