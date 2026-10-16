//! The player's field of view: Albert Ford's symmetric shadowcasting
//! (ADR 0007). Symmetry — A sees B iff B sees A, between floor tiles — is
//! what lets `ai::can_see_player` reuse the player's view instead of
//! computing one per actor.

use std::ops::RangeInclusive;

use crate::entity::Position;
use crate::map::{Grid, Map, Tile};
use crate::state::GameState;

/// Sight radius in tiles. A tile is in range when its squared distance from
/// the viewer is at most `RADIUS²`, so the view is a circle, not a square.
pub const RADIUS: i32 = 8;

/// Recomputes `state.visible` from the player's position and adds it to
/// `state.seen`. A player without a position sees nothing.
pub(crate) fn update(state: &mut GameState) {
    let origin = state
        .world
        .get::<&Position>(state.player)
        .ok()
        .map(|pos| (pos.x, pos.y));
    state.visible = match origin {
        Some(origin) => compute(&state.map, origin, RADIUS),
        None => Grid::new(state.map.width, state.map.height, false),
    };
    for y in 0..state.map.height {
        for x in 0..state.map.width {
            if state.visible.get(x, y) == Some(&true) {
                state.seen.set(x, y, true);
            }
        }
    }
}

/// Which tiles of `map` are visible from `origin` within `radius`. Anything
/// but floor is opaque (off-map included); opaque tiles bounding the view
/// are themselves visible.
pub fn compute(map: &Map, origin: (i32, i32), radius: i32) -> Grid<bool> {
    let mut visible = Grid::new(map.width, map.height, false);
    visible.set(origin.0, origin.1, true);
    for quadrant in Quadrant::ALL {
        let mut scan = Scan {
            map,
            origin,
            radius,
            quadrant,
            visible: &mut visible,
        };
        scan.row(Row {
            depth: 1,
            start: Slope { num: -1, den: 1 },
            end: Slope { num: 1, den: 1 },
        });
    }
    visible
}

/// A rational slope `num / den` (col over depth), `den > 0`. Kept exact so
/// visibility never depends on float rounding.
#[derive(Debug, Clone, Copy)]
struct Slope {
    num: i32,
    den: i32,
}

