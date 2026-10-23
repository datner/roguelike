//! Balance bench entry point: `bench [--seeds N] [--out FILE]`. Prints the report, or writes it to
//! `FILE`. Run through `scripts/balance.sh`.

use std::path::Path;
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

fn run() -> Result<(), String> {
    let mut seeds = bench::SEEDS;
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seeds" => {
                seeds = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or("--seeds takes a number")?;
            }
            "--out" => out = Some(args.next().ok_or("--out takes a file")?),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let blueprints =
        game::Blueprints::load(&root.join("../data")).map_err(|err| err.to_string())?;
    let bench = bench::load(root)?;
    let rows = bench::measure(&blueprints, &bench, seeds)?;
    let report = bench::report(&blueprints, &bench, &rows, seeds);
    match out {
        Some(path) => std::fs::write(&path, report).map_err(|err| format!("{path}: {err}")),
        None => {
            print!("{report}");
            Ok(())
        }
    }
}
