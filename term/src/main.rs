//! Terminal frontend entry point: CLI args, blueprint loading, wiring.

use std::path::{Path, PathBuf};

use game::{Blueprints, GameState, random_seed};

mod app;
mod input;
mod tui;
mod widgets;

fn default_data_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data"))
}

fn parse_args() -> (Option<u64>, PathBuf) {
    let mut seed = None;
    let mut data_dir = default_data_dir();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" => {
                if let Some(value) = args.next() {
                    seed = value.parse::<u64>().ok();
                }
            }
            "--data" => {
                if let Some(value) = args.next() {
                    data_dir = PathBuf::from(value);
                }
            }
            _ => {}
        }
    }
    (seed, data_dir)
}

fn new_game(seed: u64, data_dir: &Path) -> GameState {
    let result = Blueprints::load_dir(&data_dir.join("entities"))
        .and_then(|blueprints| GameState::new(seed, blueprints));
    match result {
        Ok(state) => state,
        Err(err) => {
            eprintln!(
                "Failed to load blueprints from {}: {err}",
                data_dir.display()
            );
            std::process::exit(1);
        }
    }
}

fn main() -> std::io::Result<()> {
    let (seed_arg, data_dir) = parse_args();
    let seed = seed_arg.unwrap_or_else(random_seed);

    let mut state = new_game(seed, &data_dir);

    let mut terminal = tui::init()?;
    let result = app::run(&mut terminal, &mut state);
    tui::restore();

    result
}
