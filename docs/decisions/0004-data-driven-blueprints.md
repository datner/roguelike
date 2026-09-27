# 0004 — Content as RON blueprints composed from parts

Status: Accepted
Date: 2026-09-27

## Context
Qud gets most of its variety from data: objects are blueprints assembling reusable parts. Hardcoding creature and item types in Rust would make content expensive to add.

## Decision
Entity kinds are defined in `data/**/*.ron` and deserialised with serde into blueprint types in `game`. A blueprint lists parts; spawning maps each part to hecs components. New behaviour is a new part (component + the commands that use it), reusable by any blueprint.

Blueprint files are named after the blueprint in PascalCase (`Goblin.ron`), may `inherits:` another blueprint, list their `components:`, and may carry `equipment:` (other blueprints spawned as `Attachments`).

## Consequences
- Adding a monster/item is usually a RON file plus maybe one new part.
- RON schema changes can break existing data files, so there should be a test that loads and spawns every file in `data/`.
- Keep the component set small and orthogonal; a new component should be usable by more than one blueprint.
