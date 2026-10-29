//! Which tiles a spell's `Shape` covers (ADR 0014): pure functions of the
//! map, the caster's tile and the aimed tile, so a cast and its preview cover
//! the same ground. Walls block. Tiles come nearest the centre first (ties by
//! row, then column), which is also the order an area is animated in.

use crate::entity::Shape;
use crate::fov;
use crate::map::{Map, Tile};
use crate::ranged::{distance, line};

/// The tiles `shape` covers, cast from `caster` at `aim`, reaching `range`
/// (cones and beams). For a burst, `aim` is where the missile landed: the
/// flight is the caller's, so it can stop at a creature.
pub(crate) fn tiles(
    map: &Map,
    shape: Shape,
    caster: (i32, i32),
    aim: (i32, i32),
    range: i32,
) -> Vec<(i32, i32)> {
    let mut tiles = match shape {
        Shape::Burst { radius } => ball(map, aim, radius),
        Shape::Nova { radius } => ball(map, caster, radius),
        Shape::Cone => cone(map, caster, aim, range),
        Shape::Beam => beam(map, caster, aim, range),
    };
    let from = centre(shape, caster, aim);
    tiles.sort_by_key(|&(x, y)| (squared(from, (x, y)), y, x));
    tiles
}

/// Where `shape` is measured from: falloff counts tiles from here.
pub(crate) fn centre(shape: Shape, caster: (i32, i32), aim: (i32, i32)) -> (i32, i32) {
    match shape {
        Shape::Burst { .. } => aim,
        Shape::Nova { .. } | Shape::Cone | Shape::Beam => caster,
    }
}

fn squared(from: (i32, i32), to: (i32, i32)) -> i32 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    dx * dx + dy * dy
}

/// Within `radius` of `from` on a circle rounded outward by half a tile, so
/// radius 1 is the 3×3 block and larger radii are round.
fn within(from: (i32, i32), to: (i32, i32), radius: i32) -> bool {
    squared(from, to) <= radius * radius + radius
}

/// Open tiles around `from`, within `radius`, that `from` can see: the
/// symmetric shadowcast of `fov`, so an area never reaches round a corner.
fn seen_around(map: &Map, from: (i32, i32), radius: i32) -> Vec<(i32, i32)> {
    // One ring further, since `fov`'s circle is the unrounded one.
    let seen = fov::compute(map, from, radius + 1);
    let mut tiles = Vec::new();
    for y in from.1 - radius..=from.1 + radius {
        for x in from.0 - radius..=from.0 + radius {
            if within(from, (x, y), radius)
                && seen.get(x, y).copied().unwrap_or(false)
                && Tile::is_transparent(map.get(x, y))
            {
                tiles.push((x, y));
            }
        }
    }
    tiles
}

fn ball(map: &Map, centre: (i32, i32), radius: i32) -> Vec<(i32, i32)> {
    seen_around(map, centre, radius.max(0))
}

/// Tiles in view of the caster, out to `range`, no more than 45° off the line
/// toward `aim`. Integer test: the angle's cosine squared is at least ½.
fn cone(map: &Map, caster: (i32, i32), aim: (i32, i32), range: i32) -> Vec<(i32, i32)> {
    let toward = (i64::from(aim.0 - caster.0), i64::from(aim.1 - caster.1));
    if toward == (0, 0) {
        return Vec::new();
    }
    let length = toward.0 * toward.0 + toward.1 * toward.1;
    seen_around(map, caster, range.max(0))
        .into_iter()
        .filter(|&tile| {
            let v = (i64::from(tile.0 - caster.0), i64::from(tile.1 - caster.1));
            let dot = v.0 * toward.0 + v.1 * toward.1;
            dot > 0 && 2 * dot * dot >= (v.0 * v.0 + v.1 * v.1) * length
        })
        .collect()
}

