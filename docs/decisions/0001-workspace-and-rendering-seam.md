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
- `tile_to_screen` in `term` is the one place tile→screen mapping lives, so 2-chars-per-tile later is a one-function change.
