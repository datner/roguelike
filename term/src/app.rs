//! The title-menu session, and the blocking, turn-based game loop and panel layout.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use game::{
    Blueprints, CaveParams, GameState, InputEvent, PlayerAction, cycle_explore_items,
    equipment_view, explore_step, inventory_view, random_seed, render_view, take_turn,
};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::input::{map_key_event, map_menu_key, map_overlay_key};
use crate::menu::{self, Action, Menu};
use crate::overlay::{self, Contents, Overlay, OverlayAction, Tab};
use crate::saves::{self, Loaded};
use crate::side_panel;
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
    let entities = setup.data_dir.join("entities");
    let blueprints = Blueprints::load_dir(&entities).map_err(|err| {
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
    let mut overlay: Option<Overlay> = None;
    terminal.draw(|frame| draw(frame, state, character, overlay.as_ref()))?;

    loop {
        match event::read()? {
            Event::Key(key) if state.game_over => {
                if key.kind != KeyEventKind::Release {
                    return Ok(());
                }
                continue;
            }
            Event::Key(key) if overlay.is_some() => {
                let Some(input) = map_overlay_key(key) else {
                    continue;
                };
                let (inventory, equipment) = (inventory_view(state), equipment_view(state));
                let contents = Contents {
                    inventory: &inventory,
                    equipment: &equipment,
                };
                match overlay.as_mut().and_then(|o| o.handle(input, &contents)) {
                    Some(OverlayAction::Close) => overlay = None,
                    Some(OverlayAction::Act(action)) => take_turn(state, action),
                    None => {}
                }
                if state.game_over {
                    overlay = None;
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
                    InputEvent::Inventory => overlay = Some(Overlay::new(Tab::Inventory)),
                    InputEvent::Equipment => overlay = Some(Overlay::new(Tab::Equipment)),
                }
            }
            Event::Resize(_, _) => {}
            _ => continue,
        }
        terminal.draw(|frame| draw(frame, state, character, overlay.as_ref()))?;
    }
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

fn draw(frame: &mut Frame, state: &GameState, character: &str, overlay: Option<&Overlay>) {
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

    let status = if state.game_over {
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

    if let Some(overlay) = overlay {
        let (inventory, equipment) = (inventory_view(state), equipment_view(state));
        let contents = Contents {
            inventory: &inventory,
            equipment: &equipment,
        };
        overlay::draw(frame, chunks[0], overlay, &contents);
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
        let blueprints = Blueprints::load_dir(&data.join("entities")).unwrap();
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
    fn the_inventory_panel_hovers_over_the_map() {
        let state = state();
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, "hero", Some(&Overlay::new(Tab::Inventory))))
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
