//! The engine's error and warning vocabulary (RFC `engine-executor.md` 4.1).
//!
//! [`EngineError`] is the failure type of a command or of the executor;
//! [`Warning`] is a non-blocking rule outcome that never fails a command
//! (RFC-0001 2.2).

use localtt_core::types::{
    EntityId,
    FeatureEntryId,
    InventoryEntryId,
    PlayerId,
    ResourceId,
    SpellEntryId,
    StatusId,
};
use thiserror::Error;

use crate::{content::ContentFault, ports::store::StoreFault};

/// Failure of a command or of the executor.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The actor may not read/write the target entity or perform the command.
    /// No U3 producer: commands carry no actor yet (U6).
    #[error("player {actor} may not perform the command on {entity:?}")]
    Unauthorized {
        /// The player attempting the command.
        actor: PlayerId,
        /// The target entity, when the command addressed one.
        entity: Option<EntityId>,
    },
    /// No such entity, or no such element inside the entity.
    #[error("no entity {entity} or element {element:?}")]
    NotFound {
        /// The entity the command addressed.
        entity: EntityId,
        /// The missing element, when the command addressed one.
        element: Option<ElementRef>,
    },
    /// Optimistic concurrency mismatch (server: 409).
    #[error("entity {entity} is at revision {actual}, not {expected}")]
    Conflict {
        /// The entity whose revision did not match.
        entity: EntityId,
        /// The revision the client expected.
        expected: u64,
        /// The revision the entity actually has.
        actual: u64,
    },
    /// Request-level invalid input (server: 400).
    #[error("malformed command: {reason}")]
    Malformed {
        /// What made the command invalid.
        reason: String,
    },
    /// A dice expression exceeded configured caps (U5); declared for the
    /// taxonomy, no U3 producer.
    #[error("{what} exceeds the limit of {limit}")]
    TooLarge {
        /// The capped quantity, e.g. `"dice count"`.
        what: &'static str,
        /// The configured cap.
        limit: u32,
    },
    /// Persistence failure, transparent.
    #[error(transparent)]
    Store(#[from] StoreFault),
    /// The command environment could not provide a service it promises.
    #[error("the command environment is unavailable: {reason}")]
    Environment {
        /// Diagnostic detail.
        reason: String,
    },
    /// A content source failed while a command resolved references.
    #[error(transparent)]
    Content(ContentFault),
}

/// A non-blocking rule outcome (RFC-0001 2.2).
///
/// `message` is a short human-readable sentence (U7 may render it); `code` is
/// the machine-readable key clients switch on. The exact sentence is not
/// contractual. Warnings never block and never fail a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// The machine-readable warning kind.
    pub code: WarningCode,
    /// A short human-readable sentence.
    pub message: String,
    /// The affected entity, when the warning is entity-scoped.
    pub entity: Option<EntityId>,
    /// The affected element, when the warning is element-scoped.
    pub element: Option<ElementRef>,
}

/// Machine-readable warning kinds; a closed set with additive additions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WarningCode {
    /// A spend left the resource below zero.
    ResourceOverspent,
    /// Damage left hit points below zero.
    NegativeHp,
    /// A concentration status was added while one was already active.
    MultipleConcentration,
    /// A clock move crossed at least one dawn instant backwards.
    DawnRewind,
    /// A formula was unevaluable where it had to be evaluated: U3 skips a
    /// refill or leaves a restore unclamped; U4 degrades a resource maximum
    /// to `None` or contributes nothing for a modifier operand (OQ-2).
    FormulaUnevaluated,
    /// A dice leaf was flattened to its flat part (0) in a static context.
    DiceInStaticContext,
    /// A content reference did not resolve to a usable entry.
    UnresolvedReference,
    /// More than one applicable body armor, or more than one shield.
    OverArmored,
    /// More than three attuned inventory entries.
    OverAttunement,
    /// An equipped armor's strength requirement is unmet.
    StrengthRequirement,
    /// An inventory entry with non-positive quantity was skipped.
    ZeroQuantity,
    /// A `Condition::Custom` was not modeled and contributed nothing.
    UnmodeledCondition,
    /// A class level named a class the static tables do not model.
    UnmodeledClass,
    /// A cast asked for concentration on a resolved spell that does not
    /// concentrate.
    ConcentrationUnsupported,
}

/// One element inside an entity document, for scoped errors and warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ElementRef {
    /// A status inside the entity's `statuses`.
    Status(StatusId),
    /// A resource inside the entity's `resources`.
    Resource(ResourceId),
    /// One inventory entry.
    Item(InventoryEntryId),
    /// One feature entry.
    Feature(FeatureEntryId),
    /// One spell entry (U5's unresolved cast warnings).
    Spell(SpellEntryId),
}
