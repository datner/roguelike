# Roguelike — Caves of Qud-inspired, Rust, terminal

## Commands

Tooling comes from the nix flake (direnv loads it). If `cargo` is not on PATH,
prefix commands with `nix develop --command`.

- **Before saying you're done:** `scripts/check.sh` — fmt, code-map freshness,
  crate-boundary check, clippy `-D warnings`, tests. Quiet on success, prints
  only the failing step on failure.
- Code map: `scripts/map.sh` regenerates the module index `docs/MAP.md`; run
  it after adding or removing a file, changing a module's first `//!`
  paragraph, or when `check.sh` says it is stale (sizes are rounded to 100
  lines). `scripts/map.sh --sigs <file or dir>...` prints `pub` signatures.
- One test: `cargo test -p game <test_name>`
- Look at the gui: `scripts/shots.sh <scratch dir>` renders every screen to
  PNGs (needs the GPU), then Read them. Do this after any drawing change.
- `nix flake check` only sees files git tracks: `git add -N` new files first.
- Play (humans only): `cargo run -p term -- --seed 42` (terminal) or
  `cargo run -p gui -- --seed 42` (window)

`term` is a full-screen TUI that blocks on keyboard input, and `gui` opens a
window. **Never run either** — they hang your shell and you can't see them. Verify behaviour with tests in `game` (or
ratatui's `TestBackend` for `term` widgets), and flows (keys → modes →
turns) by driving a `front::Session` with inputs.

## Tool usage

- Read files with Read (offset/limit for ranges), search with Grep, find with
  Glob; parallel calls beat one chained shell command. Bash is for building,
  testing, running and git; piping _command output_ into `head`/`tail` is fine.
- A PreToolUse hook rejects `cat`/`head`/`tail`/`sed`/`awk`/`less` on files. If
  it fires, switch tools; don't hunt for another shell workaround.
- For "where is this defined / who uses it", prefer the LSP tool
  (`goToDefinition`, `findReferences`, `incomingCalls`) over Grep: exact, no
  same-name noise. It runs the devShell's rust-analyzer through the project
  plugin in `.claude/skills/rust-lsp/`; the first query after start waits for
  indexing.

## Reading policy

- Orient from the module index below (each module's purpose and rough size).
  For a module's API, run `scripts/map.sh --sigs <path>` instead of opening
  the file just to learn what it contains.
- To find something, Grep first, then Read with offset/limit around the hit.
- Read a whole file only if you're editing it.
- Read an ADR only when touching its area; `docs/decisions/README.md` has the
  one-line index.

@docs/MAP.md

## Layout

- `game/` — library: all simulation and logic. See `game/CLAUDE.md`.
- `front/` — library shared by every frontend: `Session` (keyboard flows),
  title menu, save files; no drawing, no key bindings. See `front/CLAUDE.md`.
- `term/` — binary: crossterm + ratatui frontend. See `term/CLAUDE.md`.
- `gui/` — binary: eframe/egui windowed frontend. See `gui/CLAUDE.md`.
- `data/` — RON content (`data/entities/<PascalName>.ron` blueprints).
- `docs/vision.md` — where the game is going (pillars, character systems, camp,
  legacy). Read before designing a new system; `docs/plans/` hold build plans.
- `docs/decisions/` — short ADRs explaining _why_ things are the way they are.
  Read the relevant one before changing an invariant below.

## Invariants — do not break; if a task seems to require it, stop and ask

1. **Rendering seam.** `game` never depends on a terminal or graphics crate. It
   outputs `Grid<Cell>` and consumes `InputEvent`. `front` draws nothing
   either; frontends only map keys and draw. (ADR 0001, 0020, checked by
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
4. Report, following the rules below.

## Reporting

The user reads the code and asks when something is unclear. Keep reports short and factual.

- Structure: **Changed** (one line per file or area), **Left out** (only if something was deliberately skipped), **Follow-ups** (only if there are any), and **Check** (`scripts/check.sh` result). Leave out empty sections.
- One line per item. No prose paragraphs, no story of how you got there, no restating the task.
- Mention a problem you hit only if it's still unresolved or it affects a later decision.
- Don't explain Rust, the architecture, or the reasoning behind a choice unless it departs from an ADR or from what was asked.
- No closing summary and no offers of more help.
