//! Zones (ADR 0009): fixed-size areas larger than the screen, each seeded
//! from the world seed and its coordinates, and the cellular-automata cave
//! generator that fills them.

use std::collections::VecDeque;

use rand::RngExt;
use rand_pcg::Pcg64;
use serde::Deserialize;

use crate::input::Dir;
use crate::map::{Grid, Map, Tile, floor_candidates};
use crate::rng::stream;

pub const ZONE_WIDTH: i32 = 160;
pub const ZONE_HEIGHT: i32 = 60;

/// Failed generation attempts before settling for the one with the most
/// floor. Bounded so a bad parameter file can't hang world creation.
pub const CAVE_ATTEMPTS: u64 = 10;

/// A zone's place in the world. Only `START` exists so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneCoord {
    pub x: i32,
    pub y: i32,
}

impl ZoneCoord {
    pub const START: ZoneCoord = ZoneCoord { x: 0, y: 0 };
}

/// SplitMix64's output function: a fixed bijection on `u64` that spreads
/// every input bit over the whole output. Written out rather than taken from
/// `std::hash`, whose algorithms may change between Rust releases.
fn splitmix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derives a child seed: `splitmix64(seed ^ splitmix64(value))`. For a fixed
/// `seed`, distinct `value`s always give distinct results.
pub fn mix(seed: u64, value: u64) -> u64 {
    splitmix64(seed ^ splitmix64(value))
}

/// `mix(mix(world_seed, x), y)`, with each coordinate's bits taken as `u32`.
/// Nesting keeps (x, y) and (y, x) apart.
pub fn zone_seed(world_seed: u64, zone: ZoneCoord) -> u64 {
    let bits = |coord: i32| u64::from(coord.cast_unsigned());
    mix(mix(world_seed, bits(zone.x)), bits(zone.y))
}

/// Tuning for `generate_cave`, part of a dungeon's data (`data/dungeons/`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaveParams {
    /// Chance that an interior tile starts as wall.
    pub wall_chance: f64,
    /// Smoothing passes.
    pub iterations: u32,
    /// A floor tile with at least this many wall neighbours (of 8; outside
    /// the zone counts as wall) becomes wall.
    pub wall_birth: u32,
    /// A wall tile with at least this many wall neighbours stays wall.
    pub wall_survival: u32,
    /// Share of the zone the kept cave must cover, or the attempt is retried.
    pub min_floor_fraction: f64,
}

impl CaveParams {
    /// Whether these parameters can generate a cave, and why not.
    pub fn check(&self) -> Result<(), &'static str> {
        if !(0.0..=1.0).contains(&self.wall_chance) {
            return Err("wall_chance must be between 0 and 1");
        }
        if !(0.0..=1.0).contains(&self.min_floor_fraction) {
            return Err("min_floor_fraction must be between 0 and 1");
        }
        if self.wall_birth > 8 || self.wall_survival > 8 {
            return Err("wall_birth and wall_survival count neighbours, at most 8");
        }
        Ok(())
    }
}

/// A `ZONE_WIDTH` × `ZONE_HEIGHT` cave: solid border, one 8-connected floor
/// region. Attempt `n` draws only from `Pcg64::new(mix(zone_seed, n),
/// stream::MAP)`; the first attempt whose cave covers
/// `min_floor_fraction` wins, else the attempt with the most floor (the
/// first, on ties). Returns the winning attempt's stream, advanced past
/// generation, to become `Rngs::map`.
pub fn generate_cave(zone_seed: u64, params: &CaveParams) -> (Map, Pcg64) {
    let wanted = params.min_floor_fraction * f64::from(ZONE_WIDTH * ZONE_HEIGHT);
    let attempt = |n| {
        let mut rng = Pcg64::new(u128::from(mix(zone_seed, n)), stream::MAP);
        let (map, floor) = cave_attempt(&mut rng, params);
        (floor, map, rng)
    };
    let mut best = attempt(0);
    for n in 1..CAVE_ATTEMPTS {
        if best.0 as f64 >= wanted {
            break;
        }
        let next = attempt(n);
        if next.0 > best.0 {
            best = next;
        }
    }
    (best.1, best.2)
}

/// One try: random fill, smoothing, then only the largest region kept.
/// Returns the map and its floor count (at least 1).
fn cave_attempt(rng: &mut Pcg64, params: &CaveParams) -> (Map, usize) {
    let mut map = Grid::new(ZONE_WIDTH, ZONE_HEIGHT, Tile::Wall);
    for y in 1..ZONE_HEIGHT - 1 {
        for x in 1..ZONE_WIDTH - 1 {
            if rng.random_range(0.0..1.0) >= params.wall_chance {
                map.set(x, y, Tile::Floor);
            }
        }
    }
    for _ in 0..params.iterations {
        map = smooth(&map, params);
    }
    let floor = keep_largest_region(&mut map);
    (map, floor)
}

