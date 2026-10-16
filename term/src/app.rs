//! The title-menu session, and the blocking, turn-based game loop and panel layout.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use game::{
    Blueprints, CaveParams, FireStep, GameState, InputEvent, PlayerAction, Shot,
    cycle_explore_items, equipment_view, explore_step, fire_step, inventory_view, random_seed,
    render_view, spells, take_trails, take_turn, throwables,
};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::input::{map_aim_key, map_key_event, map_menu_key, map_overlay_key};
use crate::menu::{self, Action, Menu};
use crate::overlay::{self, Contents, Overlay, OverlayAction, Tab};
use crate::saves::{self, Loaded};
use crate::side_panel;
use crate::targeting::{self, AimAction, Aiming, Chooser, ChooserAction, Flying};
use crate::tui::Backend;
use crate::widgets::map_widget::{MapWidget, viewport_tiles};

type Term = ratatui::Terminal<Backend>;

pub struct Setup {
    pub data_dir: PathBuf,
    pub saves_dir: PathBuf,
    /// Seed for new games; loaded games keep their own.
    pub seed: Option<u64>,
}

/// Shows the title menu until it quits, performing its actions; quitting a
/// game saves it and returns here. `first` runs before the menu is shown.
pub fn session(terminal: &mut Term, setup: &Setup, first: Option<Action>) -> io::Result<()> {
    let mut menu = Menu::new(Vec::new());
    refresh(&mut menu, &setup.saves_dir);
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
        let dir = &setup.saves_dir;
        let outcome = match action {
            Action::Quit => return Ok(()),
            Action::Play(character) => play(terminal, setup, &character)?,
            Action::Delete(character) => {
                saves::delete(dir, &character).map(|()| format!("Deleted {character}."))
            }
            Action::Rename { from, to } => {
                saves::rename(dir, &from, &to).map(|()| format!("Renamed {from} to {to}."))
            }
        };
        let (Ok(message) | Err(message)) = outcome;
        menu.show(message);
        refresh(&mut menu, dir);
    }
}

fn refresh(menu: &mut Menu, dir: &Path) {
    match saves::list(dir) {
        Ok(list) => menu.set_saves(list),
        Err(err) => menu.show(err),
    }
}

/// Plays `character` until they quit or die, then saves. The outer error
/// is the terminal's; the inner one is a message for the menu.
fn play(terminal: &mut Term, setup: &Setup, character: &str) -> io::Result<Result<String, String>> {
    let path = match saves::path(&setup.saves_dir, character) {
        Ok(path) => path,
        Err(err) => return Ok(Err(err)),
    };
    let mut state = match start(setup, character, &path) {
        Ok(state) => state,
        Err(err) => return Ok(Err(err)),
    };
    let ran = run(terminal, &mut state, character);
    // Saved even if the terminal failed, so the run isn't lost.
    let saved = saves::save_on_quit(&path, &state);
    ran?;
    Ok(saved.map(|saved| {
        if saved {
            format!("Saved {character}.")
        } else {
            format!("{character} died on turn {}.", state.now.turn())
        }
    }))
}

/// The character's saved game, or a new one if it has no save yet.
fn start(setup: &Setup, character: &str, save_path: &Path) -> Result<GameState, String> {
    let blueprints = Blueprints::load(&setup.data_dir).map_err(|err| {
        format!(
            "Failed to load blueprints from {}: {err}",
            setup.data_dir.display()
        )
    })?;
    match saves::load(save_path, blueprints)? {
        Loaded::Saved(mut state) => {
            state.log.push(format!("Welcome back, {character}."));
            if setup.seed.is_some() {
                state
                    .log
                    .push(format!("--seed ignored: continuing seed {}.", state.seed));
            }
            Ok(*state)
        }
        Loaded::New(blueprints) => {
            let cave = CaveParams::load(&setup.data_dir.join("cave.ron"))
                .map_err(|err| format!("Failed to load cave parameters: {err}"))?;
            let seed = setup.seed.unwrap_or_else(random_seed);
            GameState::new(seed, blueprints, &cave)
                .map_err(|err| format!("Failed to start a game: {err}"))
        }
    }
}

