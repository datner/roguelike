# 0005 — Per-subsystem RNG streams

Status: Accepted
Date: 2026-09-27
Supersedes: 0003

## Context
ADR 0003 routed all randomness through a single `GameState.rng`. With one shared stream, any extra draw anywhere (a new spawn roll, a reroll, a handler that consults the RNG) shifts every later draw in every other subsystem. Adding a goblin would change combat rolls; tweaking combat would move spawns. Seeded worlds and recorded bug repros break for unrelated reasons, and tests pinned to exact values churn.

## Decision
- `Rngs` on `GameState` holds one `rand_pcg::Pcg64` per subsystem: map (generation), spawn (placement), combat (attack rolls and the RNG event handlers get).
- Each is a PCG stream seeded with the same seed and a different stream id per subsystem. Stream ids are named constants in `game/src/rng.rs`, the only place they're defined.
- A subsystem draws only from its own stream. A new subsystem that needs randomness gets a new stream id and a new `Rngs` field. It doesn't borrow another subsystem's stream.
- Everything else from 0003 stands: `game/clippy.toml` disallows `rand::rng`, `rand::random`, clock reads, and `HashMap`/`HashSet` in `game`; the fresh-seed function in `game/src/map.rs` is the single annotated exception.

## Guarantees
- Same seed + same inputs ⇒ same state (map, spawns, combat rolls, log).
- Changing how many draws one subsystem makes, or their order, changes only that subsystem's results. Tests pin this: an extra draw on the spawn stream leaves the map and combat streams, and the rolls they produce, unchanged.
- The damage preview (`game/src/combat.rs`) clones only the combat stream, so a dry run's result equals the next real attack's.

## Consequences
- Changes to one subsystem's draw order still change seeded worlds for that subsystem. Call them out in the commit, as before.
- Stream ids are part of the save/seed format: never renumber or reuse one. Changing one reshuffles that subsystem for every seed.
- Streams share the seed as initial state and differ only by increment. PCG streams built this way are distinct but not guaranteed statistically independent. That's fine for a game; revisit if we ever need stronger independence (e.g. hash the seed per stream).
- The event context (`game/src/event.rs`) exposes a single RNG, currently the combat stream, because every handler today is combat. When a non-combat subsystem starts firing events, dispatch (`game/src/command.rs`) should pass that subsystem's stream instead.
