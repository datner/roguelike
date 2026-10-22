//! Draws what the session's mode puts on and over the map: the look and aim cursors and marks, the throw and spell choosers, and the inventory panel. Pointer use on them becomes the same `front` inputs keys send, collected for `app` to hand to the session after drawing.

use eframe::egui::{
    self, Align2, Color32, Painter, Rect, RichText, ScrollArea, Stroke, StrokeKind, vec2,
};
use front::input::{AimInput, Input, OverlayInput};
use front::overlay::{LineKind, TABS, Title, Views};
use front::session::{Mode, Session};
use front::targeting::spell_text;
use front::text::{self, Seg};
use game::{GameState, Rgb, aim, camera, spells, throwables};

use crate::map_view::Layout;
use crate::panels::{TEXT, job};

const PATH: Rgb = Rgb(110, 90, 20);
const LANDING: Rgb = Rgb(170, 60, 20);
const AREA: Rgb = Rgb(120, 40, 90);

/// A tile wash: the tile's colour shows through.
fn wash(rgb: Rgb) -> Color32 {
    let Rgb(r, g, b) = rgb;
    Color32::from_rgba_unmultiplied(r, g, b, 150)
}

/// The world tile `at` on screen, if the map shows it.
fn screen_rect(state: &GameState, layout: &Layout, at: (i32, i32)) -> Option<Rect> {
    let (x, y) = camera(state, layout.width, layout.height).world_to_screen(at);
    ((0..layout.width).contains(&x) && (0..layout.height).contains(&y))
        .then(|| layout.tile_rect((x, y)))
}

fn outline(painter: &Painter, rect: Rect, colour: Color32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(2.0, colour), StrokeKind::Inside);
}

/// Marks on the map: the look cursor, or an aimed shot's path, area,
/// landing tile and cursor (from `game::aim`, like `term`'s).
pub fn over_map(painter: &Painter, layout: &Layout, session: &Session) {
    let state = session.state();
    match session.mode() {
        Mode::Look(looking) => {
            if let Some(rect) = screen_rect(state, layout, looking.cursor()) {
                outline(painter, rect, Color32::YELLOW);
            }
        }
        Mode::Aim(aiming) => {
            let Some(shot) = aim(state, aiming.shot(), aiming.cursor()) else {
                return;
            };
            let mark = |at, rgb| {
                if let Some(rect) = screen_rect(state, layout, at) {
                    painter.rect_filled(rect, 0.0, wash(rgb));
                }
            };
            for &at in &shot.path {
                mark(at, PATH);
            }
            for &at in shot.area.iter().chain(&shot.summons) {
                mark(at, AREA);
            }
            mark(shot.landing, LANDING);
            if let Some(rect) = screen_rect(state, layout, aiming.cursor()) {
                outline(painter, rect, Color32::WHITE);
            }
        }
        _ => {}
    }
}

/// The world tile under the pointer, if it is over the map.
pub fn pointed(state: &GameState, layout: &Layout, pos: Option<egui::Pos2>) -> Option<(i32, i32)> {
    let tile = layout.tile_at(pos?)?;
    Some(camera(state, layout.width, layout.height).screen_to_world(tile))
}

/// What the pointer on the map does in this mode. `moved` is the tile it
/// moved onto this frame, if it did: a resting pointer never overrides a
/// cursor the keys moved.
pub fn map_pointer(
    session: &Session,
    moved: Option<(i32, i32)>,
    response: &egui::Response,
    at: Option<(i32, i32)>,
) -> Vec<Input> {
    let mut inputs = Vec::new();
    match session.mode() {
        Mode::Play => {
            if response.clicked()
                && let Some(at) = at
            {
                inputs.push(Input::TravelTo(at));
            }
        }
        // A click goes there, like `Enter`.
        Mode::Look(_) => {
            inputs.extend(moved.map(|at| Input::Cursor(AimInput::Set(at))));
            if response.clicked()
                && let Some(at) = at
            {
                inputs.push(Input::Cursor(AimInput::Set(at)));
                inputs.push(Input::Cursor(AimInput::Confirm));
            } else if response.secondary_clicked() {
                inputs.push(Input::Cursor(AimInput::Cancel));
            }
        }
        Mode::Aim(_) => {
            inputs.extend(moved.map(|at| Input::Cursor(AimInput::Set(at))));
            if response.clicked()
                && let Some(at) = at
            {
                inputs.push(Input::Cursor(AimInput::Set(at)));
                inputs.push(Input::Cursor(AimInput::Confirm));
            } else if response.secondary_clicked() {
                inputs.push(Input::Cursor(AimInput::Cancel));
            }
        }
        _ => {}
    }
    inputs
}

/// While playing, hovering a tile shows what looking at it would.
pub fn hover_tooltip(response: &egui::Response, session: &Session, at: Option<(i32, i32)>) {
    let (Mode::Play, Some(at)) = (session.mode(), at) else {
        return;
    };
    if session.state().visible.get(at.0, at.1) != Some(&true) {
        return;
    }
    let base = Color32::LIGHT_GRAY;
    response.clone().on_hover_ui_at_pointer(|ui| {
        for row in text::inspection(session.state(), at) {
            ui.label(job(row, base));
        }
    });
}

