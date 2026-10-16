# 0008 — Save games

Status: Accepted
Date: 2026-09-28

## Context
Characters need to persist between sessions. `game` must stay free of file IO
and platform concerns (ADR 0001), a loaded game must continue exactly as if it
had never stopped (ADR 0005, 0006), and hecs `Entity` ids and `BlueprintId`s
are runtime-only, so neither can be written to disk as-is.

## Decision
- **Split.** `game/src/save.rs` converts `GameState` to and from a RON string;
  `term` owns paths, reading and writing.
- **Between turns only.** Saving is refused while the command queue on
  `GameState` is non-empty. Queued commands are transient and never persisted.
- **Versioned, no migrations.** The top-level `version` is read first, on its
  own, so an old or newer save fails with a version error rather than a parse
  error. A mismatch refuses the save; changing the format bumps
  the save version (`game/src/save.rs`).
- **Entities by value, references by index.** Each entity is a list of
  component values. Entities are ordered canonically — by `SpawnSeq`, each
  followed depth-first by what it carries (`Attachments`) — so the save
  depends only on game state, never on hecs ids or archetype layout. Every
  `Entity` reference (`Attachments`, `EffectTarget`, timers, the player) is
  written as an index into that list and remapped on load. A timer whose
  entity is already gone is kept as a no-op (`None`), so the queue and the
  clock jumps it causes are unchanged.
- **Every entity has a `SpawnSeq`.** Entities are created only by spawning
  (`game/src/state.rs`, described by `game/src/spawn.rs`), which numbers
  the entity and its equipment (clippy bans `World::spawn` in `game`); saving fails if any
  entity lacks one, so save order never falls back to hecs ids.
  The world on `GameState` is crate-private, so no other crate can spawn
  around it.
- The `SpawnSeq` invariant is checked on both save and load.
- **Nothing skipped silently.** Saving walks the save format's list of
  component kinds with an exhaustive match; an entity with a component the
  format doesn't cover fails to save. Loading matches exhaustively too.
- **Blueprints by name.** Blueprint references (`OnHit` effects) are saved as
  names and resolved against the blueprints loaded at start; an unknown name
  fails loading.
- **What is persisted:** seed, tick, timer queue with insertion seqs (same-tick
  order) and its counter, the `SpawnSeq` counter, every RNG stream's state,
  map, seen tiles, message log, the game-over flag, the player, and all entities.
  The visible set and the event registry are rebuilt on load.
- **Placeholder save policy (term).** One save per character,
  `<saves>/<name>.ron`, loaded on start (ignoring `--seed`) and written on
  quit via a temp file in the same directory plus rename. Death does not
  save, so the last save survives. A save that fails to load is never
  overwritten: the game exits with an error.

## Consequences
- Round trips are byte-identical: saving a loaded game reproduces the save,
  and a resumed run matches an uninterrupted one.
- Every new component needs a variant in both save-format enums
  (`game/src/save.rs`);
  one holding an `Entity` or `BlueprintId` needs remapping in both
  directions. Any format change bumps the save version, invalidating old saves.
- RNG state is 128-bit, so `ron` needs its `integer128` feature.
- Save-on-quit loses progress on a crash, and dying keeps an older save
  loadable (no permadeath yet). Autosave, slots, character select and
  deleting saves on death are left for a real save policy.
