//! Tiles, the generic `Grid<T>` (out-of-bounds reads are `None`), and spawn placement.

use rand::RngExt;
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tile {
    Floor,
    Wall,
    /// Down to the next floor (`>`); where auto-explore heads once the floor
    /// holds nothing else to find.
    StairsDown,
    /// Where you arrived on this floor (`<`).
    StairsUp,
    /// A closed door (`+`): blocks walking and sight until someone walks into
    /// it, which opens it (docs/plans/fights.md).
    Door,
    /// An open door (`'`): like floor until closed again.
    DoorOpen,
    /// A drop to the floor below (`:`): seen across, never walked. A
    /// creature forced into one falls (docs/plans/fights.md).
    Chasm,
    /// Tall grass (`"`): walked through, but it blocks sight past itself and
    /// hides what stands in it beyond the next tile. Fire burns it to floor.
    Grass,
    /// Rubble (`,`): walked, seen and shot across, but a step onto it costs
    /// double (docs/plans/fights-2.md).
    Rubble,
    /// Deep water (`~`): seen and shot across, never walked. A creature
    /// forced into it drowns; the player hauls out, a turn lost
    /// (docs/plans/fights-2.md).
    DeepWater,
}

impl Tile {
    /// Can be stood on and seen through. Off-map (`None` from `Grid::get`)
    /// counts as wall, so callers pass `map.get(..)` straight in.
    pub fn is_open(tile: Option<&Tile>) -> bool {
        matches!(
            tile,
            Some(
                Tile::Floor
                    | Tile::StairsDown
                    | Tile::StairsUp
                    | Tile::DoorOpen
                    | Tile::Grass
                    | Tile::Rubble
            )
        )
    }

    /// What a missile or a spreading area crosses: open ground, grass, or the
    /// air over a chasm or deep water.
    pub fn is_clear(tile: Option<&Tile>) -> bool {
        Tile::is_open(tile) || Tile::is_drop(tile)
    }

    /// Never walked, but a shove can force a creature in: a chasm or deep
    /// water.
    pub fn is_drop(tile: Option<&Tile>) -> bool {
        matches!(tile, Some(Tile::Chasm | Tile::DeepWater))
    }

    /// Seen through: clear ground but grass, which hides what is past it.
    pub fn is_transparent(tile: Option<&Tile>) -> bool {
        Tile::is_clear(tile) && tile != Some(&Tile::Grass)
    }

    /// A way to go: open, or a closed door that walking into opens. For
    /// planning a path; sight and missiles keep to `is_open`.
    pub fn is_way(tile: Option<&Tile>) -> bool {
        Tile::is_open(tile) || tile == Some(&Tile::Door)
    }
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

/// A uniformly random free floor tile at least `min_distance` steps
/// (Chebyshev, as things move) from `away_from`, using one draw from `rng`;
/// `None` if there is no such tile.
pub fn choose_spawn_position(
    grid: &Map,
    occupied: &[(i32, i32)],
    away_from: (i32, i32),
    min_distance: i32,
    rng: &mut Pcg64,
) -> Option<(i32, i32)> {
    let candidates: Vec<(i32, i32)> = floor_candidates(grid, occupied)
        .into_iter()
        .filter(|&(x, y)| (x - away_from.0).abs().max((y - away_from.1).abs()) >= min_distance)
        .collect();
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
pub(crate) mod tests {
    use super::*;
    use crate::rng::Rngs;

    /// A hand-built 40×20 room: walled border and a short interior wall at
    /// x = 8, y = 3..6. Tests that aren't about generation use this, so
    /// generator tuning can't break them.
    pub(crate) fn room() -> Map {
        let (width, height) = (40, 20);
        let mut grid = Grid::new(width, height, Tile::Floor);
        for x in 0..width {
            grid.set(x, 0, Tile::Wall);
            grid.set(x, height - 1, Tile::Wall);
        }
        for y in 0..height {
            grid.set(0, y, Tile::Wall);
            grid.set(width - 1, y, Tile::Wall);
        }
        for y in 3..6 {
            grid.set(8, y, Tile::Wall);
        }
        grid
    }

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
    fn same_seed_produces_same_spawn_position() {
        let grid = room();
        let pos_a = choose_spawn_position(&grid, &[], (1, 1), 0, &mut Rngs::new(7, 7).spawn);
        let pos_b = choose_spawn_position(&grid, &[], (1, 1), 0, &mut Rngs::new(7, 7).spawn);
        assert_eq!(pos_a, pos_b);
    }

    #[test]
    fn spawn_positions_are_free_floor_tiles_far_enough_away() {
        let grid = room();
        let mut occupied = vec![(1, 1)];
        let mut rng = Rngs::new(1, 1).spawn;
        while let Some(pos) = choose_spawn_position(&grid, &occupied, (1, 1), 10, &mut rng) {
            assert_eq!(grid.get(pos.0, pos.1), Some(&Tile::Floor));
            assert!(!occupied.contains(&pos));
            assert!(pos.0 >= 11 || pos.1 >= 11, "{pos:?} is too close");
            occupied.push(pos);
        }
        let far = floor_candidates(&grid, &[])
            .into_iter()
            .filter(|&(x, y)| x >= 11 || y >= 11)
            .count();
        assert_eq!(occupied.len() - 1, far);
    }
}
