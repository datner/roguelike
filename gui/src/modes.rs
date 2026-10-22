//! Draws what the session's mode puts on and over the map: the look and aim cursors and marks, the throw and spell choosers, and the inventory panel. Pointer use on them becomes the same `front` inputs keys send, collected for `app` to hand to the session after drawing.

use eframe::egui::{self, Align2, Color32, Frame, ScrollArea, text::LayoutJob, vec2};

const PLACEHOLDER: Color32 = Color32::PLACEHOLDER;
use front::input::{AimInput, Input, OverlayInput};
use front::overlay::{LineKind, TABS, Title, Views};
use front::session::{Mode, Session};
use front::targeting::spell_text;
use front::text::{self, Seg};
use game::{Cell, GameState, Grid, Rgb, aim, camera, phases, spells, throwables};

use crate::map_view::TILE;
use crate::map_view::{Layout, screen_tile};
use crate::panels::{SIDE_WIDTH, job};
use crate::theme::{self, BG, BRIGHT, DIM, FG, boxed};

const PATH: Rgb = Rgb(110, 90, 20);
const LANDING: Rgb = Rgb(170, 60, 20);
const AREA: Rgb = Rgb(120, 40, 90);

/// The cell showing world tile `at`, if the map shows it.
fn cell_at<'a>(
    grid: &'a mut Grid<Cell>,
    state: &GameState,
    layout: &Layout,
    at: (i32, i32),
) -> Option<&'a mut Cell> {
    let (x, y) = screen_tile(state, layout, at)?;
    grid.get_mut(x, y)
}

/// Reverse video, the cursor's mark; on an unlit blank tile, a light block.
fn reverse(cell: &mut Cell) {
    std::mem::swap(&mut cell.fg, &mut cell.bg);
    if cell.bg == Rgb(0, 0, 0) {
        cell.bg = Rgb(204, 204, 204);
    }
}

/// Marks on the map's cells before they are drawn: the look cursor, or an aimed shot's path, area and landing tile
/// as backgrounds (from `game::aim`) and its cursor, in reverse video.
pub fn mark(grid: &mut Grid<Cell>, layout: &Layout, session: &Session) {
    let state = session.state();
    match session.mode() {
        Mode::Look(looking) => {
            if let Some(cell) = cell_at(grid, state, layout, looking.cursor()) {
                reverse(cell);
            }
        }
        Mode::Aim(aiming) => {
            let Some(shot) = aim(state, aiming.shot(), aiming.cursor()) else {
                return;
            };
            let mut mark = |at, rgb| {
                if let Some(cell) = cell_at(grid, state, layout, at) {
                    cell.bg = rgb;
                }
            };
            for &at in &shot.path {
                mark(at, PATH);
            }
            for &at in shot.area.iter().chain(&shot.summons) {
                mark(at, AREA);
            }
            mark(shot.landing, LANDING);
            if let Some(cell) = cell_at(grid, state, layout, aiming.cursor()) {
                reverse(cell);
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
    response.clone().on_hover_ui_at_pointer(|ui| {
        for row in text::inspection(session.state(), at) {
            ui.label(job(row, FG));
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
        Mode::ChoosePhase(chooser) => {
            let rows = phases(state).iter().map(text::phase).collect();
            chooser_window(ui, "Wait for which phase?", rows, chooser.selected())
        }
        Mode::Overlay(overlay) => {
            let views = Views::of(state);
            let contents = views.contents();
            let view = overlay.view(&contents);
            let actions = overlay.actions(&contents);
            let mut inputs = Vec::new();
            let mut tab_inputs = Vec::new();
            let title = |ui: &mut egui::Ui| match &view.title {
                Title::Tabs(active) => {
                    for (i, (tab, name)) in TABS.into_iter().enumerate() {
                        if i > 0 {
                            ui.label(theme::line(" ", FG));
                        }
                        let shown = tab == *active;
                        let colour = if shown { PLACEHOLDER } else { DIM };
                        let label = theme::line(&format!(" {name} "), colour);
                        if ui.selectable_label(shown, label).clicked() && !shown {
                            tab_inputs.push(Input::List(tab_input(tab)));
                        }
                    }
                }
                Title::Text(text) => {
                    ui.label(theme::line(text, FG));
                }
            };
            centred_window(ui, "panel", |ui| {
                boxed(ui, title, Some(view.hint), |ui| {
                    ScrollArea::vertical()
                        .max_height(ui.ctx().content_rect().height() * 0.6)
                        .show(ui, |ui| {
                            for line in &view.lines {
                                let Some(index) = line.index else {
                                    let base = if line.kind == LineKind::Heading {
                                        BRIGHT
                                    } else {
                                        FG
                                    };
                                    ui.label(job(line.row.clone(), base));
                                    continue;
                                };
                                let selected = line.kind == LineKind::Selected;
                                let response =
                                    ui.selectable_label(selected, row(line.row.clone(), selected));
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
                                            if ui.button(theme::line(name, FG)).clicked() {
                                                inputs.push(Input::List(input));
                                                ui.close();
                                            }
                                        }
                                    });
                                }
                            }
                        });
                });
            });
            tab_inputs.extend(inputs);
            tab_inputs
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

/// A row to pick: in the widget's text colour, so hovering brightens it,
/// and all of it in reverse video when selected.
fn row(segs: Vec<Seg>, selected: bool) -> LayoutJob {
    let mut job = job(segs, PLACEHOLDER);
    if selected {
        for section in &mut job.sections {
            section.format.color = PLACEHOLDER;
        }
    }
    job
}

/// A box in the middle of the map: at least 46 cells
/// wide.
fn centred_window(ui: &egui::Ui, id: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(Frame::new().fill(BG))
        .anchor(Align2::CENTER_CENTER, vec2(-SIDE_WIDTH / 2.0, 0.0))
        .fixed_size([56.0 * TILE.x, 0.0])
        .show(ui.ctx(), contents);
}

/// A list to pick from: a click picks the row, as Enter does.
fn chooser_window(ui: &egui::Ui, title: &str, rows: Vec<Vec<Seg>>, selected: usize) -> Vec<Input> {
    let mut inputs = Vec::new();
    centred_window(ui, title, |ui| {
        boxed(
            ui,
            theme::title(title),
            Some("↑↓ choose  Enter pick  Esc cancel"),
            |ui| {
                for (index, row) in rows.into_iter().enumerate() {
                    if ui
                        .selectable_label(index == selected, self::row(row, index == selected))
                        .clicked()
                    {
                        inputs.push(Input::List(OverlayInput::Focus(index)));
                        inputs.push(Input::List(OverlayInput::Select));
                    }
                }
            },
        );
    });
    inputs
}
