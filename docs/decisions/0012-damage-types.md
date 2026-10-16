# 0012 — Damage types and per-type armor

Status: Accepted
Date: 2026-10-16

## Context

Armor was one flat number against every hit. Weapons should differ in how
they harm (a knife cuts, a fist bruises, an arrow pierces), armor should
resist kinds unevenly, and some attacks (a thrown knife) are more than one
kind. More kinds, such as elements, come later.

## Decision

- `DamageType` is `Slashing`, `Blunt` or `Piercing`. A blow carries a set of
  them (`DamageTypes`; data writes it as a list, `[Slashing, Piercing]`).
  Natural `Melee` is blunt; a `Weapon` names its own.
- `Armor` has a flat value per kind. Worn pieces add up per kind.
- A blow is resisted by the kind the armor resists **least**:
  damage = roll + stat modifier − min over the blow's kinds of total armor.
  Armor totals are summed first, so layered pieces cover each other's gaps.
- Having more kinds is therefore never worse than having either alone, and
  better against uneven armor. Its price is paid in data: a versatile
  weapon gets smaller dice or a smaller bonus. Splitting the roll across
  kinds was rejected: flat armor would apply once per share and make a
  mixed weapon strictly worse against uniform armor.
- Damage with no kind (effects such as bleeding) is not resisted, as before.
- Handlers on `BeforeTakeDamage` add their resistance to the event;
  `BeforeTakeDamage::dealt` applies it once, for real hits and previews.
- A hit is still exactly one draw from the combat stream.

## Consequences

- New kinds (elements) are a `DamageType` variant and an `Armor` field.
- `Armor` and `Weapon` formats changed: `SAVE_VERSION` 8. Leather armor
  resists slashing and blunt by 1 and piercing by 0, so melee numbers with
  the starting gear are unchanged.
- Ranged and thrown attacks will use the same rule with their own kinds.
