//! The engine's error and warning vocabulary (RFC `engine-executor.md` 4.1).
//!
//! [`EngineError`] is the failure type of a command or of the executor;
//! [`Warning`] is a non-blocking rule outcome that never fails a command
//! (RFC-0001 2.2).

use localtt_core::types::{EntityId, PlayerId, ResourceId, StatusId};
use thiserror::Error;

use crate::ports::store::StoreFault;

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
    /// A refill was skipped, or a restore left unclamped, because the maximum
    /// is a `Formula` only the rules unit (U4) can evaluate (OQ-2).
    FormulaUnevaluated,
}

/// One element inside an entity document, for scoped errors and warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ElementRef {
    /// A status inside the entity's `statuses`.
    Status(StatusId),
    /// A resource inside the entity's `resources`.
    Resource(ResourceId),
}
