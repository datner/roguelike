# term crate — terminal frontend

Thin shell around `front` and `game`: maps crossterm keys to `front` inputs and draws with ratatui. **No game rules and no flow rules here.** What a key does to the game is `front::session`; what an action does is `game`.

**Frozen (2026-10-22).** `gui` is the frontend; `term` gets no new features and is deleted after the camp-and-dives work unless it is still played. It must keep building and its tests passing: a new `front` mode gets one line, "This screen needs the window (cargo run -p gui).", with `Esc` backing out. Fixing a break is fine; extending is not.

## Module rules

What each module is for is in the index (`docs/MAP.md`); its API is
`scripts/map.sh --sigs <path>`.

- `input.rs` — the only place keys are bound; update README "Controls" when bindings change, and keep `gui`'s bindings the same.
- `app.rs` — turn-based, no tick timer; picks the key map from `Session::keys`, paces `Session::auto_step` and polls briefly between steps so a key can interrupt; animates the flights in `Session::take_fx` after each input (the other effects are `gui`'s).
- `menu.rs`, `overlay.rs`, `targeting.rs` — drawing only, from `front`'s state accessors and views.
- `tui.rs` — every terminal mode you enable must be undone on restore, including on panic.
- `side_panel.rs` — a new panel is a `Panel` variant plus its place in `PANELS`.
- `widgets/` — `color` is the one `game::Rgb` → ratatui conversion; `map_widget.rs` is the single tile↔screen mapping on the `term` side. Which world tiles are shown is `game`'s camera.


## Rules

- Import crossterm as `ratatui::crossterm::…`, never as its own dependency.
- Never refer to hecs; `GameState.world` is private to `game`. If `term` needs something from the world, add a `game` function (checked by `scripts/check.sh`).
- Never run the binary to check your work (it blocks on input). Test pure functions like `map_key_event` directly, and widgets with `ratatui::backend::TestBackend` + `Terminal::draw`, asserting on the buffer. Flow behaviour is tested in `front`.
- ratatui 0.30 API: `frame.area()`, `Block::bordered()`, `Layout::new(direction, constraints)`. Check docs.rs before using anything else.
