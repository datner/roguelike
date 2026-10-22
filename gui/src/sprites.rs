//! Sprites (ADR 0023): the atlas `game::Tiles` names, as an egui texture, and the part of it a sprite shows at a moment. Off until F2, so glyphs stay the default look while the atlas is a placeholder.

use std::time::Duration;

use eframe::egui::{self, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use game::{SpriteId, Tiles};

/// The atlas on the GPU, with the table that says where each sprite is.
pub struct Atlas {
    texture: TextureHandle,
    tiles: Tiles,
    /// The image's size in pixels.
    size: (f32, f32),
}

impl Atlas {
    /// Reads the atlas image `tiles` names. Nearest-neighbour sampling, so
    /// pixel art stays crisp when scaled to a tile.
    pub fn load(ctx: &egui::Context, tiles: &Tiles) -> Result<Self, String> {
        let image = image::open(&tiles.atlas)
            .map_err(|err| format!("{}: {err}", tiles.atlas.display()))?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let pixels = ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            image.as_flat_samples().as_slice(),
        );
        Ok(Self {
            texture: ctx.load_texture("atlas", pixels, TextureOptions::NEAREST),
            tiles: tiles.clone(),
            size: (width as f32, height as f32),
        })
    }

    pub fn texture(&self) -> egui::TextureId {
        self.texture.id()
    }

    /// The part of the atlas `sprite` shows `elapsed` into the game, in
    /// texture coordinates.
    pub fn uv(&self, sprite: SpriteId, elapsed: Duration) -> Option<Rect> {
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let (col, row) = self.tiles.get(sprite)?.frame(elapsed_ms);
        Some(uv(self.tiles.cell, self.size, (col, row)))
    }
}

/// Atlas cell `(col, row)` of `cell`-sized cells in an image of `size`
/// pixels, in texture coordinates (0 to 1).
pub fn uv(cell: (u32, u32), size: (f32, f32), (col, row): (u32, u32)) -> Rect {
    let (w, h) = (cell.0 as f32 / size.0, cell.1 as f32 / size.1);
    let min = pos2(col as f32 * w, row as f32 * h);
    Rect::from_min_max(min, pos2(min.x + w, min.y + h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_atlas_cell_is_its_share_of_the_image() {
        let rect = uv((8, 16), (40.0, 32.0), (2, 1));
        assert_eq!(rect, Rect::from_min_max(pos2(0.4, 0.5), pos2(0.6, 1.0)));
    }
}
