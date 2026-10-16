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

## Consequences

- A new way to pay is a part plus two handlers and RON; `cast` is unchanged.
  Mana proved it: the only change to the shared code was naming the
  `caster` on `CheckCast` and `PayCost`, since handlers aren't told whom
  they fire on and a pool lives on the caster.
- `view::spells` and `aim` (`Shot::Cast`) read the same `inspect_cast` /
  `ready_cast`, so the spell list and the aim cursor show what the cast
  will do and cost; a new discipline's price reaches the UI through its
  `CostLine`s.
- Not yet: areas and cones, summons, casting by AI, elemental damage types,
  mana regeneration scaling with a stat.
- `SAVE_VERSION` 11 (`Spell`, `SpellDamage`, `Recharging`, `CooldownCasting`);
  12 (`ManaCasting`, `Mana`, `Stats` gains Intelligence and Willpower,
  `SpellDamage` loses `stat`).
- `ModifyCast` (equipment and perks adjusting a cast) is planned between
  check and pay, and left out until something needs it.
