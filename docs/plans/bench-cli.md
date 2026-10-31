# Plan: the bench as its own command

Handoff document. Read `docs/plans/balance.md`, `docs/plans/floor-bot.md` and `bench/src/main.rs`
first.

## Goal

`scripts/balance.sh` takes minutes and says nothing while it runs. Every run (a build against a
scenario or through a dive, on one seed) is played one after another on one core, though each is a
game of its own and shares nothing. The bench should use the machine, say how far along it is, run
just what a question needs, and say what it found. The `bench` binary is already the command; the
script around it only adds a nix fallback, the `--check` diff and the page's git lookup. Those move
into the binary and the script goes, replaced by a cargo alias: `cargo balance [...]`.

## Decisions taken for this plan

- **No new dependencies.** Threads are `std::thread::scope`, the progress line is a rewritten stderr
  line, arguments stay hand-parsed: a handful of flags, a tool for the owner and the agent.
- **Parallel without changing a number.** All runs go in one job list; workers take jobs in turn and
  each result goes back to its job's slot, so rows come out in the order they always did and the
  report is byte-identical. Every run builds its own `GameState` from its seed (ADR 0005), so the
  order runs finish in can't change them.
- **No nix fallback in scripts.** direnv loads the flake's dev shell; a script run outside it should
  fail on `cargo` missing, not start a second shell. The rust-analyzer wrapper keeps its own:
  Claude Code starts it outside the shell.
- **A narrowed run never touches the committed report.** `--build`, `--scenario` and `--dive` print
  only what they ran; with any of them, `--check` and writing `bench/report.*` are refused.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. No nix fallbacks — done
- `scripts/check.sh` and `scripts/balance.sh` lose `exec nix develop`; `CLAUDE.md` loses the
  `nix develop --command` advice; `.claude/settings.json` loses the permissions only it needed.
- As built: the README keeps its `nix develop` lines, the setup for someone without direnv.

### 2. Parallel runs — done
- `bench::measure` and `floors::measure` build their job lists and run them on
  `std::thread::available_parallelism()` workers, results back in job order.
- Tests: a parallel measure equals a sequential one over the same seeds; `scripts/balance.sh
  --check` still matches the committed report.
- As built: `bench/src/jobs.rs` (`run_all`), one job per run (a seed of one window or one dive).
  Tested by order and first-error; the full report came out byte-identical to the committed one.
  On 10 cores: 154 s to 21 s; the dolls take under a second, the dives the rest.

### 3. Progress — done
- While a run is going and stderr is a terminal: one line, rewritten, with the part (dolls or
  dives), jobs done of all, the job just finished and the time so far; cleared at the end, then a
  last line with each part's time.
- As built: inside `run_all`; the closing line (`dives: 320 runs in 0:20`) prints either way.

### 4. Narrowed runs
- `--build NAME` (repeatable), `--scenario NAME`, `--dive NAME`: only matching rows run and print.
  `--dolls` / `--dives` run one part only. An unknown name is an error naming the known ones.
- Tests: a filter keeps only matching rows; an unknown name is refused; a narrowed run with
  `--out` or `--check` is refused.

### 5. Findings
- After a full run, on stderr: the numbers outside `bench/targets.ron`'s ranges, and, against the
  committed `bench/report.json`, which rows moved and by how much (the page's comparison, shared).

### 6. One command
- `--check` in the binary (regenerate, compare with `bench/report.md`, print the diff's rows, exit
  1 if moved); `--page` reads the previous JSON from `git show HEAD:bench/report.json` itself;
  `--calibrate` defaults to the gui's saves directory (`front::saves::default_dir`).
- `.cargo/config.toml`: `[alias] balance = "run --quiet --release -p bench --"`.
  `scripts/balance.sh` is removed; `CLAUDE.md`, `README.md`, the plans and the `/balance-report`
  skill name `cargo balance`.

## Out of scope

- A full-screen terminal UI, an argument-parsing crate, caching results between runs.

Exempt: tooling; builds no game system.
