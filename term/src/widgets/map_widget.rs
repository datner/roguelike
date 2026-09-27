use game::{Cell, Grid};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

/// Maps one map tile to one screen cell. This is the only place that knows
/// about the tile-to-screen ratio — switching to two characters per tile
/// later means editing only this function.
pub fn tile_to_screen(x: i32, y: i32) -> (i32, i32) {
    (x, y)
}

pub struct MapWidget<'a> {
    grid: &'a Grid<Cell>,
}

impl<'a> MapWidget<'a> {
    pub fn new(grid: &'a Grid<Cell>) -> Self {
        Self { grid }
    }
}

impl<'a> Widget for MapWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in 0..self.grid.height {
            for x in 0..self.grid.width {
                let Some(cell) = self.grid.get(x, y) else {
                    continue;
                };
                let (screen_x, screen_y) = tile_to_screen(x, y);
                if screen_x < 0 || screen_y < 0 {
                    continue;
                }
                if screen_x as u16 >= area.width || screen_y as u16 >= area.height {
                    continue;
                }
                let pos_x = area.x + screen_x as u16;
                let pos_y = area.y + screen_y as u16;
                if let Some(buf_cell) = buf.cell_mut((pos_x, pos_y)) {
                    buf_cell
                        .set_char(cell.glyph)
                        .set_fg(Color::Rgb(cell.fg.0, cell.fg.1, cell.fg.2))
                        .set_bg(Color::Rgb(cell.bg.0, cell.bg.1, cell.bg.2));
                }
            }
        }
    }
}
