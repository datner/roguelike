//! Save files: one per character, listed and managed from the title menu, written on quit.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use game::legacy::{self, Record};
use game::{Blueprints, GameState, save};

/// `<platform data dir>/saves`, or `None` if the platform has no home
/// directory to put it in.
pub fn default_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "roguelike").map(|dirs| dirs.data_dir().join("saves"))
}

/// The lineage's file beside the saves (docs/plans/legacy.md): one lineage per saves directory.
const LINEAGE: &str = "legacy";

/// Character names become file names, so they may not leave the directory
/// or take the lineage's.
pub fn path(dir: &Path, character: &str) -> Result<PathBuf, String> {
    if character.is_empty()
        || character.starts_with('.')
        || character.contains(['/', '\\'])
        || character == LINEAGE
    {
        return Err(format!("invalid character name {character:?}"));
    }
    Ok(dir.join(format!("{character}.ron")))
}

pub enum Loaded {
    Saved(Box<GameState>),
    /// No save yet; the blueprints come back for a new game.
    New(Box<Blueprints>),
}

/// Loads the save at `path` if there is one. Never writes: a save that
/// fails to load is left exactly as it was.
pub fn load(path: &Path, blueprints: Blueprints) -> Result<Loaded, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(Loaded::New(Box::new(blueprints)));
        }
        Err(err) => return Err(format!("cannot read {}: {err}", path.display())),
    };
    save::from_ron(&text, blueprints)
        .map(|state| Loaded::Saved(Box::new(state)))
        .map_err(|err| match err {
            save::LoadError::Version { found, expected } => {
                let name = path
                    .file_stem()
                    .map_or_else(Default::default, |s| s.to_string_lossy());
                format!(
                    "{name}'s save is from save version {found} (this build reads {expected}); \
                     start a new game as {name} to replace it."
                )
            }
            err => format!("cannot load {}: {err}", path.display()),
        })
}

/// Whether `character`'s save is from another save format, so this build
/// can't load it and a new game may take its name (ADR 0008: no
/// migrations).
pub fn outdated(dir: &Path, character: &str) -> bool {
    path(dir, character)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| save::version(&text))
        .is_some_and(|version| version != save::SAVE_VERSION)
}

/// Saves `state` to `path` unless the character is dead (then `bury` is the
/// caller's). Returns whether it saved.
pub fn save_on_quit(path: &Path, state: &GameState) -> Result<bool, String> {
    if state.game_over {
        return Ok(false);
    }
    let text = save::to_ron(state).map_err(|err| format!("cannot save: {err}"))?;
    write_atomic(path, text.as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(true)
}

/// The characters with a save in `dir`, most recently saved first, so the
/// head is the one "Continue" resumes. A missing directory has no saves.
/// Appends a life's (or a sitting's) play to `<dir>/playtime.csv`
/// (`character,keypresses,seconds`), what calibrates the pacing targets'
/// keypresses an hour (docs/plans/calibration.md). Beside the saves, never
/// inside one, so `game` never sees a clock and a death keeps the line.
pub fn record_play(
    dir: &Path,
    character: &str,
    keypresses: u64,
    seconds: u64,
) -> Result<(), String> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let path = dir.join("playtime.csv");
    let new = !path.exists();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let header = if new {
        "character,keypresses,seconds\n"
    } else {
        ""
    };
    // Commas would split the line: a name keeps its letters.
    let name = character.replace(',', " ");
    writeln!(file, "{header}{name},{keypresses},{seconds}")
        .map_err(|err| format!("{}: {err}", path.display()))
}

pub fn list(dir: &Path) -> Result<Vec<String>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("cannot list {}: {err}", dir.display())),
    };
    let mut saves = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("cannot list {}: {err}", dir.display()))?;
        let file = entry.path();
        if file.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let Some(name) = file.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if path(dir, name).is_err() {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        saves.push((modified, name.to_string()));
    }
    saves.sort_by(|(a_time, a_name), (b_time, b_name)| {
        b_time.cmp(a_time).then_with(|| a_name.cmp(b_name))
    });
    Ok(saves.into_iter().map(|(_, name)| name).collect())
}

pub fn delete(dir: &Path, character: &str) -> Result<(), String> {
    let file = path(dir, character)?;
    fs::remove_file(&file).map_err(|err| format!("cannot delete {}: {err}", file.display()))
}

/// Refuses to overwrite: renaming onto an existing character would lose
/// that character's save.
pub fn rename(dir: &Path, from: &str, to: &str) -> Result<(), String> {
    let source = path(dir, from)?;
    let target = path(dir, to)?;
    if target.exists() {
        return Err(format!("a save named {to:?} already exists"));
    }
    fs::rename(&source, &target).map_err(|err| format!("cannot rename {}: {err}", source.display()))
}

