//! Static `Formula` evaluation, tables-aware: constants, proficiency, level
//! and class-level lookups, ability modifiers over the final computed scores,
//! and saturating `i32` arithmetic (RFC `rules.md` 5.2 and section 6).

use std::collections::BTreeMap;

use localtt_core::types::{Ability, Formula, FormulaExpr};

/// The state one static evaluation reads: the final ability scores, the
/// proficiency bonus, the total level and the class name snapshots.
pub(super) struct EvalEnv<'a> {
    /// Final computed ability scores, after every contribution (`rules.md`
    /// 5.2).
    pub(super) abilities: &'a BTreeMap<Ability, i32>,
    /// The entity's proficiency bonus.
    pub(super) proficiency_bonus: i32,
    /// Total character level: the saturating sum of class levels.
    pub(super) level: i32,
    /// Class name snapshots with their levels, matched case-insensitively.
    pub(super) classes: &'a [(String, i32)],
}

/// A statically evaluated formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Evaluated {
    /// The integer value; dice leaves contribute their flat part (0).
    pub(super) value: i32,
    /// Whether a dice leaf was flattened (`rules.md` 5.2).
    pub(super) flattened_dice: bool,
}

impl Evaluated {
    /// An evaluation with no dice leaf flattened.
    fn flat(value: i32) -> Self {
        Self {
            value,
            flattened_dice: false,
        }
    }
}

/// Statically evaluates `formula` in `env`; `None` when it is unevaluable
/// (floor division by zero, or an ability the env does not know).
pub(super) fn eval(formula: &Formula, env: &EvalEnv<'_>) -> Option<Evaluated> {
    match formula {
        Formula::Const(value) => Some(Evaluated::flat(*value)),
        Formula::Expr(expr) => eval_expr(expr, env),
    }
}

/// Evaluates a tagged formula expression.
fn eval_expr(expr: &FormulaExpr, env: &EvalEnv<'_>) -> Option<Evaluated> {
    match expr {
        FormulaExpr::ProficiencyBonus => Some(Evaluated::flat(env.proficiency_bonus)),
        FormulaExpr::AbilityMod { ability } => {
            let score = *env.abilities.get(ability)?;
            Some(Evaluated::flat(ability_modifier(score)))
        }
        FormulaExpr::Level => Some(Evaluated::flat(env.level)),
        FormulaExpr::ClassLevel { class } => Some(Evaluated::flat(class_level(env.classes, class))),
        FormulaExpr::Add { lhs, rhs } => combine(lhs, rhs, env, i32::saturating_add),
        FormulaExpr::Sub { lhs, rhs } => combine(lhs, rhs, env, i32::saturating_sub),
        FormulaExpr::Mul { lhs, rhs } => combine(lhs, rhs, env, i32::saturating_mul),
        FormulaExpr::Min { lhs, rhs } => combine(lhs, rhs, env, i32::min),
        FormulaExpr::Max { lhs, rhs } => combine(lhs, rhs, env, i32::max),
        FormulaExpr::FloorDiv { lhs, rhs } => {
            let left = eval(lhs, env)?;
            let right = eval(rhs, env)?;
            Some(Evaluated {
                value: floor_div(left.value, right.value)?,
                flattened_dice: left.flattened_dice || right.flattened_dice,
            })
        }
        FormulaExpr::Dice { .. } => Some(Evaluated {
            value: 0,
            flattened_dice: true,
        }),
    }
}

/// Evaluates both operands and folds them with a saturating binary operation.
fn combine(
    lhs: &Formula,
    rhs: &Formula,
    env: &EvalEnv<'_>,
    op: fn(i32, i32) -> i32,
) -> Option<Evaluated> {
    let left = eval(lhs, env)?;
    let right = eval(rhs, env)?;
    Some(Evaluated {
        value: op(left.value, right.value),
        flattened_dice: left.flattened_dice || right.flattened_dice,
    })
}

