# 0001 — `game`/`term` workspace split with a `Cell`/`InputEvent` seam

Status: Accepted
Date: 2026-09-27

## Context
The game should run in a terminal now, and possibly get a windowed/tileset frontend later, without rewriting the simulation.

## Decision
A Cargo workspace with `game` (library: all simulation) and `term` (binary: crossterm + ratatui). `game` produces a `Grid<Cell { glyph, fg, bg }>` and consumes an abstract `InputEvent`. `game/Cargo.toml` never lists a terminal or graphics crate; `scripts/check.sh` fails if one appears in its dependency tree. `term` reaches crossterm only through `ratatui::crossterm`.

## Consequences
- A new frontend is a new crate depending on `game`; `game` is untouched.
- Anything visual (colours, glyphs, highlighting, targeting overlays) must be expressible as `Cell`s or as a new abstract output type in `game` — no reaching for terminal features from simulation code.
- The frontend sends intent only: it passes a `PlayerAction` to the turn entry point (`game/src/time.rs`) and `game` resolves the actor and the commands, so `term` never holds a `Command`, an `Entity` or any hecs value (checked by `scripts/check.sh`).
- Tile→screen mapping lives in one place (`term/src/widgets/map_widget.rs`), so 2-chars-per-tile later is a one-function change.
