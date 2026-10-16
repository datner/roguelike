---
name: add-term-ui
description: Recipe for terminal UI in the term crate — a new key binding, modal (chooser, aiming cursor, overlay view) or side-panel/status line. Use whenever a task changes what the player sees or presses, including the UI half of a new game feature.
---

# Terminal UI

`term` draws plain views from `game` and sends `PlayerAction`s back; it never decides what an action does (`term/CLAUDE.md`). You verify everything with `TestBackend`, never by running the binary.

1. **Data from `game`.** What the screen shows comes from a `pub fn` in `game/src/view.rs` (or `aim.rs` for previews) returning plain data, re-exported from `lib.rs`. Refusal reasons and previews come from the same `check_*`/`ready_*` fn the command uses, so the UI and the turn agree. Done when `term` needs no rule of its own to fill the screen.
2. **Key.** An `InputEvent` variant in `game/src/input.rs` (frontend-only ones say so in their doc comment) and its key in `term/src/input.rs::map_key_event`, with a test that the key maps. Pick a key not already bound there.
3. **Where it lives.** Take the closest existing shape and extend it rather than inventing a new one:
   - pick from a list, then aim at a tile → `targeting.rs` `Chooser` + `Aiming` with a new `game::aim::Shot` variant (the throw flow: `start_throw` in `app.rs`).
   - a modal view over the map → a view in `overlay.rs`.
   - always-visible info → a `side_panel::Panel` variant placed in `PANELS`.
   Modals are pure state: `handle(input, views) -> Option<Action>`; `app::run` holds them in `Modal` and performs the returned action with `take_turn`. Animations reuse `Trail`/`Flying`.
4. **Tests** next to the widget: `handle` transitions (open, move, confirm, cancel) as plain unit tests, and the drawn result with `Terminal::new(TestBackend::new(w, h))`, asserting on the buffer text. Follow the existing tests in `app.rs`/`overlay.rs`.
5. **README "Controls"** lists the new key; `scripts/map.sh` if you added a file.
6. `scripts/check.sh` passes.
