//! Tokens: numbers, pool tokens, keywords and punctuation (RFC `dice.md` 5.1
//! and section 6).

use std::{iter::Peekable, str::Chars};

use localtt_core::types::Keep;

use crate::error::EngineError;

/// One lexical token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Token {
    /// A non-negative integer literal.
    Number(i32),
    /// A contiguous `NdM` pool with its attached keep suffix, if any.
    Pool {
        /// Number of dice.
        count: i32,
        /// Die sides.
        sides: i32,
        /// The keep suffix.
        keep: Option<Keep>,
    },
    /// The `adv` keyword.
    Advantage,
    /// The `dis` keyword.
    Disadvantage,
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `*`.
    Star,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
}

/// Splits `notation` into tokens (RFC `dice.md` 5.1): ASCII whitespace between
/// tokens is ignored, a pool is one contiguous token, and anything else is
/// `Malformed`.
pub(super) fn tokenize(notation: &str) -> Result<Vec<Token>, EngineError> {
    let mut tokens = Vec::new();
    let mut chars = notation.chars().peekable();
    while let Some(token) = next_token(&mut chars)? {
        tokens.push(token);
    }
    Ok(tokens)
}

/// Consumes whitespace, then one token; `None` at the end of the input.
fn next_token(chars: &mut Peekable<Chars<'_>>) -> Result<Option<Token>, EngineError> {
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_whitespace() {
            break;
        }
        chars.next();
    }
    let Some(&c) = chars.peek() else {
        return Ok(None);
    };
    let token = match c {
        '+' => {
            chars.next();
            Token::Plus
        }
        '-' => {
            chars.next();
            Token::Minus
        }
        '*' => {
            chars.next();
            Token::Star
        }
        '(' => {
            chars.next();
            Token::LParen
        }
        ')' => {
            chars.next();
            Token::RParen
        }
        '0'..='9' => pool_or_number(chars)?,
        'a'..='z' => keyword(chars)?,
        _ => return Err(malformed(format!("unexpected character `{c}`"))),
    };
    Ok(Some(token))
}

/// Reads a number, or the pool form when a `d` follows the digits immediately
/// (RFC `dice.md` 5.1: whitespace inside a pool token is a syntax error).
fn pool_or_number(chars: &mut Peekable<Chars<'_>>) -> Result<Token, EngineError> {
    let count = number(chars)?;
    if chars.peek() != Some(&'d') {
        return Ok(Token::Number(count));
    }
    chars.next();
    let sides = number(chars)?;
    if chars.peek() != Some(&'k') {
        return Ok(Token::Pool {
            count,
            sides,
            keep: None,
        });
    }
    chars.next();
    let highest = match chars.next() {
        Some('h') => true,
        Some('l') => false,
        _ => return Err(malformed("a keep suffix is `kh` or `kl`")),
    };
    let kept = number(chars)?;
    let keep = if highest {
        Keep::Highest { count: kept }
    } else {
        Keep::Lowest { count: kept }
    };
    Ok(Token::Pool {
        count,
        sides,
        keep: Some(keep),
    })
}

/// Reads a non-negative literal; one that does not fit `i32` is `Malformed`
/// (RFC `dice.md` 5.1).
fn number(chars: &mut Peekable<Chars<'_>>) -> Result<i32, EngineError> {
    let mut value: i32 = 0;
    let mut any = false;
    while let Some(&c) = chars.peek() {
        let Some(digit) = c.to_digit(10) else {
            break;
        };
        chars.next();
        any = true;
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(digit as i32))
            .ok_or_else(|| malformed("a numeric literal does not fit an integer"))?;
    }
    if !any {
        return Err(malformed("a number was expected"));
    }
    Ok(value)
}

/// Reads a lowercase word; only `adv` and `dis` are keywords (RFC `dice.md`
/// 5.1).
fn keyword(chars: &mut Peekable<Chars<'_>>) -> Result<Token, EngineError> {
    let mut word = String::new();
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_lowercase() {
            break;
        }
        word.push(c);
        chars.next();
    }
    match word.as_str() {
        "adv" => Ok(Token::Advantage),
        "dis" => Ok(Token::Disadvantage),
        other => Err(malformed(format!("unexpected word `{other}`"))),
    }
}

/// The `Malformed` every notation-level refusal uses.
fn malformed(reason: impl std::fmt::Display) -> EngineError {
    EngineError::Malformed {
        reason: format!("invalid dice notation: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_pools_and_punctuation_tokenize() {
        assert_eq!(
            tokenize("1d20+3").expect("a literal expression"),
            vec![
                Token::Pool {
                    count: 1,
                    sides: 20,
                    keep: None,
                },
                Token::Plus,
                Token::Number(3),
            ]
        );
        assert_eq!(
            tokenize("  1d20  ").expect("whitespace is ignored"),
            vec![Token::Pool {
                count: 1,
                sides: 20,
                keep: None,
            }]
        );
        assert_eq!(
            tokenize("4d6kh3-1d4*2").expect("a multi-token expression"),
            vec![
                Token::Pool {
                    count: 4,
                    sides: 6,
                    keep: Some(Keep::Highest { count: 3 }),
                },
                Token::Minus,
                Token::Pool {
                    count: 1,
                    sides: 4,
                    keep: None,
                },
                Token::Star,
                Token::Number(2),
            ]
        );
        assert_eq!(
            tokenize("3d6kl1").expect("a keep-lowest suffix"),
            vec![Token::Pool {
                count: 3,
                sides: 6,
                keep: Some(Keep::Lowest { count: 1 }),
            }]
        );
        assert_eq!(
            tokenize("adv ( 1d20 )").expect("keywords and parentheses"),
            vec![
                Token::Advantage,
                Token::LParen,
                Token::Pool {
                    count: 1,
                    sides: 20,
                    keep: None,
                },
                Token::RParen,
            ]
        );
        assert_eq!(tokenize("").expect("empty input has no tokens"), Vec::new());
    }

    #[test]
    fn broken_notation_is_malformed() {
        for notation in ["1D20", "d20", "1 d20", "1d20kh", "1d20kx1", "abc", "1d20%2"] {
            assert!(
                matches!(tokenize(notation), Err(EngineError::Malformed { .. })),
                "`{notation}` must be malformed"
            );
        }
    }

    #[test]
    fn oversized_literals_are_malformed() {
        assert!(matches!(
            tokenize("99999999999d6"),
            Err(EngineError::Malformed { .. })
        ));
        assert!(matches!(
            tokenize("1d99999999999"),
            Err(EngineError::Malformed { .. })
        ));
    }
}
