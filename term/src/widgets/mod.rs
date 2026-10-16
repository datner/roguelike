//! ratatui widgets for the game's output.

use game::Rgb;
use ratatui::style::Color;

pub mod map_widget;

/// The one conversion from `game`'s colours to the terminal's.
pub fn color(Rgb(r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}
