//! Draws the game's `Grid<Cell>` into the map panel. The single tile↔pixel mapping on the `gui` side; which world tiles are shown is `game`'s camera.

use eframe::egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Vec2, vec2};
use std::time::Duration;

use game::{Cell, GameState, Grid, Lit, Rgb, camera, tint};

use crate::sprites::Atlas;

use crate::theme::TEXT;

/// One tile in pixels: a terminal cell, the same size as a cell of text
/// (`theme::TEXT`), so the map and the panels share one grid.
pub const TILE: Vec2 = vec2(10.0, 20.0);

/// The one conversion from `game`'s colours to egui's.
pub fn color(Rgb(r, g, b): Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Whole tiles that fit in `size`, at least one each way.
pub fn viewport_tiles(size: Vec2) -> (i32, i32) {
    let fit = |pixels: f32, tile: f32| ((pixels / tile).floor() as i32).max(1);
    (fit(size.x, TILE.x), fit(size.y, TILE.y))
}

/// Where a viewport of tiles sits on screen: centred in its panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    origin: Pos2,
    pub width: i32,
    pub height: i32,
}

impl Layout {
    pub fn new(panel: Rect) -> Self {
        let (width, height) = viewport_tiles(panel.size());
        let used = vec2(width as f32 * TILE.x, height as f32 * TILE.y);
        Self {
            origin: panel.center() - used / 2.0,
            width,
            height,
        }
    }

    /// The same tiles moved by `by` pixels, for a shake.
    pub fn shifted(self, by: Vec2) -> Self {
        Self {
            origin: self.origin + by,
            ..self
        }
    }

    /// The pixels of screen tile `(x, y)`.
    pub fn tile_rect(&self, (x, y): (i32, i32)) -> Rect {
        Rect::from_min_size(
            self.origin + vec2(x as f32 * TILE.x, y as f32 * TILE.y),
            TILE,
        )
    }

    /// The screen tile under a pixel, if it is on the map.
    pub fn tile_at(&self, pos: Pos2) -> Option<(i32, i32)> {
        let local = pos - self.origin;
        let (x, y) = (
            (local.x / TILE.x).floor() as i32,
            (local.y / TILE.y).floor() as i32,
        );
        ((0..self.width).contains(&x) && (0..self.height).contains(&y)).then_some((x, y))
    }
}

/// The screen tile showing world tile `at`, if the map shows it.
pub fn screen_tile(state: &GameState, layout: &Layout, at: (i32, i32)) -> Option<(i32, i32)> {
    let (x, y) = camera(state, layout.width, layout.height).world_to_screen(at);
    ((0..layout.width).contains(&x) && (0..layout.height).contains(&y)).then_some((x, y))
}

/// How strongly the light reaches the corner at the top left of cell
/// `(x, y)`: the average over the four cells that share it, dark ones
/// counting as none, so light fades smoothly across tile edges.
pub fn corner_strength(light: &Grid<Option<Lit>>, x: i32, y: i32) -> u32 {
    let strength = |x, y| {
        light
            .get(x, y)
            .copied()
            .flatten()
            .map_or(0, |lit: Lit| lit.strength_bp)
    };
    (strength(x - 1, y - 1) + strength(x, y - 1) + strength(x - 1, y) + strength(x, y)) / 4
}

/// Paints the backgrounds, lit tiles blending into their neighbours'
/// light, then each glyph, or its sprite with `sprites` (the atlas and the
/// time into the game, for animation), in its tile's light.
pub fn draw(
    painter: &Painter,
    layout: &Layout,
    grid: &Grid<Cell>,
    light: &Grid<Option<Lit>>,
    sprites: Option<(&Atlas, Duration)>,
) {
    let font = FontId::monospace(TEXT);
    let mut mesh = Mesh::default();
    let mut glyphs = Vec::new();
    let mut images = Vec::new();
    for y in 0..layout.height {
        for x in 0..layout.width {
            let Some(cell) = grid.get(x, y) else {
                continue;
            };
            let rect = layout.tile_rect((x, y));
            let lit = light.get(x, y).copied().flatten();
            match lit {
                Some(lit) => {
                    let first = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
                    for (corner, (dx, dy)) in [
                        (rect.left_top(), (0, 0)),
                        (rect.right_top(), (1, 0)),
                        (rect.right_bottom(), (1, 1)),
                        (rect.left_bottom(), (0, 1)),
                    ] {
                        let strength_bp = corner_strength(light, x + dx, y + dy);
                        let bg = tint(cell.bg, Lit { strength_bp, ..lit });
                        mesh.colored_vertex(corner, color(bg));
                    }
                    mesh.add_triangle(first, first + 1, first + 2);
                    mesh.add_triangle(first, first + 2, first + 3);
                }
                None if cell.bg != Rgb(0, 0, 0) => mesh.add_colored_rect(rect, color(cell.bg)),
                None => {}
            }
            let sprite = sprites
                .zip(cell.sprite)
                .and_then(|((atlas, elapsed), sprite)| atlas.uv(sprite, elapsed));
            if let Some(uv) = sprite {
                let shade = lit.map_or(Rgb(255, 255, 255), |lit| tint(Rgb(255, 255, 255), lit));
                images.push((rect, uv, color(shade)));
            } else if cell.glyph != ' ' {
                let fg = lit.map_or(cell.fg, |lit| tint(cell.fg, lit));
                glyphs.push((rect, cell.glyph, color(fg)));
            }
        }
    }
    painter.add(mesh);
    if let Some((atlas, _)) = sprites {
        for (rect, uv, shade) in images {
            painter.image(atlas.texture(), rect, uv, shade);
        }
    }
    for (rect, ch, fg) in glyphs {
        glyph(painter, rect, ch, fg, &font);
    }
}

/// A glyph centred in its tile.
pub fn glyph(painter: &Painter, rect: Rect, glyph: char, fg: Color32, font: &FontId) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        font.clone(),
        fg,
    );
}

#[cfg(test)]
mod tests {
    use eframe::egui::pos2;

    use super::*;

    #[test]
    fn a_corner_averages_the_light_of_the_four_cells_around_it() {
        let lit = |strength_bp| {
            Some(Lit {
                tint: Rgb(255, 0, 0),
                strength_bp,
            })
        };
        let mut light = Grid::new(3, 2, None);
        light.set(0, 0, lit(400));
        light.set(1, 0, lit(800));
        light.set(1, 1, lit(1_200));
        assert_eq!(corner_strength(&light, 1, 1), (400 + 800 + 1_200) / 4);
        assert_eq!(corner_strength(&light, 0, 0), 100, "off the grid is dark");
        assert_eq!(corner_strength(&light, 3, 2), 0);
    }

    #[test]
    fn whole_tiles_fit_and_never_none() {
        assert_eq!(viewport_tiles(vec2(TILE.x * 10.5, TILE.y * 3.9)), (10, 3));
        assert_eq!(viewport_tiles(vec2(1.0, 1.0)), (1, 1));
    }

    #[test]
    fn pixels_and_tiles_agree_both_ways() {
        let panel = Rect::from_min_size(pos2(100.0, 50.0), vec2(TILE.x * 4.0, TILE.y * 2.0));
        let layout = Layout::new(panel);
        assert_eq!((layout.width, layout.height), (4, 2));
        let rect = layout.tile_rect((3, 1));
        assert_eq!(rect.min, pos2(100.0 + TILE.x * 3.0, 50.0 + TILE.y));
        assert_eq!(layout.tile_at(rect.center()), Some((3, 1)));
        assert_eq!(layout.tile_at(pos2(99.0, 60.0)), None);
    }
}
