//! TODO

#![deny(missing_docs)]
#![deny(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

#[must_use]
#[inline]
/// TODO
pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