/// Blocks on `event::read()` and redraws after every event (including
/// resize) — no tick loop: `game::take_turn` runs the world until the
/// player is ready again.
pub fn run(terminal: &mut Term, state: &mut GameState, character: &str) -> io::Result<()> {
    let mut modal: Option<Modal> = None;
    terminal.draw(|frame| draw(frame, state, character, modal.as_ref()))?;

    loop {
        match event::read()? {
            Event::Key(key) if state.game_over => {
                if key.kind != KeyEventKind::Release {
                    return Ok(());
                }
                continue;
            }
            Event::Key(key) if modal.is_some() => {
                match modal.as_mut() {
                    Some(Modal::Overlay(overlay)) => {
                        let Some(input) = map_overlay_key(key) else {
                            continue;
                        };
                        let (inventory, equipment) = (inventory_view(state), equipment_view(state));
                        let contents = Contents {
                            inventory: &inventory,
                            equipment: &equipment,
                        };
                        match overlay.handle(input, &contents) {
                            Some(OverlayAction::Close) => modal = None,
                            Some(OverlayAction::Act(action)) => take_turn(state, action),
                            None => {}
                        }
                    }
                    Some(Modal::Choose(chooser)) => {
                        let Some(input) = map_overlay_key(key) else {
                            continue;
                        };
                        let items: Vec<_> = throwables(state).iter().map(|i| i.item).collect();
                        match chooser.handle(input, &items) {
                            Some(ChooserAction::Cancel) => modal = None,
                            Some(ChooserAction::Pick(item)) => {
                                modal = Aiming::new(state, Shot::Throw(item)).map(Modal::Aim);
                            }
                            None => {}
                        }
                    }
                    Some(Modal::ChooseSpell(chooser)) => {
                        let Some(input) = map_overlay_key(key) else {
                            continue;
                        };
                        let listed = spells(state);
                        let refs: Vec<_> = listed.iter().map(|spell| spell.spell).collect();
                        match chooser.handle(input, &refs) {
                            Some(ChooserAction::Cancel) => modal = None,
                            Some(ChooserAction::Pick(spell)) => {
                                // A spell that can't be cast keeps the list up,
                                // with the reason in the log.
                                if let Some(aiming) = Aiming::new(state, Shot::Cast(spell)) {
                                    modal = Some(Modal::Aim(aiming));
                                } else if let Some(why) = listed
                                    .into_iter()
                                    .find(|listed| listed.spell == spell)
                                    .and_then(|listed| listed.preview.blocked)
                                {
                                    state.log.push(why);
                                }
                            }
                            None => {}
                        }
                    }
                    // Only ever on screen for one frame, between keys.
                    Some(Modal::Flight(_)) => {}
                    Some(Modal::Aim(aiming)) => {
                        let Some(input) = map_aim_key(key) else {
                            continue;
                        };
                        match aiming.handle(input, state) {
                            Some(AimAction::Cancel) => modal = None,
                            Some(AimAction::Take(action)) => {
                                // A refused throw (out of range, unseen) keeps the cursor
                                // up, with the reason in the log, so it can be nudged.
                                let before = state.now;
                                take_turn(state, action);
                                animate(terminal, state, character)?;
                                if state.now != before || state.game_over {
                                    modal = None;
                                }
                            }
                            None => {}
                        }
                    }
                    None => {}
                }
                if state.game_over {
                    modal = None;
                }
            }
            Event::Key(key) => {
                let Some(input_event) = map_key_event(key) else {
                    continue;
                };
                match input_event {
                    InputEvent::Quit => return Ok(()),
                    InputEvent::Move(dir) => take_turn(state, PlayerAction::Move(dir)),
                    InputEvent::Wait => take_turn(state, PlayerAction::Wait),
                    InputEvent::PickUp => take_turn(state, PlayerAction::PickUp),
                    InputEvent::Explore => explore(terminal, state, character)?,
                    InputEvent::CycleExploreItems => cycle_explore_items(state),
                    InputEvent::Inventory => {
                        modal = Some(Modal::Overlay(Overlay::new(Tab::Inventory)))
                    }
                    InputEvent::Equipment => {
                        modal = Some(Modal::Overlay(Overlay::new(Tab::Equipment)))
                    }
                    InputEvent::Throw => modal = start_throw(state),
                    InputEvent::Fire => modal = start_fire(state),
                    InputEvent::Cast => modal = start_cast(state),
                }
            }
            Event::Resize(_, _) => {}
            _ => continue,
        }
        terminal.draw(|frame| draw(frame, state, character, modal.as_ref()))?;
    }
}

/// What is on screen over the map, if anything, and takes the keys.
enum Modal {
    Overlay(Overlay),
    /// Picking what to throw.
    Choose(Chooser),
    /// Picking a spell to cast.
    ChooseSpell(Chooser),
    Aim(Aiming),
    /// A missile mid-flight: drawn for one frame by `animate`, takes no keys.
    Flight(Flying),
}

