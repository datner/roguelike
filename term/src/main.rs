use std::path::{Path, PathBuf};

use game::{load_blueprint, random_seed, EntityBlueprint, GameState};

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

fn load_rubble_blueprint(data_dir: &Path) -> EntityBlueprint {
    let path = data_dir.join("entities/rubble.ron");
    match load_blueprint(&path) {
        Ok(blueprint) => blueprint,
        Err(err) => {
            eprintln!("Failed to load blueprint at {}: {err}", path.display());
            std::process::exit(1);
        }
    }
}

fn main() -> std::io::Result<()> {
    let (seed_arg, data_dir) = parse_args();
    let seed = seed_arg.unwrap_or_else(random_seed);

    let blueprint = load_rubble_blueprint(&data_dir);
    let mut state = GameState::new(seed, blueprint);

    let mut terminal = tui::init()?;
    let result = app::run(&mut terminal, &mut state);
    tui::restore();

    result
}
