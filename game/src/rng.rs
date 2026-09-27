//! Per-subsystem RNG streams (ADR 0005). Each subsystem draws only from its
//! own `Pcg64` stream, all derived from the world seed, so extra draws in one
//! subsystem never shift another's results.

use rand_pcg::Pcg64;

/// PCG stream ids. Every stream in the game is listed here; never reuse or
/// renumber one, since that changes every seeded world.
pub mod stream {
    pub const MAP: u128 = 1;
    pub const SPAWN: u128 = 2;
    pub const COMBAT: u128 = 3;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rngs {
    /// Map generation.
    pub map: Pcg64,
    /// Choosing where (and later, what) to spawn.
    pub spawn: Pcg64,
    /// Attack rolls and anything combat event handlers draw (`Ctx::rng`).
    pub combat: Pcg64,
}

impl Rngs {
    pub fn new(seed: u64) -> Self {
        let state = u128::from(seed);
        Self {
            map: Pcg64::new(state, stream::MAP),
            spawn: Pcg64::new(state, stream::SPAWN),
            combat: Pcg64::new(state, stream::COMBAT),
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;

    #[test]
    fn same_seed_gives_identical_streams() {
        assert_eq!(Rngs::new(5), Rngs::new(5));
    }

    #[test]
    fn streams_are_distinct_and_independent() {
        let fresh = Rngs::new(5);
        assert_ne!(fresh.map, fresh.spawn);
        assert_ne!(fresh.spawn, fresh.combat);

        let mut a = fresh.clone();
        let mut b = fresh;
        for _ in 0..10 {
            a.spawn.next_u64();
        }
        assert_eq!(a.map, b.map);
        assert_eq!(a.combat.next_u64(), b.combat.next_u64());
    }
}