/// Pressing fire: aim if an arrow is ready, nock one first (a turn) if only
/// the pack has them, otherwise say why not.
fn start_fire(state: &mut GameState) -> Option<Modal> {
    match fire_step(state) {
        FireStep::Aim => Aiming::new(state, Shot::Fire).map(Modal::Aim),
        FireStep::Nock(arrow) => {
            take_turn(state, PlayerAction::Nock(arrow));
            None
        }
        FireStep::Cannot(why) => {
            state.log.push(why);
            None
        }
    }
}

/// Straight to aiming with the only throwable; a list if there are several;
/// a log line if there is none.
fn start_throw(state: &mut GameState) -> Option<Modal> {
    let mut options = throwables(state);
    match options.len() {
        0 => {
            state.log.push("You have nothing to throw.");
            None
        }
        1 => Aiming::new(state, Shot::Throw(options.remove(0).item)).map(Modal::Aim),
        _ => Some(Modal::Choose(Chooser::default())),
    }
}

/// The spell list, even for one spell, since it shows what each costs or why
/// it can't be cast; a log line if the player knows none.
fn start_cast(state: &mut GameState) -> Option<Modal> {
    if spells(state).is_empty() {
        state.log.push("You know no spells.");
        return None;
    }
    Some(Modal::ChooseSpell(Chooser::default()))
}

/// Time a missile spends on each tile it crosses.
const MISSILE_STEP_DELAY: Duration = Duration::from_millis(40);

