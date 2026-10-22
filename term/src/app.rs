//! The title-menu loop, and the blocking, turn-based game loop and panel layout. Keys go to `front`'s `Session`; this module only reads the terminal and draws.

use std::io;
use std::time::Duration;

use front::input::Input;
use front::menu::{Action, Menu};
use front::overlay::Views;
use front::session::{Keys, Mode, Outcome, Session};
use front::title::{self, Next, Setup};
use game::{Fx, render_view, spells, throwables};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::input::{map_aim_key, map_key_event, map_menu_key, map_overlay_key};
use crate::menu;
use crate::overlay;
use crate::side_panel;
use crate::targeting::{self, Flying};
use crate::tui::Backend;
use crate::widgets::map_widget::{MapWidget, viewport_tiles};

type Term = ratatui::Terminal<Backend>;

/// Shows the title menu until it quits, performing its actions; quitting a
/// game saves it and returns here. `first` runs before the menu is shown.
pub fn session(terminal: &mut Term, setup: &Setup, first: Option<Action>) -> io::Result<()> {
    let mut menu = Menu::new(Vec::new());
    title::refresh(&mut menu, &setup.saves_dir);
    let mut pending = first;
    loop {
        let action = match pending.take() {
            Some(action) => action,
            None => {
                terminal.draw(|frame| menu::draw(frame, &menu))?;
                let Event::Key(key) = event::read()? else {
                    continue;
                };
                let Some(action) = map_menu_key(key, menu.typing()).and_then(|i| menu.handle(i))
                else {
                    continue;
                };
                action
            }
        };
        match title::perform(&mut menu, &setup.saves_dir, action) {
            Some(Next::Quit) => return Ok(()),
            Some(Next::Play(character)) => {
                let message = play(terminal, setup, &character)?;
                let (Ok(message) | Err(message)) = message;
                menu.show(message);
                title::refresh(&mut menu, &setup.saves_dir);
            }
            None => {}
        }
    }
}

/// Plays `character` until they quit or die, then saves. The outer error
/// is the terminal's; the inner one is a message for the menu.
fn play(terminal: &mut Term, setup: &Setup, character: &str) -> io::Result<Result<String, String>> {
    let mut game = match title::start(setup, character) {
        Ok(game) => game,
        Err(err) => return Ok(Err(err)),
    };
    let ran = run(terminal, &mut game.session);
    // Saved even if the terminal failed, so the run isn't lost.
    let saved = title::finish(&game);
    ran?;
    Ok(saved)
}

/// Blocks on `event::read()` and redraws after every event (including
/// resize) — no tick loop: `game::take_turn` runs the world until the
/// player is ready again. Auto-explore and practice poll briefly between
/// their steps so a key can interrupt.
pub fn run(terminal: &mut Term, session: &mut Session) -> io::Result<()> {
    terminal.draw(|frame| draw(frame, session, None))?;
    loop {
        if session.auto().is_some() {
            let going = session.auto_step();
            animate(terminal, session)?;
            terminal.draw(|frame| draw(frame, session, None))?;
            if going && key_pressed_within(AUTO_STEP_DELAY)? {
                session.stop_auto();
            }
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                let Some(input) = map_key(session.keys(), key) else {
                    continue;
                };
                if session.handle(input) == Outcome::Quit {
                    return Ok(());
                }
                animate(terminal, session)?;
            }
            Event::Resize(_, _) => {}
            _ => continue,
        }
        terminal.draw(|frame| draw(frame, session, None))?;
    }
}

/// The key, through the key map the session's mode reads.
fn map_key(keys: Keys, key: KeyEvent) -> Option<Input> {
    match keys {
        Keys::Game => map_key_event(key).map(Input::Game),
        Keys::List => map_overlay_key(key).map(Input::List),
        Keys::Cursor => map_aim_key(key).map(Input::Cursor),
        Keys::Any => (key.kind != KeyEventKind::Release).then_some(Input::Any),
    }
}

/// Time a missile spends on each tile it crosses.
const MISSILE_STEP_DELAY: Duration = Duration::from_millis(40);

