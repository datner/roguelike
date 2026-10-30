# Plan: cleanup — one answer per question, no stale pointers

Handoff document. Read `game/CLAUDE.md` (Patterns) first.

## Goal

Lessons so far: the same question answered in several places drifts (fights-2's step warning
disagreed with the step it warns about, since each asked "who stands there" its own way), and
pointers to things that are gone mislead the next reader (`term` was deleted, its name lives on in
module docs). This plan removes the duplicates the fights-2 review found and the stale pointers. It
adds no abstraction: each slice deletes a copy in favour of a function that already exists.

Out of scope: moving auxiliary fields out of `Combo` and other data (`one concern per part`
follow-ups) waits on the owner's choice; splitting the big files (`technique.rs`, `spell.rs`) is not
a complexity cut by itself.

## Slices

### 1. One "who stands there" — done

`technique::creature_at` (the living creature on a tile) is the one lookup. `command::fightable_at`
and `try_move` use it with one own-summon check (`own_summon`), and `technique::step_warning` asks
`fightable_at`, so a step that swaps with your summon onto a struck tile is warned like any step
(the bug). Test: the summon case warns.

### 2. Shove into water lands on the bank; the warning flow is tested

`shove` writes the target's position before a `Sink`, so the player hauls out where they went in
after a long shove, not where the shove started. A `Session` test drives the refuse-once flow:
refused, same key goes, another key forgets.

### 3. Dive test and call-site tidying

One test helper replaces a tile across a map (the drained/cleared loops); `dig` takes its arguments
directly instead of through a tuple built for it.

### 4. Stale pointers

Module docs no longer name `term` (`gui` input, fx, panels, theme; `game` tiles); the ADR index
lists 0017–0025 above its template. `docs/MAP.md` regenerated.
