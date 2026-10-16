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
  `CheckCast` (claim the school; veto with a reason) and `PayCost` (emit
  commands that take the price). A school with no claiming discipline cannot
  be cast. Priorities are in `prio`, so the order is reviewable.
- **Per-spell state lives on the spell entity.** The cooldown discipline
  (`CooldownCasting { school, ticks_per_power }`) stores `Recharging(Timed)`
  there, written by `Command::Recharge` and read from timestamps (ADR 0006).
  Pool resources will live on the caster.
- **One command, shared check.** `Command::Cast { actor, spell, at }` costs
  `cost::CAST`. `spell::ready_cast` runs `CheckCast` on a cloned stream and a
  scratch buffer, so `reject_input` and later previews cannot disagree with
  the cast.
- **A bolt flies along `ranged::flight`** and the first creature takes
  `DamageCause::Spell`, which is untyped, so armor passes it (like effect
  damage). The damage roll is one combat-stream draw, only on a hit.

## Consequences

- A new way to pay is a part plus two handlers and RON; `cast` is unchanged.
- Not yet: mana, areas and cones, summons, spell UI, casting by AI,
  elemental damage types, a magic stat. `SpellDamage.stat` is optional until
  one exists.
- `SAVE_VERSION` 11 (`Spell`, `SpellDamage`, `Recharging`, `CooldownCasting`).
- `ModifyCast` (equipment and perks adjusting a cast) is planned between
  check and pay, and left out until something needs it.
