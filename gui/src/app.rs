//! The window: the title menu, then a game's map, side column and bottom strip. Key presses go to `front`'s `Menu` or `Session`; this module paces auto runs and draws.

use std::time::{Duration, Instant};

use eframe::egui::{self, Sense, ViewportCommand};
use front::input::Input;
use front::menu::Menu;
use front::session::{Keys, Outcome};
use front::title::{self, Game, Next, Setup};
use game::render_layers;

use crate::fx::Effects;
use crate::input::{Press, map_cursor, map_game, map_list, map_menu, presses};
use crate::map_view::{self, Layout};
use crate::menu_view;
use crate::modes;
use crate::panels::{self, SIDE_WIDTH};
use crate::sprites::Atlas;
use crate::theme::BG;

/// Pause between auto-explore or practice steps, so the walk is visible and
/// a key press can interrupt it.
const AUTO_STEP_DELAY: Duration = Duration::from_millis(25);
/// How often the window redraws for animated sprites.
const SPRITE_FRAME: Duration = Duration::from_millis(50);

pub struct App {
    setup: Setup,
    menu: Menu,
    game: Option<Game>,
    /// When the next auto-run step is due.
    next_step: Option<Instant>,
    /// The world tile the pointer was over last frame.
    hovered: Option<(i32, i32)>,
    /// The panel row last scrolled into view.
    scrolled: Option<usize>,
    effects: Effects,
    /// Sprites instead of glyphs (F2), and their atlas once loaded.
    sprites: bool,
    atlas: Option<Atlas>,
    /// The clock sprites animate on.
    started: Instant,
}

/// Whether the window should stay open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

impl App {
    /// Opens on the title menu, or straight into `first`'s game.
    pub fn new(setup: Setup, first: Option<String>) -> Self {
        let mut menu = Menu::new(Vec::new());
        title::refresh(&mut menu, &setup.saves_dir);
        menu.set_origins(title::origins(&setup));
        let mut app = Self {
            setup,
            menu,
            game: None,
            next_step: None,
            hovered: None,
            scrolled: None,
            effects: Effects::default(),
            sprites: false,
            atlas: None,
            started: Instant::now(),
        };
        if let Some(character) = first {
            app.play(&character, None);
        }
        app
    }

    pub fn game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    fn play(&mut self, character: &str, origin: Option<&str>) {
        match title::start(&self.setup, character, origin) {
            Ok(game) => self.game = Some(game),
            Err(err) => self.menu.show(err),
        }
    }

    /// Saves and returns to the menu with what happened.
    fn leave_game(&mut self) {
        if let Some(game) = self.game.take() {
            let (Ok(message) | Err(message)) = title::finish(&game);
            self.menu.show(message);
            title::refresh(&mut self.menu, &self.setup.saves_dir);
        }
        self.next_step = None;
        self.effects.finish();
    }

    /// Feeds this frame's key presses to the menu or the game, then takes an
    /// auto-run step if one is due. Any key finishes the effects playing.
    pub fn update(&mut self, presses: &[Press], now: Instant) -> Flow {
        if !presses.is_empty() {
            self.effects.finish();
        }
        for &press in presses {
            if press == Press::F2 {
                self.sprites = !self.sprites;
                continue;
            }
            if self.press(press) == Flow::Exit {
                return Flow::Exit;
            }
        }
        if let Some(game) = &mut self.game {
            let session = &mut game.session;
            if session.auto().is_some() {
                let due = self.next_step.get_or_insert(now + AUTO_STEP_DELAY);
                if now >= *due {
                    session.auto_step();
                    *due = now + AUTO_STEP_DELAY;
                }
            } else {
                self.next_step = None;
            }
            self.effects.add(session.take_fx(), now);
        }
        Flow::Continue
    }

