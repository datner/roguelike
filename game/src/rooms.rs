//! Rooms and corridors: a floor of rectangular rooms, each joined to the one placed before it by an L-shaped corridor, so every room is reachable. Built halls (a castle) beside the caves' grown ones (`zone::generate_cave`); a dungeon's data picks which (`dungeon::Generator`).

use rand::RngExt;
use rand_pcg::Pcg64;
use serde::Deserialize;

use crate::map::{Grid, Map, Tile};
use crate::rng::stream;
use crate::zone::{ZONE_HEIGHT, ZONE_WIDTH, mix};

/// Tuning for `generate_rooms`, part of a dungeon's data.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomsParams {
    /// Tries at placing a room; one that would overlap another is skipped.
    pub attempts: u32,
    /// A room's inside, in tiles, each way.
    pub min_size: i32,
    pub max_size: i32,
}

impl RoomsParams {
    /// Whether these parameters can lay out a floor, and why not.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.attempts == 0 {
            return Err("attempts must be at least 1");
        }
        if self.min_size < 2 || self.max_size < self.min_size {
            return Err("room sizes must be at least 2, max_size no less than min_size");
        }
        if self.max_size + 2 > ZONE_HEIGHT {
            return Err("max_size must leave room for walls");
        }
        Ok(())
    }
}

/// An axis-aligned room's inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Room {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Room {
    fn centre(self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// Overlapping, or touching: rooms keep a wall between them.
    fn crowds(self, other: Room) -> bool {
        self.x <= other.x + other.w
            && other.x <= self.x + self.w
            && self.y <= other.y + other.h
            && other.y <= self.y + self.h
    }
}

fn carve(map: &mut Map, (x, y): (i32, i32)) {
    if x > 0 && y > 0 && x < map.width - 1 && y < map.height - 1 {
        map.set(x, y, Tile::Floor);
    }
}

/// A `ZONE_WIDTH` × `ZONE_HEIGHT` floor of rooms. Draws only from
/// `Pcg64::new(mix(zone_seed, 0), stream::MAP)`, like the cave's first
/// attempt, and returns that stream to become `Rngs::map`, and the centre
/// of the room nearest the middle: arriving in a room, not a corridor.
/// Always at least one room.
pub fn generate_rooms(zone_seed: u64, params: &RoomsParams) -> (Map, Pcg64, (i32, i32)) {
    let mut rng = Pcg64::new(u128::from(mix(zone_seed, 0)), stream::MAP);
    let mut map = Grid::new(ZONE_WIDTH, ZONE_HEIGHT, Tile::Wall);
    let mut rooms: Vec<Room> = Vec::new();
    for _ in 0..params.attempts {
        let w = rng.random_range(params.min_size..=params.max_size);
        let h = rng.random_range(params.min_size..=params.max_size);
        let x = rng.random_range(1..=(ZONE_WIDTH - w - 1).max(1));
        let y = rng.random_range(1..=(ZONE_HEIGHT - h - 1).max(1));
        let room = Room { x, y, w, h };
        if rooms.iter().any(|other| room.crowds(*other)) {
            continue;
        }
        for ry in room.y..room.y + room.h {
            for rx in room.x..room.x + room.w {
                carve(&mut map, (rx, ry));
            }
        }
        if let Some(previous) = rooms.last() {
            // Horizontal first or vertical first, so corridors vary.
            let ((ax, ay), (bx, by)) = (previous.centre(), room.centre());
            let corner = if rng.random_bool(0.5) {
                (bx, ay)
            } else {
                (ax, by)
            };
            for (from, to) in [((ax, ay), corner), (corner, (bx, by))] {
                let (mut x, mut y) = from;
                carve(&mut map, (x, y));
                while (x, y) != to {
                    x += (to.0 - x).signum();
                    y += (to.1 - y).signum();
                    carve(&mut map, (x, y));
                }
            }
        }
        rooms.push(room);
    }
    let (cx, cy) = (ZONE_WIDTH / 2, ZONE_HEIGHT / 2);
    if rooms.is_empty() {
        // Only with no attempts at all, which `check` refuses.
        for y in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                carve(&mut map, (x, y));
            }
        }
    }
    let arrival = rooms
        .iter()
        .map(|room| room.centre())
        .min_by_key(|&(x, y)| (x - cx).pow(2) + (y - cy).pow(2))
        .unwrap_or((cx, cy));
    (map, rng, arrival)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::zone::region;

    pub(crate) fn test_rooms() -> RoomsParams {
        RoomsParams {
            attempts: 60,
            min_size: 4,
            max_size: 10,
        }
    }

    #[test]
    fn the_same_seed_lays_out_the_same_floor() {
        assert_eq!(
            generate_rooms(5, &test_rooms()).0,
            generate_rooms(5, &test_rooms()).0
        );
        assert_ne!(
            generate_rooms(5, &test_rooms()).0,
            generate_rooms(6, &test_rooms()).0
        );
    }

    #[test]
    fn every_room_is_reachable_and_the_border_is_wall() {
        for seed in 0..10 {
            let (map, _, start) = generate_rooms(seed, &test_rooms());
            assert_eq!(map.get(start.0, start.1), Some(&Tile::Floor), "seed {seed}");
            let reached = region(&map, start, &mut Grid::new(map.width, map.height, false)).len();
            let floor = (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| map.get(x, y) == Some(&Tile::Floor))
                .count();
            assert_eq!(reached, floor, "seed {seed}: every floor tile connected");
            assert!(floor > 100, "seed {seed}: more than one small room");
            for x in 0..map.width {
                assert_eq!(map.get(x, 0), Some(&Tile::Wall));
                assert_eq!(map.get(x, map.height - 1), Some(&Tile::Wall));
            }
        }
    }

    #[test]
    fn bad_tuning_is_refused() {
        assert_eq!(test_rooms().check(), Ok(()));
        let mut params = test_rooms();
        params.min_size = 1;
        assert!(params.check().is_err());
        params = test_rooms();
        params.max_size = 3;
        assert!(params.check().is_err());
    }
}
