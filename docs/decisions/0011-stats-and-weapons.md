# 0011 — Creature stats and weapon attributes

Status: Accepted
Date: 2026-10-16

## Context

Damage came straight from a creature's `Melee` range, and gear could only
add on-hit effects. Outcomes should come from the creature's attributes
meeting the attributes of what it wields, as in Caves of Qud.

## Decision

- Creatures have `Stats` (`strength`, `agility`; 16 is average). The
  modifier is Qud's: `(score - 16).div_euclid(2)`.
- An item's `Weapon { min, max, stat }` names its dice and which stat
  boosts it (a knife uses Agility). The wielded weapon is the `Weapon` on
  the lowest-numbered equipped body part; with none, natural `Melee` is a
  Strength weapon.
- `combat::strike` is the one place this is decided; `roll_melee` (real
  hits and `preview_damage`) and the character sheet both use it. Damage =
  roll + modifier, never below 0, then armor as before.
- A hit is still exactly one draw from the combat stream.

## Consequences

- New weapons and stat-based items are data only.
- Only the stats something reads exist; Toughness, to-hit (Agility vs
  dodge) and penetration are not modelled. Add each with the rule that
  reads it, and expect it to change combat RNG draws per hit.
- Stats are all 16 in data today, so balance is unchanged until tuned.
