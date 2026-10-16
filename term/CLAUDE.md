# term crate — terminal frontend

Thin shell around `game`. **No game rules here.** If you're about to decide what a move/attack/pickup *does*, it belongs in `game` as a `Command`.

## Module rules

What each module is for is in the index (`docs/MAP.md`); its API is
`scripts/map.sh --sigs <path>`.

- `menu.rs`, `overlay.rs` — pure state: `handle` takes input (plus fresh `game` views for the overlay) and returns an action; `app.rs` performs it. Neither touches files or the game directly.
- `saves.rs` — rename never overwrites; a save that failed to load is never overwritten; no save after death. ADR 0008.
- `tui.rs` — every terminal mode you enable must be undone on restore, including on panic.
- `input.rs` — the only place keys are bound; update README "Controls" when bindings change.
- `targeting.rs` — the throw/spell chooser and aiming cursor: pure state like `overlay.rs`; what a shot does is `game::aim`, what a spell costs is `game::spells` (never name a resource here).
- `app.rs` — turn-based, no tick timer; auto-explore only polls briefly between its steps so a key can interrupt.
- `side_panel.rs` — a new panel is a `Panel` variant plus its place in `PANELS`.
- `widgets/` — `color` is the one `game::Rgb` → ratatui conversion; `map_widget.rs` is the single tile↔screen mapping on the `term` side. Which world tiles are shown is `game`'s camera.


## Rules

- Import crossterm as `ratatui::crossterm::…`, never as its own dependency.
- Never refer to hecs; `GameState.world` is private to `game`. If `term` needs something from the world, add a `game` function (checked by `scripts/check.sh`).
- Never run the binary to check your work (it blocks on input). Test pure functions like `map_key_event` directly, and widgets with `ratatui::backend::TestBackend` + `Terminal::draw`, asserting on the buffer.
- ratatui 0.30 API: `frame.area()`, `Block::bordered()`, `Layout::new(direction, constraints)`. Check docs.rs before using anything else.
