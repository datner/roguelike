---
name: add-ui
description: Recipe for player-facing UI — a new key, mode (chooser, cursor, panel, a screen like camp), side-panel text or status line, drawn in the gui. Use whenever a task changes what the player sees or presses, including the UI half of a new game feature.
---

# Player-facing UI

Three layers, each with one job: `game` says what is true, `front` decides what a key does in which mode, `gui` binds keys and draws (ADR 0020, 0021). `term` is frozen: it only gets a fallback for new modes. Verify with tests and `scripts/shots.sh`, never by running a binary.

1. **Data from `game`.** What the screen shows comes from a `pub fn` in `game/src/view.rs` (or `aim.rs` for previews) returning plain data, re-exported from `lib.rs`. Refusal reasons and previews come from the same `check_*`/`ready_*` fn the command uses, so the UI and the turn agree.
2. **Flow in `front`.** A new mode is a `Mode` variant in `session.rs` with pure state (`handle(input) -> Option<Action>`, like `targeting.rs`/`overlay.rs`), and `Mode::keys` says which `Input` it reads. Text the player reads is built in `front` (`text.rs`, a `view()`), styling is the gui's. Test the flow by driving a `Session` with `Input`s.
3. **Key in `gui/src/input.rs`.** Map it from `Press`; game keys stay the same as `term`'s bindings, so pick one not bound there either. README "Controls" lists it.
4. **Draw in `gui`.** Over or on the map → `modes.rs` (cell styles before the map is drawn, windows after); around it → `panels.rs`; through `theme.rs` (`boxed`, `line`, `job`). Pointer use returns the same `front` inputs keys send. A new screen gets a shot in `app/shots.rs`; run `scripts/shots.sh --check` and `--update` the references when the change is intended.
5. **`term` fallback.** `term` must still compile: for a new mode, draw one line, "This screen needs the window (cargo run -p gui).", and let `Esc` back out. Nothing more (`term/CLAUDE.md`).
6. `scripts/check.sh` passes; `scripts/map.sh` if you added a file.
