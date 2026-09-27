# Roguelike — Caves of Qud-inspired, Rust, terminal

## Commands

Tooling comes from the nix flake (direnv loads it). If `cargo` is not on PATH,
prefix commands with `nix develop --command`.

- **Before saying you're done:** `scripts/check.sh` — fmt, crate-boundary check,
  clippy `-D warnings`, tests. Quiet on success, prints only the failing step on
  failure.
- One test: `cargo test -p game <test_name>`
- Play (humans only): `cargo run -p term -- --seed 42`

`term` is a full-screen TUI that blocks on keyboard input. **Never run it** — it
hangs your shell and you can't see it. Verify behaviour with tests in `game` (or
ratatui's `TestBackend` for `term` widgets).

## Tool usage

- Read files with Read (offset/limit for ranges), search with Grep, find with
  Glob; parallel calls beat one chained shell command. Bash is for building,
  testing, running and git; piping _command output_ into `head`/`tail` is fine.
- A PreToolUse hook rejects `cat`/`head`/`tail`/`sed`/`awk`/`less` on files. If
  it fires, switch tools; don't hunt for another shell workaround.

## Layout

- `game/` — library: all simulation and logic. See `game/CLAUDE.md`.
- `term/` — binary: crossterm + ratatui frontend. See `term/CLAUDE.md`.
- `data/` — RON content (`data/entities/<PascalName>.ron` blueprints).
- `docs/decisions/` — short ADRs explaining _why_ things are the way they are.
  Read the relevant one before changing an invariant below.

## Invariants — do not break; if a task seems to require it, stop and ask

1. **Rendering seam.** `game` never depends on a terminal or graphics crate. It
   outputs `Grid<Cell>` and consumes `InputEvent`. (ADR 0001, checked by
   `scripts/check.sh`.)
2. **crossterm only via `ratatui::crossterm`.** `term` has no direct crossterm
   dependency, so the versions can't drift.
3. **Mutation through the command queue.** Input and systems push `Command`s
   onto `GameState.commands`; `apply_commands` drains and applies them. Don't
   mutate the world while iterating a query. (ADR 0002)
4. **Determinism.** Same seed + same inputs = same state. All randomness comes
   from `GameState.rngs`, one stream per subsystem (map, spawn, combat; ids in
   `game/src/rng.rs`). No `rand::rng()`, no clock, no `HashMap`/`HashSet` in
   `game` — clippy enforces this via `game/clippy.toml`. (ADR 0005,
   superseding 0003)
5. **Content is data.** New kinds of things are RON blueprints composed from
   reusable parts, not hardcoded spawns. (ADR 0004)
6. **Time is relative and timestamp-checked.** Timed state stores `start` +
   `duration` against `GameState.now`; never countdowns or stored end ticks.
   "Is it active" is read from timestamps, never from timers; timers are
   idempotent checks. Durations change only via `ModifyDuration`. Scheduling
   ties break on `SpawnSeq`, never `Entity` order. (ADR 0006)

## Conventions

- Edition 2024; toolchain pinned in `rust-toolchain.toml`.
- Dependencies go in root `[workspace.dependencies]` and are referenced with
  `.workspace = true`. Justify any new crate; prefer std. **Check the current
  API on docs.rs or in `~/.cargo/registry` instead of trusting memory** — e.g.
  rand 0.10 uses `rand::rng()`, `RngExt::random_range`; ratatui 0.30 uses
  `frame.area()`, `Block::bordered()`; hecs 0.11 changed its query iterator API.
- No `unwrap`/`expect` in library code outside tests; return `Result` or handle
  `None` (out-of-bounds `Grid::get` = treat as wall).
- Unit tests sit in `#[cfg(test)] mod tests` next to the code. Tests reading
  `data/` build the path from `env!("CARGO_MANIFEST_DIR")`.
- Doc-comment the _why_, not the what.
- Commits: conventional, scoped by crate — `feat(game): …`, `fix(term): …`,
  `docs: …`. One logical change per commit.

## Scope discipline

- Do the task as scoped. Don't build systems "for later"; the part/event system
  grows one slice at a time.
- A prompt's "Out of scope" list is a hard boundary. Mention follow-ups instead
  of doing them.
- If a design fights the borrow checker, stop and propose the idiomatic
  alternative (collect-then-apply, emit commands, split borrows) rather than
  papering over it with `Rc<RefCell<_>>` or clones.

## Finishing a task

1. `scripts/check.sh` passes.
2. README's Structure/Controls updated if user-visible behaviour changed.
3. New architectural decision? Add an ADR in `docs/decisions/`.
4. Report: what changed, what you deliberately left out, suggested follow-ups.
