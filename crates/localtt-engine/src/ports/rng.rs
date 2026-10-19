//! The die source port (RFC `dice.md` 4.1; engine-architecture 4.4).

/// Uniform die source (engine-architecture 4.4).
///
/// Contract: `roll_die(sides)` returns a value in `1..=sides` with equal
/// probability for every `sides >= 2`; `roll_die(1) == 1`. Callers cap
/// `sides` and counts (section 5.2); `sides == 0` is a caller bug.
pub trait Rng: Send {
    /// Draws one die.
    fn roll_die(&mut self, sides: u32) -> u32;
}
