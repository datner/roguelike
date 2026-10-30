//! Light as data: how strongly the player's held light reaches each tile in view, fading with distance. `render_view` bakes it into cell colours, as a terminal needs; a frontend that can blend draws it from `render_layers` instead.

use crate::camera::camera;
use crate::cell::{Cell, Rgb};
use crate::entity::Position;
use crate::fov;
use crate::map::Grid;
use crate::state::{GameState, render_unlit};

/// Light on one tile: colours move `strength_bp` / 10 000 of the way toward
/// `tint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lit {
    pub tint: Rgb,
    pub strength_bp: u32,
}

/// The light, if any, the player's held light casts on world tile `(x, y)`:
/// tiles in view within its radius, full at the holder and fading toward
/// the edge. Tiles out of view stay unlit, so remembered ground never glows.
pub(crate) fn lit_by(state: &GameState) -> impl Fn(i32, i32) -> Option<Lit> + '_ {
    let source = fov::held_light(&state.world, state.player).zip(
        state
            .world
            .get::<&Position>(state.player)
            .ok()
            .map(|pos| *pos),
    );
    // The phase's LightReach, the one place it is read.
    let farther = crate::phase::rule(state, |rule| match rule {
        crate::phase::Rule::LightReach(pct) => Some(pct),
        _ => None,
    });
    let source = source.map(|(mut light, at)| {
        light.radius = i32::try_from(crate::phase::grown(i64::from(light.radius), farther))
            .unwrap_or(light.radius);
        (light, at)
    });
    move |x, y| {
        let (light, at) = source?;
        let (dx, dy) = (x - at.x, y - at.y);
        let distance_sq = dx * dx + dy * dy;
        if state.visible.get(x, y) != Some(&true) || distance_sq > light.radius * light.radius {
            return None;
        }
        let reach = (light.radius + 1) * (light.radius + 1);
        let fade = u32::try_from(reach - distance_sq).unwrap_or(0);
        let reach = u32::try_from(reach).unwrap_or(1).max(1);
        Some(Lit {
            tint: light.tint,
            strength_bp: light.tint_bp.min(10_000) * fade / reach,
        })
    }
}

/// `colour` under `lit`.
pub fn tint(Rgb(r, g, b): Rgb, lit: Lit) -> Rgb {
    let Rgb(tr, tg, tb) = lit.tint;
    let bp = lit.strength_bp.min(10_000) as i32;
    let mix = |c: u8, t: u8| (c as i32 + (t as i32 - c as i32) * bp / 10_000) as u8;
    Rgb(mix(r, tr), mix(g, tg), mix(b, tb))
}

/// `render_view`'s cells before light, and the light on each, for a
/// frontend that draws light itself.
pub fn render_layers(
    state: &GameState,
    width: i32,
    height: i32,
) -> (Grid<Cell>, Grid<Option<Lit>>) {
    let cells = render_unlit(state, width, height);
    let view = camera(state, width, height);
    let lit = lit_by(state);
    let mut light = Grid::new(view.width, view.height, None);
    for y in 0..view.height {
        for x in 0..view.width {
            let (wx, wy) = view.screen_to_world((x, y));
            light.set(x, y, lit(wx, wy));
        }
    }
    (cells, light)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Light;
    use crate::input::PlayerAction;
    use crate::state::tests::bare_state;
    use crate::testkit::thing;

    #[test]
    fn a_held_light_fades_with_distance_and_stops_at_the_view() {
        let mut state = bare_state();
        let player = state.player;
        let torch = thing("torch")
            .item()
            .equipped_on(player, 4)
            .make(&mut state);
        let tint = Rgb(255, 150, 50);
        let light = Light {
            radius: 3,
            tint,
            tint_bp: 1_600,
        };
        state.world.insert_one(torch, light).unwrap();
        crate::time::take_turn(&mut state, PlayerAction::Wait);

        {
            let lit = lit_by(&state);
            let strength = |x, y| lit(x, y).map(|lit| lit.strength_bp);
            // The player stands at (1, 1); reach is (3 + 1)² = 16.
            assert_eq!(strength(1, 1), Some(1_600));
            assert_eq!(strength(2, 1), Some(1_600 * 15 / 16));
            assert_eq!(strength(4, 1), Some(1_600 * 7 / 16));
            assert_eq!(strength(5, 1), None, "beyond the radius");
        }
        state.visible.set(3, 1, false);
        assert_eq!(lit_by(&state)(3, 1), None, "out of view");

        let (cells, light) = render_layers(&state, 40, 20);
        assert_eq!(cells.get(2, 1).unwrap().bg, Rgb(0, 0, 0), "unlit cells");
        assert_eq!(
            *light.get(2, 1).unwrap(),
            Some(Lit {
                tint,
                strength_bp: 1_500
            })
        );
    }
}
