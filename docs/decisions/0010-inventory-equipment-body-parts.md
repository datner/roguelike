# 0010 — Inventory, equipment and body parts

Status: Accepted
Date: 2026-10-16

## Context

Items can now be carried and worn. Event dispatch already treats everything
in a creature's `Attachments` as part of it (ADR 0004 parts, `game/src/event.rs`),
and timed effects live there too. A carried item must not act (a knife in the
pack must not cause bleeding), and Caves of Qud equips items on a creature's
own anatomy rather than on fixed slots. Frontends need to name items without
seeing hecs types (ADR 0001).

## Decision

- **Carried vs in use.** `Inventory(Vec<Entity>)` holds carried items and is
  never dispatched to. Equipped items are in `Attachments` (so their parts
  fire) and carry `EquippedOn(part index)`. Effects stay in `Attachments`
  without `EquippedOn`, which is how the two are told apart.
- **Anatomy is data.** `Body` is a per-creature list of `BodyPart { kind,
  name }` from the blueprint (a `Humanoid` blueprint for now); an item's
  `Equippable { part }` names the kind it fits. Parts are addressed by index,
  so a `Body` never reorders. One item per part.
- **Blueprint equipment is fitted at load.** Each `equipment` entry must be
  `Equippable` and fit a free part in list order, or loading fails
  (`game/src/blueprint.rs`); spawning can't fail to place it.
- **Equip and unequip are commands** costing a standard action. Equipping
  onto a taken part swaps the old item into the inventory. An impossible
  player equip/unequip is refused up front (`game/src/command.rs`),
  costing no time.
- **Frontends get views, not entities.** `game/src/view.rs` returns plain
  structs (inventory, equipment, character sheet) and an `ItemRef`, which
  wraps the item's `SpawnSeq` (unique, stable, already saved).
  `PlayerAction`s take `ItemRef`s and the turn entry point resolves them
  against what the player holds.

## Consequences

- New wearable kinds need only data: a `PartKind` variant if the anatomy
  needs one, otherwise just `Equippable` on the item.
- A flat part list has no hierarchy, so losing an arm can't take its hand
  with it. Revisit (a parent index per part) when dismemberment arrives.
- Two-handed items and parts holding several items are not modelled.
- Death still despawns everything a creature carries; dropping loot is a
  follow-up.
