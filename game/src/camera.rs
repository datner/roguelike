//! Which part of the zone a viewport shows (ADR 0009). The only place that
//! converts between screen tiles and world tiles.

use crate::entity::Position;
use crate::state::GameState;

/// A viewport of `width` × `height` tiles; screen tile (0, 0) shows world
/// tile `origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    pub origin: (i32, i32),
    pub width: i32,
    pub height: i32,
}

impl Camera {
    /// Centred on `focus`, but never showing past the zone's edge. Along an
    /// axis where the viewport is larger than the zone, the zone is centred
    /// instead and the rest is padding (a negative origin).
    pub fn follow(focus: (i32, i32), zone: (i32, i32), viewport: (i32, i32)) -> Self {
        let axis = |focus: i32, zone: i32, view: i32| {
            if view >= zone {
                -((view - zone) / 2)
            } else {
                (focus - view / 2).clamp(0, zone - view)
            }
        };
        let (width, height) = (viewport.0.max(0), viewport.1.max(0));
        Self {
            origin: (axis(focus.0, zone.0, width), axis(focus.1, zone.1, height)),
            width,
            height,
        }
    }

    pub fn screen_to_world(self, (x, y): (i32, i32)) -> (i32, i32) {
        (x + self.origin.0, y + self.origin.1)
    }

    /// May land outside the viewport; callers check.
    pub fn world_to_screen(self, (x, y): (i32, i32)) -> (i32, i32) {
        (x - self.origin.0, y - self.origin.1)
    }
}

/// The camera following the player over the current zone. Without a player
/// position it centres on the zone.
pub fn camera(state: &GameState, width: i32, height: i32) -> Camera {
    let zone = (state.map.width, state.map.height);
    let focus = state
        .world
        .get::<&Position>(state.player)
        .map(|pos| (pos.x, pos.y))
        .unwrap_or((zone.0 / 2, zone.1 / 2));
    Camera::follow(focus, zone, (width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONE: (i32, i32) = (160, 60);
    const VIEW: (i32, i32) = (40, 20);

    #[test]
    fn a_player_in_the_corner_clamps_the_view_to_that_corner() {
        assert_eq!(Camera::follow((1, 1), ZONE, VIEW).origin, (0, 0));
        assert_eq!(Camera::follow((158, 58), ZONE, VIEW).origin, (120, 40));
    }

    #[test]
    fn a_player_in_the_middle_is_centred() {
        let camera = Camera::follow((80, 30), ZONE, VIEW);
        assert_eq!(camera.origin, (60, 20));
        assert_eq!(camera.world_to_screen((80, 30)), (20, 10));
    }

    #[test]
    fn a_viewport_larger_than_the_zone_centres_it_with_padding() {
        let camera = Camera::follow((1, 1), (40, 20), (60, 30));
        assert_eq!(camera.origin, (-10, -5));
        assert_eq!(camera.world_to_screen((0, 0)), (10, 5));
        // Larger along one axis only: that axis pads, the other follows.
        let camera = Camera::follow((150, 30), ZONE, (200, 20));
        assert_eq!(camera.origin, (-20, 20));
    }

    #[test]
    fn screen_and_world_coordinates_round_trip() {
        for camera in [
            Camera::follow((80, 30), ZONE, VIEW),
            Camera::follow((3, 57), ZONE, VIEW),
            Camera::follow((0, 0), (40, 20), (61, 31)),
        ] {
            for point in [(0, 0), (5, 7), (39, 19), (-3, 100)] {
                assert_eq!(camera.world_to_screen(camera.screen_to_world(point)), point);
                assert_eq!(camera.screen_to_world(camera.world_to_screen(point)), point);
            }
        }
    }
}
