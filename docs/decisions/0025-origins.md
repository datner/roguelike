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
- An **ability** is a technique or part marked `Ability`: it comes from what
  the creature is. Load refuses one named by a manual (`Teaches`), a style's
  `invents` or a discovery, so only a blueprint (an origin's, a monster's)
  can have one. Lists show abilities apart from what was learned. Saved as a
  marker; the save version moved to 43.

- A new game asks for the origin after the name (`front::menu`), from
  `game::origins`, which says what each starts with in plain words, so
  every frontend says the same. Loading a save never asks. The character
  sheet heads with the origin's name.

- The first origins: the wanderer (bow, quiver, torch, Archery 1; the
  default), the duelist (rapier, the off hand empty, fencing followed at 1),
  the hedge mage (torch, three spells) and the pit fighter (leather, torch,
  Brawling known, the headbutt as its ability). The seven spells every
  character used to know went to the hedge mage (three) or wait for magic
  styles to invent them (ADR 0016).

- Origins are not balanced against each other: there is no difficulty
  setting, and choosing an origin is the difficulty choice (vision). The
  bench measures how far apart they start; a gap is not a defect.

## Consequences

- A new origin is a blueprint and an entry; no code.
- Origins are compared on the bench like any build: a build names the
  origin it starts as (`origin:`, the first if absent) and keeps its kit
  unless it lists `equipment`; its skills are set over the origin's, and
  its XP cost counts only what it adds. Each origin has a starter build,
  and an origin is named as a source of what it starts with.
