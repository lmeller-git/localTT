//! Literal helpers: parse or build a value from a string or number literal.
//!
//! Convenience for fixtures, importers and tests; the main builder API lives
//! in the parent module.

use crate::types::{ContentRef, DiceExpr, RefId, RefIdError, StatKey, StatKeyError};

/// Builds a [`ContentRef`] from a literal id and name.
///
/// Returns an error instead of panicking so it is safe on any call path.
#[inline]
pub fn content_ref(id: &str, name: &str) -> Result<ContentRef, RefIdError> {
    Ok(ContentRef {
        id: RefId::parse(id)?,
        name: name.to_owned(),
    })
}

/// Parses a [`StatKey`] from a literal.
#[inline]
pub fn stat(raw: &str) -> Result<StatKey, StatKeyError> {
    StatKey::parse(raw)
}

/// Parses a [`RefId`] from a literal.
#[inline]
pub fn ref_id(raw: &str) -> Result<RefId, RefIdError> {
    RefId::parse(raw)
}

/// Builds a plain dice pool, e.g. `dice_pool(1, 4)` for `1d4`.
#[must_use]
#[inline]
pub fn dice_pool(count: i32, sides: i32) -> DiceExpr {
    DiceExpr::Roll {
        count,
        sides,
        keep: None,
    }
}
