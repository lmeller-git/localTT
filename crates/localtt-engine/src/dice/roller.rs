//! Plan plus `Rng` to `DiceResult`: keeps with roll-order tie-breaks,
//! advantage/disadvantage and saturating arithmetic (RFC `dice.md` 5.4 and
//! section 6).

use std::cmp::Reverse;

use localtt_core::types::{DiceExpr, DiceResult, DieResult, Keep, TermResult};

use super::RollPlan;
use crate::ports::rng::Rng;

/// Rolls every term of `plan` in order (RFC `dice.md` 5.4): exactly one
/// `TermResult` per `TermPlan`, `source` copied, totals saturating.
pub(super) fn roll(plan: &RollPlan, rng: &mut dyn Rng) -> DiceResult {
    let mut terms = Vec::with_capacity(plan.terms.len());
    let mut total = 0_i32;
    for term in &plan.terms {
        let (dice, term_total) = roll_expr(&term.expr, rng);
        total = total.saturating_add(term_total);
        terms.push(TermResult {
            dice,
            total: term_total,
            source: term.source.clone(),
        });
    }
    DiceResult { terms, total }
}

/// Rolls one term expression: its dice in draw order and its total.
fn roll_expr(expr: &DiceExpr, rng: &mut dyn Rng) -> (Vec<DieResult>, i32) {
    match expr {
        DiceExpr::Const { value } => (Vec::new(), *value),
        DiceExpr::Roll { count, sides, keep } => roll_pool(*count, *sides, *keep, rng),
        DiceExpr::Add { lhs, rhs } => binary(lhs, rhs, rng, i32::saturating_add),
        DiceExpr::Sub { lhs, rhs } => binary(lhs, rhs, rng, i32::saturating_sub),
        DiceExpr::Mul { lhs, rhs } => binary(lhs, rhs, rng, i32::saturating_mul),
        DiceExpr::Advantage { inner } => advantage(inner, rng, true),
        DiceExpr::Disadvantage { inner } => advantage(inner, rng, false),
    }
}

/// Draws `count` dice left to right and applies the keep rule: the kept dice
/// are the `k` highest (lowest) values, ties keeping the earliest in roll
/// order; the total is the kept sum (RFC `dice.md` 5.4).
fn roll_pool(
    count: i32,
    sides: i32,
    keep: Option<Keep>,
    rng: &mut dyn Rng,
) -> (Vec<DieResult>, i32) {
    let sides = u32::try_from(sides).expect("a validated pool has positive sides");
    let drawn: Vec<i32> = (0..count)
        .map(|_| i32::try_from(rng.roll_die(sides)).expect("a die face fits an i32"))
        .collect();
    let mut kept = vec![true; drawn.len()];
    if let Some(keep) = keep {
        let kept_count = match keep {
            Keep::Highest { count } | Keep::Lowest { count } => {
                usize::try_from(count).expect("a validated keep count is positive")
            }
        };
        let mut order: Vec<usize> = (0..drawn.len()).collect();
        match keep {
            Keep::Highest { .. } => {
                order.sort_by_key(|&index| (Reverse(drawn[index]), index));
            }
            Keep::Lowest { .. } => {
                order.sort_by_key(|&index| (drawn[index], index));
            }
        }
        kept = vec![false; drawn.len()];
        for index in order.into_iter().take(kept_count) {
            kept[index] = true;
        }
    }
    let total = drawn
        .iter()
        .zip(&kept)
        .filter(|(_, kept)| **kept)
        .fold(0_i32, |sum, (value, _)| sum.saturating_add(*value));
    let dice = drawn
        .into_iter()
        .zip(kept)
        .map(|(value, kept)| DieResult { value, kept })
        .collect();
    (dice, total)
}

