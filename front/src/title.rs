//! Between games: performing the title menu's file actions, starting a character's game (their save, or a new one) and saving it on the way out. Frontends show the messages these return.

use std::path::{Path, PathBuf};

use game::{Blueprints, GameState, OriginView, Place, random_seed};

use crate::menu::{Action, Menu};
use crate::saves::{self, Loaded};
use crate::session::Session;

pub struct Setup {
    pub data_dir: PathBuf,
    pub saves_dir: PathBuf,
    /// Seed for new games; loaded games keep their own.
    pub seed: Option<u64>,
}

/// What a menu action leaves for the frontend once files are dealt with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Play the character: their save, or a new game as `origin` (a key;
    /// `None` for the first) if they have none.
    Play {
        character: String,
        origin: Option<String>,
    },
    Quit,
}

/// Performs the menu's file actions (delete, rename), showing the result in
/// the menu; playing and quitting are left to the frontend.
pub fn perform(menu: &mut Menu, setup: &Setup, action: Action) -> Option<Next> {
    let saves_dir = setup.saves_dir.as_path();
    let outcome = match action {
        Action::Quit => return Some(Next::Quit),
        Action::Play(character) => {
            return Some(Next::Play {
                character,
                origin: None,
            });
        }
        Action::New { character, origin } => return Some(Next::Play { character, origin }),
        Action::Replace { character, origin } => match saves::delete(saves_dir, &character) {
            Ok(()) => return Some(Next::Play { character, origin }),
            Err(err) => Err(err),
        },
        Action::Delete(character) => {
            abandon(setup, &character).map(|()| format!("Deleted {character}."))
        }
        Action::Rename { from, to } => {
            saves::rename(saves_dir, &from, &to).map(|()| format!("Renamed {from} to {to}."))
        }
    };
    let (Ok(message) | Err(message)) = outcome;
    menu.show(message);
    refresh(menu, setup);
    None
}

/// Deleting a living character ends its life like a death
/// (docs/plans/legacy.md), so making and deleting characters can't reroll the
/// ruins. A save this build can't load has no life to record and is only
/// deleted.
fn abandon(setup: &Setup, character: &str) -> Result<(), String> {
    let path = saves::path(&setup.saves_dir, character)?;
    let loaded = Blueprints::load(&setup.data_dir)
        .ok()
        .and_then(|blueprints| saves::load(&path, blueprints).ok());
    match loaded {
        Some(Loaded::Saved(state)) => saves::bury(&setup.saves_dir, character, &state),
        _ => saves::delete(&setup.saves_dir, character),
    }
}

/// Reloads the menu's save list and the lineage's fallen from disk.
pub fn refresh(menu: &mut Menu, setup: &Setup) {
    let saves_dir = setup.saves_dir.as_path();
    match (saves::lineage(saves_dir), Blueprints::load(&setup.data_dir)) {
        (Ok(records), Ok(blueprints)) => menu.set_fallen(
            records
                .iter()
                .map(|record| crate::text::life(&game::summary(&blueprints, record)))
                .collect(),
        ),
        // Starting a game says why the content fails to load.
        (Err(err), _) => menu.show(err),
        (_, Err(_)) => {}
    }
    match saves::list(saves_dir) {
        Ok(list) => {
            let outdated = list
                .iter()
                .filter(|name| saves::outdated(saves_dir, name))
                .cloned()
                .collect();
            menu.set_saves(list);
            menu.set_outdated(outdated);
        }
        Err(err) => menu.show(err),
    }
}

/// What a new character can start as, for the menu; none if the content
/// fails to load (starting a game then says why).
pub fn origins(setup: &Setup) -> Vec<OriginView> {
    Blueprints::load(&setup.data_dir)
        .map(|blueprints| game::origins(&blueprints))
        .unwrap_or_default()
}

/// A game being played and where it saves.
pub struct Game {
    pub session: Session,
    path: PathBuf,
    /// Where the player was at the last save, so a change of place saves.
    saved_at: Place,
}

/// The character's saved game, or a new one as `origin` if it has no save
/// yet. The error is a message for the menu.
pub fn start(setup: &Setup, character: &str, origin: Option<&str>) -> Result<Game, String> {
    let path = saves::path(&setup.saves_dir, character)?;
    let state = load_or_new(setup, character, origin, &path)?;
    Ok(Game {
        saved_at: state.place.clone(),
        session: Session::new(state, character),
        path,
    })
}

/// Saves when the player has changed place (into a dive, down the stairs,
/// back to camp) since the last save, so a crash or a killed window loses
/// one floor at most, and a death can't be undone past it. A failure is
/// said in the log; play goes on.
pub fn checkpoint(game: &mut Game) {
    let place = &game.session.state().place;
    if *place == game.saved_at {
        return;
    }
    game.saved_at = place.clone();
    if let Err(err) = saves::save_on_quit(&game.path, game.session.state()) {
        game.session.log(err);
    }
}

fn load_or_new(
    setup: &Setup,
    character: &str,
    origin: Option<&str>,
    save_path: &Path,
) -> Result<GameState, String> {
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
            let seed = setup.seed.unwrap_or_else(random_seed);
            // A lineage this build can't read leaves no ruins; it is kept
            // until the next ending replaces it.
            let lineage = saves::lineage(&setup.saves_dir).unwrap_or_default();
            GameState::new_in_lineage(seed, *blueprints, origin, &lineage)
                .map_err(|err| format!("Failed to start a game: {err}"))
        }
    }
}

