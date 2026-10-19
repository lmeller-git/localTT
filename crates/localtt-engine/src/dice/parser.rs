//! Recursive descent over the token stream, producing a `DiceExpr` (RFC
//! `dice.md` 5.1 and section 6).

use localtt_core::types::DiceExpr;

use super::{lexer::Token, validate::DEPTH_LIMIT};
use crate::error::EngineError;

/// Parses one expression off the whole token stream; leftover tokens are a
/// syntax error (RFC `dice.md` 5.1).
pub(super) fn parse(tokens: &[Token]) -> Result<DiceExpr, EngineError> {
    let mut cursor = Cursor {
        tokens,
        position: 0,
        nesting: 0,
    };
    let expr = cursor.expression()?;
    if cursor.peek().is_some() {
        return Err(malformed("trailing tokens after the expression"));
    }
    Ok(expr)
}

/// A cursor over the token stream.
struct Cursor<'a> {
    tokens: &'a [Token],
    position: usize,
    /// The open `(`/`adv`/`dis` nesting, bounded by [`DEPTH_LIMIT`].
    nesting: u32,
}

impl Cursor<'_> {
    /// The next token, without consuming it.
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.position).copied()
    }

    /// Consumes the next token.
    fn next(&mut self) -> Option<Token> {
        let token = self.peek();
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    /// Enters one nesting level: a parenthesized group or an `adv`/`dis`
    /// wrapper. Deeper notation is refused with the depth cap's payload
    /// instead of recursing until the stack overflows (RFC `dice.md` 5.2).
    fn enter(&mut self) -> Result<(), EngineError> {
        self.nesting += 1;
        if self.nesting > DEPTH_LIMIT {
            return Err(EngineError::TooLarge {
                what: "expression depth",
                limit: DEPTH_LIMIT,
            });
        }
        Ok(())
    }

    /// Leaves the level [`enter`](Cursor::enter) opened; only the success
    /// path needs it, since an error aborts the whole parse.
    fn leave(&mut self) {
        self.nesting -= 1;
    }

    /// `expr = term (("+" | "-") term)*`, left-associative.
    fn expression(&mut self) -> Result<DiceExpr, EngineError> {
        let mut lhs = self.term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.next();
                    let rhs = self.term()?;
                    lhs = DiceExpr::Add {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Token::Minus) => {
                    self.next();
                    let rhs = self.term()?;
                    lhs = DiceExpr::Sub {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => return Ok(lhs),
            }
        }
    }

    /// `term = factor ("*" factor)*`, left-associative.
    fn term(&mut self) -> Result<DiceExpr, EngineError> {
        let mut lhs = self.factor()?;
        while self.peek() == Some(Token::Star) {
            self.next();
            let rhs = self.factor()?;
            lhs = DiceExpr::Mul {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// `factor = "adv" "(" expr ")" | "dis" "(" expr ")" | pool | number |
    /// "(" expr ")"`; parentheses are transparent to the AST.
    fn factor(&mut self) -> Result<DiceExpr, EngineError> {
        match self.next() {
            Some(Token::Advantage) => {
                self.enter()?;
                let inner = self.parenthesized()?;
                self.leave();
                Ok(DiceExpr::Advantage {
                    inner: Box::new(inner),
                })
            }
            Some(Token::Disadvantage) => {
                self.enter()?;
                let inner = self.parenthesized()?;
                self.leave();
                Ok(DiceExpr::Disadvantage {
                    inner: Box::new(inner),
                })
            }
            Some(Token::Pool { count, sides, keep }) => Ok(DiceExpr::Roll { count, sides, keep }),
            Some(Token::Number(value)) => Ok(DiceExpr::Const { value }),
            Some(Token::LParen) => {
                self.enter()?;
                let inner = self.expression()?;
                self.expect_rparen()?;
                self.leave();
                Ok(inner)
            }
            Some(other) => Err(malformed(format!("unexpected `{other:?}`"))),
            None => Err(malformed("unexpected end of the expression")),
        }
    }

    /// `"(" expr ")"`, for the `adv`/`dis` wrappers.
    fn parenthesized(&mut self) -> Result<DiceExpr, EngineError> {
        match self.next() {
            Some(Token::LParen) => {}
            Some(other) => {
                return Err(malformed(format!(
                    "`adv` and `dis` need parentheses, got `{other:?}`"
                )));
            }
            None => return Err(malformed("`adv` and `dis` need parentheses")),
        }
        let inner = self.expression()?;
        self.expect_rparen()?;
        Ok(inner)
    }

    /// Consumes the closing parenthesis of an open group.
    fn expect_rparen(&mut self) -> Result<(), EngineError> {
        match self.next() {
            Some(Token::RParen) => Ok(()),
            Some(other) => Err(malformed(format!("expected `)`, got `{other:?}`"))),
            None => Err(malformed("expected `)`, got the end of the expression")),
        }
    }
}

/// The `Malformed` every syntax-level refusal uses.
fn malformed(reason: impl std::fmt::Display) -> EngineError {
    EngineError::Malformed {
        reason: format!("invalid dice notation: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::types::Keep;

    use super::*;
    use crate::dice::lexer::tokenize;

    /// Parses notation through the lexer, as `parse` does.
    fn parse_notation(notation: &str) -> Result<DiceExpr, EngineError> {
        parse(&tokenize(notation)?)
    }

    /// A plain pool expression.
    fn roll(count: i32, sides: i32) -> DiceExpr {
        DiceExpr::Roll {
            count,
            sides,
            keep: None,
        }
    }

    /// A constant expression.
    fn constant(value: i32) -> DiceExpr {
        DiceExpr::Const { value }
    }

    #[test]
    fn precedence_and_associativity_build_the_expected_ast() {
        assert_eq!(
            parse_notation("2d6+3").expect("a flat addend"),
            DiceExpr::Add {
                lhs: Box::new(roll(2, 6)),
                rhs: Box::new(constant(3)),
            }
        );
        assert_eq!(
            parse_notation("4d6kh3+1d4-1").expect("a multi-term expression"),
            DiceExpr::Sub {
                lhs: Box::new(DiceExpr::Add {
                    lhs: Box::new(DiceExpr::Roll {
                        count: 4,
                        sides: 6,
                        keep: Some(Keep::Highest { count: 3 }),
                    }),
                    rhs: Box::new(roll(1, 4)),
                }),
                rhs: Box::new(constant(1)),
            }
        );
        assert_eq!(
            parse_notation("1d20*2+3").expect("multiplication binds tighter"),
            DiceExpr::Add {
                lhs: Box::new(DiceExpr::Mul {
                    lhs: Box::new(roll(1, 20)),
                    rhs: Box::new(constant(2)),
                }),
                rhs: Box::new(constant(3)),
            }
        );
        assert_eq!(
            parse_notation("1d6+2+3").expect("addition is left-associative"),
            DiceExpr::Add {
                lhs: Box::new(DiceExpr::Add {
                    lhs: Box::new(roll(1, 6)),
                    rhs: Box::new(constant(2)),
                }),
                rhs: Box::new(constant(3)),
            }
        );
        assert_eq!(
            parse_notation("(1d6+2)*3").expect("parentheses group"),
            DiceExpr::Mul {
                lhs: Box::new(DiceExpr::Add {
                    lhs: Box::new(roll(1, 6)),
                    rhs: Box::new(constant(2)),
                }),
                rhs: Box::new(constant(3)),
            }
        );
    }

    #[test]
    fn advantage_and_disadvantage_wrap_their_parenthesized_expression() {
        assert_eq!(
            parse_notation("adv(1d20)").expect("advantage"),
            DiceExpr::Advantage {
                inner: Box::new(roll(1, 20)),
            }
        );
        assert_eq!(
            parse_notation("dis ( adv( 1d20 + 2 ) )").expect("nested wrappers"),
            DiceExpr::Disadvantage {
                inner: Box::new(DiceExpr::Advantage {
                    inner: Box::new(DiceExpr::Add {
                        lhs: Box::new(roll(1, 20)),
                        rhs: Box::new(constant(2)),
                    }),
                }),
            }
        );
    }

    #[test]
    fn syntax_errors_are_malformed() {
        for notation in ["1D20", "d20", "adv 1d20", "", "1d20+", "1d20+*2", "(1d20"] {
            assert!(
                matches!(parse_notation(notation), Err(EngineError::Malformed { .. })),
                "`{notation}` must be malformed"
            );
        }
    }

    #[test]
    fn nesting_is_refused_at_the_depth_cap_instead_of_overflowing_the_stack() {
        // Far past any stack budget: the guard must refuse before recursing.
        let deep = format!("{}1d20{}", "(".repeat(200_000), ")".repeat(200_000));
        assert!(matches!(
            parse_notation(&deep),
            Err(EngineError::TooLarge {
                what: "expression depth",
                limit: 16
            })
        ));

        // The cap itself is allowed and parentheses stay transparent to the
        // AST; one level past it is refused.
        let sixteen = format!("{}1d20{}", "(".repeat(16), ")".repeat(16));
        assert_eq!(
            parse_notation(&sixteen).expect("the cap itself is allowed"),
            roll(1, 20)
        );
        let seventeen = format!("{}1d20{}", "(".repeat(17), ")".repeat(17));
        assert!(matches!(
            parse_notation(&seventeen),
            Err(EngineError::TooLarge {
                what: "expression depth",
                limit: 16
            })
        ));
    }
}
