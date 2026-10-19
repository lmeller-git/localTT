//! Dice notation and the roller (RFC `dice.md`): parsing, cap validation,
//! composition with U4's derived roll parts, and infallible rolls.

mod lexer;
mod parser;
mod roller;
mod validate;

use localtt_core::types::{DiceExpr, DiceResult};

use crate::{
    error::EngineError,
    ports::rng::Rng,
    rules::{RollHint, RollParts},
};

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
/// `parse`) against the caps: count, sides, keep count, depth and terms. The
/// command seam validates a composed plan's `expression` with it, so derived
/// dice meet the same caps as client input.
#[inline]
pub fn validate(expr: &DiceExpr) -> Result<(), EngineError> {
    validate::validate(expr)
}

/// Composes one roll from a base expression and derived roll parts (U4,
/// section 5.8): wraps the base in the hint, adds the flat part when it is
/// non-zero, then appends one attributed term per `DiceTerm`.
///
/// Composition does not inspect the expressions it folds: the caller owes
/// [`validate`] on the base and on every `DiceTerm.expr` before the plan is
/// rolled (validating the composed `expression` covers both, since every
/// term is a subexpression of the fold).
#[must_use]
#[inline]
pub fn compose(base: DiceExpr, parts: &RollParts) -> RollPlan {
    let base = match parts.hint {
        RollHint::Normal => base,
        RollHint::Advantage => DiceExpr::Advantage {
            inner: Box::new(base),
        },
        RollHint::Disadvantage => DiceExpr::Disadvantage {
            inner: Box::new(base),
        },
    };
    let mut terms = Vec::with_capacity(parts.dice.len() + 2);
    terms.push(TermPlan {
        expr: base.clone(),
        source: None,
    });
    if parts.flat != 0 {
        terms.push(TermPlan {
            expr: DiceExpr::Const { value: parts.flat },
            source: None,
        });
    }
    for term in &parts.dice {
        terms.push(TermPlan {
            expr: term.expr.clone(),
            source: term.source.clone(),
        });
    }
    let expression = terms.iter().skip(1).fold(base, |lhs, term| DiceExpr::Add {
        lhs: Box::new(lhs),
        rhs: Box::new(term.expr.clone()),
    });
    RollPlan { expression, terms }
}

/// Rolls a plan. Infallible for a validated plan: `parse`/`validate`
/// guarantee term limits, and all arithmetic saturates. A plan composed from
/// unchecked parts is a caller bug; the command seam validates it first.
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
    use crate::rules::DiceTerm;

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

    #[test]
    fn compose_wraps_the_hint_and_folds_the_terms() {
        let d20 = || DiceExpr::Roll {
            count: 1,
            sides: 20,
            keep: None,
        };
        let d4 = DiceExpr::Roll {
            count: 1,
            sides: 4,
            keep: None,
        };

        // A plain check with no parts is one base term.
        let parts = RollParts {
            flat: 0,
            dice: Vec::new(),
            hint: RollHint::Normal,
        };
        assert_eq!(
            compose(d20(), &parts),
            RollPlan {
                expression: d20(),
                terms: vec![TermPlan {
                    expr: d20(),
                    source: None,
                }],
            }
        );

        // The hint wraps the base; the flat part and the attributed dice
        // follow in order; the expression is the left fold of `Add`.
        let parts = RollParts {
            flat: 4,
            dice: vec![DiceTerm {
                source: Some("Bless".to_owned()),
                expr: d4.clone(),
            }],
            hint: RollHint::Advantage,
        };
        let advantaged = DiceExpr::Advantage {
            inner: Box::new(d20()),
        };
        let plan = compose(d20(), &parts);
        assert_eq!(
            plan.terms,
            vec![
                TermPlan {
                    expr: advantaged.clone(),
                    source: None,
                },
                TermPlan {
                    expr: DiceExpr::Const { value: 4 },
                    source: None,
                },
                TermPlan {
                    expr: d4.clone(),
                    source: Some("Bless".to_owned()),
                },
            ]
        );
        assert_eq!(
            plan.expression,
            DiceExpr::Add {
                lhs: Box::new(DiceExpr::Add {
                    lhs: Box::new(advantaged),
                    rhs: Box::new(DiceExpr::Const { value: 4 }),
                }),
                rhs: Box::new(d4),
            }
        );
    }
}
