# Plan: a windowed frontend (`gui`)

Handoff document. Read `CLAUDE.md`, `game/CLAUDE.md`, `term/CLAUDE.md`, ADR 0001 (the seam), ADR 0013
(aiming previews) and ADR 0020 (this frontend, and when to move to Bevy) first.

## Goal

A windowed frontend on eframe/egui that does everything `term` does, with the same keyboard-centric
controls, real panels and readable text instead of one squeezed character grid, mouse shortcuts as a
complement, and room for lighting, animated tiles, damage numbers and hit effects. `term` keeps working throughout; retiring it is a later decision.

## Ground rules (from the project; do not break)

- `game` never depends on a terminal or graphics crate (now including egui/eframe/epaint).
- `gui` sends `PlayerAction`s and reads plain views. It never names hecs, `Command` or `Entity`; it never
  works out a rule itself (what a shot does comes from `game::aim`, what looking reveals from `lore`).
- **Keyboard first** (ADR 0020). Every action has a key; bindings match `term` unless a slice says
  otherwise. Each slice builds the keyboard flow first, then adds mouse shortcuts onto the same flow
  (hover moves the cursor, click confirms). No mouse-only action.
- Animation is frontend state on the frontend's clock. Nothing it does reaches the simulation, so
  determinism (ADR 0005) is untouched. `game` reports what happened; it never waits for a frontend.
- **Never run `gui`** (it opens a window you can't see, and blocks). Keep logic in pure state structs
  like `term/src/overlay.rs` and unit-test them; test interaction headless (see slice 1); the user
  checks drawing.
- **Bevy check:** before starting any slice, compare it with the triggers in ADR 0020. If it hits one,
  stop and suggest moving to `bevy_egui` instead of building it on egui.
- Every slice ends with `scripts/check.sh` green. Do not commit unless asked. Commits scoped `gui`.
- Check egui/eframe APIs on docs.rs or in `~/.cargo/registry`; they change between releases.

## Where things stand

- `game::render_view` returns `Grid<Cell { glyph, fg, bg }>` sized to a viewport; `camera.rs` converts
  screen tiles to world tiles. Light from a held `Light` (the torch) is baked into `Cell` colours in
  `render_view` (uncommitted work at the time of writing).
- `view.rs`, `aim.rs`, `lore.rs` give plain data for panels, aiming and looking.
- Animation data today: `ranged::take_trails` hands over missile flights (`Trail`), animation-only.
  `term` plays them a tile per frame, blocking.
- Save file IO lives in `term/src/saves.rs` (list, load, save on quit, rename, delete).

## Design (decided)

1. **Crate `gui`**, a workspace member beside `term`; eframe with its default (wgpu) renderer. Same CLI
   as `term` (`--seed`). `check.sh`'s boundary step covers it like `term`.
2. **Shared save IO.** `term/src/saves.rs` moves to a small crate `saves` that both frontends use, so
   the save format and file layout can't drift between them.
3. **Map drawing:** a fixed tile size in pixels times a zoom; each visible tile is a coloured quad
   (background) plus a glyph from a bundled monospace font with the glyphs the game uses. The viewport
   in tiles is derived from the panel's size and fed to `render_view`.
4. **Presentation events (`Fx`)** replace `Trail`: one stream `GameState` fills while commands apply
   and frontends drain after a turn (`take_fx`). Never read by the simulation, never saved. Variants
   are added by the slice that animates them.
5. **Animations never block input.** A key or click during playback finishes all pending animations
   instantly, then acts.
6. **Modal flows are shared state, not widgets.** Look, aim, choosers and the overlay are pure state
   structs driven by keys; the mouse produces the same moves (set cursor, confirm, cancel). Logic is
   tested once through the keys.
7. **Mouse → tile:** pixel → screen tile in `gui`, screen tile → world tile through `camera.rs`.

## Slices

### 0. Groundwork
- `saves` crate from `term/src/saves.rs`; `term` uses it unchanged in behaviour.
- `gui` crate: an empty eframe window. Workspace dependency for eframe (justify in the commit).
- `check.sh`: add egui/eframe/epaint to the crates `game` must not depend on; apply the `term` boundary
  checks (no hecs, no `Command`/`Entity`) to `gui`.
- Docs: `gui/CLAUDE.md` (never run it; how to test), root `CLAUDE.md` layout and commands, `MAP.md`.
- Nix: confirm the devShell builds it on macOS; note Linux needs (Wayland/X11, Vulkan) for later.

### 1. Map, log and status; keyboard play
- Central panel draws the map; bottom panel the log; side panel the status lines `term` shows.
- Keyboard movement, wait, pick up, explore, quit, mapped to the same `PlayerAction`s as `term`.
- Headless tests: try `egui_kittest` (check it is current) for "press key → state changed". If it
  doesn't fit, test the key → `PlayerAction` mapping as a pure function, like `term/src/input.rs`.

### 2. Effects stream and the first effects
- `game`: `Trail` → `Fx::Flight`; add `Fx::Damage { at, amount, kind }`, `Fx::Miss`, `Fx::Graze`,
  `Fx::Deflect`, `Fx::Death` where the matching log lines are written. `term` keeps animating flights
  from `Fx::Flight` and ignores the rest.
- `gui`: flights (smooth, not a tile per frame), floating damage numbers, a hit flash on the struck
  tile, small screen shake on the player being hit. Instant finish on input.

### 3. Looking
- The look cursor, as in `term`, with what `lore::inspect` reveals in a panel beside the map.
- Mouse: hovering a tile moves the look cursor there (and shows the same text as a tooltip).

### 4. Aiming and casting
- Throw/cast choosers and the aiming cursor, as in `term`, previewing from `game::aim` (path, who is
  hit, damage range, area tiles) drawn on the map instead of in glyphs.
- Mouse: hovering moves the aim cursor, click fires, right-click cancels.

### 5. Inventory, equipment, skills and styles
- The overlay's tabs and row focus from `term/src/overlay.rs`, as an egui window over the map, from
  the same views, with item details beside the focused row.
- Mouse: click focuses a row, a context menu offers the same actions as the row's keys.

### 6. Title menu and saves
- New game, continue, load, rename, delete through the `saves` crate; save on window close, like
  `term` on quit.

### 7. Travel
- Needs game work first: a `PlayerAction` to travel toward a known tile, stepping like auto-explore
  and stopping on the same interruptions.
- Keyboard: a key in the look cursor travels to the looked-at tile (in `term` too). Mouse: click a
  tile to travel there.

### 8. Light as data
- Bevy check first (ADR 0020, triggers 1 and 4).
- `game` exposes per-tile light (level and tint) as a view; `render_view` keeps baking it for `term`.
  `gui` draws it with per-vertex colours so light fades across tiles instead of stepping.

### 9. Animated tiles and a tileset
- Bevy check first (triggers 3 and 5).
- Needs a decision (and an ADR): how a blueprint names its sprite and animation frames, next to
  `Appearance`, so `term` keeps its glyph. Then `gui` draws sprites with frame cycling.

## Out of scope

- Retiring or freezing `term`; sound; a web (wasm) build; sprite art. Each is a follow-up decision.
