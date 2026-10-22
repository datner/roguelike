//! Draws the title menu (`front::menu`): new game, continue, load, and save management.

use eframe::egui::{self, RichText};
use front::menu::{Body, Menu};

use crate::panels::TEXT;

pub fn draw(ui: &mut egui::Ui, menu: &Menu) {
    let view = menu.view();
    egui::Window::new(view.title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .min_width(360.0)
        .show(ui.ctx(), |ui| {
            match view.body {
                Body::List(rows) => {
                    for row in rows {
                        let marker = if row.selected { "> " } else { "  " };
                        let mut text = RichText::new(format!("{marker}{}", row.text))
                            .monospace()
                            .size(TEXT + 2.0);
                        if row.selected {
                            text = text
                                .strong()
                                .background_color(ui.visuals().selection.bg_fill);
                        }
                        if row.disabled {
                            text = text.weak();
                        }
                        ui.label(text);
                    }
                }
                Body::Text(lines) => {
                    for line in lines {
                        ui.label(RichText::new(line).size(TEXT + 2.0));
                    }
                }
                Body::Field { prompt, text } => {
                    ui.label(RichText::new(prompt).size(TEXT + 2.0));
                    ui.label(
                        RichText::new(format!("> {text}_"))
                            .monospace()
                            .size(TEXT + 2.0),
                    );
                }
            }
            ui.add_space(6.0);
            if let Some(message) = view.message {
                ui.label(message);
            }
            ui.label(RichText::new(view.hint).weak());
        });
}
