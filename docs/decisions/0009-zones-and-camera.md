# 0009 — Zones, per-zone seeds, and the camera in `game`

Status: Accepted
Date: 2026-09-28

## Context
The fixed 40×20 room fit on screen and ignored its seed. We want generated
caves bigger than the screen, and a path to a Qud-style world of many
zones without building it yet.

## Decision
- **Discrete fixed-size zones.** A zone is 160×60 tiles (constants in
  `game/src/zone.rs`), addressed by `ZoneCoord`. Only `(0, 0)` exists now; it
  is the whole map and saves as before.
- **Per-location seeds.** A zone's seed is the world seed mixed with the
  zone's x, then y, where mixing `s` with `v` is
  `splitmix64(s ^ splitmix64(v))`, written out in `game/src/zone.rs`. Never `std::hash`, whose
  algorithms may change between releases. A zone therefore regenerates
  identically from the world seed alone, whichever zones were visited
  first.
- **Streams.** The map and spawn streams (ADR 0005) are seeded from the
  zone seed; combat stays on the world seed. Generation attempt `n` uses
  the zone seed mixed with `n`, on the map stream id; the accepted
  attempt's stream becomes the map stream.
- **Cave generator.** Random fill, cellular-automata smoothing with a solid
  border, then only the largest 8-connected floor region (matching
  movement) is kept. If it covers less than `min_floor_fraction`, retry
  with the next derived seed, up to a fixed number of attempts, then
  settle for the largest. Tuning is data (`data/cave.ron`, ADR 0004).
- **Camera in `game`.** The view `game/src/state.rs` renders is
  exactly the viewport, centred on the player and clamped to the zone;
  a viewport larger than the zone centres it with blank padding.
  `game/src/camera.rs` holds the only screen↔world conversion. `term` only
  says how many tiles fit (`term/src/widgets/map_widget.rs`).

## Consequences
- Every frontend gets identical scrolling for free; a frontend can't show
  tiles the camera didn't pick.
- Changing `data/cave.ron`, the mixing function or the generator's draw
  order changes every seeded world. Saves are unaffected: they store the
  map, not the seed-derived result.
- Tests other than the generator's use a hand-built room, so tuning the
  generator can't break them.
- Multiple zones, edge transitions and per-zone saves will need zone
  identity in `GameState` and the save format; revisit then. A deadzone
  camera would change only `game/src/camera.rs`.
