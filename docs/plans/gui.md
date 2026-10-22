# Plan: a windowed frontend (`gui`)

Handoff document. Read `CLAUDE.md`, `game/CLAUDE.md`, `front/CLAUDE.md`, `term/CLAUDE.md`, ADR 0001
(the seam), ADR 0013 (aiming previews), ADR 0020 (this frontend, keyboard first, and when to move to
Bevy) and ADR 0021 (the shared `front` crate) first.

## Goal

A windowed frontend on eframe/egui that does everything `term` does, with the same keyboard-centric
controls, real panels and readable text instead of one squeezed character grid, mouse shortcuts as a
complement, and room for lighting, animated tiles, damage numbers and hit effects. `term` keeps working
throughout; retiring it is a later decision.

## Ground rules (from the project; do not break)

- `game` and `front` never depend on a terminal or graphics crate.
- `gui` maps keys to `front` inputs and draws. It never names hecs, `Command` or `Entity`, never decides
  a flow (that is `front::Session`) or a rule (that is `game`).
- **Keyboard first** (ADR 0020). Bindings match `term/src/input.rs`. Each slice builds the keyboard
  flow first, then adds mouse shortcuts onto the same flow (hover moves the cursor, click confirms).
  No mouse-only action.
- Animation is frontend state on the frontend's clock; nothing reaches the simulation (ADR 0005).
- **Never run `gui`** (it opens a window you can't see). Test logic in pure functions and `front`;
  the user checks drawing.
- **Bevy check:** before slices 3, 8 and 9, and any effect not listed here, compare the work with the
  triggers in ADR 0020. If it hits one, stop and suggest moving to `bevy_egui`.
- Every slice ends with `scripts/check.sh` green. Do not commit unless asked. Commits scoped by crate.
- Check egui/eframe 0.37 APIs on docs.rs or in `~/.cargo/registry`.

## Slices

### 0. `front` crate; `term` on top of it — done
`Session`, modes, menu state, title flow and saves moved out of `term` (ADR 0021). `term` maps keys by
`Session::keys` and draws.

### 1. `gui` crate skeleton — done
- Workspace dep `eframe = "0.37"` (`default-features = false` + wgpu renderer, default fonts, native
  window backends). `gui/src/main.rs` with the same args as `term`, an empty `App`.
- `check.sh` boundary covers `gui/src` (hecs, `Command`/`Entity`); `map.sh` includes `gui`.
  `gui/CLAUDE.md`; root `CLAUDE.md` (human-only `cargo run -p gui -- --seed 42`, never run it); README.
- `cargo build -p gui` in the devShell; `nix flake check`. Linux libs only if the Linux check needs them.

### 2. Title menu and game screen, keyboard play — done
- `app.rs`: `Screen::{Menu, Game}`; key presses mapped by `Session::keys` (or `Menu::typing`), then
  `handle`; `auto_step` paced with `request_repaint_after`; quit or window close → `title::finish` →
  menu with its message.
- `input.rs`: the only bindings (`egui::Key` + `Modifiers`, `Event::Text` for typing).
- `map_view.rs`: the one tile↔pixel mapping (`TILE`, `viewport_tiles`), `render_view` drawn as
  background quads and centred monospace glyphs; one `Rgb → Color32`.
- `panels.rs`: side panel (character sheet, equipment), bottom panel (status, newest log lines). A
  rule hiding in `term/src/side_panel.rs` formatting (mana pace rounding) moves to `game::view`.
- `menu_view.rs`: draws `Menu::view`.
- Tests: every binding; `viewport_tiles`; an `egui_kittest` test if it runs headless, else pure tests.
- **Checkpoint:** the user launches `gui` once before effects are built on it.

### 2b. Terminal look — done
`theme.rs`: one monospace font on the map's cell grid, black background, `term`'s titled boxes and
layout (30-cell side column, 7-row log, 3-row status), reverse-video selection and cursors, aim marks as
cell backgrounds. An ignored `screenshots` test renders each screen to PNGs for review.

### 3. Effects stream (`Fx`) and the first effects — done (ADR 0022)
- `game/src/fx.rs`: `Fx::{Flight(Trail), Damage { at, amount, blocked }, Outcome { at, kind }, Death
  { at }}`; `GameState.fx` replaces `trails` (never saved, never read by the simulation); `take_fx`
  replaces `take_trails`. Pushed where flights start (`ranged.rs`, `spell.rs`), where damage is logged
  (`DealDamage` applier), in `combat::announce`, and in `command::die`.
- `front::Session::take_fx`; `term` animates `Fx::Flight` only.
- `gui/src/fx.rs`: smooth flights, floating damage numbers, hit flash, shake when the player is hurt,
  miss/graze/deflect text; any input finishes them. Timing math tested.

### 4. Looking — done
- Look cursor outlined on the map; `inspect` text in the side panel; status from `Session::status`.
- Mouse: `AimInput::Set(at)` (new, world tile) from hover; hover in play shows the same text as a
  tooltip. Pixel → screen tile in `gui`, screen → world through `game::camera`.

### 5. Throwing, firing and casting — done
- Chooser lists from `throwables`/`spells` (`front::targeting::spell_text`); aim mode draws
  `game::aim` (path, area, landing, cursor).
- Mouse: hover sets the aim cursor, click confirms, right-click cancels, click picks a chooser row.

### 6. Inventory, equipment, skills and styles — done
- Move the overlay's row text from `term/src/overlay.rs` to `front` (plain rows: text, quiet notes,
  item glyph and colour, focus), then draw it in an `egui::Window` with item details beside the
  focused row; `term` draws the same rows.
- Mouse: click focuses a row; a context menu sends the row's `OverlayInput`s.

### 7. Travel — done
- `game`: `PlayerAction::Travel { to }` and `travel_step(state, to) -> bool`, stepping along
  `explore`'s known-ground paths and stopping on the same interruptions as `explore_step`.
- `front`: `Auto::Travel(to)`; `Enter` in look mode travels to the looked-at tile (both frontends;
  README Controls). `gui`: click on the map in play travels there.

### 8. Light as data — done
- `game`: a public `lighting(state, w, h) -> Grid<Lit>` (tint and strength falling off with distance,
  visible tiles only) split out of `render_view`'s baking, which `term` keeps.
- `gui`: tile quads as an `egui::Mesh` with per-vertex colours averaged from neighbouring light.

### 9. Sprites and animated tiles — done (ADR 0023)
- ADR 0023: `Appearance` may name a sprite; `data/tiles.ron` maps sprite names to atlas cells and
  frames; `Cell` carries an optional load-time `SpriteId`; `term` ignores it.
- `game` loads and validates the table; `gui` loads the atlas into a texture, draws sprites with frame
  cycling, falls back to glyphs. A placeholder atlas ships; art is the user's.
- README; decide with the user whether `term` is frozen or kept at parity.

## Out of scope

- Retiring or freezing `term`; sound; a web (wasm) build; sprite art.
