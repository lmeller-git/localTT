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
    let tokens = lexer::tokenize(notation)?;
    let expr = parser::parse(&tokens)?;
    validate::validate(&expr)?;
    Ok(expr)
}

/// Validates an arbitrary AST (a wire payload that never went through
/// `parse`) against the caps: count, sides, keep count, depth and terms.
#[inline]
pub fn validate(expr: &DiceExpr) -> Result<(), EngineError> {
    validate::validate(expr)
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
#[must_use]
#[inline]
pub fn roll(plan: &RollPlan, rng: &mut dyn Rng) -> DiceResult {
    roller::roll(plan, rng)
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

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// An `Rng` that returns scripted values in order and refuses impossible
    /// ones; the determinism tool for every dice test.
    pub(crate) struct ScriptedRng {
        script: VecDeque<u32>,
    }

    impl ScriptedRng {
        /// A source over the given draw sequence.
        pub(crate) fn new(script: &[u32]) -> Self {
            Self {
                script: script.iter().copied().collect(),
            }
        }
    }

    impl Rng for ScriptedRng {
        fn roll_die(&mut self, sides: u32) -> u32 {
            let value = self
                .script
                .pop_front()
                .unwrap_or_else(|| panic!("the script has no value left for a d{sides}"));
            assert!(
                (1..=sides).contains(&value),
                "the scripted value {value} is impossible on a d{sides}"
            );
            value
        }
    }

    #[test]
    fn parse_is_tokenize_build_and_validate() {
        assert_eq!(
            parse("1d20+3").expect("a literal expression"),
            DiceExpr::Add {
                lhs: Box::new(DiceExpr::Roll {
                    count: 1,
                    sides: 20,
                    keep: None,
                }),
                rhs: Box::new(DiceExpr::Const { value: 3 }),
            }
        );
        assert!(matches!(
            parse("101d6"),
            Err(EngineError::TooLarge {
                what: "dice count",
                limit: 100
            })
        ));
        assert!(matches!(
            parse("1d20kh2"),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(parse(""), Err(EngineError::Malformed { .. })));
    }

    #[test]
    fn validate_accepts_what_parse_produces() {
        for notation in [
            "1d20",
            "2d6+3",
            "4d6kh3+1d4-1",
            "adv(1d20)",
            "dis(adv(1d20+2))",
        ] {
            let expr = parse(notation).expect("a literal expression");
            assert!(validate(&expr).is_ok(), "`{notation}` validates");
        }
    }
}
