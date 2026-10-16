# Architecture decisions

One short file per decision: what we chose, why, and what it costs. Agents: read the relevant ADR before changing something it covers; add a new one when you make a decision a future reader would otherwise have to reverse-engineer.

Name files `NNNN-kebab-title.md`. Never rewrite an accepted ADR — supersede it with a new one and set the old one's status to `Superseded by NNNN`.

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