/// Evaluates `inner` twice, first draw first; the higher total wins
/// (disadvantage: the lower), equal totals keep the first evaluation, and
/// every die of the loser is stamped unkept (RFC `dice.md` 5.4).
fn advantage(inner: &DiceExpr, rng: &mut dyn Rng, higher_wins: bool) -> (Vec<DieResult>, i32) {
    let (mut first, first_total) = roll_expr(inner, rng);
    let (mut second, second_total) = roll_expr(inner, rng);
    let keep_first = if higher_wins {
        first_total >= second_total
    } else {
        first_total <= second_total
    };
    let (total, loser) = if keep_first {
        (first_total, &mut second)
    } else {
        (second_total, &mut first)
    };
    for die in loser.iter_mut() {
        die.kept = false;
    }
    first.extend(second);
    (first, total)
}

/// Applies a binary operation to two term totals, dice concatenated
/// left-then-right with saturating arithmetic.
fn binary(
    lhs: &DiceExpr,
    rhs: &DiceExpr,
    rng: &mut dyn Rng,
    op: fn(i32, i32) -> i32,
) -> (Vec<DieResult>, i32) {
    let (mut dice, left) = roll_expr(lhs, rng);
    let (right_dice, right) = roll_expr(rhs, rng);
    dice.extend(right_dice);
    (dice, op(left, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dice::TermPlan;

    /// An `Rng` that returns ones and counts every draw.
    struct CountingRng {
        draws: u32,
    }

    impl Rng for CountingRng {
        fn roll_die(&mut self, sides: u32) -> u32 {
            self.draws += 1;
            assert!(sides >= 2, "the roller never draws a one-sided die");
            1
        }
    }

    /// A one-term plan over `expr`, with no attribution.
    fn plan(expr: DiceExpr) -> RollPlan {
        RollPlan {
            terms: vec![TermPlan {
                expr: expr.clone(),
                source: None,
            }],
            expression: expr,
        }
    }

    /// A plain pool expression.
    fn roll_pool_expr(count: i32, sides: i32) -> DiceExpr {
        DiceExpr::Roll {
            count,
            sides,
            keep: None,
        }
    }

    /// One die result.
    fn die(value: i32, kept: bool) -> DieResult {
        DieResult { value, kept }
    }

    #[test]
    fn keep_rules_drop_the_rest_and_prefer_earliest_ties() {
        let mut rng = crate::dice::tests::ScriptedRng::new(&[3, 5, 5, 1]);
        let result = roll(
            &plan(DiceExpr::Roll {
                count: 4,
                sides: 6,
                keep: Some(Keep::Highest { count: 3 }),
            }),
            &mut rng,
        );
        assert_eq!(
            result,
            DiceResult {
                terms: vec![TermResult {
                    dice: vec![die(3, true), die(5, true), die(5, true), die(1, false)],
                    total: 13,
                    source: None,
                }],
                total: 13,
            }
        );

        let mut rng = crate::dice::tests::ScriptedRng::new(&[4, 4, 2]);
        let result = roll(
            &plan(DiceExpr::Roll {
                count: 3,
                sides: 6,
                keep: Some(Keep::Lowest { count: 1 }),
            }),
            &mut rng,
        );
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(4, false), die(4, false), die(2, true)],
                total: 2,
                source: None,
            }]
        );

        let mut rng = crate::dice::tests::ScriptedRng::new(&[3, 3]);
        let result = roll(
            &plan(DiceExpr::Roll {
                count: 2,
                sides: 6,
                keep: Some(Keep::Highest { count: 1 }),
            }),
            &mut rng,
        );
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(3, true), die(3, false)],
                total: 3,
                source: None,
            }]
        );
    }

    #[test]
    fn advantage_and_disadvantage_keep_one_of_two_evaluations() {
        let advantage = |inner: DiceExpr| DiceExpr::Advantage {
            inner: Box::new(inner),
        };
        let disadvantage = |inner: DiceExpr| DiceExpr::Disadvantage {
            inner: Box::new(inner),
        };

        let mut rng = crate::dice::tests::ScriptedRng::new(&[13, 20]);
        let result = roll(&plan(advantage(roll_pool_expr(1, 20))), &mut rng);
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(13, false), die(20, true)],
                total: 20,
                source: None,
            }]
        );

        // The bonus applies inside each evaluation, not once.
        let mut rng = crate::dice::tests::ScriptedRng::new(&[10, 18]);
        let result = roll(
            &plan(advantage(DiceExpr::Add {
                lhs: Box::new(roll_pool_expr(1, 20)),
                rhs: Box::new(DiceExpr::Const { value: 2 }),
            })),
            &mut rng,
        );
        assert_eq!(result.total, 20);
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(10, false), die(18, true)],
                total: 20,
                source: None,
            }]
        );

        let mut rng = crate::dice::tests::ScriptedRng::new(&[18, 10]);
        let result = roll(&plan(disadvantage(roll_pool_expr(1, 20))), &mut rng);
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(18, false), die(10, true)],
                total: 10,
                source: None,
            }]
        );

        // Nat detection stays visible: the kept die carries its own face.
        let mut rng = crate::dice::tests::ScriptedRng::new(&[1, 20]);
        let result = roll(&plan(advantage(roll_pool_expr(1, 20))), &mut rng);
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(1, false), die(20, true)],
                total: 20,
                source: None,
            }]
        );
        let mut rng = crate::dice::tests::ScriptedRng::new(&[1, 20]);
        let result = roll(&plan(disadvantage(roll_pool_expr(1, 20))), &mut rng);
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(1, true), die(20, false)],
                total: 1,
                source: None,
            }]
        );

        // Equal totals keep the first evaluation.
        let mut rng = crate::dice::tests::ScriptedRng::new(&[7, 7]);
        let result = roll(&plan(advantage(roll_pool_expr(1, 20))), &mut rng);
        assert_eq!(
            result.terms,
            vec![TermResult {
                dice: vec![die(7, true), die(7, false)],
                total: 7,
                source: None,
            }]
        );
    }

    #[test]
    fn arithmetic_saturates_and_terms_fold_in_order() {
        let mut rng = crate::dice::tests::ScriptedRng::new(&[]);
        let saturated = roll(
            &plan(DiceExpr::Mul {
                lhs: Box::new(DiceExpr::Const { value: i32::MAX }),
                rhs: Box::new(DiceExpr::Const { value: 2 }),
            }),
            &mut rng,
        );
        assert_eq!(saturated.total, i32::MAX);

        let plan = RollPlan {
            terms: vec![
                TermPlan {
                    expr: DiceExpr::Const { value: i32::MAX },
                    source: None,
                },
                TermPlan {
                    expr: DiceExpr::Const { value: 1 },
                    source: Some("Bless".to_owned()),
                },
            ],
            expression: DiceExpr::Const { value: 0 },
        };
        let result = roll(&plan, &mut rng);
        assert_eq!(result.total, i32::MAX, "the outer sum saturates too");
        assert_eq!(result.terms[1].source.as_deref(), Some("Bless"));
    }

    #[test]
    fn a_plan_draws_exactly_what_its_structure_needs() {
        // adv(2d6) + 1d4: two evaluations of two dice, then one d4.
        let expr = DiceExpr::Add {
            lhs: Box::new(DiceExpr::Advantage {
                inner: Box::new(roll_pool_expr(2, 6)),
            }),
            rhs: Box::new(roll_pool_expr(1, 4)),
        };
        let mut rng = CountingRng { draws: 0 };
        let result = roll(&plan(expr), &mut rng);
        assert_eq!(rng.draws, 5, "the roller never draws ahead");
        assert_eq!(result.total, 3, "advantage over two 1s keeps a total of 2");

        // Constants draw nothing.
        let mut rng = CountingRng { draws: 0 };
        let _ = roll(
            &plan(DiceExpr::Add {
                lhs: Box::new(DiceExpr::Const { value: 3 }),
                rhs: Box::new(DiceExpr::Const { value: 4 }),
            }),
            &mut rng,
        );
        assert_eq!(rng.draws, 0);
    }
}
