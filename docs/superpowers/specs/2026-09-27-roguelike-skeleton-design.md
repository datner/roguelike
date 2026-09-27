# Roguelike skeleton design

Date: 2026-09-27
Status: Approved

## Purpose

Establish a small, correctly-architected skeleton for a terminal roguelike
(Caves of Qud-inspired) so that later feature work has the right seams
already in place: simulation logic (`game`) is compiler-enforced to be
independent of any terminal/rendering crate, and the terminal frontend
(`term`) is a thin consumer of an abstract `Cell` grid and `InputEvent`
stream. This is a skeleton, not a game: one hardcoded/generated map, one
player entity, movement + wall collision, one data-driven blueprint loaded
from RON, and a clean dev environment (nix flake + direnv).

## Dev environment

- `flake.nix`:
  - Inputs: `nixpkgs` (unstable), `rust-overlay` (`github:oxalica/rust-overlay`),
    `flake-utils` (or manual `eachDefaultSystem`-style handling) for
    `aarch64-darwin` and `x86_64-linux`.
  - Rust toolchain pinned via `rust-toolchain.toml` (channel = stable,
    components = `rustfmt`, `clippy`, `rust-src`, `rust-analyzer`) and
    consumed with `rust-bin.fromRustupToolchainFile ./rust-toolchain.toml`.
    The default rustup profile already ships clippy/rustfmt; we only need to
    add `rust-src` and `rust-analyzer` as extra components.
  - `devShells.default`: just the toolchain, no native/system deps (pure
    terminal app).
- `.envrc`: `use flake`.
- `.gitignore`: `target/`, `.direnv/`.

## Workspace layout

```
Cargo.toml               # virtual workspace manifest
rust-toolchain.toml
flake.nix
.envrc
.gitignore
game/
  Cargo.toml
  src/
    lib.rs
    cell.rs              # Cell, Rgb
    input.rs             # InputEvent, Dir
    map.rs                # Tile, Grid<T>, map generation
    command.rs            # Command, apply_commands
    state.rs              # GameState, MessageLog
    blueprint.rs           # EntityBlueprint, RON loading
data/
  entities/
    rubble.ron            # example blueprint: an obstacle that blocks movement
term/
  Cargo.toml
  src/
    main.rs                # CLI parsing, seed, wiring
    tui.rs                 # init/restore, panic hook, kitty enhancement flags
    input.rs                # crossterm KeyEvent -> game::InputEvent
    app.rs                  # main loop (blocking read, redraw per event)
    widgets/
      map_widget.rs         # ratatui Widget impl over &Grid<Cell>
README.md
```

Root `Cargo.toml`:

```toml
[workspace]
members = ["game", "term"]
resolver = "3"

[workspace.package]
edition = "2024"

[workspace.dependencies]
hecs = "0.11.1"
serde = { version = "1.0.229", features = ["derive"] }
ron = "0.12.2"
rand = "0.10.3"
rand_pcg = "0.10.2"
ratatui = "0.30.2"
```

`game` and `term` set `edition.workspace = true` and pull shared deps with
`dep.workspace = true`. `term` depends on `ratatui` only — no direct
`crossterm` dependency. It reaches crossterm exclusively through
`ratatui::crossterm` (the `crossterm` feature is on by default in ratatui
0.30), so there is exactly one crossterm version in the dependency graph and
no risk of it drifting from what ratatui expects.

## Rendering seam (`game`)

- `cell.rs`: `struct Rgb(pub u8, pub u8, pub u8)`; `struct Cell { glyph: char,
  fg: Rgb, bg: Rgb }`. No crate types leak in.
- `input.rs`: `enum Dir { N, S, E, W, NE, NW, SE, SW }`, `enum InputEvent {
  Move(Dir), Wait, Quit }`. `term` is the only place that maps raw keys to
  these.
- `map.rs`: `enum Tile { Floor, Wall }`, `struct Grid<T> { width, height,
  cells: Vec<T> }` with bounds-checked get/set. `generate_map(seed: u64) ->
  (Grid<Tile>, Pcg64)` builds a fixed bordered room with a couple of interior
  wall segments. The layout itself is currently deterministic/fixed
  regardless of seed (no procedural branching yet), but the function still
  takes and threads a seeded `rand_pcg::Pcg64` so callers/tests can assert
  "same seed → same map" now and real procedural variation slots in later
  without changing the signature.
- `state.rs`: `struct GameState { world: hecs::World, map: Grid<Tile>, log:
  MessageLog, rng: Pcg64, commands: VecDeque<Command> }` — disjoint fields,
  no interior mutability needed. `render_view(state: &GameState) ->
  Grid<Cell>` is the entire read surface `term` uses; it walks entities +
  map tiles and produces the glyph/color grid.
- `command.rs`: `enum Command { TryMove(Entity, Dir) }`. Input handling
  translates `InputEvent::Move(dir)` into `Command::TryMove(player, dir)`
  and pushes it to `state.commands`. A separate `apply_commands(&mut
  GameState)` drains the queue once per input event: for `TryMove`, compute
  the target tile, check `Tile::Wall` and check for a blocking entity there
  (via a `BlocksMovement` marker component used by the RON-loaded obstacle),
  and either move the entity or push a message like "You bump into a wall."
  / "Something blocks your way." to the log. This is the minimal
  command-queue seam the spec calls for; no generalized event/part system
  yet.