/// The line from the caster through `aim`, carried on to `range` tiles,
/// stopping before the first wall.
fn beam(map: &Map, caster: (i32, i32), aim: (i32, i32), range: i32) -> Vec<(i32, i32)> {
    let step = distance(caster, aim);
    if step == 0 {
        return Vec::new();
    }
    // Far enough along the same line that `range` tiles fit before it.
    let reach = range.max(0) / step + 1;
    let far = (
        caster.0 + (aim.0 - caster.0) * reach,
        caster.1 + (aim.1 - caster.1) * reach,
    );
    line(caster, far)
        .into_iter()
        .take(range.max(0) as usize)
        .take_while(|&(x, y)| Tile::is_transparent(map.get(x, y)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tests::room;

    #[test]
    fn a_radius_one_burst_is_the_block_around_it_centre_first() {
        let tiles = tiles(&room(), Shape::Burst { radius: 1 }, (1, 1), (20, 10), 6);
        assert_eq!(tiles.len(), 9);
        assert_eq!(tiles[0], (20, 10));
        assert_eq!(&tiles[1..5], [(20, 9), (19, 10), (21, 10), (20, 11)]);
    }

    #[test]
    fn larger_bursts_are_round() {
        let tiles = tiles(&room(), Shape::Burst { radius: 2 }, (1, 1), (20, 10), 6);
        assert!(tiles.contains(&(22, 11)));
        assert!(!tiles.contains(&(22, 12)), "corners are cut");
        assert_eq!(tiles.len(), 21);
    }

    #[test]
    fn walls_stop_a_burst_and_are_never_in_it() {
        // The room's wall runs down x = 8 from y = 3 to 5.
        let tiles = tiles(&room(), Shape::Burst { radius: 2 }, (1, 1), (7, 4), 6);
        assert!(tiles.contains(&(6, 4)));
        assert!(!tiles.contains(&(8, 4)), "the wall itself");
        assert!(!tiles.contains(&(9, 4)), "behind the wall");
    }

    #[test]
    fn a_burst_at_the_room_edge_stays_inside() {
        let tiles = tiles(&room(), Shape::Burst { radius: 1 }, (5, 5), (1, 1), 6);
        assert_eq!(tiles, [(1, 1), (2, 1), (1, 2), (2, 2)]);
    }

    #[test]
    fn a_cone_spreads_45_degrees_each_side_and_not_behind() {
        let tiles = tiles(&room(), Shape::Cone, (20, 10), (21, 10), 3);
        assert!(tiles.contains(&(23, 10)), "straight ahead, at range");
        assert!(tiles.contains(&(22, 12)), "on the 45° edge");
        assert!(!tiles.contains(&(21, 12)), "past the edge");
        assert!(!tiles.contains(&(19, 10)), "behind");
        assert!(!tiles.contains(&(20, 10)), "the caster");
        assert!(!tiles.contains(&(24, 10)), "out of range");
    }

    #[test]
    fn a_cone_aimed_at_the_caster_covers_nothing() {
        assert!(tiles(&room(), Shape::Cone, (20, 10), (20, 10), 3).is_empty());
    }

    #[test]
    fn a_beam_runs_past_the_aimed_tile_to_its_range_or_a_wall() {
        let open = tiles(&room(), Shape::Beam, (20, 10), (21, 10), 4);
        assert_eq!(open, [(21, 10), (22, 10), (23, 10), (24, 10)]);
        let walled = tiles(&room(), Shape::Beam, (5, 4), (6, 4), 8);
        assert_eq!(walled, [(6, 4), (7, 4)]);
    }

    #[test]
    fn a_nova_surrounds_the_caster_wherever_it_aims() {
        let nova = tiles(&room(), Shape::Nova { radius: 1 }, (20, 10), (30, 2), 6);
        assert_eq!(nova.len(), 9);
        assert_eq!(nova[0], (20, 10));
        assert_eq!(
            centre(Shape::Nova { radius: 1 }, (20, 10), (30, 2)),
            (20, 10)
        );
    }
}