/// Plays back the flights the last turn launched, a frame per tile. The
/// world has already resolved the shot, so this is only a picture of how it
/// got there; any key press cuts it short.
fn animate(terminal: &mut Term, session: &mut Session) -> io::Result<()> {
    let flights = session.take_fx().into_iter().filter_map(|fx| match fx {
        Fx::Flight(trail) => Some(trail),
        _ => None,
    });
    for trail in flights {
        for &at in &trail.path {
            let flying = Flying {
                glyph: trail.glyph,
                fg: trail.fg,
                at,
            };
            terminal.draw(|frame| draw(frame, session, Some(flying)))?;
            if key_pressed_within(MISSILE_STEP_DELAY)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Pause between auto-explore or practice steps, so the walk is visible and
/// a key press can interrupt it.
const AUTO_STEP_DELAY: Duration = Duration::from_millis(25);

/// Key releases are ignored, or releasing the key that started a run would
/// stop it.
fn key_pressed_within(timeout: Duration) -> io::Result<bool> {
    while event::poll(timeout)? {
        if let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn draw(frame: &mut Frame, session: &Session, flying: Option<Flying>) {
    let state = session.state();
    let (main, side) = side_panel::split(frame.area());
    if let Some(side) = side {
        side_panel::draw(
            frame,
            side,
            &side_panel::Context {
                state,
                character: session.character(),
            },
        );
    }
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(3),
        ],
    )
    .split(main);

    let (width, height) = viewport_tiles(chunks[0]);
    let view = render_view(state, width, height);
    frame.render_widget(MapWidget::new(&view), chunks[0]);

    let mut log_lines: Vec<Line> = state
        .log
        .recent(5)
        .map(|m| Line::from(m.as_str()))
        .collect();
    log_lines.reverse();
    frame.render_widget(
        Paragraph::new(log_lines).block(Block::bordered().title("Log")),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(session.status()).block(Block::bordered().title("Status")),
        chunks[2],
    );

    match session.mode() {
        Mode::Overlay(panel) => {
            let views = Views::of(state);
            overlay::draw(frame, chunks[0], panel, &views.contents());
        }
        Mode::Choose(chooser) => {
            let lines = targeting::throwable_lines(&throwables(state));
            targeting::draw_chooser(frame, chunks[0], chooser, "Throw what?", lines);
        }
        Mode::ChooseSpell(chooser) => {
            let lines = targeting::spell_lines(&spells(state));
            targeting::draw_chooser(frame, chunks[0], chooser, "Cast what?", lines);
        }
        Mode::Aim(aiming) => targeting::draw_aim(frame, chunks[0], state, aiming),
        Mode::Look(looking) => targeting::draw_look(frame, chunks[0], state, looking),
        // Frozen (term/CLAUDE.md): screens added since only point to the
        // window; Esc backs out as in any list.
        Mode::ChoosePhase(_) => window_only(frame, chunks[0]),
        Mode::Play => {}
    }
    if let Some(flying) = flying {
        targeting::draw_missile(frame, chunks[0], state, flying);
    }
}

/// A screen `term` doesn't draw: one line pointing to the window.
fn window_only(frame: &mut Frame, area: Rect) {
    let line = "This screen needs the window (cargo run -p gui). Esc backs out.";
    let width = u16::try_from(line.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.width);
    let box_area = area.centered(Constraint::Length(width), Constraint::Length(3));
    frame.render_widget(Clear, box_area);
    frame.render_widget(Paragraph::new(line).block(Block::bordered()), box_area);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use game::{Blueprints, GameState, InputEvent, Shot, Tick};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use front::input::OverlayInput;

    use super::*;

    fn state() -> GameState {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        // A floor, not the camp: these draw the map around the player.
        let blueprints = Blueprints::load(&data).unwrap().with_camp(None);
        GameState::new(3, blueprints).unwrap()
    }

    fn session() -> Session {
        Session::new(state(), "hero")
    }

    /// One string per row of a `width`×30 terminal: a `width`×20 map panel
    /// above the 7-row log and 3-row status.
    fn drawn_rows(session: &Session, width: u16, flying: Option<Flying>) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| draw(frame, session, flying)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    fn screen(session: &Session) -> String {
        drawn_rows(session, 60, None).concat()
    }

    fn buffer(session: &Session) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal.draw(|frame| draw(frame, session, None)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn the_map_panel_shows_the_viewport_around_the_player() {
        let rows = drawn_rows(&session(), 60, None);
        // The player starts mid-zone, so the camera centres on them.
        assert_eq!(rows[10].chars().nth(30), Some('☺'));
        let panel: String = rows[..20].concat();
        assert_eq!(panel.matches('☺').count(), 1);
        assert!(panel.contains('#') && panel.contains('.'));
    }

    #[test]
    fn aiming_marks_the_cursor_and_describes_the_shot_in_the_status_bar() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Fire));
        let buffer = buffer(&session);
        // Nothing in view at the start: the cursor rests on the player, centred.
        let cursor = buffer.cell((30, 10)).unwrap();
        assert_eq!(cursor.symbol(), "☺");
        assert!(cursor.modifier.contains(ratatui::style::Modifier::REVERSED));
        let status: String = (27..30)
            .flat_map(|y| (0..60).map(move |x| (x, y)))
            .map(|at| buffer.cell(at).unwrap().symbol())
            .collect();
        assert!(status.contains("Enter: fire"), "{status}");
    }

    #[test]
    fn the_spell_list_shows_each_spell_with_its_disciplines_price() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Cast));
        let screen = screen(&session);
        assert!(screen.contains("Cast what?"), "{screen}");
        // Willpower 18 and Intelligence 18: +1 each.
        assert!(
            screen.contains("firebolt  r7  4-7  6 turn recharge"),
            "{screen}"
        );
        assert!(
            screen.contains("magic missile  r8  3-5  3 mana"),
            "{screen}"
        );
        assert!(
            screen.contains("fireball  r6  burst 1  3-6  12 turn recharge"),
            "{screen}"
        );
        assert!(
            screen.contains("arcane lance  r8  beam  3-5  6 mana"),
            "{screen}"
        );
        assert!(
            screen.contains("summon wolf  r5  1 spirit wolf  9 mana"),
            "{screen}"
        );
    }

    #[test]
    fn aiming_an_area_spell_marks_its_ground() {
        let mut session = session();
        let listed = spells(session.state());
        let index = listed
            .iter()
            .position(|spell| spell.name == "fireball")
            .unwrap();
        let shot = Shot::Cast(listed[index].spell);
        session.handle(Input::Game(InputEvent::Cast));
        for _ in 0..index {
            session.handle(Input::List(OverlayInput::Down));
        }
        session.handle(Input::List(OverlayInput::Select));
        assert!(matches!(session.mode(), Mode::Aim(_)));
        let (origin, _) = game::aim_range(session.state(), shot).unwrap();
        // With nothing in view the cursor rests on the player, so the burst
        // is centred there; its second tile is a neighbour.
        let area = game::aim(session.state(), shot, origin).unwrap().area;
        let tile = area[1];
        let at = (
            u16::try_from(30 + tile.0 - origin.0).unwrap(),
            u16::try_from(10 + tile.1 - origin.1).unwrap(),
        );
        let cell = buffer(&session).cell(at).unwrap().clone();
        assert_eq!(cell.bg, crate::widgets::color(targeting::AREA));
    }

    #[test]
    fn aiming_a_spell_says_cast_in_the_status_bar() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Cast));
        session.handle(Input::List(OverlayInput::Select));
        assert!(screen(&session).contains("Enter: cast"));
    }

    #[test]
    fn a_flying_missile_is_drawn_on_its_tile() {
        let session = session();
        let (origin, _) = game::aim_range(session.state(), Shot::Fire).unwrap();
        let flying = Flying {
            glyph: '\\',
            fg: game::Rgb(1, 2, 3),
            at: origin,
        };
        // The camera centres on the player.
        let rows = drawn_rows(&session, 60, Some(flying));
        assert_eq!(rows[10].chars().nth(30), Some('\\'));
    }

    #[test]
    fn the_inventory_panel_hovers_over_the_map() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Inventory));
        let rows = drawn_rows(&session, 60, None);
        let map: String = rows[..20].concat();
        assert!(map.contains("Inventory") && map.contains("You carry nothing."));
        assert!(rows[20..].concat().contains("Log"), "the log stays visible");
    }

    #[test]
    fn wide_terminals_get_the_side_column_beside_the_map() {
        let rows = drawn_rows(&session(), 100, None);
        let side_x = usize::from(100 - side_panel::WIDTH);
        let side: String = rows
            .iter()
            .map(|row| row.chars().skip(side_x).collect::<String>())
            .collect();
        let main: String = rows
            .iter()
            .map(|row| row.chars().take(side_x).collect::<String>())
            .collect();
        assert!(side.contains("hero") && side.contains("Equipment"));
        assert_eq!(
            main.matches('☺').count(),
            1,
            "the map still shows the player"
        );
        assert!(main.contains("Log") && !side.contains("Log"));
    }

    #[test]
    fn status_shows_the_turn() {
        let mut state = state();
        state.now = Tick(250);
        assert!(screen(&Session::new(state, "hero")).contains("Turn: 2  Seed: 3"));
    }

    #[test]
    fn status_announces_game_over() {
        let mut state = state();
        state.now = Tick(700);
        // What the frontend reads of a death; how dying works is `game`'s.
        state.game_over = true;
        let screen = screen(&Session::new(state, "hero"));
        assert!(screen.contains("You died on turn 7. Press any key to return to the title."));
        assert!(screen.contains('☺'), "the dead player is still drawn");
    }

    #[test]
    fn keys_go_through_the_map_for_the_mode() {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            map_key(Keys::List, key),
            Some(Input::List(OverlayInput::Select))
        );
        assert_eq!(map_key(Keys::Any, key), Some(Input::Any));
        assert_eq!(map_key(Keys::Game, key), None);
    }
}
