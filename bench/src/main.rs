//! Balance bench entry point (`--seeds` sets the doll and the dive seeds alike): `bench [--seeds N] [--out FILE] [--json FILE] [--page FILE
//! [--previous FILE]]`. Prints the report, or writes it to `FILE`; `--json` also writes every
//! number as JSON, `--page` the readable page (`bench/page.html` filled in, with what moved since
//! `--previous`, a report's JSON). Run through `scripts/balance.sh`.

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
    let mut seeds = None;
    let (mut out, mut json, mut page, mut previous) = (None, None, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut file = || args.next().ok_or(format!("{arg} takes a file"));
        match arg.as_str() {
            "--seeds" => {
                seeds = Some(
                    args.next()
                        .and_then(|n| n.parse().ok())
                        .ok_or("--seeds takes a number")?,
                );
            }
            "--out" => out = Some(file()?),
            "--json" => json = Some(file()?),
            "--page" => page = Some(file()?),
            "--previous" => previous = Some(file()?),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let blueprints =
        game::Blueprints::load(&root.join("../data")).map_err(|err| err.to_string())?;
    let bench = bench::load(root)?;
    // `--seeds` sets both, for a quick look; else each its own default.
    let dive_seeds = seeds.unwrap_or(bench.dives.seeds);
    let seeds = seeds.unwrap_or(bench::SEEDS);
    let rows = bench::measure(&blueprints, &bench, seeds)?;
    let dives = bench::measure_dives(&blueprints, &bench, dive_seeds)?;
    let write =
        |path: &str, text: &str| std::fs::write(path, text).map_err(|err| format!("{path}: {err}"));
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))
    };
    let numbers = bench::json(&blueprints, &bench, &rows, &dives, seeds);
    if let Some(path) = &json {
        write(path, &numbers)?;
    }
    if let Some(path) = &page {
        let template = read(&root.join("page.html"))?;
        let previous = previous
            .as_deref()
            .map(|p| read(Path::new(p)))
            .transpose()?;
        write(path, &bench::page(&template, &numbers, previous.as_deref()))?;
    }
    let report = bench::report(&blueprints, &bench, &rows, &dives, (seeds, dive_seeds));
    match out {
        Some(path) => write(&path, &report),
        None if page.is_some() || json.is_some() => Ok(()),
        None => {
            print!("{report}");
            Ok(())
        }
    }
}
