//! The four caps over an arbitrary AST: count, sides, keep count, depth and
//! terms (RFC `dice.md` 5.2 and section 6).

use localtt_core::types::{DiceExpr, Keep};

use crate::error::EngineError;

/// The dice-count cap.
const COUNT_LIMIT: u32 = 100;
/// The die-sides cap.
const SIDES_LIMIT: u32 = 1000;
/// The expression-depth cap.
const DEPTH_LIMIT: u32 = 16;
/// The expression-terms cap.
const TERM_LIMIT: u32 = 32;

/// Validates an arbitrary AST (a wire payload that never went through `parse`)
/// against the caps (RFC `dice.md` 5.2): every pool leaf, then the depth, then
/// the term count.
pub(super) fn validate(expr: &DiceExpr) -> Result<(), EngineError> {
    check_pools(expr)?;
    if depth(expr) > DEPTH_LIMIT {
        return Err(EngineError::TooLarge {
            what: "expression depth",
            limit: DEPTH_LIMIT,
        });
    }
    if term_count(expr) > TERM_LIMIT {
        return Err(EngineError::TooLarge {
            what: "expression terms",
            limit: TERM_LIMIT,
        });
    }
    Ok(())
}

/// Re-checks every `Roll` leaf: count `1..=100`, sides `2..=1000`, keep count
/// `1..=count` (RFC `dice.md` 5.2).
fn check_pools(expr: &DiceExpr) -> Result<(), EngineError> {
    let mut stack = vec![expr];
    while let Some(node) = stack.pop() {
        match node {
            DiceExpr::Roll { count, sides, keep } => {
                if *count < 1 {
                    return Err(malformed(format!(
                        "a pool needs at least one die, got {count}"
                    )));
                }
                if i64::from(*count) > i64::from(COUNT_LIMIT) {
                    return Err(EngineError::TooLarge {
                        what: "dice count",
                        limit: COUNT_LIMIT,
                    });
                }
                if *sides < 2 {
                    return Err(malformed(format!(
                        "a die needs at least two sides, got {sides}"
                    )));
                }
                if i64::from(*sides) > i64::from(SIDES_LIMIT) {
                    return Err(EngineError::TooLarge {
                        what: "dice sides",
                        limit: SIDES_LIMIT,
                    });
                }
                if let Some(keep) = keep {
                    let kept = match keep {
                        Keep::Highest { count } | Keep::Lowest { count } => *count,
                    };
                    if kept < 1 || kept > *count {
                        return Err(malformed(format!(
                            "a keep count must be between 1 and {count}, got {kept}"
                        )));
                    }
                }
            }
            DiceExpr::Const { .. } => {}
            DiceExpr::Advantage { inner } | DiceExpr::Disadvantage { inner } => stack.push(inner),
            DiceExpr::Add { lhs, rhs }
            | DiceExpr::Sub { lhs, rhs }
            | DiceExpr::Mul { lhs, rhs } => {
                stack.push(lhs);
                stack.push(rhs);
            }
        }
    }
    Ok(())
}

/// The node count on the longest root-to-leaf path (RFC `dice.md` 5.2); a bare
/// die is 1, every wrapper or operator on the path adds one. Iterative, so a
/// hostile wire AST cannot overflow the stack.
fn depth(expr: &DiceExpr) -> u32 {
    let mut stack = vec![(expr, 1_u32)];
    let mut deepest = 0;
    while let Some((node, at)) = stack.pop() {
        deepest = deepest.max(at);
        if deepest > DEPTH_LIMIT {
            return deepest;
        }
        match node {
            DiceExpr::Const { .. } | DiceExpr::Roll { .. } => {}
            DiceExpr::Advantage { inner } | DiceExpr::Disadvantage { inner } => {
                stack.push((inner, at + 1));
            }
            DiceExpr::Add { lhs, rhs }
            | DiceExpr::Sub { lhs, rhs }
            | DiceExpr::Mul { lhs, rhs } => {
                stack.push((lhs, at + 1));
                stack.push((rhs, at + 1));
            }
        }
    }
    deepest
}

