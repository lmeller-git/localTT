//! The command vocabulary: the [`Command`] enum and its supporting value types.

use localtt_core::types::{
    Ability,
    AnyEntity,
    Audience,
    CampaignTime,
    Coins,
    DeathSaves,
    DiceExpr,
    EntityId,
    Formula,
    InventoryEntryId,
    InventoryItem,
    ItemState,
    Modifier,
    PlayerId,
    Recharge,
    RefId,
    RefOrText,
    ResourceId,
    ResourceKind,
    SpellEntryId,
    SpellcastingBlockId,
    StatKey,
    StatusDuration,
    StatusId,
    StatusKind,
    StatusSource,
    TokenHash,
};

/// The largest number of player seats the engine accepts.
///
/// DM seats do not count against this cap.
pub const MAX_PLAYERS: usize = 64;

/// The longest display name, in characters after trimming.
pub const MAX_DISPLAY_NAME: usize = 80;

/// The command surface: U3's clock, statuses, hp, resources and rests, plus
/// U5's dice, casting and hit-die commands. Later units add variants
/// (inventory, entities, seats).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Moves the clock by `by_seconds`; a DM command.
    AdvanceClock {
        /// The signed move in campaign seconds.
        by_seconds: i64,
    },
    /// Sets the clock to an absolute instant; a DM command.
    SetClock {
        /// The new campaign instant; pre-origin times are legal.
        to: CampaignTime,
    },
    /// Adds a participant to the initiative order, or replaces its value.
    ///
    /// The DM may target any statblock and adds a missing entry. A player may
    /// only update a queued statblock they own, so participants are still
    /// chosen by the DM. The entry is kept in descending-value order; ties
    /// keep their previous relative order.
    SetInitiative {
        /// The participating entity.
        entity: EntityId,
        /// The rolled or entered initiative value.
        value: i32,
    },
    /// Removes one participant from the initiative order; a DM command.
    RemoveInitiative {
        /// The participant to remove.
        entity: EntityId,
    },
    /// Empties the initiative order and clears the current turn and round; a
    /// DM command.
    ClearInitiative,
    /// Advances to the next participant; a DM command.
    ///
    /// Starting an encounter selects the first participant without moving the
    /// clock. Moving to a later participant also leaves the clock alone;
    /// wrapping back to the first one moves it by
    /// [`SECONDS_PER_ROUND`](localtt_core::types::SECONDS_PER_ROUND) and
    /// increments the round.
    AdvanceInitiative,
    /// Makes one participant the current turn without moving the clock; a DM
    /// command. Starting an encounter sets the round to one.
    SetInitiativeTurn {
        /// The participant whose turn it becomes.
        entity: EntityId,
    },
    /// Creates one entity from a client-supplied document.
    ///
    /// The command layer rejects a nonzero revision, stamps revision 1 and
    /// both timestamps from the environment, and sets the schema version to
    /// the current build. A player may create only their own entities and may
    /// set no ACL grants; the DM may create for any roster member and set
    /// grants. The entity id must be fresh.
    CreateEntity {
        /// The draft document; its header fields are validated and stamped.
        entity: Box<AnyEntity>,
    },
    /// Adds one inventory entry to a statblock.
    ///
    /// The command layer mints the entry id and stamps the revision. `qty`
    /// must not be negative; a zero quantity is stored and derivation warns.
    AddInventoryItem {
        /// The target statblock.
        entity: EntityId,
        /// The item definition or reference.
        item: Box<InventoryItem>,
        /// Quantity; must not be negative.
        qty: i32,
        /// Whether the entry starts carried, equipped or stowed.
        state: ItemState,
        /// Whether the entry starts attuned.
        attuned: bool,
        /// Free-text notes.
        notes: String,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Edits one inventory entry; `None` leaves a field unchanged.
    EditInventoryItem {
        /// The target statblock.
        entity: EntityId,
        /// The entry to edit.
        entry: InventoryEntryId,
        /// New quantity, when set; must not be negative.
        qty: Option<i32>,
        /// New carried/equipped/stowed state, when set.
        state: Option<ItemState>,
        /// New attunement flag, when set.
        attuned: Option<bool>,
        /// New notes, when set.
        notes: Option<String>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Removes one inventory entry.
    RemoveInventoryItem {
        /// The target statblock.
        entity: EntityId,
        /// The entry to remove.
        entry: InventoryEntryId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Copies a handout's item into a recipient's inventory; a DM command.
    ///
    /// The handout document is left untouched, so every call adds another
    /// copy. A text handout has no inventory effect and is `Malformed`.
    GiveHandout {
        /// The handout to give.
        handout: EntityId,
        /// The statblock receiving the copy.
        recipient: EntityId,
        /// Optional compare-and-swap on the recipient's revision.
        expected_rev: Option<u64>,
    },
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
    /// Adds a status; the command layer mints its id and stamps
    /// `started_at` from the campaign clock.
    AddStatus {
        /// The target entity.
        entity: EntityId,
        /// The client-supplied part of the new status.
        status: StatusSpec,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Removes a status from an entity.
    RemoveStatus {
        /// The target entity.
        entity: EntityId,
        /// The status to remove.
        status: StatusId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Replaces a status's duration.
    EditStatusDuration {
        /// The target entity.
        entity: EntityId,
        /// The status to edit.
        status: StatusId,
        /// The new duration.
        duration: StatusDuration,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Applies damage: temp HP absorbs first, then current.
    ApplyDamage {
        /// The target entity.
        entity: EntityId,
        /// The damage amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Heals: current HP rises, never above max; temp HP is untouched.
    Heal {
        /// The target entity.
        entity: EntityId,
        /// The healing amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Sets temp HP, non-stacking (keeps the higher value).
    SetTempHp {
        /// The target entity.
        entity: EntityId,
        /// The temp HP amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Replaces an entity's death-save tally.
    SetDeathSaves {
        /// The target entity.
        entity: EntityId,
        /// The new tally; fields above 3 are `Malformed`.
        value: DeathSaves,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Spends a tracked resource; overspending is allowed and warned.
    SpendResource {
        /// The target entity.
        entity: EntityId,
        /// The resource to spend.
        resource: ResourceId,
        /// The amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Restores a tracked resource; clamped when the maximum is a constant.
    RestoreResource {
        /// The target entity.
        entity: EntityId,
        /// The resource to restore.
        resource: ResourceId,
        /// The amount; negative amounts are `Malformed`.
        amount: i32,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Adds one tracked resource to a statblock.
    ///
    /// The command layer mints the resource id. A spell slot's level and a hit
    /// die's sides are validated here.
    AddResource {
        /// The target statblock.
        entity: EntityId,
        /// Display name; trimmed and non-empty.
        name: String,
        /// Resource kind.
        kind: ResourceKind,
        /// Maximum; a constant must be non-negative.
        max: Formula,
        /// Starting value; defaults to the constant maximum, or zero for an
        /// expression maximum.
        current: Option<i32>,
        /// Recharge trigger.
        recharge: Recharge,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Edits one tracked resource; the kind is immutable.
    EditResource {
        /// The target statblock.
        entity: EntityId,
        /// The resource to edit.
        resource: ResourceId,
        /// New display name, when set.
        name: Option<String>,
        /// New maximum, when set; a constant must be non-negative.
        max: Option<Formula>,
        /// New current value, when set.
        current: Option<i32>,
        /// New recharge trigger, when set.
        recharge: Option<Recharge>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Removes one tracked resource.
    RemoveResource {
        /// The target statblock.
        entity: EntityId,
        /// The resource to remove.
        resource: ResourceId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// A short rest: refills `Recharge::Short` resources and clears
    /// `UntilRest { rest: Short }` statuses.
    ShortRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are `Malformed`.
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// A long rest: short plus `Recharge::Long` refills, `UntilRest { rest:
    /// Long }` clears, and the long-rest recovery (hit points, death
    /// saves, one exhaustion level).
    LongRest {
        /// The target entity.
        entity: EntityId,
        /// Optional clock advance; negative values are `Malformed`.
        advance_seconds: Option<i64>,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Rolls dice; the actor comes from [`Executor::execute_as`].
    ///
    /// [`Executor::execute_as`]: crate::executor::Executor::execute_as
    Roll {
        /// The entity the roll is for; may be absent for a free expression.
        entity: Option<EntityId>,
        /// What to roll.
        spec: RollSpec,
        /// Audience override; `None` means the campaign default.
        audience: Option<Audience>,
    },
    /// Casts a known spell from a tracked slot.
    CastSpell {
        /// The caster.
        entity: EntityId,
        /// The known spell entry inside one of the entity's blocks.
        spell: SpellEntryId,
        /// The slot resource to spend.
        slot: ResourceId,
        /// The declared upcast in slot levels above the spell's level.
        upcast_slots: u32,
        /// The client's concentration request; the resolved spell may
        /// concentrate on its own.
        concentrate: bool,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Adds one spell entry to a spellcasting block.
    ///
    /// The command layer mints the spell id, and the block id when the
    /// placement asks for a new block.
    AddSpell {
        /// The target statblock.
        entity: EntityId,
        /// Where the spell goes.
        placement: SpellPlacement,
        /// The client-supplied spell fields.
        spell: NewSpell,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Replaces one spell entry's client-editable fields.
    ///
    /// This is a full replacement: an absent reference or level clears the
    /// stored snapshot.
    EditSpell {
        /// The target statblock.
        entity: EntityId,
        /// The spell entry to edit.
        spell: SpellEntryId,
        /// The new name snapshot; trimmed and non-empty.
        name: String,
        /// The new content reference, or `None` to clear it.
        reference: Option<RefId>,
        /// The new level snapshot; `None` means unknown.
        level: Option<u8>,
        /// Whether the spell is prepared.
        prepared: bool,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Removes one spell entry from its block.
    RemoveSpell {
        /// The target statblock.
        entity: EntityId,
        /// The spell entry to remove.
        spell: SpellEntryId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Starts (or replaces) concentration from a client-supplied status.
    BeginConcentration {
        /// The concentrating entity.
        entity: EntityId,
        /// The new status; `kind` must be `Concentration`.
        status: StatusSpec,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Ends every concentration status on the entity.
    EndConcentration {
        /// The entity.
        entity: EntityId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Spends one hit die and rolls its healing; the actor comes from
    /// [`Executor::execute_as`].
    ///
    /// [`Executor::execute_as`]: crate::executor::Executor::execute_as
    SpendHitDie {
        /// The creature spending the die.
        entity: EntityId,
        /// The `ResourceKind::HitDie` resource to spend.
        resource: ResourceId,
        /// Optional compare-and-swap on the entity's revision.
        expected_rev: Option<u64>,
    },
    /// Adds a player seat to the campaign; a DM command.
    ///
    /// The id must be fresh. The display name is trimmed and bounded, the role
    /// is always the player role, and `created_at` comes from the environment.
    /// A token hash may be supplied when the seat is created with an invite.
    AddPlayer {
        /// The new seat id.
        id: PlayerId,
        /// The display name; trimmed and bounded.
        display_name: String,
        /// The invite hash, when the seat starts invited.
        token_hash: Option<TokenHash>,
    },
    /// Replaces or clears a player's invite hash; a DM command.
    ///
    /// The target must be a player seat. `None` revokes the invite.
    SetPlayerToken {
        /// The player seat.
        player: PlayerId,
        /// The new hash, or `None` to revoke.
        token_hash: Option<TokenHash>,
    },
    /// Replaces a DM seat's bearer-token hash; a DM command.
    ///
    /// The target must be a DM seat. The server uses this for the DM key and
    /// for `--rotate-dm-key`.
    RotateDmKey {
        /// The DM seat.
        player: PlayerId,
        /// The new key hash.
        token_hash: TokenHash,
    },
    /// Removes a player seat and transfers its entities; a DM command.
    ///
    /// The target must be a player seat. Every entity owned by the seat is
    /// reassigned to `reassign_to`, and ACL entries naming the seat are
    /// removed. DM seats are protected.
    RemovePlayer {
        /// The player seat to remove.
        player: PlayerId,
        /// The DM seat that receives the removed player's entities.
        reassign_to: PlayerId,
    },
}

/// What a `Roll` command rolls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RollSpec {
    /// Rolls a client-supplied expression.
    Expression {
        /// The expression; validated before rolling.
        expr: DiceExpr,
        /// The stat the roll is for; drives `RollContext.kind` and the
        /// context's `stat`, and has no mechanical effect on the dice.
        target: Option<StatKey>,
    },
    /// Rolls a check for one stat: the engine derives the flat part, the
    /// attributed dice and the hint, then rolls `1d20` composed with them.
    Check {
        /// The stat, e.g. `"save.wis"`.
        stat: StatKey,
        /// The DC, when the client compares one.
        target_dc: Option<i32>,
    },
}

/// One inventory entry, or part of one, moved by [`Command::TransferItems`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemMove {
    /// The source entry.
    pub entry: InventoryEntryId,
    /// How many units to move; positive and at most the stored quantity.
    pub qty: i32,
}

/// Where [`Command::AddSpell`] puts a new spell entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpellPlacement {
    /// Append to an existing spellcasting block.
    Existing(SpellcastingBlockId),
    /// Create a block and append the spell to it.
    New {
        /// Class or other source, referenced or inline.
        source: RefOrText,
        /// Spellcasting ability.
        ability: Ability,
    },
}

/// The client-supplied part of a new spell entry; the command layer mints the
/// id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSpell {
    /// Name snapshot; trimmed and non-empty.
    pub name: String,
    /// Content reference, when the spell comes from a source.
    pub reference: Option<RefId>,
    /// Level snapshot: 0 for a cantrip, 1 to 9, or `None` for unknown.
    pub level: Option<u8>,
    /// Whether the spell starts prepared.
    pub prepared: bool,
}

/// The client-supplied part of a new status; the command layer mints the id
/// and stamps `started_at` from the campaign clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSpec {
    /// The status kind; adding a `Concentration` status while one is already
    /// active warns `MultipleConcentration` (5.4) and appends the new status.
    /// Contrast [`begin_concentration`], which replaces the active
    /// concentration instead of appending.
    pub kind: StatusKind,
    /// Display name.
    pub name: String,
    /// Optional reference into the content catalogs.
    pub reference: Option<RefId>,
    /// Optional free text (markdown, opaque).
    pub text: Option<String>,
    /// How the status ends.
    pub duration: StatusDuration,
    /// Optional provenance of the status.
    pub source: Option<StatusSource>,
    /// Stat modifiers the status applies.
    pub modifiers: Vec<Modifier>,
}
