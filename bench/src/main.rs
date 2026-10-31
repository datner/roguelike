//! Balance bench entry point, run as `cargo balance` (docs/plans/bench-cli.md). With no mode it
//! runs everything and rewrites `bench/report.md` and `bench/report.json`, then says what moved;
//! `--check` compares instead and fails if anything moved; `--print` prints; `--page FILE` writes
//! the readable page (`bench/page.html` filled in, with what moved since the committed JSON);
//! `--calibrate [FILE]` reads a `playtime.csv` (the gui's, by default) and prints keypresses an
//! hour. `--seeds N` sets the doll and the dive seeds alike; `--build`, `--scenario`, `--dive`
//! (each repeatable), `--dolls` and `--dives` narrow a run. A run unlike the committed one
//! (narrowed, or other seeds) only prints.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("bench: {err}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Write,
    Check,
    Print,
    Page(String),
}

fn run() -> Result<(), String> {
    let mut seeds = None;
    let mut mode = Mode::Write;
    let mut only = bench::Narrow::default();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} takes a value"));
        match arg.as_str() {
            "--build" => only.builds.push(value()?),
            "--scenario" => only.scenarios.push(value()?),
            "--dive" => only.dives.push(value()?),
            "--dolls" => only.part = Some(bench::Part::Dolls),
            "--dives" => only.part = Some(bench::Part::Dives),
            "--seeds" => {
                seeds = Some(value()?.parse().map_err(|_| "--seeds takes a number")?);
            }
            "--check" => mode = Mode::Check,
            "--print" => mode = Mode::Print,
            "--page" => mode = Mode::Page(value()?),
            // Reads play time instead of running the bench.
            "--calibrate" => {
                let path = match args.next_if(|next| !next.starts_with("--")) {
                    Some(path) => PathBuf::from(path),
                    None => front::saves::default_dir()
                        .ok_or("no home directory: name the playtime.csv")?
                        .join("playtime.csv"),
                };
                let text = std::fs::read_to_string(&path)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
                print!("{}", bench::calibrate::calibrate(&text));
                return Ok(());
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    // Only a run like the committed one may stand for it or be compared with it.
    let comparable = only.is_full() && seeds.is_none();
    if !comparable && matches!(mode, Mode::Check | Mode::Page(_)) {
        return Err("--check and --page need the full run: drop narrowing and --seeds".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let blueprints =
        game::Blueprints::load(&root.join("../data")).map_err(|err| err.to_string())?;
    let bench = bench::narrow(bench::load(root)?, &only)?;
    // `--seeds` sets both, for a quick look; else each its own default.
    let dive_seeds = seeds.unwrap_or(bench.dives.seeds);
    let seeds = seeds.unwrap_or(bench::SEEDS);
    let rows = bench::measure(&blueprints, &bench, seeds)?;
    let dives = bench::measure_dives(&blueprints, &bench, dive_seeds)?;
    let write = |path: &Path, text: &str| {
        std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
    };
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))
    };
    let report = bench::report(&blueprints, &bench, &rows, &dives, (seeds, dive_seeds));
    let numbers = || bench::json(&blueprints, &bench, &rows, &dives, seeds);
    let committed = root.join("report.md");
    if !comparable || mode == Mode::Print {
        print!("{report}");
        return Ok(());
    }
    let off = bench::misses(&dives, &bench.targets).len();
    eprintln!("{off} numbers off target (the report's Off target)");
    // No committed report yet: nothing to compare with.
    let changes = read(&committed)
        .map(|old| (old != report, bench::moved::moved(&old, &report)))
        .unwrap_or((true, Vec::new()));
    match mode {
        Mode::Check => {
            let (differs, changes) = changes;
            if !differs {
                eprintln!("nothing moved against bench/report.md");
                return Ok(());
            }
            if changes.is_empty() {
                eprintln!("no row moved; the report's other text changed");
            } else {
                eprint!("{}", bench::moved::summary(&changes));
            }
            Err("balance moved: run `cargo balance` and commit bench/report.md and .json with the change".into())
        }
        Mode::Page(path) => {
            let template = read(&root.join("page.html"))?;
            write(
                Path::new(&path),
                &bench::page(&template, &numbers(), committed_json().as_deref()),
            )
        }
        Mode::Write | Mode::Print => {
            eprint!("{}", bench::moved::summary(&changes.1));
            write(&committed, &report)?;
            write(&root.join("report.json"), &numbers())
        }
    }
}

/// The committed `bench/report.json`, for the page's "what moved"; `None` outside a repository or
/// before the first commit of it.
fn committed_json() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["show", "HEAD:bench/report.json"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
