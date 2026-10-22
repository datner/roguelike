//! The panels around the map: the side column (character sheet, equipment) and the bottom strip (log and status), drawn from `front::text` and `Session::status` in `term`'s boxes.

use eframe::egui::{self, Color32, text::LayoutJob};
use front::session::{Mode, Session};
use front::text::{self, Seg};

use crate::map_view::{TILE, color};
use crate::theme::{self, DIM, FG, boxed, title};

/// Width of the side column: `term`'s 30 cells.
pub const SIDE_WIDTH: f32 = 30.0 * TILE.x;
/// Log lines shown under the map, newest last.
const LOG_LINES: usize = 5;
/// The log and status boxes: `term`'s 7 and 3 rows.
pub const BOTTOM_HEIGHT: f32 = 10.0 * TILE.y;

/// `front`'s text as one laid-out line: quiet text dim, glyphs in their
/// colour, the rest in `base`.
pub fn job(row: Vec<Seg>, base: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for seg in row {
        match seg {
            Seg::Plain(text) => theme::append(&mut job, &text, base),
            Seg::Quiet(text) => theme::append(&mut job, &text, DIM),
            Seg::Glyph(glyph, fg) => theme::append(&mut job, &glyph.to_string(), color(fg)),
        }
    }
    job
}

fn rows(ui: &mut egui::Ui, rows: Vec<Vec<Seg>>) {
    for row in rows {
        ui.label(job(row, FG));
    }
}

pub fn side(ui: &mut egui::Ui, session: &Session) {
    let state = session.state();
    boxed(ui, title("Where"), None, |ui| {
        rows(ui, text::whereabouts(state));
    });
    boxed(ui, title(session.character()), None, |ui| {
        rows(ui, text::character(state));
    });
    boxed(ui, title("Equipment"), None, |ui| {
        rows(ui, text::equipment(state));
    });
    if let Mode::Look(looking) = session.mode() {
        boxed(ui, title("Looking at"), None, |ui| {
            rows(ui, text::inspection(state, looking.cursor()));
        });
    }
}

pub fn bottom(ui: &mut egui::Ui, session: &Session) {
    let mut lines: Vec<&str> = session
        .state()
        .log
        .recent(LOG_LINES)
        .map(String::as_str)
        .collect();
    lines.reverse();
    // Always five rows high, as in `term`.
    lines.resize(LOG_LINES, "");
    boxed(ui, title("Log"), None, |ui| {
        for line in lines {
            ui.add(egui::Label::new(theme::line(line, FG)).truncate());
        }
    });
    boxed(ui, title("Status"), None, |ui| {
        ui.add(egui::Label::new(theme::line(&session.status(), FG)).truncate());
    });
}
