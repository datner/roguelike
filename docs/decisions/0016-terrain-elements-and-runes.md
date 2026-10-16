# 0016 — Terrain overlays, element interactions and rune composition

Status: Proposed
Date: 2026-10-17

Not built. The last slice of `docs/plans/spells.md` asks for three systems
that each change something every module relies on, so they need a decision
before any code.

## Context

- **Terrain-making spells** (a wall of ice, a pool of oil) change what is
  passable and opaque. Today `Map` is a fixed `Grid<Tile>`, and
  `Tile::is_open` is the one walk/see-through check behind movement, FOV,
  flight, areas and AI paths.
- **Element interactions** (wet + shock, oil + fire) need state on tiles
  and creatures that elemental damage (now `Fire`, `Cold`, `Shock` in
  `DamageType`) can read and change.
- **Rune composition** (Form × Element × Modifier) builds a spell from parts
  at cast time, with a cost from those parts.

## Proposal

1. **A timed overlay on the map, not new tiles.** `Grid<Option<Overlay>>`
   beside `Map`, each overlay a `Timed` with what it makes the tile (wall,
   water, oil, fire). `Tile::is_open` becomes `map::is_open(map, overlay,
   at)`, and every caller moves to it in one change; `check.sh` can ban the
   old call. Expiry is lazy (read against `now`), plus one idempotent timer
   per overlay for side effects such as fire spreading (ADR 0006). Saved
   with the map.
2. **Statuses are effects; interactions are handlers.** `Wet`, `Oiled`,
   `Burning` become timed effect entities on creatures (ADR 0006), and
   overlays on tiles. Interactions are `BeforeTakeDamage` handlers keyed on
   the status part (`Wet` doubles `Shock`, `Oiled` adds a `Burning` effect on
   `Fire`), with priorities in `prio.rs`. Nothing in `combat` names an
   element. Tile interactions run in the applier that changes the overlay.
3. **Runes are a fourth discipline plus an assembly step.** A rune is an
   item part (`Rune::Form(Shape)`, `Rune::Element(DamageType)`,
   `Rune::Modifier(..)`); casting from runes builds a transient spell entity
   from the chosen parts (spawned, attached, despawned after the cast), so
   `CheckCast`/`PayCost` and `footprint` stay unchanged. A `RuneCasting`
   discipline prices it from the parts: power is their sum.

## Consequences

- The overlay touches FOV, movement, flight, areas, AI and saves at once:
  plan it as its own slice, with the `is_open` migration first and no
  behaviour change.
- Interactions only need the overlay for tiles; creature statuses can come
  first, on the existing effect system.
- Runes need a frontend for choosing parts; the transient-entity approach
  keeps the game side small.