/// Every AST node counts as a term: leaves, operators and wrappers;
/// parentheses are transparent (RFC `dice.md` 5.2). Iterative for the same
/// reason as [`depth`].
fn term_count(expr: &DiceExpr) -> u32 {
    let mut stack = vec![expr];
    let mut count = 0;
    while let Some(node) = stack.pop() {
        count += 1;
        if count > TERM_LIMIT {
            return count;
        }
        match node {
            DiceExpr::Const { .. } | DiceExpr::Roll { .. } => {}
            DiceExpr::Advantage { inner } | DiceExpr::Disadvantage { inner } => stack.push(inner),
            DiceExpr::Add { lhs, rhs }
            | DiceExpr::Sub { lhs, rhs }
            | DiceExpr::Mul { lhs, rhs } => {
                stack.push(lhs);
                stack.push(rhs);
            }
        }
    }
    count
}

/// The `Malformed` every AST-level refusal uses.
fn malformed(reason: impl std::fmt::Display) -> EngineError {
    EngineError::Malformed {
        reason: format!("invalid dice expression: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain pool expression.
    fn roll(count: i32, sides: i32) -> DiceExpr {
        DiceExpr::Roll {
            count,
            sides,
            keep: None,
        }
    }

    /// Wraps `expr` in `wrappers` advantage layers.
    fn wrapped(expr: DiceExpr, wrappers: u32) -> DiceExpr {
        (0..wrappers).fold(expr, |inner, _| DiceExpr::Advantage {
            inner: Box::new(inner),
        })
    }

    /// A balanced `Add` tree with `leaves` constant leaves, so the node count
    /// is `2 * leaves - 1` while the depth stays logarithmic.
    fn added(leaves: u32) -> DiceExpr {
        if leaves == 1 {
            return DiceExpr::Const { value: 1 };
        }
        let left = leaves / 2 + leaves % 2;
        DiceExpr::Add {
            lhs: Box::new(added(left)),
            rhs: Box::new(added(leaves - left)),
        }
    }

    #[test]
    fn pool_ranges_gate_the_count_and_the_sides() {
        assert!(validate(&roll(1, 20)).is_ok());
        assert!(validate(&roll(100, 1000)).is_ok());
        assert!(matches!(
            validate(&roll(101, 6)),
            Err(EngineError::TooLarge {
                what: "dice count",
                limit: 100
            })
        ));
        assert!(matches!(
            validate(&roll(0, 6)),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            validate(&roll(-1, 6)),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            validate(&roll(1, 1001)),
            Err(EngineError::TooLarge {
                what: "dice sides",
                limit: 1000
            })
        ));
        assert!(matches!(
            validate(&roll(1, 1)),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            validate(&roll(1, 0)),
            Err(EngineError::Malformed { .. })
        ));
    }

    #[test]
    fn keep_counts_stay_within_the_pool() {
        assert!(
            validate(&DiceExpr::Roll {
                count: 4,
                sides: 6,
                keep: Some(Keep::Highest { count: 3 }),
            })
            .is_ok()
        );
        assert!(
            validate(&DiceExpr::Roll {
                count: 4,
                sides: 6,
                keep: Some(Keep::Lowest { count: 4 }),
            })
            .is_ok()
        );
        for keep in [
            Keep::Highest { count: 0 },
            Keep::Highest { count: 4 },
            Keep::Lowest { count: -1 },
        ] {
            assert!(
                matches!(
                    validate(&DiceExpr::Roll {
                        count: 3,
                        sides: 6,
                        keep: Some(keep),
                    }),
                    Err(EngineError::Malformed { .. })
                ),
                "keep {keep:?} must be malformed for a 3-die pool"
            );
        }
    }

    #[test]
    fn depth_and_terms_are_capped() {
        assert!(validate(&wrapped(roll(1, 20), 15)).is_ok());
        assert!(matches!(
            validate(&wrapped(roll(1, 20), 16)),
            Err(EngineError::TooLarge {
                what: "expression depth",
                limit: 16
            })
        ));
        assert!(validate(&added(16)).is_ok(), "31 nodes fit");
        assert!(matches!(
            validate(&added(17)),
            Err(EngineError::TooLarge {
                what: "expression terms",
                limit: 32
            })
        ));
    }
}
