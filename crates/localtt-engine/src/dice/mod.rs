//! Dice notation and the roller (RFC `dice.md`): parsing, cap validation,
//! composition with U4's derived roll parts, and infallible rolls.

mod lexer;
mod parser;
mod roller;
mod validate;

use localtt_core::types::{DiceExpr, DiceResult};

use crate::{error::EngineError, ports::rng::Rng, rules::RollParts};

/// Parses dice notation and validates the result against the caps.
///
/// Errors with `Malformed` for a syntax error or a below-range literal, and
/// with `TooLarge` for an exceeded cap.
#[inline]
pub fn parse(notation: &str) -> Result<DiceExpr, EngineError> {
    todo!()
}

/// Validates an arbitrary AST (a wire payload that never went through
/// `parse`) against the caps: count, sides, keep count, depth and terms.
#[inline]
pub fn validate(expr: &DiceExpr) -> Result<(), EngineError> {
    todo!()
}

/// Composes one roll from a base expression and derived roll parts (U4,
/// section 5.8): wraps the base in the hint, adds the flat part when it is
/// non-zero, then appends one attributed term per `DiceTerm`.
#[must_use]
#[inline]
pub fn compose(base: DiceExpr, parts: &RollParts) -> RollPlan {
    todo!()
}

/// Rolls a plan. Infallible: `parse`/`validate` guarantee term limits, and
/// all arithmetic saturates.
#[inline]
pub fn roll(plan: &RollPlan, rng: &mut dyn Rng) -> DiceResult {
    todo!()
}

/// One composed roll: the attributed terms and the expression they fold
/// into. Exactly one `TermResult` per `TermPlan` comes back from `roll`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollPlan {
    /// The expression actually rolled: a left fold of `Add` over `terms`.
    pub expression: DiceExpr,
    /// The terms, in roll order.
    pub terms: Vec<TermPlan>,
}

/// One planned term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermPlan {
    /// The term expression.
    pub expr: DiceExpr,
    /// Attribution label for the composed term, e.g. `"Bless"`; `None` for
    /// the base and the flat part.
    pub source: Option<String>,
}
