//! ratatui widgets for the game's output.

use front::text::Seg;
use game::Rgb;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub mod map_widget;

/// The one conversion from `game`'s colours to the terminal's.
pub fn color(Rgb(r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}

/// `front`'s text as spans: quiet text dimmed, glyphs in their colour.
pub fn spans(row: Vec<Seg>) -> Vec<Span<'static>> {
    row.into_iter()
        .map(|seg| match seg {
            Seg::Plain(text) => Span::raw(text),
            Seg::Quiet(text) => Span::styled(text, Style::new().add_modifier(Modifier::DIM)),
            Seg::Glyph(glyph, fg) => Span::styled(glyph.to_string(), Style::new().fg(color(fg))),
        })
        .collect()
}

pub fn line(row: Vec<Seg>) -> Line<'static> {
    Line::from(spans(row))
}
