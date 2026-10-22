//! Terminal frontend entry point: CLI args and wiring.

use std::path::PathBuf;
use std::process::ExitCode;

mod app;
mod input;
mod menu;
mod overlay;
mod side_panel;
mod targeting;
mod tui;
mod widgets;

struct Args {
    seed: Option<u64>,
    data_dir: PathBuf,
    /// Plays this character straight away instead of opening on the menu.
    character: Option<String>,
    saves_dir: Option<PathBuf>,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data"))
}

fn parse_args() -> Args {
    let mut parsed = Args {
        seed: None,
        data_dir: default_data_dir(),
        character: None,
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
                    parsed.character = Some(value);
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

fn main() -> ExitCode {
    let args = parse_args();
    let Some(saves_dir) = args.saves_dir.clone().or_else(front::saves::default_dir) else {
        eprintln!("No data directory on this platform; pass --saves <dir>.");
        return ExitCode::FAILURE;
    };
    let setup = front::title::Setup {
        data_dir: args.data_dir,
        saves_dir,
        seed: args.seed,
    };

    let mut terminal = match tui::init() {
        Ok(terminal) => terminal,
        Err(err) => {
            eprintln!("Cannot start the terminal UI: {err}");
            return ExitCode::FAILURE;
        }
    };
    let ran = app::session(
        &mut terminal,
        &setup,
        args.character.map(front::menu::Action::Play),
    );
    tui::restore();

    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Terminal error: {err}");
            ExitCode::FAILURE
        }
    }
}
