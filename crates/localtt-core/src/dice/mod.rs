//! Dice notation: the parser and the cap validator over [`DiceExpr`].
//!
//! The AST is part of the serialized [`Roll`](crate::types::Roll), and the
//! grammar and caps are shared by every client that builds a roll, so they live
//! here with it. Rolling, composition with derived roll parts and the die
//! source stay in the engine. The grammar and caps are specified in
//! `docs/engine.md` section 4.

mod lexer;
mod parser;
mod validate;

use thiserror::Error;

use crate::types::DiceExpr;

/// Why a notation or an expression was refused.
///
/// The display strings are the messages clients show for a refused roll, so
/// they are kept stable.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DiceError {
    /// A syntax error or a below-range literal.
    #[error("malformed command: {reason}")]
    Malformed {
        /// Diagnostic detail.
        reason: String,
    },
    /// An exceeded cap: dice count, sides, expression depth or terms.
    #[error("{what} exceeds the limit of {limit}")]
    TooLarge {
        /// The capped quantity.
        what: &'static str,
        /// The cap.
        limit: u32,
    },
}

/// Parses dice notation and validates the result against the caps.
///
/// # Errors
///
/// [`DiceError::Malformed`] for a syntax error or a below-range literal, and
/// [`DiceError::TooLarge`] for an exceeded cap.
///
/// # Examples
///
/// ```
/// use localtt_core::dice::parse;
///
/// assert!(parse("4d6kh3+1").is_ok());
/// assert!(parse("1d").is_err());
/// ```
#[inline]
pub fn parse(notation: &str) -> Result<DiceExpr, DiceError> {
    let tokens = lexer::tokenize(notation)?;
    let expr = parser::parse(&tokens)?;
    validate::validate(&expr)?;
    Ok(expr)
}

/// Validates an arbitrary AST (a wire payload that never went through
/// [`parse`]) against the caps: count, sides, keep count, depth and terms.
///
/// # Errors
///
/// [`DiceError::Malformed`] for an out-of-range pool and
/// [`DiceError::TooLarge`] for an exceeded cap.
#[inline]
pub fn validate(expr: &DiceExpr) -> Result<(), DiceError> {
    validate::validate(expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_keep_the_messages_clients_display() {
        let malformed = parse("1d").expect_err("a pool needs its sides");
        let too_large = parse("101d6").expect_err("the dice count is capped");

        assert!(
            malformed
                .to_string()
                .starts_with("malformed command: invalid dice notation:")
        );
        assert_eq!(too_large.to_string(), "dice count exceeds the limit of 100");
    }

    #[test]
    fn parse_runs_the_caps_and_validate_agrees() {
        let expr = parse("4d6kh3+1d4-1").expect("a multi-term expression");

        assert_eq!(validate(&expr), Ok(()));
    }
}
