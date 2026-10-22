# gui crate — windowed frontend

eframe/egui (ADR 0020). Thin shell around `front` and `game`: maps keys to `front` inputs and draws. **No game rules and no flow rules here**: what a key does is `front::session`, what an action does is `game`. Plan: `docs/plans/gui.md`.

## Module rules

What each module is for is in the index (`docs/MAP.md`); its API is
`scripts/map.sh --sigs <path>`.

- `input.rs` — the only place keys are bound; README "Controls" lists every binding. Printable keys come from `Event::Text` (`Press::Char`), named keys from `Event::Key`.
- `app.rs` — picks the key map from `Session::keys` (or `Menu::typing`), paces `Session::auto_step` on the frame clock, saves through `title::finish` on quit and on window close (`on_exit`).
- `theme.rs` — the terminal look (fonts, colours, `boxed` titled borders, reverse video). Draw text through `theme::line`/`panels::job` and boxes through `boxed`, so the window keeps its terminal look; no other fonts, sizes, rounding or shadows.
- `map_view.rs` — the single tile↔pixel mapping (`TILE`, square, apart from the text grid's `theme::CELL`; `Layout`) and the one `Rgb` → `Color32` conversion. Draws `game::render_layers`, never `render_view` (that bakes light in): backgrounds as one mesh whose corners average the neighbouring light. Which world tiles are shown is `game`'s camera.
- `panels.rs`, `menu_view.rs` — drawing only, from `front::text`, `Session::status` and `Menu::view`.
- `fx.rs` — effects (`Session::take_fx`, ADR 0022) on the frame clock: `Effects` schedules them (flights in turn, hits after their flight), marks cells before the map is drawn and draws over it. Any input calls `finish`. Keep them glyphs and text on the cell grid.
- `sprites.rs` — the atlas texture and frame UVs (ADR 0023). Sprites are off until F2 (`Press::F2`, the window's own setting; never send it to `front`).
- `modes.rs` — what each `Mode` draws (cursors and aim marks as cell styles before the map is drawn; choosers, the panel window) and what the pointer does in it, returned as `front` inputs that `App::show` hands to the session after drawing. A hover only moves a cursor when the pointer moves to a new tile, so it never undoes a key.

## Rules

- **Keyboard first.** Every action has a key; the mouse only drives the same flows (hover moves a cursor, click confirms). No mouse-only action.
- Never refer to hecs, `Command` or `Entity` (checked by `scripts/check.sh`).
- **Never run the binary** (it opens a window you can't see). Test key mapping and geometry as pure functions, `App::update` with `Press`es and an `Instant`, and drawing with `egui_kittest::Harness` (headless, no GPU; it panics on a glyph no font has). A new glyph in content must be in `assets/DejaVuSansMono.ttf` (or egui's fonts). Window tests use `testing.rs` (`window`, `type_text`, `press_key`, `set_clock` to stop time).
- Looking at drawing: `scripts/shots.sh` (root `CLAUDE.md`). Shots live in `app/shots.rs`: a new screen or effect gets one (`SHOTS`, `scene`), on the stopped clock so it matches its reference every run; a drawing change runs `--check`, and if the change is intended, `--update` and commit the references with it.
- Bevy check: before any effect, lighting or sprite work, compare it with the triggers in ADR 0020; if it hits one, stop and suggest moving to `bevy_egui`.
- egui 0.37 API: `App::ui(&mut self, ui, frame)`, `egui::Panel::{right,bottom}(id).show(ui, …)`, `CentralPanel::default().show(ui, …)`. Check `~/.cargo/registry` before using anything else.
