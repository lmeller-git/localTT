//! Item and entity transfer handlers (RFC `trading.md`).
//!
//! A transfer is one-way: the recipient gains the asset immediately and can
//! hand it back with the same commands. Item batches are atomic across the
//! source and destination, and entity ownership moves change the header only.

use localtt_core::types::{EntityId, PlayerId};

use crate::{
    commands::{CommandEnv, ItemMove, Transition},
    error::EngineError,
    state::WorkingSet,
};

/// The inputs of a `TransferItems` command, grouped to keep the handler small.
#[derive(Debug)]
pub(super) struct ItemTransfer {
    /// The statblock the items leave.
    pub(super) source: EntityId,
    /// The lines to move.
    pub(super) moves: Vec<ItemMove>,
    /// The player receiving the items.
    pub(super) recipient: PlayerId,
    /// The recipient's statblock, when the DM pinned one.
    pub(super) destination: Option<EntityId>,
    /// Optional compare-and-swap on the source's revision.
    pub(super) expected_source_rev: Option<u64>,
    /// Optional compare-and-swap on the destination's revision.
    pub(super) expected_destination_rev: Option<u64>,
}

/// The inputs of a `TransferEntity` command.
#[derive(Debug)]
pub(super) struct EntityTransfer {
    /// The entity whose owner changes.
    pub(super) entity: EntityId,
    /// The player who becomes the owner.
    pub(super) recipient: PlayerId,
    /// Optional compare-and-swap on the entity's revision.
    pub(super) expected_rev: Option<u64>,
}

/// Moves a batch of inventory lines to the recipient's statblock.
///
/// The whole batch lands on both sheets in one transition, or neither sheet
/// changes.
pub(super) fn transfer_items_command(
    _state: &dyn WorkingSet,
    _transfer: ItemTransfer,
    _actor: Option<PlayerId>,
    _env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    todo!()
}

/// Moves one entity's ownership to the recipient.
///
/// Only the owner header, revision and update stamp change.
pub(super) fn transfer_entity_command(
    _state: &dyn WorkingSet,
    _transfer: EntityTransfer,
    _actor: Option<PlayerId>,
    _env: &mut dyn CommandEnv,
) -> Result<Transition, EngineError> {
    todo!()
}