/// Windows over the map: the throw or spell chooser, or the inventory
/// panel. `scrolled` remembers the row last scrolled to, so the panel
/// follows the keys without fighting the mouse wheel.
pub fn windows(ui: &egui::Ui, session: &Session, scrolled: &mut Option<usize>) -> Vec<Input> {
    let state = session.state();
    match session.mode() {
        Mode::Choose(chooser) => {
            let rows = throwables(state).iter().map(text::item).collect();
            chooser_window(ui, "Throw what?", rows, chooser.selected())
        }
        Mode::ChooseSpell(chooser) => {
            let rows = spells(state)
                .iter()
                .map(|spell| {
                    let (main, notes) = spell_text(spell);
                    let mut row = vec![Seg::Plain(main)];
                    row.extend(
                        notes
                            .into_iter()
                            .map(|note| Seg::Quiet(format!("  {note}"))),
                    );
                    row
                })
                .collect();
            chooser_window(ui, "Cast what?", rows, chooser.selected())
        }
        Mode::Overlay(overlay) => {
            let views = Views::of(state);
            let contents = views.contents();
            let view = overlay.view(&contents);
            let actions = overlay.actions(&contents);
            let mut inputs = Vec::new();
            centred_window(ui, "panel", |ui| {
                match &view.title {
                    Title::Tabs(active) => {
                        ui.horizontal(|ui| {
                            for (tab, name) in TABS {
                                let shown = tab == *active;
                                let label = RichText::new(name).size(TEXT + 1.0);
                                if ui.selectable_label(shown, label).clicked() && !shown {
                                    inputs.push(Input::List(tab_input(tab)));
                                }
                            }
                        });
                    }
                    Title::Text(text) => {
                        ui.label(RichText::new(text).strong().size(TEXT + 1.0));
                    }
                }
                ui.separator();
                let base = ui.visuals().text_color();
                ScrollArea::vertical()
                    .max_height(ui.ctx().content_rect().height() * 0.55)
                    .show(ui, |ui| {
                        for line in &view.lines {
                            let job = job(line.row.clone(), base);
                            let Some(index) = line.index else {
                                if line.kind == LineKind::Heading {
                                    ui.label(RichText::new(text::flat(&line.row)).strong());
                                } else {
                                    ui.label(job);
                                }
                                continue;
                            };
                            let selected = line.kind == LineKind::Selected;
                            let response = ui.selectable_label(selected, job);
                            if selected && *scrolled != Some(index) {
                                response.scroll_to_me(None);
                                *scrolled = Some(index);
                            }
                            if response.double_clicked() {
                                inputs.push(Input::List(OverlayInput::Focus(index)));
                                inputs.push(Input::List(OverlayInput::Select));
                            } else if response.clicked() || response.secondary_clicked() {
                                inputs.push(Input::List(OverlayInput::Focus(index)));
                            }
                            if selected {
                                response.context_menu(|ui| {
                                    for &(name, input) in &actions {
                                        if ui.button(name).clicked() {
                                            inputs.push(Input::List(input));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        }
                    });
                ui.separator();
                ui.label(RichText::new(view.hint).weak());
            });
            inputs
        }
        Mode::Play | Mode::Aim(_) | Mode::Look(_) => {
            *scrolled = None;
            Vec::new()
        }
    }
}

fn tab_input(tab: front::overlay::Tab) -> OverlayInput {
    use front::overlay::Tab;
    match tab {
        Tab::Inventory => OverlayInput::Inventory,
        Tab::Equipment => OverlayInput::Equipment,
        Tab::Skills => OverlayInput::Skills,
        Tab::Styles => OverlayInput::Styles,
    }
}

/// A fixed window in the middle of the screen, no title bar.
fn centred_window(ui: &egui::Ui, id: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(
            Align2::CENTER_CENTER,
            vec2(-crate::panels::SIDE_WIDTH / 2.0, 0.0),
        )
        .min_width(480.0)
        .show(ui.ctx(), contents);
}

/// A list to pick from: a click picks the row, keys move and pick as in
/// `term`.
fn chooser_window(ui: &egui::Ui, title: &str, rows: Vec<Vec<Seg>>, selected: usize) -> Vec<Input> {
    let mut inputs = Vec::new();
    centred_window(ui, title, |ui| {
        ui.label(RichText::new(title).strong().size(TEXT + 1.0));
        ui.separator();
        let base = ui.visuals().text_color();
        for (index, row) in rows.into_iter().enumerate() {
            if ui
                .selectable_label(index == selected, job(row, base))
                .clicked()
            {
                inputs.push(Input::List(OverlayInput::Focus(index)));
                inputs.push(Input::List(OverlayInput::Select));
            }
        }
        ui.separator();
        ui.label(RichText::new("↑↓ choose  Enter pick  Esc cancel").weak());
    });
    inputs
}