    fn press(&mut self, press: Press) -> Flow {
        let Some(game) = &mut self.game else {
            let Some(input) = map_menu(press, self.menu.typing()) else {
                return Flow::Continue;
            };
            let Some(action) = self.menu.handle(input) else {
                return Flow::Continue;
            };
            return match title::perform(&mut self.menu, &self.setup.saves_dir, action) {
                Some(Next::Quit) => Flow::Exit,
                Some(Next::Play { character, origin }) => {
                    self.play(&character, origin.as_deref());
                    Flow::Continue
                }
                None => Flow::Continue,
            };
        };
        let session = &mut game.session;
        let input = match session.keys() {
            Keys::Game => map_game(press).map(Input::Game),
            Keys::List => map_list(press).map(Input::List),
            Keys::Cursor => map_cursor(press).map(Input::Cursor),
            Keys::Any => Some(Input::Any),
        };
        // During a run any key only stops it, mapped or not.
        let input = match (input, session.auto()) {
            (Some(input), _) => input,
            (None, Some(_)) => Input::Any,
            (None, None) => return Flow::Continue,
        };
        if session.handle(input) == Outcome::Quit {
            self.leave_game();
        }
        Flow::Continue
    }

    /// Draws the menu or the game, then hands the session whatever the
    /// pointer did to the map and windows, as the same inputs keys send.
    pub fn show(&mut self, ui: &mut egui::Ui, now: Instant) {
        let Some(game) = &mut self.game else {
            egui::CentralPanel::default().show(ui, |ui| menu_view::draw(ui, &self.menu));
            return;
        };
        if self.sprites && self.atlas.is_none() {
            match Atlas::load(ui.ctx(), game.session.state().blueprints.tiles()) {
                Ok(atlas) => self.atlas = Some(atlas),
                Err(err) => {
                    eprintln!("No sprites: {err}");
                    self.sprites = false;
                }
            }
        }
        let session = &game.session;
        let hovered = &mut self.hovered;
        let effects = &self.effects;
        let sprites = self
            .atlas
            .as_ref()
            .filter(|_| self.sprites)
            .map(|atlas| (atlas, now - self.started));
        let mut inputs = Vec::new();
        egui::Panel::right("side")
            .frame(egui::Frame::new().fill(BG))
            .show_separator_line(false)
            .resizable(false)
            .exact_size(SIDE_WIDTH)
            .show(ui, |ui| panels::side(ui, session));
        egui::Panel::bottom("log")
            .frame(egui::Frame::new().fill(BG))
            .show_separator_line(false)
            .resizable(false)
            .exact_size(panels::BOTTOM_HEIGHT)
            .show(ui, |ui| panels::bottom(ui, session));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ui, |ui| {
                let rect = ui.max_rect();
                let response = ui.interact(rect, ui.scope_id().with("map"), Sense::click());
                let layout = Layout::new(rect).shifted(effects.shake(now));
                let state = session.state();
                let (mut grid, light) = render_layers(state, layout.width, layout.height);
                modes::mark(&mut grid, &layout, session);
                effects.mark(&mut grid, state, &layout, now);
                map_view::draw(ui.painter(), &layout, &grid, &light, sprites);
                effects.draw(ui.painter(), state, &layout, now);
                let at = modes::pointed(session.state(), &layout, response.hover_pos());
                let moved = if at == *hovered { None } else { at };
                *hovered = at;
                inputs.extend(modes::map_pointer(session, moved, &response, at));
                modes::hover_tooltip(&response, session, at);
            });
        inputs.extend(modes::windows(ui, session, &mut self.scrolled));
        if !inputs.is_empty() {
            self.effects.finish();
        }
        let mut quit = false;
        for input in inputs {
            quit |= game.session.handle(input) == Outcome::Quit;
        }
        self.effects.add(game.session.take_fx(), now);
        if quit {
            self.leave_game();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = ui.input(|input| input.events.clone());
        let now = Instant::now();
        if self.update(&presses(&events), now) == Flow::Exit {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
        self.show(ui, now);
        if self.effects.update(now) {
            ui.ctx().request_repaint();
        } else if self.sprites && self.game.is_some() {
            // Animated sprites turn on the clock, not on input.
            ui.ctx().request_repaint_after(SPRITE_FRAME);
        }
        if self
            .game
            .as_ref()
            .is_some_and(|game| game.session.auto().is_some())
        {
            ui.ctx().request_repaint_after(AUTO_STEP_DELAY);
        }
    }

    /// Closing the window mid-game saves it, like quitting.
    fn on_exit(&mut self) {
        self.leave_game();
    }
}

#[cfg(test)]
mod shots;
#[cfg(test)]
mod tests;
