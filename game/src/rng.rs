//! Per-subsystem RNG streams (ADR 0005). Each subsystem draws only from its
//! own `Pcg64` stream, so extra draws in one subsystem never shift another's
//! results. Streams about a place (map, spawn) come from its zone seed
//! (ADR 0009); the rest from the world seed.

use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};

/// PCG stream ids. Every stream in the game is listed here; never reuse or
/// renumber one, since that changes every seeded world.
pub mod stream {
    pub const MAP: u128 = 1;
    pub const SPAWN: u128 = 2;
    pub const COMBAT: u128 = 3;
    pub const SPELL: u128 = 4;
    /// Ruins, built once from the lineage when a game starts
    /// (`legacy::build_ruins`), so it has no `Rngs` field.
    pub const LEGACY: u128 = 5;
    pub const CRAFT: u128 = 6;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rngs {
    /// Map changes. Generation itself runs on per-attempt streams
    /// (`zone::generate_cave`); this continues the accepted one.
    pub map: Pcg64,
    /// Choosing where (and later, what) to spawn.
    pub spawn: Pcg64,
    /// Attack rolls and anything combat event handlers draw (`Ctx::rng`).
    pub combat: Pcg64,
    /// Spell outcomes that are not damage: where summons appear, fizzles.
    pub spell: Pcg64,
    /// What a kill yields and how a craft turns out (docs/plans/crafting.md).
    pub craft: Pcg64,
}

impl Rngs {
    pub fn new(world_seed: u64, zone_seed: u64) -> Self {
        let (world, zone) = (u128::from(world_seed), u128::from(zone_seed));
        Self {
            map: Pcg64::new(zone, stream::MAP),
            spawn: Pcg64::new(zone, stream::SPAWN),
            combat: Pcg64::new(world, stream::COMBAT),
            spell: Pcg64::new(world, stream::SPELL),
            craft: Pcg64::new(world, stream::CRAFT),
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;

    #[test]
    fn same_seed_gives_identical_streams() {
        assert_eq!(Rngs::new(5, 5), Rngs::new(5, 5));
    }

    #[test]
    fn streams_are_distinct_and_independent() {
        let fresh = Rngs::new(5, 5);
        assert_ne!(fresh.map, fresh.spawn);
        assert_ne!(fresh.spawn, fresh.combat);
        assert_ne!(fresh.combat, fresh.spell);

        let mut a = fresh.clone();
        let mut b = fresh;
        for _ in 0..10 {
            a.spawn.next_u64();
        }
        assert_eq!(a.map, b.map);
        assert_eq!(a.combat.next_u64(), b.combat.next_u64());
    }
}
