//! Between games: performing the title menu's file actions, starting a character's game (their save, or a new one) and saving it on the way out. Frontends show the messages these return.

use std::path::{Path, PathBuf};

use game::{Blueprints, CaveParams, GameState, random_seed};

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
    Play(String),
    Quit,
}

/// Performs the menu's file actions (delete, rename), showing the result in
/// the menu; playing and quitting are left to the frontend.
pub fn perform(menu: &mut Menu, saves_dir: &Path, action: Action) -> Option<Next> {
    let outcome = match action {
        Action::Quit => return Some(Next::Quit),
        Action::Play(character) => return Some(Next::Play(character)),
        Action::Replace(character) => match saves::delete(saves_dir, &character) {
            Ok(()) => return Some(Next::Play(character)),
            Err(err) => Err(err),
        },
        Action::Delete(character) => {
            saves::delete(saves_dir, &character).map(|()| format!("Deleted {character}."))
        }
        Action::Rename { from, to } => {
            saves::rename(saves_dir, &from, &to).map(|()| format!("Renamed {from} to {to}."))
        }
    };
    let (Ok(message) | Err(message)) = outcome;
    menu.show(message);
    refresh(menu, saves_dir);
    None
}

/// Reloads the menu's save list from disk.
pub fn refresh(menu: &mut Menu, saves_dir: &Path) {
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

/// A game being played and where it saves.
pub struct Game {
    pub session: Session,
    path: PathBuf,
}

/// The character's saved game, or a new one if it has no save yet. The
/// error is a message for the menu.
pub fn start(setup: &Setup, character: &str) -> Result<Game, String> {
    let path = saves::path(&setup.saves_dir, character)?;
    let state = load_or_new(setup, character, &path)?;
    Ok(Game {
        session: Session::new(state, character),
        path,
    })
}

fn load_or_new(setup: &Setup, character: &str, save_path: &Path) -> Result<GameState, String> {
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
            GameState::new(seed, *blueprints, &cave)
                .map_err(|err| format!("Failed to start a game: {err}"))
        }
    }
}

/// Saves the game on the way back to the menu (never after death) and
/// returns the menu's message.
pub fn finish(game: &Game) -> Result<String, String> {
    let state = game.session.state();
    let character = game.session.character();
    saves::save_on_quit(&game.path, state).map(|saved| {
        if saved {
            format!("Saved {character}.")
        } else {
            format!("{character} died on turn {}.", state.now.turn())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_deletes_the_old_save_and_plays_the_name() {
        let dir = std::env::temp_dir().join(format!("roguelike-title-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = saves::path(&dir, "old").unwrap();
        std::fs::write(&file, "(version: 1)").unwrap();
        let mut menu = Menu::new(Vec::new());
        let next = perform(&mut menu, &dir, Action::Replace("old".into()));
        assert!(matches!(next, Some(Next::Play(name)) if name == "old"));
        assert!(!file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
