# 0013 — Thrown and fired attacks

Status: Accepted
Date: 2026-10-16

## Context

Everything so far was melee by bumping. Throwing weapons and bows need an
aimed attack at a distance, without breaking the command queue, determinism
or the rendering seam.

## Decision

- **Aim at a tile.** `PlayerAction::Throw { item, at }` carries a world
  tile; `term` never sees entities. What a shot would do (path, target,
  damage after armor, why it is refused) comes from `game::aim`, which
  calls the same functions the command does.
- **Flight is a line, no roll to hit.** `game/src/ranged.rs`: Bresenham from
  the thrower, at most `range` tiles (diagonals cost one), stopping before a
  wall or at the first living creature. Whoever is first in line is hit.
- **One command.** `Command::Throw` moves the item out of its holder (pack
  or equipped), puts it down where it lands, and queues `DealDamage` with
  `DamageCause::Ranged`. It costs `cost::THROW`.
- **Damage** comes from the item's `Throwable { range, min, max, stat,
  types }`, a part separate from `Weapon`: one combat-stream draw, only when
  something is hit. Armor resists it by kind exactly like melee (ADR 0012).
- **Missiles never trigger on-hit parts** of the thrower: `AfterHit` is a
  melee event.
- The player may only aim at tiles in view.

## Consequences

- Thrown items are not lost: they land as found floor items and can be
  picked up again (auto-explore's item memory covers them).
- A knife that is both `Weapon` and `Throwable` has independent dice, so
  versatility is tuned in data.
- `SAVE_VERSION` 9 (new `Throwable` component).
- AI does not throw or shoot yet.
