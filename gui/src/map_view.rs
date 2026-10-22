//! Draws the game's `Grid<Cell>` into the map panel. The single tile↔pixel mapping on the `gui` side; which world tiles are shown is `game`'s camera.

use eframe::egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use eframe::egui::{
    self, Align2, Color32, FontData, FontFamily, FontId, Painter, Pos2, Rect, Vec2, vec2,
};
use game::{Cell, Grid, Rgb};

/// One tile in pixels: taller than wide, like a terminal cell, so glyphs
/// read naturally.
pub const TILE: Vec2 = vec2(14.0, 20.0);

/// egui's fonts lack some of what the game shows (`☺` on the map, `↑↓` in
/// hints); DejaVu Sans Mono backs up both families. Licence beside the
/// file in `gui/assets`.
pub fn install_fonts(ctx: &egui::Context) {
    let fallback = |family| InsertFontFamily {
        family,
        priority: FontPriority::Lowest,
    };
    ctx.add_font(FontInsert::new(
        "DejaVu Sans Mono",
        FontData::from_static(include_bytes!("../assets/DejaVuSansMono.ttf")),
        vec![
            fallback(FontFamily::Monospace),
            fallback(FontFamily::Proportional),
        ],
    ));
}

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

/// Paints each cell's background and its glyph.
pub fn draw(painter: &Painter, layout: &Layout, grid: &Grid<Cell>) {
    let font = FontId::monospace(TILE.y * 0.8);
    for y in 0..layout.height {
        for x in 0..layout.width {
            let Some(cell) = grid.get(x, y) else {
                continue;
            };
            let rect = layout.tile_rect((x, y));
            if cell.bg != Rgb(0, 0, 0) {
                painter.rect_filled(rect, 0.0, color(cell.bg));
            }
            if cell.glyph != ' ' {
                glyph(painter, rect, cell.glyph, color(cell.fg), &font);
            }
        }
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

/// The whole tile's pixels from its corner, for highlighting.
pub fn outline_rect(layout: &Layout, tile: (i32, i32)) -> Rect {
    layout.tile_rect(tile).shrink(0.5)
}

#[cfg(test)]
mod tests {
    use eframe::egui::pos2;

    use super::*;

    #[test]
    fn whole_tiles_fit_and_never_none() {
        assert_eq!(viewport_tiles(vec2(14.0 * 10.5, 20.0 * 3.9)), (10, 3));
        assert_eq!(viewport_tiles(vec2(1.0, 1.0)), (1, 1));
    }

    #[test]
    fn pixels_and_tiles_agree_both_ways() {
        let panel = Rect::from_min_size(pos2(100.0, 50.0), vec2(14.0 * 4.0, 20.0 * 2.0));
        let layout = Layout::new(panel);
        assert_eq!((layout.width, layout.height), (4, 2));
        let rect = layout.tile_rect((3, 1));
        assert_eq!(rect.min, pos2(100.0 + 42.0, 50.0 + 20.0));
        assert_eq!(layout.tile_at(rect.center()), Some((3, 1)));
        assert_eq!(layout.tile_at(pos2(99.0, 60.0)), None);
    }
}