impl Slope {
    /// Through the near corner of the tile at (`depth`, `col`) on the
    /// low-column side.
    fn at_tile_edge(depth: i32, col: i32) -> Self {
        Self {
            num: 2 * col - 1,
            den: 2 * depth,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Row {
    depth: i32,
    start: Slope,
    end: Slope,
}

impl Row {
    /// `round_ties_up(depth × start) ..= round_ties_down(depth × end)`.
    fn cols(&self) -> RangeInclusive<i32> {
        let (d, s, e) = (self.depth, self.start, self.end);
        let min = (2 * d * s.num + s.den).div_euclid(2 * s.den);
        let max = -((e.den - 2 * d * e.num).div_euclid(2 * e.den));
        min..=max
    }

    /// Whether the tile's centre lies inside the row's sector. Floor tiles
    /// are revealed only then; that rule is what makes the result symmetric.
    fn is_symmetric(&self, col: i32) -> bool {
        col * self.start.den >= self.depth * self.start.num
            && col * self.end.den <= self.depth * self.end.num
    }

    fn next(self) -> Self {
        Self {
            depth: self.depth + 1,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Quadrant {
    North,
    East,
    South,
    West,
}

impl Quadrant {
    const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    fn to_map(self, (ox, oy): (i32, i32), depth: i32, col: i32) -> (i32, i32) {
        match self {
            Self::North => (ox + col, oy - depth),
            Self::South => (ox + col, oy + depth),
            Self::East => (ox + depth, oy + col),
            Self::West => (ox - depth, oy + col),
        }
    }
}

struct Scan<'a> {
    map: &'a Map,
    origin: (i32, i32),
    radius: i32,
    quadrant: Quadrant,
    visible: &'a mut Grid<bool>,
}

impl Scan<'_> {
    /// Recursion depth is bounded by `radius`: rows past it are never
    /// scanned, and every tile in them would be out of range anyway.
    fn row(&mut self, mut row: Row) {
        if row.depth > self.radius {
            return;
        }
        let mut prev_wall = None;
        for col in row.cols() {
            let (x, y) = self.quadrant.to_map(self.origin, row.depth, col);
            let wall = !matches!(self.map.get(x, y), Some(Tile::Floor));
            let in_range = row.depth * row.depth + col * col <= self.radius * self.radius;
            if in_range && (wall || row.is_symmetric(col)) {
                self.visible.set(x, y, true);
            }
            if prev_wall == Some(true) && !wall {
                row.start = Slope::at_tile_edge(row.depth, col);
            }
            if prev_wall == Some(false) && wall {
                let mut next = row.next();
                next.end = Slope::at_tile_edge(row.depth, col);
                self.row(next);
            }
            prev_wall = Some(wall);
        }
        if prev_wall == Some(false) {
            self.row(row.next());
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::RngExt;

    use super::*;
    use crate::map::floor_candidates;
    use crate::map::tests::room;
    use crate::rng::Rngs;

    fn sees(view: &Grid<bool>, (x, y): (i32, i32)) -> bool {
        view.get(x, y) == Some(&true)
    }

    /// The test room with interior walls scattered from the seed's map
    /// stream, so each seed gives different shadows.
    fn scattered_map(seed: u64) -> Map {
        let mut rng = Rngs::new(seed, seed).map;
        let mut map = room();
        for _ in 0..80 {
            let x = rng.random_range(1..map.width - 1);
            let y = rng.random_range(1..map.height - 1);
            map.set(x, y, Tile::Wall);
        }
        map
    }

    #[test]
    fn visibility_between_floor_tiles_is_symmetric() {
        for seed in 0..6 {
            let map = scattered_map(seed);
            let floors = floor_candidates(&map, &[]);
            let views: Vec<_> = floors.iter().map(|&at| compute(&map, at, RADIUS)).collect();
            let mut hidden_pairs = 0;
            for (i, &a) in floors.iter().enumerate() {
                for (j, &b) in floors.iter().enumerate().skip(i + 1) {
                    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
                    if dx * dx + dy * dy > RADIUS * RADIUS {
                        continue;
                    }
                    let (a_sees_b, b_sees_a) = (sees(&views[i], b), sees(&views[j], a));
                    assert_eq!(a_sees_b, b_sees_a, "seed {seed}: {a:?} <-> {b:?}");
                    hidden_pairs += usize::from(!a_sees_b);
                }
            }
            assert!(hidden_pairs > 0, "seed {seed}: the walls shadow something");
        }
    }

    /// An 11×11 walled room, viewer in the middle, one pillar two tiles
    /// north of it.
    fn pillar_room() -> (Map, (i32, i32)) {
        let mut map = Grid::new(11, 11, Tile::Floor);
        for i in 0..11 {
            for (x, y) in [(i, 0), (i, 10), (0, i), (10, i)] {
                map.set(x, y, Tile::Wall);
            }
        }
        map.set(5, 3, Tile::Wall);
        (map, (5, 5))
    }

    #[test]
    fn a_pillar_shadows_the_tiles_behind_it() {
        let (map, origin) = pillar_room();
        let view = compute(&map, origin, RADIUS);
        assert!(sees(&view, (5, 4)));
        assert!(sees(&view, (5, 3)), "the pillar itself");
        for behind in [(5, 2), (5, 1), (5, 0)] {
            assert!(!sees(&view, behind), "{behind:?}");
        }
        assert!(sees(&view, (4, 1)), "beside the shadow");
    }

    #[test]
    fn walls_bounding_the_view_are_visible() {
        let (map, origin) = pillar_room();
        let view = compute(&map, origin, RADIUS);
        for wall in [(0, 5), (10, 5), (5, 10), (0, 0), (10, 10), (2, 0)] {
            assert!(sees(&view, wall), "{wall:?}");
        }
        assert!(!sees(&view, (-1, 5)), "nothing off the map");
    }

    #[test]
    fn the_sight_radius_is_circular() {
        let map = Grid::new(30, 30, Tile::Floor);
        let view = compute(&map, (10, 10), RADIUS);
        assert!(sees(&view, (10 + RADIUS, 10)));
        assert!(!sees(&view, (11 + RADIUS, 10)));
        // Chebyshev 6, but 6² + 6² > 8².
        assert!(!sees(&view, (16, 16)));
        assert!(sees(&view, (15, 15)));
    }
}
