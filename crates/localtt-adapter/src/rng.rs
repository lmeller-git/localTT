//! The shipped `Rng`: ChaCha20 via `rand_chacha` (RFC `dice.md` 4.6).

use getrandom::SysRng;
use localtt_engine::ports::rng::Rng;
use rand_chacha::{
    ChaCha20Rng,
    rand_core::{Rng as _, SeedableRng as _},
};

/// The shipped ChaCha20-based `Rng` (rand_chacha; OS seeding via
/// getrandom's `SysRng`).
pub struct ChaChaRng {
    core: ChaCha20Rng,
}

impl ChaChaRng {
    /// OS-seeded: fresh entropy per process.
    ///
    /// Panics when the OS entropy source is unavailable; there is no
    /// meaningful fallback seed.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        let core =
            ChaCha20Rng::try_from_rng(&mut SysRng).expect("the OS entropy source is unavailable");
        Self { core }
    }

    /// Seed-reproducible: equal seeds replay equal sequences, so a campaign
    /// can be replayed from a seed.
    #[must_use]
    #[inline]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            core: ChaCha20Rng::from_seed(seed),
        }
    }
}

impl Default for ChaChaRng {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Rng for ChaChaRng {
    #[inline]
    fn roll_die(&mut self, sides: u32) -> u32 {
        if sides <= 1 {
            return 1;
        }
        // Rejection sampling: accepting only draws below the largest multiple
        // of `sides` keeps every face equally likely.
        let limit = u32::MAX - u32::MAX % sides;
        loop {
            let draw = self.core.next_u32();
            if draw < limit {
                return draw % sides + 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_stay_in_range_and_a_one_sided_die_is_one() {
        let mut rng = ChaChaRng::new();
        for _ in 0..256 {
            let value = rng.roll_die(20);
            assert!((1..=20).contains(&value), "out of range: {value}");
        }
        assert_eq!(rng.roll_die(1), 1);
        assert_eq!(rng.roll_die(0), 1);
    }

    #[test]
    fn equal_seeds_replay_equal_sequences() {
        let mut first = ChaChaRng::from_seed([7; 32]);
        let mut second = ChaChaRng::from_seed([7; 32]);
        let rolled: Vec<u32> = (0..128).map(|_| first.roll_die(20)).collect();
        let replayed: Vec<u32> = (0..128).map(|_| second.roll_die(20)).collect();
        assert_eq!(rolled, replayed);
    }

    #[test]
    fn a_seeded_source_covers_every_face_of_a_die() {
        let mut rng = ChaChaRng::from_seed([11; 32]);
        let mut seen = [false; 6];
        for _ in 0..600 {
            seen[usize::try_from(rng.roll_die(6) - 1).expect("a d6 face fits usize")] = true;
        }
        assert!(seen.iter().all(|face| *face), "every face appears");
    }
}