/// The 5e ability modifier, `floor((score - 10) / 2)`.
pub(super) fn ability_modifier(score: i32) -> i32 {
    score.saturating_sub(10).div_euclid(2)
}

/// The saturating sum of the levels of every class named `class`,
/// case-insensitively.
fn class_level(classes: &[(String, i32)], class: &str) -> i32 {
    classes
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(class))
        .fold(0, |total, &(_, level)| total.saturating_add(level))
}

/// Floor division, saturating; `None` when `rhs` is zero.
fn floor_div(lhs: i32, rhs: i32) -> Option<i32> {
    if rhs == 0 {
        return None;
    }
    let quotient = lhs.saturating_div(rhs);
    // Overflow only when `lhs` is `i32::MIN` and `rhs` is `-1`, where the
    // mathematical remainder is zero.
    let remainder = lhs.checked_rem(rhs).unwrap_or(0);
    if remainder != 0 && ((remainder < 0) != (rhs < 0)) {
        Some(quotient.saturating_sub(1))
    } else {
        Some(quotient)
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::builders::dice_pool;

    use super::*;

    fn env<'a>(abilities: &'a BTreeMap<Ability, i32>, classes: &'a [(String, i32)]) -> EvalEnv<'a> {
        EvalEnv {
            abilities,
            proficiency_bonus: 3,
            level: 5,
            classes,
        }
    }

    fn intelligence(score: i32) -> BTreeMap<Ability, i32> {
        BTreeMap::from([(Ability::Intelligence, score)])
    }

    #[test]
    fn ability_modifiers_floor_the_final_score() {
        let classes = [];
        for (score, expected) in [(18, 4), (10, 0), (8, -1), (7, -2)] {
            let abilities = intelligence(score);
            let evaluated = eval(
                &Formula::ability_mod(Ability::Intelligence),
                &env(&abilities, &classes),
            )
            .expect("the score is known");
            assert_eq!(evaluated.value, expected, "score {score}");
            assert!(!evaluated.flattened_dice);
        }
    }

    #[test]
    fn arithmetic_saturates_and_floor_division_by_zero_is_unevaluable() {
        let abilities = BTreeMap::new();
        let env = env(&abilities, &[]);

        let product = Formula::Expr(FormulaExpr::Mul {
            lhs: Box::new(Formula::Const(i32::MAX)),
            rhs: Box::new(Formula::Const(2)),
        });
        assert_eq!(eval(&product, &env).expect("constant math").value, i32::MAX);

        let quotient = Formula::Expr(FormulaExpr::FloorDiv {
            lhs: Box::new(Formula::Const(-3)),
            rhs: Box::new(Formula::Const(2)),
        });
        assert_eq!(eval(&quotient, &env).expect("floor division").value, -2);

        let by_zero = Formula::Expr(FormulaExpr::FloorDiv {
            lhs: Box::new(Formula::Const(1)),
            rhs: Box::new(Formula::Const(0)),
        });
        assert_eq!(eval(&by_zero, &env), None);
    }

    #[test]
    fn dice_leaves_flatten_to_zero_and_flag_the_betrayal() {
        let abilities = BTreeMap::new();
        let env = env(&abilities, &[]);
        let formula = Formula::Expr(FormulaExpr::Add {
            lhs: Box::new(Formula::dice(dice_pool(1, 4))),
            rhs: Box::new(Formula::Const(3)),
        });

        let evaluated = eval(&formula, &env).expect("the flat part evaluates");

        assert_eq!(evaluated.value, 3);
        assert!(evaluated.flattened_dice);
    }

    #[test]
    fn class_levels_match_case_insensitively() {
        let abilities = BTreeMap::new();
        let classes = [("Wizard".to_owned(), 3), ("wizard".to_owned(), 2)];
        let env = env(&abilities, &classes);
        let formula = Formula::Expr(FormulaExpr::ClassLevel {
            class: "WIZARD".to_owned(),
        });

        assert_eq!(eval(&formula, &env).expect("names match").value, 5);
    }
}
