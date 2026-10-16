//! Save files: one per character, loaded at start and written on quit.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use game::{Blueprints, GameState, save};

/// `<platform data dir>/saves`, or `None` if the platform has no home
/// directory to put it in.
pub fn default_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "roguelike").map(|dirs| dirs.data_dir().join("saves"))
}

/// Character names become file names, so they may not leave the directory.
pub fn path(dir: &Path, character: &str) -> Result<PathBuf, String> {
    if character.is_empty() || character.starts_with('.') || character.contains(['/', '\\']) {
        return Err(format!("invalid character name {character:?}"));
    }
    Ok(dir.join(format!("{character}.ron")))
}

pub enum Loaded {
    Saved(Box<GameState>),
    /// No save yet; the blueprints come back for a new game.
    New(Blueprints),
}

/// Loads the save at `path` if there is one. Never writes: a save that
/// fails to load is left exactly as it was.
pub fn load(path: &Path, blueprints: Blueprints) -> Result<Loaded, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Loaded::New(blueprints)),
        Err(err) => return Err(format!("cannot read {}: {err}", path.display())),
    };
    save::from_ron(&text, blueprints)
        .map(|state| Loaded::Saved(Box::new(state)))
        .map_err(|err| format!("cannot load {}: {err}", path.display()))
}

/// Saves `state` to `path` unless the character is dead, in which case the
/// last save is left as it was. Returns whether it saved.
pub fn save_on_quit(path: &Path, state: &GameState) -> Result<bool, String> {
    if state.game_over {
        return Ok(false);
    }
    let text = save::to_ron(state).map_err(|err| format!("cannot save: {err}"))?;
    write_atomic(path, text.as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(true)
}

/// Writes a temp file next to `path`, then renames it over `path`, so a
/// crash mid-write never leaves a truncated save.
fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = temp_path(path);
    let written = File::create(&temp).and_then(|mut file| {
        file.write_all(contents)?;
        file.sync_all()
    });
    let result = written.and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use game::CaveParams;

    use super::*;

    fn blueprints() -> Blueprints {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/entities");
        Blueprints::load_dir(&dir).unwrap()
    }

    fn state() -> GameState {
        let cave = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/cave.ron");
        GameState::new(3, blueprints(), &CaveParams::load(&cave).unwrap()).unwrap()
    }

    /// A fresh, empty directory under the system temp dir.
    fn temp_dir(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("roguelike-saves-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn quitting_writes_the_save_and_leaves_no_temp_file() {
        let dir = temp_dir("quit");
        let path = path(&dir.join("saves"), "hero").unwrap();
        let state = state();

        assert_eq!(save_on_quit(&path, &state), Ok(true));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            save::to_ron(&state).unwrap()
        );
        assert_eq!(files(&dir.join("saves")), ["hero.ron"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quitting_goes_through_the_temp_file() {
        let dir = temp_dir("temp");
        let path = path(&dir, "hero").unwrap();
        fs::write(&path, "old save").unwrap();
        // Blocks the temp file, so only a write that bypassed it could succeed.
        fs::create_dir(temp_path(&path)).unwrap();

        assert!(save_on_quit(&path, &state()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old save");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quitting_after_death_keeps_the_last_save() {
        let dir = temp_dir("dead");
        let path = path(&dir, "hero").unwrap();
        fs::write(&path, "last save").unwrap();
        let mut state = state();
        state.game_over = true;

        assert_eq!(save_on_quit(&path, &state), Ok(false));
        assert_eq!(fs::read_to_string(&path).unwrap(), "last save");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_saved_character_is_loaded() {
        let dir = temp_dir("load");
        let path = path(&dir, "hero").unwrap();
        let state = state();
        save_on_quit(&path, &state).unwrap();

        let Ok(Loaded::Saved(loaded)) = load(&path, blueprints()) else {
            panic!("the save loads");
        };
        assert_eq!(loaded.log, state.log);
        assert!(matches!(
            load(&dir.join("nobody.ron"), blueprints()),
            Ok(Loaded::New(_))
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_load_leaves_the_file_untouched() {
        let dir = temp_dir("corrupt");
        let path = path(&dir, "hero").unwrap();
        fs::write(&path, "(version: 999)").unwrap();

        let Err(err) = load(&path, blueprints()) else {
            panic!("a save from another version must not load");
        };
        assert!(err.contains("version 999"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "(version: 999)");
        assert_eq!(files(&dir), ["hero.ron"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn character_names_cannot_escape_the_saves_directory() {
        let dir = Path::new("saves");
        assert_eq!(path(dir, "hero"), Ok(PathBuf::from("saves/hero.ron")));
        for bad in ["", "../hero", "a/b", ".hidden"] {
            assert!(path(dir, bad).is_err(), "{bad:?}");
        }
    }
}
