# 0023 — Sprites named in content, drawn by frontends that can

Status: Accepted
Date: 2026-10-22

## Context

The windowed frontend (ADR 0020) can draw images, and animated tiles were
part of why it was chosen. `term` can only draw glyphs. What a thing looks
like is content (ADR 0004), and `game` outputs `Grid<Cell>` (ADR 0001).

## Decision

- `data/tiles.ron` names an atlas image beside it, the size of one cell
  (a terminal cell's shape, 8×16), each sprite's frames as atlas cells
  with a frame time, and which sprite each terrain uses.
- An `Appearance` may name a sprite. Names are checked against the table
  when blueprints load, so a missing sprite fails at load. Saves keep the
  name; the save version moved to 27.
- `Cell.sprite` carries a `SpriteId` (assigned at load, never saved), set
  only on tiles in view: remembered ground stays dim glyphs, which keeps
  memory readable and needs no "remembered" flag on the cell. A thing
  without a sprite shows its glyph, not the ground's sprite.
- Frontends pick the frame from their own clock (`Sprite::frame`); `term`
  ignores sprites. `gui` loads the atlas with `image` (PNG only), draws
  sprites shaded by the light, and starts with glyphs: F2 switches, a
  window setting rather than a game key.

## Consequences

- New art is a PNG and table entries; no code.
- The shipped `data/tiles.png` is a placeholder (floor, wall, exit, the
  player, an animated goblin and torch).
- A sprite is one layer: an item on a floor tile hides the floor's sprite.
  Layers would be a later change to `Cell`.
