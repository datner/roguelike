//! Balance bench entry point (`--seeds` sets the doll and the dive seeds alike): `bench [--seeds N] [--out FILE] [--json FILE] [--page FILE
//! [--previous FILE]]`. Prints the report, or writes it to `FILE`; `--json` also writes every
//! number as JSON, `--page` the readable page (`bench/page.html` filled in, with what moved since
//! `--previous`, a report's JSON). `--calibrate FILE` reads a `playtime.csv` instead and prints
//! keypresses an hour. `--build`, `--scenario`, `--dive` (each repeatable), `--dolls` and `--dives`
//! narrow a run, which then only prints. Run through `scripts/balance.sh`.

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
    let mut only = bench::Narrow::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} takes a value"));
        match arg.as_str() {
            "--build" => only.builds.push(value()?),
            "--scenario" => only.scenarios.push(value()?),
            "--dive" => only.dives.push(value()?),
            "--dolls" => only.part = Some(bench::Part::Dolls),
            "--dives" => only.part = Some(bench::Part::Dives),
            "--seeds" => {
                seeds = Some(
                    args.next()
                        .and_then(|n| n.parse().ok())
                        .ok_or("--seeds takes a number")?,
                );
            }
            "--out" => out = Some(value()?),
            "--json" => json = Some(value()?),
            "--page" => page = Some(value()?),
            "--previous" => previous = Some(value()?),
            // Reads play time instead of running the bench.
            "--calibrate" => {
                let path = value()?;
                let text =
                    std::fs::read_to_string(&path).map_err(|err| format!("{path}: {err}"))?;
                print!("{}", bench::calibrate::calibrate(&text));
                return Ok(());
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    // A narrowed run answers a question; only a full one may stand for the report.
    if !only.is_full() && (out.is_some() || json.is_some() || page.is_some()) {
        return Err("a narrowed run only prints: drop --out, --json and --page".into());
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
