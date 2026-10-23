# 0025 — Origins: what a character starts as, as data

Status: Accepted
Date: 2026-10-23

## Context

The vision has no classes: a character starts from what it is, with a few
skills, a kit, perhaps a style under way, and abilities nobody can teach.
Every character was one fixed `Player` blueprint carrying the bow, the
torch, every spell and the trip. Legacy will need to know where a character
began.

## Decision

- `data/origins.ron` lists origins in menu order: a key, the name and a
  one-line summary the player reads, the blueprint the character is made
  from, the styles it starts knowing and the one it follows. The first is
  the default (`GameState::new`; `new_as` takes a key).
- An origin's blueprint inherits `Player`, which is the bare person: body,
  stats, disciplines, `Learner`. Everything held or known at the start comes
  from the origin's blueprint. Load refuses an origin whose blueprint is not
  a player, whose styles are not styles, or that follows a style it does not
  list.
- The player carries `Origin(key)`, saved by key; the save version moved to
  42.
- The origin is part of a new game's input, like the seed. Spawning it draws
  no RNG, so a seed builds the same world for every origin; only `SpawnSeq`
  numbers after the player shift.
- Content without origins (tests' blueprints) starts from `Player`.

## Consequences

- A new origin is a blueprint and an entry; no code.
- Origins are compared on the bench like any build.
