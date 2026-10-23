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

### Launchers and ammunition

- `Launcher { range, ammo, stat }` on a bow; `Ammo(kind)` and `Projectile
  { min, max, types, break_bp }` on arrows. The shot rolls the arrow's dice
  plus the shooter's stat modifier, and the arrow's own kinds resist as in
  ADR 0012. A hit may break the arrow (a second combat-stream draw); a miss
  never does.
- **Loose arrows are single entities and don't stack.** A `Quiver` (worn on
  the back) is the only place they do: its arrows are the entities in its
  own `Inventory`. Reusing `Inventory` means they are inert, saved and
  despawned with it for free. Picking ammunition up fills an equipped
  quiver before the pack; `Command::Quiver` moves loose ones in by hand.
- **A nocked arrow is the one thing in the launcher's own `Inventory`.**
  `Fire` uses it first, else the next arrow in an equipped quiver. So
  `Command::Nock` fetches a special arrow from the pack, even with a full
  quiver; it costs its own turn (`cost::NOCK`), which is the price of
  overriding the quiver. Unequipping the bow returns the arrow to the pack.
- With no nocked arrow and no quiver supply, pressing fire in `term` nocks a
  loose arrow first (a turn); a second press aims.
- The player starts with a short bow and a quiver of 12 arrows.

### Animation

- The shot resolves instantly. `launch` also leaves a `Trail` (glyph, colour,
  tiles crossed) in a transient `GameState` list that `take_trails` drains;
  it is never read by the simulation and never saved. `term` replays it a
  frame per tile (`app::animate`), so a key press cuts it short and the
  world shown underneath is already the after-state.

## Consequences

- Thrown items are not lost: they land as found floor items and can be
  picked up again (auto-explore's item memory covers them).
- A knife that is both `Weapon` and `Throwable` has independent dice, so
  versatility is tuned in data.
- `SAVE_VERSION` 10 (`Throwable`, `Ammo`, `Projectile`, `Launcher`, `Quiver`).
- AI does not throw or shoot yet.

## Amendment (ADR 0017): blows can miss

Blows no longer always land: each resolves to a hit, graze, deflection,
miss or perfect dodge (`combat::odds` / `combat::resolve`, ADR 0017), melee
and missiles alike. The flight path stays RNG-free: only how the missile
meets the first creature in line is rolled. A graze or deflection stops it
like a hit (and may break an arrow); a miss or perfect dodge comes to rest
at the target's feet, so the path the aim preview drew stays true. `aim`
shows the hit and graze chances from the same odds.

## Amendment (docs/plans/bow-dancing.md): point blank

Firing a launcher with a living enemy on any of the 8 tiles around the
shooter costs `ranged::POINT_BLANK` (4) accuracy: a bow wants room to draw.
It applies to every shooter, not only the player, and is worked out in
`ranged::ready_fire`, which the command, `aim` and AI share, so the preview
shows the lower chance and flags it (`Aim::point_blank`). Thrown things are
not affected. A style may waive it (bow dancing, its own slice).
