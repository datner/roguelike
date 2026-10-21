# 0020 — Windowed frontend on egui, with a line for moving to Bevy

Status: Accepted
Date: 2026-10-21

## Context

Terminal cells make the game awkward to play: everything, map, panels,
overlays and cursors, is squeezed into one character grid. The
keyboard-centric controls are not the problem and stay; the mouse is a
complement. The vision also wants lighting, animated tiles, damage
numbers and hit effects. ADR 0001 kept room for a windowed frontend; this
is it. Plan: `docs/plans/gui.md`.

The candidates were eframe/egui, macroquad and Bevy. Most of the game's
interaction is panels and lists, where egui is strongest and macroquad
weakest. Bevy draws effects best but costs slow compiles, breaking releases
and a second ECS. UI written for egui moves into Bevy through `bevy_egui`
almost unchanged, so starting on egui risks only the map renderer.

## Decision

- **A new crate, `gui`, on eframe/egui**, beside `term`, with the same
  rules: it depends on `game`, sends `PlayerAction`s, reads plain views,
  never names hecs, `Command` or `Entity` (checked by `scripts/check.sh`).
  `term` stays and keeps working.
- **Keyboard first.** Every action has a key, and the keyboard flow
  (choosers, cursors, overlays) is designed first; the mouse adds hover
  and click shortcuts onto the same flows, never a mouse-only action.
- **The map is drawn with egui's painter**: tile quads with per-vertex
  colours for light, glyphs (later sprites) on top, a fixed tile size
  scaled by zoom. No custom wgpu shaders.
- **Effects are frontend state driven by data from `game`.** `game`
  reports what happened and where (generalising `ranged::Trail`); the
  frontend animates it on its own clock. Animation never feeds back into
  the simulation, so determinism is untouched.
- **Light is computed in `game`.** Light levels per tile are simulation
  (sight, later stealth); the frontend only decides how they look.

## When to move to `bevy_egui`

Suggest the move **before** starting any of these, not after:

1. Light that must blend smoothly across tiles beyond per-vertex
   interpolation: soft falloff that ignores tile edges, flicker with
   blurred edges, coloured light mixing from many sources.
2. Glow, bloom, a CRT look, distortion, or any full-screen post-process.
3. Particles: more than a few dozen short-lived sprites at once (blood,
   sparks, spell trails, weather).
4. The first wgpu paint callback (`egui::PaintCallback`) or hand-written
   shader in `gui`.
5. `gui`'s effects code becoming its own scene system: sprite batching,
   layers with z-order, tweening engines, a camera with smooth motion.
6. Frame time over ~8 ms on a full zone view in a release build.
7. Audio beyond one-shot sounds per event (mixing, spatial sound,
   music that reacts to play).

Any one of these means building engine features by hand. The move then
costs the map renderer and the effects layer; panels carry over through
`bevy_egui`.

## Consequences

- Real panels, readable text, tooltips and scrollable lists are cheap, and
  so are mouse shortcuts layered on the keyboard flows.
- Lighting stays per-tile tinting, with smooth gradients only through
  vertex colours. Animated tiles, damage numbers, flashes and shake are
  plain frontend tweens.
- Two frontends: every user-visible feature needs UI in both until `term`
  is retired or frozen. That decision waits until `gui` covers everything
  `term` does.
- `gui` can't be run by an agent. Its logic is tested headless; drawing is
  checked by the user.
