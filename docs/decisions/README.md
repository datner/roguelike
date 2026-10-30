# Architecture decisions

One short file per decision: what we chose, why, and what it costs. Agents: read the relevant ADR before changing something it covers; add a new one when you make a decision a future reader would otherwise have to reverse-engineer.

Name files `NNNN-kebab-title.md`. Never rewrite an accepted ADR — supersede it with a new one and set the old one's status to `Superseded by NNNN`. Updating a code reference after a rename is fine; changing the decision is not.

Point at code by module path (`game/src/combat.rs`), component, type or `Command` — never by function, method or constant name. Those get renamed; a module path or a domain type rarely does, so the ADR stays true without upkeep.

| # | Decision | Status |
|---|----------|--------|
| [0001](0001-workspace-and-rendering-seam.md) | `game`/`term` split with a `Cell`/`InputEvent` seam | Accepted |
| [0002](0002-command-queue.md) | World mutation only through a command queue | Accepted |
| [0003](0003-determinism.md) | Seeded determinism, enforced by clippy | Superseded by 0005 |
| [0004](0004-data-driven-blueprints.md) | Content as RON blueprints composed from parts | Accepted |
| [0005](0005-rng-streams.md) | Per-subsystem RNG streams derived from the seed | Accepted |
| [0006](0006-time-model.md) | Energy scheduling, timer queue, relative durations | Accepted |
| [0007](0007-fov-and-ai-sight.md) | Symmetric shadowcasting FOV; AI sight reuses the player's view | Accepted |
| [0008](0008-save-games.md) | Versioned RON saves with index-remapped entities; per-character save on quit | Accepted |
| [0009](0009-zones-and-camera.md) | Fixed-size zones larger than the screen, per-zone seeds, camera in `game` | Accepted |
| [0010](0010-inventory-equipment-body-parts.md) | Inventory vs equipped items; Qud-style body parts; item views for frontends | Accepted |
| [0011](0011-stats-and-weapons.md) | Creature stats meet weapon attributes in one combat rule | Accepted |
| [0012](0012-damage-types.md) | Damage kinds, per-kind armor, multi-kind blows use the weakest spot | Accepted |
| [0013](0013-ranged-attacks.md) | Aimed throws and shots: tile targets, line flight, no to-hit roll | Accepted |
| [0014](0014-spells-and-disciplines.md) | Spells say what they do; the caster's disciplines say what casting costs | Accepted |
| [0015](0015-sides-and-summons.md) | Two derived sides; summons are spawns held by a timed `Binding`; one shared view | Accepted |
| [0016](0016-terrain-elements-and-runes.md) | Timed map overlay, statuses as effects with damage handlers, runes as a discipline (not built) | Proposed |
| [0017](0017-skills.md) | Flat data-defined skills, weighted-average level, XP-only storage, use-based training that decays with repetition | Accepted |
| [0018](0018-discovery.md) | Data-defined discoveries checked after uses with stakes; proficiencies are attached blueprints; a legible journal | Accepted |
| [0019](0019-styles.md) | Styles: carried when known, attached when followed, own skill, principles by level | Accepted |
| [0020](0020-windowed-frontend-egui.md) | Windowed `gui` frontend on eframe/egui, keyboard first; triggers for moving to `bevy_egui` | Accepted |
| [0021](0021-shared-front-crate.md) | Frontends share flows, menu and saves through a drawing-free `front` crate | Accepted |
| [0022](0022-effects-stream.md) | `game::fx`: an animation-only stream of flights, damage, outcomes and deaths the player sees | Accepted |
| [0023](0023-sprites.md) | Sprites named in `data/tiles.ron` and `Appearance`, drawn by `gui` (F2), glyphs in `term` and for memory | Accepted |
| [0024](0024-techniques.md) | Techniques: attached like spells, paid in turns, recharge and (dormant) stamina; their blows are ordinary blows | Accepted |
| [0025](0025-origins.md) | Origins: `data/origins.ron` names a blueprint inheriting the bare `Player`, its styles; `Origin(key)` saved | Accepted |
| [0026](0026-legacy.md) | Legacy: a life ends in a `Record` of names in `<saves>/legacy.ron`; spawns carry `Made(name)` | Accepted |

## Template

```markdown
# NNNN — Title

Status: Proposed | Accepted | Superseded by NNNN
Date: YYYY-MM-DD

## Context
What forced a decision.

## Decision
What we're doing.

## Consequences
What this makes easier, what it makes harder, what we'd revisit and when.
```
