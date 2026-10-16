# 0014 — Spells and disciplines

Status: Accepted
Date: 2026-10-16

## Context

We want several ways to pay for magic (cooldowns, mana, heat, reagents), and
schools or classes that cast the same spell differently. A cost field on the
spell would fix one way of paying for every caster.

## Decision

- **A spell is a blueprint entity attached to its caster** (`Attachments`,
  not `Inventory`: it is "in use", not carried). `Spell { school, range,
  power }` says how it is aimed and how much it asks, in abstract `power`;
  payload parts such as `SpellDamage` say what it does. Nothing on a spell
  names a resource.
- **A discipline is a part on the caster** that handles two events:
  `CheckCast` (claim the school, describe the price as `CostLine`s for
  frontends, or veto with a reason) and `PayCost` (emit
  commands that take the price). A school with no claiming discipline cannot
  be cast. Priorities are in `prio`, so the order is reviewable.
- **Per-spell state lives on the spell entity, pools on the caster.** The
  cooldown discipline (`CooldownCasting { school, ticks_per_power, stat }`)
  stores `Recharging(Timed)` on the spell, written by `Command::Recharge` and
  read from timestamps (ADR 0006). The mana discipline (`ManaCasting {
  school, mana_per_power, stat }`) spends from the caster's `Mana { value,
  max, as_of, regen_ticks }`, which is computed on read (`value` plus a point
  per `regen_ticks` since `as_of`), so no timer runs per tick;
  `Command::SpendMana` banks the regained points and keeps partial progress.
  The pool grows with stats (`Mana.growth`): per modifier point, each
  listed stat adds capacity and regeneration pace (basis points; never below
  a quarter pace), computed on read with the rest. The player's Intelligence
  mostly deepens it, Willpower mostly quickens it, so the two pull a mage
  toward a big reserve or a fast trickle.
- **The discipline names the stat a spell scales with**, not the spell: a
  claiming handler sets `CheckCast.stat`, and the damage adds the caster's
  modifier for it. The same spell cast through another discipline (another
  class) scales differently. The player's pyromancy uses Willpower, their
  arcane magic Intelligence.
- **One discipline per school per caster**, checked at blueprint load
  (`SchoolClaimedTwice`), so which one pays never depends on handler order.
  Handlers also report their price when they then refuse, so a spell list
  can show what a blocked spell would cost.
- **One command, shared check.** `Command::Cast { actor, spell, at }` costs
  `cost::CAST`. `spell::ready_cast` runs `CheckCast` on a cloned stream and a
  scratch buffer, so `reject_input` and later previews cannot disagree with
  the cast.
- **A bolt flies along `ranged::flight`** and the first creature takes
  `DamageCause::Spell`, which is untyped, so armor passes it (like effect
  damage). The damage roll is one combat-stream draw, only on a hit.

- **Areas are a spell part, `Area { shape, hits, falloff }`**; a spell
  without one is a bolt. The shape decides how it is aimed: a `Burst`
  flies like a bolt and goes off where it lands (a point, checked like a
  missile); a `Cone` (fixed 90°) or a `Beam` (through creatures, to its
  range or a wall) takes a direction, any tile but the caster's; a `Nova`
  surrounds the caster and is not aimed. `area::tiles` is pure and walls
  block it through the same shadowcasting as sight (ADR 0007), so an area
  never reaches round a corner. `spell::footprint` works out the flight,
  the tiles and who is caught, for both the cast and `aim`.
- **Everyone caught takes their own roll**, from the combat stream in
  `SpawnSeq` order, less `falloff` per tile from the centre. `hits` is
  `All` (the caster too), `Enemies` or `Allies`; until there are factions,
  a side is whether a creature is `Hostile`.

- **The pipeline is `ModifyCast` → `CheckCast` → `CastRisk` → `PayCost`.**
  `ModifyCast` (at the caster) lets worn parts change range, power and
  damage first; a `Focus` is the first such part. `CheckCast` is asked of
  the spell itself, then of the caster: a `Scroll` claims its own cast
  (carried, so the caster's events never reach it), and a discipline skips
  a cast already claimed, so no caster pays for a scroll. `CastRisk` sums
  a fizzle chance (`Fizzle`: armor worn against a stat); `PayCost` goes to
  whoever claimed. A fizzle is paid for and does nothing, one spell-stream
  draw, made only when the chance is above zero.
- **Blood magic** (`BloodCasting`) is a third discipline: health per
  power, refused when it would leave the caster at 0. It needed nothing
  from the pipeline but the `caster` field.

- **Elements are damage types.** `Fire`, `Cold` and `Shock` join
  `DamageType`, and `Armor` grows a resistance for each, read the same way
  (ADR 0012). `SpellDamage.types` carries them, `DamageCause::Spell` is
  typed, and untyped spells (raw force, blood) still pass all armor. Armor
  *weight* (fizzling) counts the physical kinds only.
- **More payloads, same pipeline.** `Chain { jumps, reach }` carries a bolt
  on to the nearest enemy not yet hit, with open ground between. `Delay {
  ticks }` lays a `Rune` where the spell lands: a timed effect on the
  caster, holding the damage rolled-for at cast time (`Blast`), that
  bursts on expiry through `effect::expire_effect`. Both are read in
  `footprint` and `cast`; neither touches pricing.
- **Creatures know spells from data** (`spells:` in a blueprint, like
  `equipment`: inherited unless replaced, checked at load, spawned
  attached). `SpellAi` makes a creature cast: the first spell, in the
  order learned, that `ready_cast` and `check_target` allow and whose
  footprint hits its target and none of its own side. The goblin shaman
  pays with blood.

## Consequences

- A new way to pay is a part plus two handlers and RON; `cast` is unchanged.
  Mana proved it: the only change to the shared code was naming the
  `caster` on `CheckCast` and `PayCost`, since handlers aren't told whom
  they fire on and a pool lives on the caster.
- `view::spells` and `aim` (`Shot::Cast`) read the same `inspect_cast` /
  `ready_cast`, so the spell list and the aim cursor show what the cast
  will do and cost; a new discipline's price reaches the UI through its
  `CostLine`s.
- Not yet: element interactions, terrain-making spells and rune
  composition (proposed in ADR 0016), heat, wands and reagents, cone
  widths other than 90°.
- `SAVE_VERSION` 11 (`Spell`, `SpellDamage`, `Recharging`, `CooldownCasting`);
  12 (`ManaCasting`, `Mana`, `Stats` gains Intelligence and Willpower,
  `SpellDamage` loses `stat`); 13 (`Area`); 15 (`BloodCasting`, `Scroll`,
  `Focus`, `Fizzle`); 16 (`Chain`); 17 (`Delay`, `Blast`, `SpellAi`,
  elemental `DamageType`s and `Armor` resistances); 18 (`Mana.growth`). Summons are ADR 0015.
- One goblin shaman joins each new zone, placed after the items, so the
  goblins and items of a seed stay where they were.
- New loot (the scroll of fireball, the ember circlet) changes which items
  seeded worlds scatter: the pick is by index into every `Item` blueprint.

## Amendment (ADR 0017): skill at magic

`inspect_cast` also asks `skill::cast` which skills the spell uses and
raises the strike and lowers the fizzle chance by the caster's level at
them. Disciplines and `cast` are unchanged; a cast that catches an enemy
trains those skills.
