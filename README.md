# roguelike

A terminal roguelike skeleton (Caves of Qud-inspired), structured so the
rendering/simulation seam is enforced by the compiler rather than by
convention.

## Structure

This is a Cargo workspace with two crates:

- `game/` — a library with all simulation and game logic. It has **no**
  dependency on crossterm, ratatui, or any other terminal/graphics crate —
  that's enforced by `game/Cargo.toml` simply never listing one. It exposes:
  - `Cell { glyph, fg, bg }` (with its own `Rgb` type) — the only rendering
    output, a grid of these for the visible map.
  - `InputEvent` (`Move(Dir)`, `Wait`, `Quit`) — the only input a frontend
    can send in; raw keys never reach `game`.
  - A `GameState` holding an `hecs::World`, the tile map, a message log,
    and a seeded RNG, mutated only by draining a `Command` queue
    (`apply_commands`) rather than directly from input handlers.
  - `EntityBlueprint`, loaded from `data/entities/*.ron` via serde + RON,
    as the pattern for data-driven content going forward.
- `term/` — the binary frontend: crossterm (reached only through
  `ratatui::crossterm`, never as a direct dependency) + ratatui. It maps
  raw key events to `game::InputEvent`, renders `Grid<Cell>` through a
  clipped custom `MapWidget`, and owns all terminal lifecycle concerns
  (raw mode, alternate screen, Kitty keyboard protocol enhancement when
  supported, panic-safe restore).

A future windowed/tileset frontend would be a new crate depending on
`game`, with no changes to `game` itself.

## Controls

- Arrow keys or `hjkl`: move orthogonally.
- `y` / `u` / `b` / `n`: move diagonally (NW/NE/SW/SE).
- `.`: wait.
- `q` or `Esc`: quit.

## Running

```bash
nix develop --command cargo run -p term
# or, with a fixed seed:
nix develop --command cargo run -p term -- --seed 42
```

With direnv installed and `direnv allow`'d in this directory, the `nix
develop` shell activates automatically and you can just run `cargo run -p
term`.

## Development

```bash
nix develop --command cargo test --workspace
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
```

## Notes on the rendering seam

- One map tile currently renders as one terminal character. The
  tile-to-screen mapping lives in a single function,
  `term::widgets::map_widget::tile_to_screen`, so switching to two
  characters per tile (terminal cells are roughly 1:2, width:height) later
  means editing only that function.
- Movement currently goes through a minimal command queue
  (`game::Command` + `game::apply_commands`); the full part/event system
  this will eventually grow into is intentionally not built yet.
