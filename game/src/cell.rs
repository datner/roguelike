//! The rendering seam's output (ADR 0001): one glyph and its colours per screen cell.

use serde::{Deserialize, Serialize};

use crate::tiles::SpriteId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub fg: Rgb,
    pub bg: Rgb,
    /// What a frontend with sprites draws instead of the glyph (ADR 0023):
    /// only on tiles in view, so remembered ground stays glyphs.
    pub sprite: Option<SpriteId>,
}
