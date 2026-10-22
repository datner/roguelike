//! Windowed frontend entry point: CLI args (as `term`'s) and wiring.

use std::path::PathBuf;
use std::process::ExitCode;

use front::title::Setup;
use gui::app::App;

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
    let Some(saves_dir) = args.saves_dir.or_else(front::saves::default_dir) else {
        eprintln!("No data directory on this platform; pass --saves <dir>.");
        return ExitCode::FAILURE;
    };
    let setup = Setup {
        data_dir: args.data_dir,
        saves_dir,
        seed: args.seed,
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Roguelike")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    let character = args.character;
    let ran = eframe::run_native(
        "Roguelike",
        options,
        Box::new(|creation| {
            gui::theme::apply(&creation.egui_ctx);
            Ok(Box::new(App::new(setup, character)))
        }),
    );
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Window error: {err}");
            ExitCode::FAILURE
        }
    }
}
