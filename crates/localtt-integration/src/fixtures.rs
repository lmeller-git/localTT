//! Golden-character fixtures (RFC-0003 section 3).
//!
//! Every fixture is data only: it types in the base data the engine will later
//! derive from. Fixtures use deterministic ids and times (see the internal
//! `ids` module),
//! so their serialized bytes are reproducible.
#![expect(
    clippy::todo,
    unused_variables,
    reason = "skeleton phase: every body is `todo!()`; removed in the implement phase"
)]

use localtt_core::types::{
    AnyEntity,
    Campaign,
    HandoutEntity,
    NoteEntity,
    PlayerId,
    StatblockEntity,
};

/// A whole campaign with its cast; the unit this suite round-trips.
pub struct Party {
    /// The campaign document, including the player roster.
    pub campaign: Campaign,
    /// The DM slot id.
    pub dm: PlayerId,
    /// The player slot id who owns the character entities.
    pub player: PlayerId,
    /// Aria, the level-5 high-elf evoker.
    pub pc: StatblockEntity,
    /// Aria's wolf companion (`parent` = the PC).
    pub companion: StatblockEntity,
    /// Aria's owl familiar (`parent` = the PC).
    pub familiar: StatblockEntity,
    /// A DM-only session note.
    pub note: NoteEntity,
    /// An item handout.
    pub handout: HandoutEntity,
}

impl Party {
    /// Every entity as the runtime-tagged type storage speaks.
    #[must_use]
    #[inline]
    pub fn entities(&self) -> Vec<AnyEntity> {
        todo!()
    }
}

/// Aria: a level-5 high-elf evoker.
#[must_use]
#[inline]
pub fn golden_wizard(owner: PlayerId) -> StatblockEntity {
    todo!()
}

/// Borin: a level-3 champion fighter.
#[must_use]
#[inline]
pub fn golden_fighter(owner: PlayerId) -> StatblockEntity {
    todo!()
}

/// Lyra: a level-2 warlock.
#[must_use]
#[inline]
pub fn golden_warlock(owner: PlayerId) -> StatblockEntity {
    todo!()
}

/// Mira: a level-5 wizard 3 / cleric 2 multiclass character.
#[must_use]
#[inline]
pub fn golden_multiclass(owner: PlayerId) -> StatblockEntity {
    todo!()
}

/// A goblin: a CR 1/4 creature.
#[must_use]
#[inline]
pub fn goblin(owner: PlayerId) -> StatblockEntity {
    todo!()
}

/// The assembled party used by most tests.
#[must_use]
#[inline]
pub fn party() -> Party {
    todo!()
}