- `blueprint.rs`: `struct EntityBlueprint { name: String, glyph: char, fg:
  Rgb, bg: Rgb, blocks_movement: bool }`, `fn load_blueprint(path: &Path) ->
  Result<EntityBlueprint, BlueprintError>` via `serde` + `ron`. `data/entities/rubble.ron`
  is loaded at startup and spawned onto one fixed floor tile as a
  `BlocksMovement`-tagged entity, so the blueprint's `blocks_movement` field
  has a real, testable effect (walking into it blocks and logs a message,
  exactly like a wall) rather than being inert decoration.

## `term` crate

- `tui.rs`: does **not** reuse `ratatui::init()`'s built-in panic hook,
  because that hook doesn't know about the Kitty keyboard enhancement
  flags. Instead:
  - `init() -> io::Result<Terminal<CrosstermBackend<Stdout>>>`: enable raw
    mode, enter alternate screen, and — only if
    `ratatui::crossterm::terminal::supports_keyboard_enhancement()?` is
    true — push keyboard enhancement flags (disambiguate escape codes +
    report event types, the minimal set ratatui/crossterm recommend for key
    repeat/release disambiguation). Track in a `bool` whether flags were
    pushed.
  - `restore(pushed_flags: bool)`: best-effort pop keyboard enhancement
    flags if `pushed_flags`, leave alternate screen, disable raw mode. All
    steps are attempted independently (log/ignore individual errors) so a
    partial failure doesn't skip the rest.
  - A panic hook is installed once at startup (wrapping
    `std::panic::take_hook()`): on panic, call `restore(...)` best-effort,
    then invoke the previous hook, so a panic never leaves the terminal
    stuck in raw/alt-screen/kitty-enhanced mode.
  - Normal exit restores via the same `restore()` call (e.g. a small guard
    struct with `Drop`, or an explicit call right before `main` returns) —
    the point is there's exactly one restore implementation used by both
    paths.
- `widgets/map_widget.rs`: `MapWidget<'a>(&'a Grid<Cell>)` implementing
  `ratatui::widgets::Widget`. Clips to the given `Rect` by only writing
  cells whose mapped screen position is inside `area`, and uses
  `buf.cell_mut(pos)` (which returns `Option<&mut Buffer_Cell>`) rather than
  direct buffer indexing — so a terminal smaller than the map, or a map
  smaller than its panel, can never panic. `tile_to_screen(x: u16, y: u16)
  -> (u16, u16)` is a single free function (currently the identity, one map
  tile = one screen cell) that both `MapWidget` and any future
  hit-testing/scrolling code call, so switching to 2 chars/tile later means
  editing one function.
- `input.rs`: maps arrow keys and hjkl + yubn (vi-key diagonals) to `Dir`,
  `q` and `Esc` to `InputEvent::Quit`; unrecognized keys produce no event.
- `app.rs`: main loop blocks on `event::read()` (no poll/tick loop yet).
  Each iteration: read one crossterm event; if it's a `Key` event, map to
  `InputEvent` (ignore key-release events so held/repeated keys under the
  Kitty protocol don't double-fire), push the resulting `Command`s, and run
  `apply_commands`; if it's `Resize`, do nothing but fall through to
  redraw. After handling the event, always redraw (`terminal.draw(...)`)
  before looping again. `InputEvent::Quit` breaks the loop.
- `main.rs`: parses `--seed <u64>` and `--data <path>` by hand (no `clap`
  at this scope — two optional flags don't need a parsing crate). Seed
  defaults to `rand::rng().random()` if not given, and is always logged to
  the message log on startup ("Seed: 12345"). The data directory defaults
  to `concat!(env!("CARGO_MANIFEST_DIR"), "/../data")` (resolved at compile
  time relative to the `term` crate, not the process's current working
  directory) and can be overridden with `--data`.

## Testing

`game` unit tests (no terminal/ratatui in the dependency graph):
1. Move onto an open floor tile succeeds and updates position.
2. Move into a `Tile::Wall` is blocked, position unchanged, and a message
   is appended to the log.
3. Move into the RON-loaded `BlocksMovement` entity is blocked and logs a
   distinct message (exercises `blocks_movement` end-to-end).
4. Diagonal move (e.g. via a yubn-equivalent `Dir`) succeeds the same way
   as an orthogonal one.
5. `generate_map(seed)` called twice with the same seed produces
   identical `Grid<Tile>` contents (same seed → same map).

## Acceptance

- `cargo run -p term` launches into map / message-log / status panels,
  `@` moves with arrows and hjkl/yubn, walls and the blueprint obstacle
  block movement and log a message, `q`/`Esc` quits cleanly, terminal is
  restored on both normal exit and panic, resize is handled, seed is
  configurable and always printed.
- `cargo test` passes.
- `cargo clippy --workspace -- -D warnings` is clean.
- README documents the workspace structure, controls, and how to run it.

## Out of scope (explicitly deferred)

- Procedural map generation beyond the fixed layout.
- General part/event system beyond the single-purpose movement command
  queue.
- Two-characters-per-tile rendering (only the seam for it).
- Windowed/tileset backend (only the guarantee that `game` doesn't block
  one).
- `clap`, tick-based input polling, save/load, combat, inventory, FOV.
