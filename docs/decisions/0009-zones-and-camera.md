# 0009 — Zones, per-zone seeds, and the camera in `game`

Status: Accepted
Date: 2026-09-28

## Context
The fixed 40×20 room fit on screen and ignored its seed. We want generated
caves bigger than the screen, and a path to a Qud-style world of many
zones without building it yet.

## Decision
- **Discrete fixed-size zones.** A zone is `ZONE_WIDTH` × `ZONE_HEIGHT`
  (160×60) tiles, addressed by `ZoneCoord`. Only `(0, 0)` exists now; it
  is the whole map and saves as before.
- **Per-location seeds.** `zone_seed(world_seed, coord)` is
  `mix(mix(world_seed, x), y)`, where `mix(s, v) = splitmix64(s ^
  splitmix64(v))` is written out in `zone.rs`. Never `std::hash`, whose
  algorithms may change between releases. A zone therefore regenerates
  identically from the world seed alone, whichever zones were visited
  first.
- **Streams.** The map and spawn streams (ADR 0005) are seeded from the
  zone seed; combat stays on the world seed. Generation attempt `n` uses
  `Pcg64::new(mix(zone_seed, n), stream::MAP)`; the accepted attempt's
  stream becomes `Rngs::map`.
- **Cave generator.** Random fill, cellular-automata smoothing with a solid
  border, then only the largest 8-connected floor region (matching
  movement) is kept. If it covers less than `min_floor_fraction`, retry
  with the next derived seed, up to `CAVE_ATTEMPTS`, then settle for the
  largest. Tuning is data (`data/cave.ron`, ADR 0004).
- **Camera in `game`.** `render_view(state, width, height)` returns
  exactly the viewport, centred on the player and clamped to the zone;
  a viewport larger than the zone centres it with blank padding.
  `camera::Camera` holds the only screen↔world conversion. `term` only
  says how many tiles fit (`viewport_tiles`, beside `tile_to_screen`).

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
  camera would change only `Camera::follow`.
