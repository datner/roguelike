//! Rooms and corridors: a floor of rectangular rooms, each joined by a short L-shaped corridor to the nearest room already joined, so every room is reachable. Built halls (a castle) beside the caves' grown ones (`zone::generate_cave`); a dungeon's data picks which (`dungeon::Generator`).

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
    /// The chance, in percent, of a door in each doorway where a corridor
    /// meets a room. 0 draws nothing, so a dungeon without doors keeps its
    /// layouts.
    #[serde(default)]
    pub doors: u32,
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
        rooms.push(room);
    }
    connect(&mut map, &rooms, &mut rng);
    if params.doors > 0 {
        hang_doors(&mut map, &rooms, params.doors, &mut rng);
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

/// Joins every room to the rest, nearest first (a spanning tree grown from
/// the first room): each time, the unjoined room nearest a joined one digs
/// an L-shaped corridor toward it, and stops at the first joined floor it
/// reaches. So corridors stay short and never run beside one another to the
/// same place.
fn connect(map: &mut Map, rooms: &[Room], rng: &mut Pcg64) {
    let mut joined = Grid::new(map.width, map.height, false);
    let mut done = vec![false; rooms.len()];
    let join = |joined: &mut Grid<bool>, room: Room| {
        for y in room.y..room.y + room.h {
            for x in room.x..room.x + room.w {
                joined.set(x, y, true);
            }
        }
    };
    let Some(&first) = rooms.first() else {
        return;
    };
    join(&mut joined, first);
    done[0] = true;
    let distance = |a: Room, b: Room| {
        let ((ax, ay), (bx, by)) = (a.centre(), b.centre());
        (ax - bx).pow(2) + (ay - by).pow(2)
    };
    while let Some((next, to)) = (0..rooms.len())
        .filter(|&i| !done[i])
        .flat_map(|i| (0..rooms.len()).filter(|&j| done[j]).map(move |j| (i, j)))
        .min_by_key(|&(i, j)| (distance(rooms[i], rooms[j]), i, j))
    {
        let ((ax, ay), (bx, by)) = (rooms[next].centre(), rooms[to].centre());
        // Horizontal first or vertical first, so corridors vary.
        let corner = if rng.random_bool(0.5) {
            (bx, ay)
        } else {
            (ax, by)
        };
        let (mut x, mut y) = (ax, ay);
        'dig: for target in [corner, (bx, by)] {
            while (x, y) != target {
                x += (target.0 - x).signum();
                y += (target.1 - y).signum();
                if joined.get(x, y) == Some(&true) {
                    break 'dig;
                }
                carve(map, (x, y));
                joined.set(x, y, true);
            }
        }
        join(&mut joined, rooms[next]);
        done[next] = true;
    }
}

/// A door, at `percent` chance each, in every doorway: a one-tile gap in a
/// room's surrounding wall, with wall on both sides along it, and no door
/// already within two tiles. Rooms in the
/// order placed, each ring clockwise from its top-left corner.
fn hang_doors(map: &mut Map, rooms: &[Room], percent: u32, rng: &mut Pcg64) {
    let wall = |map: &Map, x, y| map.get(x, y) == Some(&Tile::Wall);
    for room in rooms {
        let (left, top, right, bottom) = (room.x - 1, room.y - 1, room.x + room.w, room.y + room.h);
        let ring = (left + 1..right)
            .map(|x| (x, top, true))
            .chain((top + 1..bottom).map(|y| (right, y, false)))
            .chain((left + 1..right).rev().map(|x| (x, bottom, true)))
            .chain((top + 1..bottom).rev().map(|y| (left, y, false)));
        for (x, y, across) in ring.collect::<Vec<_>>() {
            let doorway = map.get(x, y) == Some(&Tile::Floor)
                && if across {
                    wall(map, x - 1, y) && wall(map, x + 1, y)
                } else {
                    wall(map, x, y - 1) && wall(map, x, y + 1)
                };
            // One door to a passage: two rooms a step apart would hang one each.
            let near_door = (-2..=2)
                .flat_map(|dy| (-2..=2).map(move |dx| (x + dx, y + dy)))
                .any(|(nx, ny)| map.get(nx, ny) == Some(&Tile::Door));
            if doorway && !near_door && rng.random_range(0..100) < percent {
                map.set(x, y, Tile::Door);
            }
        }
    }
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
            doors: 0,
        }
    }

    #[test]
    fn doors_hang_only_in_doorways_and_only_when_asked() {
        let plain = generate_rooms(7, &test_rooms());
        let doors = |map: &Map| {
            (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| map.get(x, y) == Some(&Tile::Door))
                .collect::<Vec<_>>()
        };
        assert!(doors(&plain.0).is_empty());
        let with = generate_rooms(
            7,
            &RoomsParams {
                doors: 100,
                ..test_rooms()
            },
        );
        let hung = doors(&with.0);
        assert!(!hung.is_empty());
        for (x, y) in hung {
            let wall = |x, y| with.0.get(x, y) == Some(&Tile::Wall);
            assert!(
                (wall(x - 1, y) && wall(x + 1, y)) || (wall(x, y - 1) && wall(x, y + 1)),
                "a door at ({x}, {y}) between two walls"
            );
        }
        // Every tile but the doors is laid out the same.
        let mut opened = with.0.clone();
        for y in 0..opened.height {
            for x in 0..opened.width {
                if opened.get(x, y) == Some(&Tile::Door) {
                    opened.set(x, y, Tile::Floor);
                }
            }
        }
        assert_eq!(opened, plain.0);
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
    fn no_door_stands_just_behind_another() {
        for seed in 0..10 {
            let params = RoomsParams {
                doors: 100,
                ..test_rooms()
            };
            let (map, ..) = generate_rooms(seed, &params);
            let doors: Vec<(i32, i32)> = (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| map.get(x, y) == Some(&Tile::Door))
                .collect();
            for &(ax, ay) in &doors {
                for &(bx, by) in &doors {
                    let apart = (ax - bx).abs().max((ay - by).abs());
                    assert!(
                        apart == 0 || apart > 2,
                        "seed {seed}: ({ax}, {ay}) and ({bx}, {by})"
                    );
                }
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
