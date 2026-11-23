//! Construction and parse errors for the validated types.

use thiserror::Error;

// Intra-doc link targets only.
#[cfg(doc)]
use crate::types::{Ability, ChallengeRating, Namespace, RefId, RulesetId, StatKey, TokenHash};
use crate::types::{BodyKind, EntityKind};

/// Failure building or deserializing an entity.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EntityError {
    /// `kind` and body belong to different families.
    #[error("kind {kind:?} cannot carry a {body_kind:?} body")]
    KindBodyMismatch {
        /// The entity kind.
        kind: EntityKind,
        /// The body family of the payload.
        body_kind: BodyKind,
    },
}

/// Failure parsing a [`RefId`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum RefIdError {
    /// The string is not a `<source>:<kind>/<slug>` reference.
    #[error("invalid reference id")]
    Invalid,
}

/// Failure parsing a [`RulesetId`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum RulesetIdError {
    /// The string is not a valid ruleset id.
    #[error("invalid ruleset id")]
    Invalid,
}

/// Failure parsing a [`Namespace`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum NamespaceError {
    /// The string is not a valid namespace.
    #[error("invalid namespace")]
    Invalid,
}

/// Failure parsing a [`StatKey`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum StatKeyError {
    /// The string is neither a canonical known stat nor a valid custom key.
    #[error("invalid stat key")]
    Invalid,
}

/// Failure parsing a [`ChallengeRating`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ChallengeRatingError {
    /// The string is not a challenge rating.
    #[error("invalid challenge rating")]
    Invalid,
}

/// Failure parsing a [`TokenHash`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum TokenHashError {
    /// The string is not a non-empty lowercase-hex digest.
    #[error("invalid token hash")]
    Invalid,
}

/// Failure parsing an [`Ability`] abbreviation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("unknown ability abbreviation")]
pub struct AbilityError;
