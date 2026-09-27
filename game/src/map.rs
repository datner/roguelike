use rand::{RngExt, SeedableRng};
use rand_pcg::Pcg64;

pub const MAP_WIDTH: i32 = 20;
pub const MAP_HEIGHT: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Floor,
    Wall,
}

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
}

/// Builds the (currently fixed) starting room: a bordered rectangle with a
/// short interior wall segment. The layout doesn't vary by seed yet — real
/// procedural generation is out of scope for this skeleton — but the
/// function still returns a seeded `Pcg64` so callers (and tests) can
/// treat map setup as seed-driven from the start.
pub fn generate_map(seed: u64) -> (Grid<Tile>, Pcg64) {
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

    let rng = Pcg64::seed_from_u64(seed);
    (grid, rng)
}

/// Interior floor tiles that are safe to place a spawned entity on,
/// excluding the player's fixed starting tile.
pub fn floor_candidates(grid: &Grid<Tile>) -> Vec<(i32, i32)> {
    let mut candidates = Vec::new();
    for y in 1..grid.height - 1 {
        for x in 1..grid.width - 1 {
            if grid.get(x, y) != Some(&Tile::Floor) {
                continue;
            }
            if (x, y) == (1, 1) {
                continue;
            }
            candidates.push((x, y));
        }
    }
    candidates
}

pub fn choose_obstacle_position(grid: &Grid<Tile>, rng: &mut Pcg64) -> (i32, i32) {
    let candidates = floor_candidates(grid);
    let idx = rng.random_range(0..candidates.len());
    candidates[idx]
}

/// A fresh, non-deterministic seed for when the player doesn't pass `--seed`.
pub fn random_seed() -> u64 {
    rand::rng().random()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (grid_a, _) = generate_map(42);
        let (grid_b, _) = generate_map(42);
        assert_eq!(grid_a, grid_b);
    }

    #[test]
    fn same_seed_produces_same_obstacle_position() {
        let (grid, mut rng_a) = generate_map(7);
        let (_, mut rng_b) = generate_map(7);
        let pos_a = choose_obstacle_position(&grid, &mut rng_a);
        let pos_b = choose_obstacle_position(&grid, &mut rng_b);
        assert_eq!(pos_a, pos_b);
    }
}
