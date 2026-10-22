//! The panels around the map: the side column (character sheet, equipment) and the bottom strip (status and log), drawn from `front::text` and `Session::status`.

use eframe::egui::{self, Color32, FontId, RichText, TextFormat, text::LayoutJob};
use front::session::{Mode, Session};
use front::text::{self, Seg};

use crate::map_view::color;

/// Width of the side column in points.
pub const SIDE_WIDTH: f32 = 300.0;
/// Log lines shown under the map, newest last.
const LOG_LINES: usize = 5;

/// Body text size, in points.
pub const TEXT: f32 = 14.0;

/// `front`'s text as one laid-out line: quiet text weaker, glyphs in their colour.
pub fn job(row: Vec<Seg>, base: Color32) -> LayoutJob {
    let font = FontId::monospace(TEXT);
    let quiet = base.gamma_multiply(0.55);
    let mut job = LayoutJob::default();
    for seg in row {
        let (text, colour) = match seg {
            Seg::Plain(text) => (text, base),
            Seg::Quiet(text) => (text, quiet),
            Seg::Glyph(glyph, fg) => (glyph.to_string(), color(fg)),
        };
        job.append(&text, 0.0, TextFormat::simple(font.clone(), colour));
    }
    job
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(TEXT + 2.0));
}

pub fn side(ui: &mut egui::Ui, session: &Session) {
    let base = ui.visuals().text_color();
    heading(ui, session.character());
    for row in text::character(session.state()) {
        ui.label(job(row, base));
    }
    ui.add_space(8.0);
    heading(ui, "Equipment");
    for row in text::equipment(session.state()) {
        ui.label(job(row, base));
    }
    if let Mode::Look(looking) = session.mode() {
        ui.add_space(8.0);
        heading(ui, "Looking at");
        for row in text::inspection(session.state(), looking.cursor()) {
            ui.label(job(row, base));
        }
    }
}

pub fn bottom(ui: &mut egui::Ui, session: &Session) {
    let mut lines: Vec<&String> = session.state().log.recent(LOG_LINES).collect();
    lines.reverse();
    for line in lines {
        ui.label(RichText::new(line).monospace().size(TEXT));
    }
    ui.separator();
    ui.label(
        RichText::new(session.status())
            .monospace()
            .size(TEXT)
            .strong(),
    );
}
