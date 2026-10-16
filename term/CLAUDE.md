# term crate — terminal frontend

Thin shell around `game`. **No game rules here.** If you're about to decide what a move/attack/pickup *does*, it belongs in `game` as a `Command`.

## Module map

- `main.rs` — CLI args (`--seed`, `--data`, `--character`, `--saves`), loads blueprints and the character's save, saves on quit.
- `saves.rs` — save file paths (`directories` for the default), loading, atomic save-on-quit (skipped after death). Never overwrites a save that failed to load. ADR 0008.
- `tui.rs` — terminal init/restore, Kitty keyboard enhancement, panic hook. Every mode you enable must be undone in `restore()`, including on panic.
- `input.rs` — `KeyEvent` → `game::InputEvent`. Only place keys are bound; update README "Controls" when bindings change.
- `app.rs` — blocking event loop (turn-based: no tick timer) and panel layout.
- `widgets/map_widget.rs` — renders `Grid<Cell>`. `tile_to_screen` is the single place that maps tile coords to screen coords, and `viewport_tiles` its inverse for the panel size passed to `render_view`; keep it that way. The camera (which world tiles are shown) is `game::camera`.

## Rules

- Import crossterm as `ratatui::crossterm::…`, never as its own dependency.
- Never refer to hecs; `GameState.world` is private to `game`. If `term` needs something from the world, add a `game` function (checked by `scripts/check.sh`).
- Never run the binary to check your work (it blocks on input). Test pure functions like `map_key_event` directly, and widgets with `ratatui::backend::TestBackend` + `Terminal::draw`, asserting on the buffer.
- ratatui 0.30 API: `frame.area()`, `Block::bordered()`, `Layout::new(direction, constraints)`. Check docs.rs before using anything else.