/// The lives recorded in `dir`, newest first. A missing file is an empty
/// lineage; one this build can't read is an error and left as it is.
pub fn lineage(dir: &Path) -> Result<Vec<Record>, String> {
    let file = dir.join(format!("{LINEAGE}.ron"));
    match fs::read_to_string(&file) {
        Ok(text) => legacy::from_ron(&text).map_err(|err| format!("{}: {err}", file.display())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(format!("cannot read {}: {err}", file.display())),
    }
}

/// Ends `character` for good: `record` joins the lineage, then the save is
/// removed (if there is one), so a failure never loses both. A lineage this
/// build can't read is replaced, starting over from this record.
pub fn bury(dir: &Path, character: &str, record: Record) -> Result<(), String> {
    let save = path(dir, character)?;
    let mut records = lineage(dir).unwrap_or_default();
    legacy::add(&mut records, record);
    let text = legacy::to_ron(&records).map_err(|err| format!("cannot record a life: {err}"))?;
    let file = dir.join(format!("{LINEAGE}.ron"));
    write_atomic(&file, text.as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", file.display()))?;
    match fs::remove_file(&save) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => {
            Err(format!("cannot delete {}: {err}", save.display()))
        }
        _ => Ok(()),
    }
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

    use super::*;

    fn blueprints() -> Blueprints {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        Blueprints::load(&dir).unwrap()
    }

    fn state() -> GameState {
        GameState::new(3, blueprints()).unwrap()
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
    fn a_save_in_another_format_is_outdated() {
        let dir = temp_dir("outdated");
        let current = save::to_ron(&state()).unwrap();
        fs::write(path(&dir, "now").unwrap(), &current).unwrap();
        let old = current.replacen(
            &format!("version: {}", save::SAVE_VERSION),
            &format!("version: {}", save::SAVE_VERSION - 1),
            1,
        );
        fs::write(path(&dir, "old").unwrap(), old).unwrap();
        fs::write(path(&dir, "junk").unwrap(), "not a save").unwrap();

        assert!(!outdated(&dir, "now"));
        assert!(outdated(&dir, "old"));
        assert!(!outdated(&dir, "junk"), "only a known older format");
        assert!(!outdated(&dir, "missing"));
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
        assert!(err.contains("new game as hero"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "(version: 999)");
        assert_eq!(files(&dir), ["hero.ron"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn character_names_cannot_escape_the_saves_directory() {
        let dir = Path::new("saves");
        assert_eq!(path(dir, "hero"), Ok(PathBuf::from("saves/hero.ron")));
        for bad in ["", "../hero", "a/b", ".hidden", "legacy"] {
            assert!(path(dir, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn burying_removes_the_save_and_keeps_the_newest_lives() {
        let dir = temp_dir("bury");
        let record = game::legacy::record(&state(), "hero");
        for name in ["a", "b", "c", "d"] {
            let save = path(&dir, name).unwrap();
            fs::write(&save, "save").unwrap();
            let life = Record {
                name: name.into(),
                ..record.clone()
            };
            bury(&dir, name, life).unwrap();
            assert!(!save.exists(), "{name}");
        }
        let names: Vec<String> = lineage(&dir).unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["d", "c", "b"]);
        assert_eq!(
            list(&dir).unwrap(),
            Vec::<String>::new(),
            "no character named legacy"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_outdated_lineage_is_left_alone_until_the_next_ending_replaces_it() {
        let dir = temp_dir("lineage");
        let file = dir.join("legacy.ron");
        fs::write(&file, "(version: 0)").unwrap();
        assert!(lineage(&dir).unwrap_err().contains("version 0"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "(version: 0)");
        bury(&dir, "hero", game::legacy::record(&state(), "hero")).unwrap();
        assert_eq!(lineage(&dir).unwrap().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    fn touch(file: &Path, secs_after_epoch: u64) {
        fs::write(file, "save").unwrap();
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs_after_epoch);
        File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    #[test]
    fn saves_are_listed_most_recent_first() {
        let dir = temp_dir("list");
        touch(&dir.join("old.ron"), 100);
        touch(&dir.join("new.ron"), 300);
        touch(&dir.join("mid.ron"), 200);
        touch(&dir.join("new.ron.tmp"), 400);
        touch(&dir.join(".hidden.ron"), 400);
        touch(&dir.join("notes.txt"), 400);

        assert_eq!(
            list(&dir),
            Ok(vec!["new".into(), "mid".into(), "old".into()])
        );
        assert_eq!(list(&dir.join("missing")), Ok(Vec::new()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deleting_removes_only_that_save() {
        let dir = temp_dir("delete");
        touch(&dir.join("hero.ron"), 100);
        touch(&dir.join("other.ron"), 100);

        assert_eq!(delete(&dir, "hero"), Ok(()));
        assert_eq!(files(&dir), ["other.ron"]);
        assert!(delete(&dir, "hero").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn renaming_never_overwrites_another_save() {
        let dir = temp_dir("rename");
        touch(&dir.join("hero.ron"), 100);
        touch(&dir.join("other.ron"), 100);

        assert!(rename(&dir, "hero", "other").is_err());
        assert!(rename(&dir, "hero", "../escape").is_err());
        assert_eq!(files(&dir), ["hero.ron", "other.ron"]);
        assert_eq!(rename(&dir, "hero", "champion"), Ok(()));
        assert_eq!(files(&dir), ["champion.ron", "other.ron"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
