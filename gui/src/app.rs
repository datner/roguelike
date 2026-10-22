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
/// a key press can interrupt it (as in `term`).
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
            app.play(&character);
        }
        app
    }

    pub fn game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    fn play(&mut self, character: &str) {
        match title::start(&self.setup, character) {
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
                Some(Next::Play(character)) => {
                    self.play(&character);
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
mod tests {
    use std::path::Path;

    use eframe::egui::Event;
    use egui_kittest::Harness;
    use game::Tick;

    use super::*;

    /// A fresh saves directory per test, under the system temp dir.
    fn setup(name: &str) -> Setup {
        let saves_dir =
            std::env::temp_dir().join(format!("roguelike-gui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&saves_dir);
        Setup {
            data_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
            saves_dir,
            seed: Some(3),
        }
    }

    fn now(app: &App) -> Tick {
        app.game().unwrap().session.state().now
    }

    #[test]
    fn a_named_character_starts_straight_away_and_moves() {
        let mut app = App::new(setup("moves"), Some("hero".into()));
        let before = now(&app);
        app.update(&[Press::Char('.')], Instant::now());
        assert!(now(&app) > before);
    }

    #[test]
    fn quitting_saves_and_returns_to_the_menu() {
        let setup = setup("quits");
        let saves = setup.saves_dir.clone();
        let mut app = App::new(setup, Some("hero".into()));
        app.update(&[Press::Char('q')], Instant::now());
        assert!(app.game().is_none());
        assert_eq!(front::saves::list(&saves).unwrap(), ["hero"]);
        // Esc on the menu closes the window.
        assert_eq!(app.update(&[Press::Esc], Instant::now()), Flow::Exit);
    }

    #[test]
    fn auto_explore_steps_on_its_clock_and_any_key_stops_it() {
        let mut app = App::new(setup("explores"), Some("hero".into()));
        let start = Instant::now();
        app.update(&[Press::Char('x')], start);
        let before = now(&app);
        app.update(&[], start);
        assert_eq!(now(&app), before, "not due yet");
        app.update(&[], start + AUTO_STEP_DELAY);
        assert!(now(&app) > before, "a step was taken");
        let stepped = now(&app);
        app.update(&[Press::Char('z')], start + AUTO_STEP_DELAY * 2);
        assert_eq!(now(&app), stepped, "an unmapped key only stops the run");
        assert!(app.game().unwrap().session.auto().is_none());
    }

    #[test]
    fn a_shot_plays_its_flight_until_a_key_finishes_it() {
        let mut app = App::new(setup("fx"), Some("hero".into()));
        let now = Instant::now();
        let aim_and_shoot = [
            Press::Char('f'),
            Press::Right,
            Press::Right,
            Press::Right,
            Press::Enter,
        ];
        app.update(&aim_and_shoot, now);
        assert!(app.effects.update(now), "the arrow is in flight");
        app.update(&[Press::Char('z')], now);
        assert!(!app.effects.update(now), "a key finished it");
    }

    #[test]
    fn f2_switches_sprites_without_taking_a_turn() {
        let mut app = App::new(setup("sprites"), Some("hero".into()));
        let before = now(&app);
        app.update(&[Press::F2], Instant::now());
        assert!(app.sprites);
        assert_eq!(now(&app), before);
        app.update(&[Press::F2], Instant::now());
        assert!(!app.sprites);
    }

    /// The app in a headless window, fed key and pointer events like eframe
    /// does. Fonts added in one pass apply from the next, as `main` adds
    /// them before the first. The harness panics on a glyph no font has, so
    /// every test through it also checks what it drew can be drawn.
    fn window(name: &str) -> Harness<'static, App> {
        harness(App::new(setup(name), Some("hero".into())))
    }

    fn harness(app: App) -> Harness<'static, App> {
        let mut fonts_installed = false;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(
                move |ui, app: &mut App| {
                    if !fonts_installed {
                        crate::theme::apply(ui.ctx());
                        fonts_installed = true;
                        ui.ctx().request_repaint();
                        return;
                    }
                    let events = ui.input(|input| input.events.clone());
                    let now = Instant::now();
                    app.update(&presses(&events), now);
                    app.show(ui, now);
                },
                app,
            );
        harness.run();
        harness
    }

    fn type_text(harness: &mut Harness<'static, App>, text: &str) {
        harness.event(Event::Text(text.into()));
        harness.run();
    }

    fn mode(harness: &Harness<'static, App>) -> String {
        format!("{:?}", harness.state().game().unwrap().session.mode())
    }

    #[test]
    fn the_window_plays_from_typed_keys() {
        let mut harness = window("window");
        let before = now(harness.state());
        type_text(&mut harness, ".");
        assert!(now(harness.state()) > before);
    }

    #[test]
    fn every_mode_draws_over_the_map() {
        let mut harness = window("modes");
        for (open, shown) in [
            ("i", "Overlay"),
            ("q", "Play"),
            ("e", "Overlay"),
            ("q", "Play"),
            ("S", "Overlay"),
            ("q", "Play"),
            (";", "Look"),
            ("q", "Play"),
            ("c", "ChooseSpell"),
            ("q", "Play"),
            ("f", "Aim"),
        ] {
            type_text(&mut harness, open);
            assert!(
                mode(&harness).starts_with(shown),
                "{open}: {}",
                mode(&harness)
            );
        }
    }

    #[test]
    fn hovering_the_map_moves_the_aim_cursor_and_a_click_shoots() {
        let mut harness = window("pointer");
        type_text(&mut harness, "f");
        let before = now(harness.state());
        // The map fills the window left of the side column, above the log;
        // a point a few tiles right of its middle is on the map.
        let map_middle = egui::pos2((1280.0 - SIDE_WIDTH) / 2.0, 300.0);
        harness.hover_at(map_middle + egui::vec2(3.0 * crate::map_view::TILE.x, 0.0));
        harness.run();
        assert!(mode(&harness).starts_with("Aim"));
        harness.event(Event::PointerButton {
            pos: map_middle + egui::vec2(3.0 * crate::map_view::TILE.x, 0.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.event(Event::PointerButton {
            pos: map_middle + egui::vec2(3.0 * crate::map_view::TILE.x, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        assert!(now(harness.state()) > before, "the click fired");
        assert!(mode(&harness).starts_with("Play"));
    }

    /// Renders each screen to PNGs for a human to look at, since nobody may
    /// run the window from here. Needs a GPU, so it only runs when asked:
    /// `GUI_SCREENSHOTS=<dir> cargo test -p gui screenshots -- --ignored`.
    #[test]
    #[ignore = "needs a GPU; writes PNGs to GUI_SCREENSHOTS"]
    fn screenshots() {
        let dir = std::path::PathBuf::from(std::env::var("GUI_SCREENSHOTS").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let mut menu = harness(App::new(setup("screenshots-menu"), None));
        let image = menu.render().unwrap();
        image.save(dir.join("menu.png")).unwrap();
        let mut harness = window("screenshots");
        for (keys, name) in [
            ("", "play"),
            ("i", "inventory"),
            ("q;", "look"),
            ("qc", "spells"),
            ("qf", "aim"),
        ] {
            for key in keys.chars() {
                type_text(&mut harness, &key.to_string());
            }
            let image = harness.render().unwrap();
            image.save(dir.join(format!("{name}.png"))).unwrap();
        }
        // Effects around the player, caught early in their rise.
        type_text(&mut harness, "q;");
        let front::session::Mode::Look(looking) = harness.state().game().unwrap().session.mode()
        else {
            panic!("looking");
        };
        let (x, y) = looking.cursor();
        type_text(&mut harness, "q");
        let fx = vec![
            game::Fx::Damage {
                at: (x, y),
                amount: 3,
                blocked: 0,
                to_player: true,
            },
            game::Fx::Outcome {
                at: (x + 3, y),
                kind: game::Outcome::Deflect,
            },
            game::Fx::Death { at: (x - 2, y) },
            game::Fx::Flight(game::Trail {
                glyph: '|',
                fg: game::Rgb(200, 160, 90),
                path: (1..8).map(|dx| (x, y - dx)).collect(),
            }),
        ];
        harness.state_mut().effects.add(fx, Instant::now());
        harness.step();
        let image = harness.render().unwrap();
        image.save(dir.join("effects.png")).unwrap();
        harness.event(Event::Key {
            key: egui::Key::F2,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
        assert!(harness.state().atlas.is_some(), "the atlas loaded");
        let image = harness.render().unwrap();
        image.save(dir.join("sprites.png")).unwrap();
    }
}
