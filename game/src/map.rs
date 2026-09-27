//! Tiles, the generic `Grid<T>` (out-of-bounds reads are `None`), and map generation.

use rand::RngExt;
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};

pub const MAP_WIDTH: i32 = 40;
pub const MAP_HEIGHT: i32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tile {
    Floor,
    Wall,
}

pub type Map = Grid<Tile>;

#[derive(Debug, Clone, PartialEq)]
pub struct Grid<T> {
    pub width: i32,
    pub height: i32,
    cells: Vec<T>,
}

impl<T: Clone> Grid<T> {
    pub fn new(width: i32, height: i32, fill: T) -> Self {
        Self {
            width,
            height,
            cells: vec![fill; (width * height) as usize],
        }
    }
}

impl<T> Grid<T> {
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            None
        } else {
            Some((y * self.width + x) as usize)
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<&T> {
        self.index(x, y).map(|i| &self.cells[i])
    }

    pub fn get_mut(&mut self, x: i32, y: i32) -> Option<&mut T> {
        self.index(x, y).map(|i| &mut self.cells[i])
    }

    pub fn set(&mut self, x: i32, y: i32, value: T) {
        if let Some(i) = self.index(x, y) {
            self.cells[i] = value;
        }
    }

    /// Row-major, for saving.
    pub(crate) fn cells(&self) -> &[T] {
        &self.cells
    }

    /// `None` unless `cells` holds exactly `width * height` values.
    pub(crate) fn from_cells(width: i32, height: i32, cells: Vec<T>) -> Option<Self> {
        let len = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?;
        (cells.len() == len).then_some(Self {
            width,
            height,
            cells,
        })
    }
}

/// Builds the (currently fixed) starting room: a bordered rectangle with a
/// short interior wall segment. The layout doesn't vary by seed yet — real
/// procedural generation is out of scope for this skeleton — but it already
/// takes the map stream (`Rngs::map`), the only RNG map generation may use.
pub fn generate_map(_rng: &mut Pcg64) -> Map {
    let mut grid = Grid::new(MAP_WIDTH, MAP_HEIGHT, Tile::Floor);

    for x in 0..MAP_WIDTH {
        grid.set(x, 0, Tile::Wall);
        grid.set(x, MAP_HEIGHT - 1, Tile::Wall);
    }
    for y in 0..MAP_HEIGHT {
        grid.set(0, y, Tile::Wall);
        grid.set(MAP_WIDTH - 1, y, Tile::Wall);
    }
    for y in 3..6 {
        grid.set(8, y, Tile::Wall);
    }

    grid
}

/// Floor tiles not listed in `occupied`, in row-major order.
pub fn floor_candidates(grid: &Map, occupied: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut candidates = Vec::new();
    for y in 0..grid.height {
        for x in 0..grid.width {
            if grid.get(x, y) == Some(&Tile::Floor) && !occupied.contains(&(x, y)) {
                candidates.push((x, y));
            }
        }
    }
    candidates
}

/// A uniformly random free floor tile (one draw from `rng`), or `None` if
/// every floor tile is occupied.
pub fn choose_spawn_position(
    grid: &Map,
    occupied: &[(i32, i32)],
    rng: &mut Pcg64,
) -> Option<(i32, i32)> {
    let candidates = floor_candidates(grid, occupied);
    if candidates.is_empty() {
        return None;
    }
    Some(candidates[rng.random_range(0..candidates.len())])
}

/// A fresh, non-deterministic seed for when the player doesn't pass `--seed`.
#[expect(
    clippy::disallowed_methods,
    reason = "the one sanctioned entropy source: picks the seed, never used mid-simulation"
)]
pub fn random_seed() -> u64 {
    rand::rng().random()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rngs;

    #[test]
    fn grid_get_set_respects_bounds() {
        let mut grid = Grid::new(3, 2, Tile::Floor);
        grid.set(2, 1, Tile::Wall);
        assert_eq!(grid.get(2, 1), Some(&Tile::Wall));
        assert_eq!(grid.get(0, 0), Some(&Tile::Floor));
        assert_eq!(grid.get(3, 0), None);
        assert_eq!(grid.get(-1, 0), None);
    }

    #[test]
    fn same_seed_produces_same_map() {
        let grid_a = generate_map(&mut Rngs::new(42).map);
        let grid_b = generate_map(&mut Rngs::new(42).map);
        assert_eq!(grid_a, grid_b);
    }

    #[test]
    fn same_seed_produces_same_spawn_position() {
        let grid = generate_map(&mut Rngs::new(7).map);
        let pos_a = choose_spawn_position(&grid, &[], &mut Rngs::new(7).spawn);
        let pos_b = choose_spawn_position(&grid, &[], &mut Rngs::new(7).spawn);
        assert_eq!(pos_a, pos_b);
    }

    #[test]
    fn spawn_positions_are_free_floor_tiles() {
        let grid = generate_map(&mut Rngs::new(1).map);
        let mut occupied = vec![(1, 1)];
        let mut rng = Rngs::new(1).spawn;
        while let Some(pos) = choose_spawn_position(&grid, &occupied, &mut rng) {
            assert_eq!(grid.get(pos.0, pos.1), Some(&Tile::Floor));
            assert!(!occupied.contains(&pos));
            occupied.push(pos);
        }
        assert_eq!(occupied.len() - 1, floor_candidates(&grid, &[(1, 1)]).len());
    }
}