/// One cellular-automata pass over the interior; the border stays wall.
fn smooth(map: &Map, params: &CaveParams) -> Map {
    let mut next = Grid::new(map.width, map.height, Tile::Wall);
    for y in 1..map.height - 1 {
        for x in 1..map.width - 1 {
            let walls = Dir::ALL
                .into_iter()
                .filter(|dir| {
                    let (dx, dy) = dir.delta();
                    map.get(x + dx, y + dy) != Some(&Tile::Floor)
                })
                .count() as u32;
            let threshold = if map.get(x, y) == Some(&Tile::Wall) {
                params.wall_survival
            } else {
                params.wall_birth
            };
            if walls < threshold {
                next.set(x, y, Tile::Floor);
            }
        }
    }
    next
}

/// Walls in every floor region but the largest (the first found in
/// row-major order, on ties) and returns its size. With no floor at all it
/// opens the centre tile, so a zone always has somewhere to stand.
fn keep_largest_region(map: &mut Map) -> usize {
    let mut seen = Grid::new(map.width, map.height, false);
    let mut largest: Vec<(i32, i32)> = Vec::new();
    for start in floor_candidates(map, &[]) {
        if seen.get(start.0, start.1) == Some(&true) {
            continue;
        }
        let region = region(map, start, &mut seen);
        if region.len() > largest.len() {
            largest = region;
        }
    }
    if largest.is_empty() {
        map.set(map.width / 2, map.height / 2, Tile::Floor);
        return 1;
    }
    let mut keep = Grid::new(map.width, map.height, false);
    for &(x, y) in &largest {
        keep.set(x, y, true);
    }
    for (x, y) in floor_candidates(map, &[]) {
        if keep.get(x, y) != Some(&true) {
            map.set(x, y, Tile::Wall);
        }
    }
    largest.len()
}

/// Every floor tile reachable from `start` in 8-way steps (how things
/// move), marking each in `seen`. Closed doors and rubble are walked through
/// but never listed, so nothing is placed on one.
pub(crate) fn region(map: &Map, start: (i32, i32), seen: &mut Grid<bool>) -> Vec<(i32, i32)> {
    let mut found = Vec::new();
    if map.get(start.0, start.1) != Some(&Tile::Floor) {
        return found;
    }
    seen.set(start.0, start.1, true);
    let mut queue = VecDeque::from([start]);
    while let Some((x, y)) = queue.pop_front() {
        if map.get(x, y) == Some(&Tile::Floor) {
            found.push((x, y));
        }
        for dir in Dir::ALL {
            let (dx, dy) = dir.delta();
            let next = (x + dx, y + dy);
            if matches!(
                map.get(next.0, next.1),
                Some(Tile::Floor | Tile::Door | Tile::Rubble)
            ) && seen.get(next.0, next.1) == Some(&false)
            {
                seen.set(next.0, next.1, true);
                queue.push_back(next);
            }
        }
    }
    found
}

/// The floor tile nearest the map's centre by straight-line distance; ties
/// go to the first in row-major order.
pub fn start_position(map: &Map) -> Option<(i32, i32)> {
    let (cx, cy) = (map.width / 2, map.height / 2);
    floor_candidates(map, &[])
        .into_iter()
        .min_by_key(|&(x, y)| (x - cx).pow(2) + (y - cy).pow(2))
}

