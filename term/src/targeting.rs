//! Draws throwing, casting and looking (`front::targeting`): the chooser list, the aiming marks, a missile in flight and the look cursor. What a shot would do comes from `game::aim`, never worked out here.

use front::targeting::{Aiming, Chooser, Looking, spell_text};
use game::camera;
use game::{GameState, ItemView, Rgb, SpellView, aim};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::overlay::item_spans;
use crate::widgets::color;
use crate::widgets::map_widget::{tile_to_screen, viewport_tiles};

/// One line per throwable.
pub fn throwable_lines(items: &[ItemView]) -> Vec<Line<'static>> {
    items
        .iter()
        .map(|item| Line::from(item_spans(item)))
        .collect()
}

/// One line per spell: its reach and damage, then, dimmed, its price and
/// why it can't be cast now, if it can't.
pub fn spell_lines(spells: &[SpellView]) -> Vec<Line<'static>> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    spells
        .iter()
        .map(|spell| {
            let (main, notes) = spell_text(spell);
            let mut spans = vec![Span::raw(main)];
            spans.extend(
                notes
                    .into_iter()
                    .map(|note| Span::styled(format!("  {note}"), dim)),
            );
            Line::from(spans)
        })
        .collect()
}

pub fn draw_chooser(
    frame: &mut Frame,
    area: Rect,
    chooser: &Chooser,
    title: &str,
    lines: Vec<Line<'static>>,
) {
    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 48.min(area.width);
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let lines: Vec<Line> = (0..)
        .zip(lines)
        .map(|(index, line)| {
            if index == chooser.selected() {
                line.patch_style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title.to_string())),
        panel,
    );
}

const PATH: Rgb = Rgb(110, 90, 20);
const LANDING: Rgb = Rgb(170, 60, 20);
pub(crate) const AREA: Rgb = Rgb(120, 40, 90);

/// Marks the missile's path, a spell's area and the landing tile on the
/// already-drawn map, and reverses the cursor's tile.
pub fn draw_aim(frame: &mut Frame, map: Rect, state: &GameState, aiming: &Aiming) {
    let Some(shot) = aim(state, aiming.shot(), aiming.cursor()) else {
        return;
    };
    let (width, height) = viewport_tiles(map);
    let view = camera(state, width, height);
    let mut mark = |tile: (i32, i32), style: Style| {
        let (x, y) = tile_to_screen(view.world_to_screen(tile).0, view.world_to_screen(tile).1);
        if !(0..width).contains(&x) || !(0..height).contains(&y) {
            return;
        }
        if let Some(cell) = frame
            .buffer_mut()
            .cell_mut((map.x + x as u16, map.y + y as u16))
        {
            cell.set_style(style);
        }
    };
    for &tile in &shot.path {
        mark(tile, Style::new().bg(color(PATH)));
    }
    for &tile in &shot.area {
        mark(tile, Style::new().bg(color(AREA)));
    }
    for &tile in &shot.summons {
        mark(tile, Style::new().bg(color(AREA)));
    }
    mark(shot.landing, Style::new().bg(color(LANDING)));
    mark(
        aiming.cursor(),
        Style::new().add_modifier(Modifier::REVERSED),
    );
}

/// One frame of a missile's flight: its glyph at one world tile.
#[derive(Debug, Clone, Copy)]
pub struct Flying {
    pub glyph: char,
    pub fg: Rgb,
    pub at: (i32, i32),
}

/// Draws the flying missile over the already-drawn map.
pub fn draw_missile(frame: &mut Frame, map: Rect, state: &GameState, flying: Flying) {
    let (width, height) = viewport_tiles(map);
    let (x, y) = camera(state, width, height).world_to_screen(flying.at);
    let (x, y) = tile_to_screen(x, y);
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return;
    }
    if let Some(cell) = frame
        .buffer_mut()
        .cell_mut((map.x + x as u16, map.y + y as u16))
    {
        cell.set_char(flying.glyph).set_fg(color(flying.fg));
    }
}

/// Reverses the cursor's tile on the already-drawn map.
pub fn draw_look(frame: &mut Frame, map: Rect, state: &GameState, looking: &Looking) {
    let (width, height) = viewport_tiles(map);
    let view = camera(state, width, height);
    let (x, y) = view.world_to_screen(looking.cursor());
    let (x, y) = tile_to_screen(x, y);
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return;
    }
    if let Some(cell) = frame
        .buffer_mut()
        .cell_mut((map.x + x as u16, map.y + y as u16))
    {
        cell.set_style(Style::new().add_modifier(Modifier::REVERSED));
    }
}
