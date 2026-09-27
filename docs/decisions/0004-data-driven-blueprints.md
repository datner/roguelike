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

## Addendum (2026-09-28): `BlueprintId` and on-hit dispatch

- Every blueprint reference in data (`inherits`, `equipment`, on-hit effects) is checked at load, against the blueprint that wrote it; a missing name fails loading with an error naming both. Load then assigns each blueprint a `BlueprintId` (index in name order) and resolves references to ids, so the runtime never looks names up and `Command` stays `Copy`.
- `BlueprintId` is runtime-only: it depends on which files were loaded, so it has no serde derives. Anything persisted stores blueprint names and re-resolves them on load.
- On-hit effects are a part (`OnHit`), not a weapon feature: after a melee `DealDamage` that deals more than 0, `AfterHit` is fired at the attacker, so its own parts and its equipment's handle it like any event. `DealDamage` carries a `DamageCause`; effect damage never fires `AfterHit`, so effects cannot chain. Rolls use the combat stream, one per entry per hit, and each success applies an independent effect instance.
- Load also rejects equipment cycles (after flattening, since a cycle can run through inherited lists; the error lists the blueprints around it) and any on-hit effect blueprint without a `Duration`, possibly inherited (the error names the referrer and the effect). Spawning and `ApplyEffect` therefore don't re-check either; `ApplyEffect` keeps only a debug assertion.
- On a lethal melee hit, `Die` is queued before the commands `AfterHit` emits. `AfterHit` still fires and rolls every entry, so combat-stream draws don't depend on whether the hit killed; effects aimed at the dead victim are silent no-ops, while effects aimed at anyone still alive apply.
