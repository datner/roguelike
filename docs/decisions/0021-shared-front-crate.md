# 0021 — Frontends share their flows through a `front` crate

Status: Accepted
Date: 2026-10-21

## Context

ADR 0020 adds a windowed frontend beside `term`. `term`'s keyboard flows,
the modal dispatch, the inventory/equipment panel, the throw and spell
choosers, the aiming and look cursors, the title menu and save files, were
ratatui-free apart from drawing, and the dispatch carries real rules: a
refused shot keeps the cursor up, an uncastable spell keeps the list up
with the reason logged, any key stops auto-explore. Two copies of those
would drift.

## Decision

- A library crate `front` holds what every frontend shares: `Session`
  (game state, the current `Mode`, auto runs, the status line), the modes'
  pure state machines, the title menu's state, the title flow (`title`:
  start a character's game, save on the way out) and save file IO.
- A frontend maps its keys to `front::input` values, choosing the map from
  `Session::keys`, and draws `Session::mode`, `Menu::view` and `game`'s
  views. It paces `Session::auto_step` and animates `Session::take_trails`
  on its own clock.
- `front` draws nothing and binds no keys: no terminal or graphics crate,
  no hecs, no `Command` or `Entity` (checked by `scripts/check.sh`). Text
  the player reads (status lines, row text, hints) is built in `front`;
  styling is the frontend's.

## Consequences

- A flow change is made once and both frontends follow; flows are tested
  by driving a `Session` with inputs, without a terminal.
- Key bindings stay per frontend, so `term/src/input.rs` and the windowed
  frontend's bindings must be kept the same by hand; hint text in `front`
  names the shared bindings.
- Flights launched on any turn are now animated after the input that
  caused them (before, only after a shot), so enemy missiles are seen when
  they fly.
- The overlay's row text is still built in `term`; it moves to `front` when
  the windowed frontend needs it (docs/plans/gui.md, slice 6).
