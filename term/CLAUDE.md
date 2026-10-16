# term crate — terminal frontend

Thin shell around `game`. **No game rules here.** If you're about to decide what a move/attack/pickup *does*, it belongs in `game` as a `Command`.

## Module map

- `main.rs` — CLI args (`--seed`, `--data`, `--character`, `--saves`), terminal init, hands off to `app::session`.
- `menu.rs` — title menu (new, continue, load; rename/delete saves). Pure state: `Menu::handle` returns an `Action`, it never touches files; drawing is tested with `TestBackend`.
- `saves.rs` — save file paths (`directories` for the default), listing (newest first), loading, rename (never overwrites), delete, atomic save-on-quit (skipped after death). Never overwrites a save that failed to load. ADR 0008.
- `tui.rs` — terminal init/restore, Kitty keyboard enhancement, panic hook. Every mode you enable must be undone in `restore()`, including on panic.
- `input.rs` — `KeyEvent` → `game::InputEvent` in game, → `OverlayInput` while the hovering panel is open, → `MenuInput` on the title menu. Only place keys are bound; update README "Controls" when bindings change.
- `app.rs` — `session` (menu ↔ game loop, performs menu actions, loads/starts/saves games), the blocking game loop (turn-based, takes the character name for the side column: no tick timer; auto-explore only polls briefly between its steps so a key can interrupt) and panel layout.
- `overlay.rs` — the hovering panel over the map: inventory and equipment views (`Tab`), and a picker for equipping a chosen empty body part. Pure state: `Overlay::handle` takes an `OverlayInput` plus fresh `game` views (`Contents`) and returns `Close` or a `PlayerAction`; `app::run` owns the `Option<Overlay>` and routes keys to it while open.
- `side_panel.rs` — the side column: `PANELS` (an ordered list of `Panel` variants, each a title + lines from `game` views and the character name) stacked to the right; `split` hides it below `MIN_TERMINAL_WIDTH`. A new panel is a `Panel` variant plus its place in `PANELS`.
- `widgets/mod.rs` — `color`, the one `game::Rgb` → ratatui colour conversion.
- `widgets/map_widget.rs` — renders `Grid<Cell>`. `tile_to_screen` is the single place that maps tile coords to screen coords, and `viewport_tiles` its inverse for the panel size passed to `render_view`; keep it that way. The camera (which world tiles are shown) is `game::camera`.

## Rules

- Import crossterm as `ratatui::crossterm::…`, never as its own dependency.
- Never refer to hecs; `GameState.world` is private to `game`. If `term` needs something from the world, add a `game` function (checked by `scripts/check.sh`).
- Never run the binary to check your work (it blocks on input). Test pure functions like `map_key_event` directly, and widgets with `ratatui::backend::TestBackend` + `Terminal::draw`, asserting on the buffer.
- ratatui 0.30 API: `frame.area()`, `Block::bordered()`, `Layout::new(direction, constraints)`. Check docs.rs before using anything else.
