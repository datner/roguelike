//! Draws the title menu (`front::menu`): new game, continue, load, and save management, in a titled box.

use eframe::egui::{self, Align2, Frame, Vec2};
use front::menu::{Body, Menu};

use crate::theme::CELL;
use crate::theme::{self, BG, DIM, FG, boxed, title};

/// 48 text cells.
const WIDTH: f32 = 48.0 * CELL.x;

pub fn draw(ui: &mut egui::Ui, menu: &Menu) {
    let view = menu.view();
    egui::Window::new("menu")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(Frame::new().fill(BG))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size([WIDTH, 0.0])
        .show(ui.ctx(), |ui| {
            boxed(ui, title(view.title), None, |ui| {
                match view.body {
                    Body::List(rows) => {
                        for row in rows {
                            let marker = if row.selected { "> " } else { "  " };
                            let text = format!("{marker}{}", row.text);
                            let colour = if row.disabled { DIM } else { FG };
                            ui.label(if row.selected {
                                theme::reversed(&text)
                            } else {
                                theme::line(&text, colour)
                            });
                        }
                    }
                    Body::Text(lines) => {
                        for line in lines {
                            ui.label(theme::line(&line, FG));
                        }
                    }
                    Body::Field { prompt, text } => {
                        ui.label(theme::line(prompt, FG));
                        ui.label(theme::line(&format!("> {text}_"), FG));
                    }
                }
                ui.add_space(CELL.y);
                if let Some(message) = view.message {
                    ui.label(theme::line(&message, FG));
                }
                ui.label(theme::line(view.hint, DIM));
            });
        });
}
