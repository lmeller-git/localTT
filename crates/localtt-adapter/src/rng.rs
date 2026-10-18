//! The shipped `Rng`: ChaCha20 via `rand_chacha` (RFC `dice.md` 4.6).

use localtt_engine::ports::rng::Rng;

/// The shipped ChaCha20-based `Rng` (rand_chacha; the dependency is added
/// to the adapter during implement).
pub struct ChaChaRng {
    // The seeded core lands with the implement phase.
}

impl ChaChaRng {
    /// OS-seeded: fresh entropy per process.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        todo!()
    }

    /// Seed-reproducible: equal seeds replay equal sequences, so a campaign
    /// can be replayed from a seed.
    #[must_use]
    #[inline]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        todo!()
    }
}

impl Default for ChaChaRng {
    #[inline]
    fn default() -> Self {
        todo!()
    }
}

impl Rng for ChaChaRng {
    #[inline]
    fn roll_die(&mut self, sides: u32) -> u32 {
        todo!()
    }
}
