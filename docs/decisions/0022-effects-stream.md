# 0022 — An effects stream for frontends to animate

Status: Accepted
Date: 2026-10-22

## Context

ADR 0013 left missile `Trail`s on `GameState` for `term` to play back. The
windowed frontend (ADR 0020) also wants damage numbers, hit flashes, misses
and deaths. Frontends cannot work these out from the log's text or by
diffing states, and must not reach into the world.

## Decision

- `game::fx` holds `Fx::{Flight(Trail), Damage, Outcome, Death}`, pushed
  where the thing happens (flights where they start, damage where it is
  dealt, outcomes in `combat::announce`, deaths in `die`), with positions
  read then. `GameState.fx` replaces `trails`; `take_fx` drains it.
- Positioned effects go through `fx::at`, which drops those on tiles the
  player can't see: effects show no more than the map does.
- Effects are animation only: nothing in the simulation reads them, they
  are never saved, and they carry no randomness or time (ADR 0005).
- `front::Session::take_fx` hands them on. Each frontend picks what it
  shows on its own clock: `term` flights only, `gui` all of them. Any input
  finishes what is playing.

## Consequences

- A new kind of effect is a variant plus a push at its site; frontends
  that don't know it ignore it.
- Effects pile up if a frontend never takes them; both do after every
  input and auto step.
- Effects in `gui` are glyphs and text on the cell grid, which stays within
  egui (ADR 0020's triggers are not hit).