/// Turns the floor tile farthest from `start` (in steps; BFS order breaks
/// ties) into the stairs down, so reaching them means crossing the floor. Draws no
/// randomness, so placing it changes no stream. Returns where it went;
/// `None` if `start` has no other floor to reach.
pub fn place_stairs_down(map: &mut Map, start: (i32, i32)) -> Option<(i32, i32)> {
    let mut seen = Grid::new(map.width, map.height, false);
    let exit = region(map, start, &mut seen)
        .last()
        .copied()
        .filter(|&at| at != start)?;
    map.set(exit.0, exit.1, Tile::StairsDown);
    Some(exit)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_stairs_down_are_the_farthest_floor_from_the_start() {
        let mut map = Grid::new(7, 3, Tile::Wall);
        for x in 1..6 {
            map.set(x, 1, Tile::Floor);
        }
        assert_eq!(place_stairs_down(&mut map, (2, 1)), Some((5, 1)));
        assert_eq!(map.get(5, 1), Some(&Tile::StairsDown));

        let mut lone = Grid::new(3, 3, Tile::Wall);
        lone.set(1, 1, Tile::Floor);
        assert_eq!(place_stairs_down(&mut lone, (1, 1)), None);
        assert_eq!(lone.get(1, 1), Some(&Tile::Floor));
    }

    /// Same values as the shipped caves, inline so retuning the data file
    /// can't break tests that only need some cave.
    pub(crate) fn test_cave() -> CaveParams {
        CaveParams {
            wall_chance: 0.45,
            iterations: 5,
            wall_birth: 5,
            wall_survival: 4,
            min_floor_fraction: 0.35,
        }
    }

    fn cave(world_seed: u64, zone: ZoneCoord) -> Map {
        generate_cave(zone_seed(world_seed, zone), &test_cave()).0
    }

    #[test]
    fn the_same_seed_gives_the_same_cave_and_stream() {
        let seed = zone_seed(42, ZoneCoord::START);
        let (map_a, rng_a) = generate_cave(seed, &test_cave());
        let (map_b, rng_b) = generate_cave(seed, &test_cave());
        assert_eq!(map_a.cells(), map_b.cells());
        assert_eq!(rng_a, rng_b);
    }

    #[test]
    fn other_zone_coordinates_give_other_caves() {
        let start = cave(42, ZoneCoord::START);
        for zone in [
            ZoneCoord { x: 1, y: 0 },
            ZoneCoord { x: 0, y: 1 },
            ZoneCoord { x: -1, y: 0 },
        ] {
            assert_ne!(start, cave(42, zone), "{zone:?}");
        }
        assert_ne!(
            zone_seed(42, ZoneCoord { x: 1, y: 2 }),
            zone_seed(42, ZoneCoord { x: 2, y: 1 })
        );
        assert_ne!(start, cave(43, ZoneCoord::START));
    }

    #[test]
    fn caves_are_walled_connected_and_big_enough() {
        let params = test_cave();
        for world_seed in 0..8 {
            let map = cave(world_seed, ZoneCoord::START);
            assert_eq!((map.width, map.height), (ZONE_WIDTH, ZONE_HEIGHT));
            for x in 0..map.width {
                assert_eq!(map.get(x, 0), Some(&Tile::Wall));
                assert_eq!(map.get(x, map.height - 1), Some(&Tile::Wall));
            }
            for y in 0..map.height {
                assert_eq!(map.get(0, y), Some(&Tile::Wall));
                assert_eq!(map.get(map.width - 1, y), Some(&Tile::Wall));
            }

            let floor = floor_candidates(&map, &[]);
            let start = start_position(&map).unwrap();
            let mut seen = Grid::new(map.width, map.height, false);
            let reachable = region(&map, start, &mut seen);
            assert_eq!(reachable.len(), floor.len(), "seed {world_seed}");
            assert!(
                floor.len() as f64
                    >= params.min_floor_fraction * f64::from(ZONE_WIDTH * ZONE_HEIGHT),
                "seed {world_seed}: {} floor tiles",
                floor.len()
            );
        }
    }

    #[test]
    fn an_unreachable_minimum_still_yields_a_connected_cave() {
        let params = CaveParams {
            min_floor_fraction: 1.0,
            ..test_cave()
        };
        let (map, _) = generate_cave(7, &params);
        let floor = floor_candidates(&map, &[]);
        let mut seen = Grid::new(map.width, map.height, false);
        assert!(!floor.is_empty());
        assert_eq!(region(&map, floor[0], &mut seen).len(), floor.len());
    }

    #[test]
    fn a_zone_that_fills_solid_still_has_a_floor_tile() {
        let params = CaveParams {
            wall_chance: 1.0,
            ..test_cave()
        };
        let (map, _) = generate_cave(7, &params);
        assert_eq!(
            floor_candidates(&map, &[]),
            [(ZONE_WIDTH / 2, ZONE_HEIGHT / 2)]
        );
    }

    #[test]
    fn start_ties_break_to_the_first_in_row_major_order() {
        let mut map = Grid::new(5, 5, Tile::Wall);
        for (x, y) in [(2, 1), (1, 2), (3, 2), (2, 3)] {
            map.set(x, y, Tile::Floor);
        }
        assert_eq!(start_position(&map), Some((2, 1)));
    }

    #[test]
    fn out_of_range_parameters_are_refused() {
        let mut params = test_cave();
        assert_eq!(params.check(), Ok(()));
        params.wall_chance = 1.5;
        assert!(params.check().is_err());
        params = test_cave();
        params.wall_birth = 9;
        assert!(params.check().is_err());
    }
}