/// Plays back the flights the last action launched, a frame per tile. The
/// world has already resolved the shot, so this is only a picture of how it
/// got there; any key press cuts it short.
fn animate(terminal: &mut Term, state: &mut GameState, character: &str) -> io::Result<()> {
    for trail in take_trails(state) {
        for &at in &trail.path {
            let flying = Flying {
                glyph: trail.glyph,
                fg: trail.fg,
                at,
            };
            terminal.draw(|frame| draw(frame, state, character, Some(&Modal::Flight(flying))))?;
            if key_pressed_within(MISSILE_STEP_DELAY)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Pause between auto-explore steps, so the walk is visible and a key
/// press can interrupt it.
const EXPLORE_STEP_DELAY: Duration = Duration::from_millis(25);

/// Steps until `game` stops auto-explore or any key is pressed. Key
/// releases are ignored, or releasing the key that started it would stop it.
fn explore(terminal: &mut Term, state: &mut GameState, character: &str) -> io::Result<()> {
    while explore_step(state) {
        terminal.draw(|frame| draw(frame, state, character, None))?;
        if key_pressed_within(EXPLORE_STEP_DELAY)? {
            break;
        }
    }
    Ok(())
}

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

fn draw(frame: &mut Frame, state: &GameState, character: &str, modal: Option<&Modal>) {
    let (main, side) = side_panel::split(frame.area());
    if let Some(side) = side {
        side_panel::draw(frame, side, &side_panel::Context { state, character });
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

    let status = if let Some(Modal::Aim(aiming)) = modal {
        targeting::status(state, aiming)
    } else if state.game_over {
        format!(
            "You died on turn {}. Press any key to return to the title.",
            state.now.turn()
        )
    } else {
        format!("Seed: {}  Turn: {}", state.seed, state.now.turn())
    };
    frame.render_widget(
        Paragraph::new(status).block(Block::bordered().title("Status")),
        chunks[2],
    );

    match modal {
        Some(Modal::Overlay(overlay)) => {
            let (inventory, equipment) = (inventory_view(state), equipment_view(state));
            let contents = Contents {
                inventory: &inventory,
                equipment: &equipment,
            };
            overlay::draw(frame, chunks[0], overlay, &contents);
        }
        Some(Modal::Choose(chooser)) => {
            let lines = targeting::throwable_lines(&throwables(state));
            targeting::draw_chooser(frame, chunks[0], chooser, "Throw what?", lines);
        }
        Some(Modal::ChooseSpell(chooser)) => {
            let lines = targeting::spell_lines(&spells(state));
            targeting::draw_chooser(frame, chunks[0], chooser, "Cast what?", lines);
        }
        Some(Modal::Aim(aiming)) => targeting::draw_aim(frame, chunks[0], state, aiming),
        Some(Modal::Flight(flying)) => targeting::draw_missile(frame, chunks[0], state, *flying),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use game::{Blueprints, CaveParams, Tick};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn state() -> GameState {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load(&data).unwrap();
        let cave = CaveParams::load(&data.join("cave.ron")).unwrap();
        GameState::new(3, blueprints, &cave).unwrap()
    }

    /// 60×30: a 60×20 map panel above the 7-row log and 3-row status.
    fn rows(state: &GameState) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, state, "hero", None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    fn screen(state: &GameState) -> String {
        rows(state).concat()
    }

    #[test]
    fn the_map_panel_shows_the_viewport_around_the_player() {
        let state = state();
        let rows = rows(&state);
        // The player starts mid-zone, so the camera centres on them.
        assert_eq!(rows[10].chars().nth(30), Some('☺'));
        let panel: String = rows[..20].concat();
        assert_eq!(panel.matches('☺').count(), 1);
        assert!(panel.contains('#') && panel.contains('.'));
    }

    #[test]
    fn a_new_game_starts_with_a_bow_and_a_stocked_quiver() {
        let state = state();
        assert_eq!(fire_step(&state), FireStep::Aim);
        let worn: Vec<(String, Option<String>)> = equipment_view(&state)
            .into_iter()
            .filter_map(|part| part.item)
            .map(|item| (item.name, item.note))
            .collect();
        assert_eq!(
            worn,
            [
                ("quiver".to_string(), Some("12/20".to_string())),
                ("short bow".to_string(), None)
            ]
        );
    }

    #[test]
    fn aiming_marks_the_cursor_and_describes_the_shot_in_the_status_bar() {
        let state = state();
        let aiming = Aiming::new(&state, Shot::Fire).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, "hero", Some(&Modal::Aim(aiming))))
            .unwrap();
        let buffer = terminal.backend().buffer();
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

    fn drawn(state: &GameState, modal: &Modal) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, state, "hero", Some(modal)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer.content().iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn the_spell_list_shows_each_spell_with_its_disciplines_price() {
        let mut state = state();
        let modal = start_cast(&mut state).unwrap();
        let screen = drawn(&state, &modal);
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
        let state = state();
        let fireball = spells(&state)
            .into_iter()
            .find(|spell| spell.name == "fireball")
            .unwrap()
            .spell;
        let shot = Shot::Cast(fireball);
        let aiming = Aiming::new(&state, shot).unwrap();
        let (origin, _) = game::aim_range(&state, shot).unwrap();
        // With nothing in view the cursor rests on the player, so the burst
        // is centred there; its second tile is a neighbour.
        let area = game::aim(&state, shot, origin).unwrap().area;
        let tile = area[1];
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, "hero", Some(&Modal::Aim(aiming))))
            .unwrap();
        let screen = (
            u16::try_from(30 + tile.0 - origin.0).unwrap(),
            u16::try_from(10 + tile.1 - origin.1).unwrap(),
        );
        let cell = terminal.backend().buffer().cell(screen).unwrap();
        assert_eq!(cell.bg, crate::widgets::color(targeting::AREA));
    }

    #[test]
    fn aiming_a_spell_says_cast_in_the_status_bar() {
        let state = state();
        let firebolt = spells(&state)[0].spell;
        let aiming = Aiming::new(&state, Shot::Cast(firebolt)).unwrap();
        let screen = drawn(&state, &Modal::Aim(aiming));
        assert!(screen.contains("Enter: cast"), "{screen}");
    }

    #[test]
    fn a_flying_missile_is_drawn_on_its_tile() {
        let state = state();
        let (origin, _) = game::aim_range(&state, Shot::Fire).unwrap();
        let flying = Flying {
            glyph: '\\',
            fg: game::Rgb(1, 2, 3),
            at: origin,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, "hero", Some(&Modal::Flight(flying))))
            .unwrap();
        // The camera centres on the player.
        assert_eq!(
            terminal.backend().buffer().cell((30, 10)).unwrap().symbol(),
            "\\"
        );
    }

    #[test]
    fn the_inventory_panel_hovers_over_the_map() {
        let state = state();
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &state,
                    "hero",
                    Some(&Modal::Overlay(Overlay::new(Tab::Inventory))),
                )
            })
            .unwrap();
        let rows: Vec<String> = {
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .chunks(usize::from(buffer.area.width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                .collect()
        };
        let map: String = rows[..20].concat();
        assert!(map.contains("Inventory") && map.contains("You carry nothing."));
        assert!(rows[20..].concat().contains("Log"), "the log stays visible");
    }

    #[test]
    fn wide_terminals_get_the_side_column_beside_the_map() {
        let state = state();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, "hero", None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
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
        assert!(screen(&state).contains("Seed: 3  Turn: 2"));
    }

    #[test]
    fn status_announces_game_over() {
        let mut state = state();
        state.now = Tick(700);
        // What the frontend reads of a death; how dying works is `game`'s.
        state.game_over = true;
        let screen = screen(&state);
        assert!(screen.contains("You died on turn 7. Press any key to return to the title."));
        assert!(screen.contains('☺'), "the dead player is still drawn");
    }
}