/// Saves the game on the way back to the menu, or after a death buries the
/// character (docs/plans/legacy.md), and returns the menu's message.
pub fn finish(game: &Game, seconds: u64) -> Result<String, String> {
    let state = game.session.state();
    let character = game.session.character();
    let dir = game.path.parent().unwrap_or(Path::new("."));
    // Calibration only: a failure here never costs the save.
    let _ = saves::record_play(dir, character, game.session.keypresses(), seconds);
    if state.game_over {
        saves::bury(dir, character, state)?;
        return Ok(format!("{character} died on turn {}.", state.now.turn()));
    }
    saves::save_on_quit(&game.path, state).map(|_| format!("Saved {character}."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::MenuInput;

    #[test]
    fn leaving_a_game_records_its_play_beside_the_saves() {
        let dir = std::env::temp_dir().join(format!("roguelike-play-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let setup = Setup {
            data_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
            saves_dir: dir.clone(),
            seed: Some(3),
        };
        let mut game = start(&setup, "hero", None).unwrap();
        game.session
            .handle(crate::input::Input::Game(game::InputEvent::Wait));
        finish(&game, 90).unwrap();
        finish(&game, 30).unwrap();
        let text = std::fs::read_to_string(dir.join("playtime.csv")).unwrap();
        assert_eq!(text, "character,keypresses,seconds\nhero,1,90\nhero,1,30\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A fresh saves directory under the system temp dir, with the real data.
    fn setup(test: &str) -> Setup {
        let dir = std::env::temp_dir().join(format!("roguelike-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Setup {
            data_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
            saves_dir: dir,
            seed: Some(3),
        }
    }

    #[test]
    fn changing_place_saves_and_staying_does_not() {
        let setup = setup("checkpoint");
        let mut game = start(&setup, "hero", None).unwrap();
        checkpoint(&mut game);
        assert!(!game.path.exists(), "still where it started");
        // As if the last save was on a floor and the player is back at camp.
        game.saved_at = Place::Floor {
            dungeon: "Caves".into(),
            phase: "stillness".into(),
            dive: 1,
            floor: 1,
        };
        checkpoint(&mut game);
        assert!(game.path.exists());
        assert_eq!(game.saved_at, game.session.state().place);
        let _ = std::fs::remove_dir_all(&setup.saves_dir);
    }

    #[test]
    fn a_death_buries_the_character() {
        let setup = setup("death");
        let game = start(&setup, "hero", None).unwrap();
        finish(&game, 1).unwrap();
        assert!(game.path.exists(), "a living quit saves as before");
        let blueprints = Blueprints::load(&setup.data_dir).unwrap();
        let Ok(Loaded::Saved(mut state)) = saves::load(&game.path, blueprints) else {
            panic!("the save loads");
        };
        // What the frontend reads of a death; how dying works is `game`'s.
        state.game_over = true;
        let dead = Game {
            saved_at: state.place.clone(),
            session: Session::new(*state, "hero"),
            path: game.path.clone(),
        };
        assert_eq!(finish(&dead, 1).unwrap(), "hero died on turn 0.");
        assert!(!game.path.exists(), "death is final");
        let lineage = saves::lineage(&setup.saves_dir).unwrap();
        assert_eq!(lineage.len(), 1);
        assert_eq!(lineage[0].name, "hero");
        let _ = std::fs::remove_dir_all(&setup.saves_dir);
    }

    #[test]
    fn deleting_a_living_character_records_it_as_abandoned() {
        let setup = setup("abandon");
        let game = start(&setup, "hero", None).unwrap();
        finish(&game, 1).unwrap();
        let mut menu = Menu::new(Vec::new());
        assert_eq!(
            perform(&mut menu, &setup, Action::Delete("hero".into())),
            None
        );
        assert!(!game.path.exists());
        let lineage = saves::lineage(&setup.saves_dir).unwrap();
        assert_eq!(lineage[0].ending, game::legacy::Ending::Abandoned);
        assert_eq!(saves::list(&setup.saves_dir).unwrap(), Vec::<String>::new());
        for input in [
            MenuInput::Down,
            MenuInput::Down,
            MenuInput::Down,
            MenuInput::Select,
        ] {
            menu.handle(input);
        }
        let crate::menu::Body::Text(fallen) = menu.view().body else {
            panic!("the Fallen list");
        };
        assert!(fallen[0].starts_with("hero"), "{fallen:?}");
        let heir = start(&setup, "heir", None).unwrap();
        let ruin = heir
            .session
            .state()
            .blueprints
            .camp()
            .map(|camp| camp.ruins[0])
            .unwrap();
        let state = heir.session.state();
        let look = game::inspect(state, ruin).expect("the ruin is in view at camp");
        assert_eq!(
            look.name, "abandoned camp",
            "the next character finds the ruins"
        );
        let _ = std::fs::remove_dir_all(&setup.saves_dir);
    }

    #[test]
    fn replacing_deletes_the_old_save_and_plays_the_name() {
        let setup = setup("title");
        let dir = setup.saves_dir.clone();
        let file = saves::path(&dir, "old").unwrap();
        std::fs::write(&file, "(version: 1)").unwrap();
        let mut menu = Menu::new(Vec::new());
        let replace = Action::Replace {
            character: "old".into(),
            origin: Some("Wanderer".into()),
        };
        let next = perform(&mut menu, &setup, replace);
        assert_eq!(
            next,
            Some(Next::Play {
                character: "old".into(),
                origin: Some("Wanderer".into())
            })
        );
        assert!(!file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
