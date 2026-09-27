//! Terminal frontend entry point: CLI args, blueprint loading, save loading, wiring.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use game::{Blueprints, GameState, random_seed};

mod app;
mod input;
mod saves;
mod tui;
mod widgets;

use saves::Loaded;

struct Args {
    seed: Option<u64>,
    data_dir: PathBuf,
    character: String,
    saves_dir: Option<PathBuf>,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data"))
}

fn parse_args() -> Args {
    let mut parsed = Args {
        seed: None,
        data_dir: default_data_dir(),
        character: "default".to_string(),
        saves_dir: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => {
                if let Some(value) = args.next() {
                    parsed.seed = value.parse::<u64>().ok();
                }
            }
            "--data" => {
                if let Some(value) = args.next() {
                    parsed.data_dir = PathBuf::from(value);
                }
            }
            "--character" => {
                if let Some(value) = args.next() {
                    parsed.character = value;
                }
            }
            "--saves" => {
                if let Some(value) = args.next() {
                    parsed.saves_dir = Some(PathBuf::from(value));
                }
            }
            _ => {}
        }
    }
    parsed
}

/// The character's saved game, or a new one if it has no save yet.
fn start(args: &Args, save_path: &Path) -> Result<GameState, String> {
    let entities = args.data_dir.join("entities");
    let blueprints = Blueprints::load_dir(&entities).map_err(|err| {
        format!(
            "Failed to load blueprints from {}: {err}",
            args.data_dir.display()
        )
    })?;
    match saves::load(save_path, blueprints)? {
        Loaded::Saved(mut state) => {
            state.log.push(format!("Welcome back, {}.", args.character));
            if args.seed.is_some() {
                state
                    .log
                    .push(format!("--seed ignored: continuing seed {}.", state.seed));
            }
            Ok(*state)
        }
        Loaded::New(blueprints) => {
            let seed = args.seed.unwrap_or_else(random_seed);
            GameState::new(seed, blueprints).map_err(|err| format!("Failed to start a game: {err}"))
        }
    }
}

fn main() -> ExitCode {
    let args = parse_args();
    let Some(saves_dir) = args.saves_dir.clone().or_else(saves::default_dir) else {
        eprintln!("No data directory on this platform; pass --saves <dir>.");
        return ExitCode::FAILURE;
    };
    let result = saves::path(&saves_dir, &args.character)
        .and_then(|save_path| Ok((start(&args, &save_path)?, save_path)));
    let (mut state, save_path) = match result {
        Ok(started) => started,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    let mut terminal = match tui::init() {
        Ok(terminal) => terminal,
        Err(err) => {
            eprintln!("Cannot start the terminal UI: {err}");
            return ExitCode::FAILURE;
        }
    };
    let ran = app::run(&mut terminal, &mut state);
    tui::restore();

    let mut code = ExitCode::SUCCESS;
    if let Err(err) = ran {
        eprintln!("Terminal error: {err}");
        code = ExitCode::FAILURE;
    }
    match saves::save_on_quit(&save_path, &state) {
        Ok(true) => println!("Saved to {}.", save_path.display()),
        Ok(false) => {}
        Err(err) => {
            eprintln!("{err}");
            code = ExitCode::FAILURE;
        }
    }
    code
}
