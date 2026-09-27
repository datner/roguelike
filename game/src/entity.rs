use crate::cell::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct Appearance {
    pub glyph: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

/// Marker component: an entity with this blocks movement onto its tile.
#[derive(Debug, Clone, Copy)]
pub struct BlocksMovement;
