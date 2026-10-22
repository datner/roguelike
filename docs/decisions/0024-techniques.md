# 0024 — Techniques

Status: Accepted
Date: 2026-10-22

## Context

Styles change how a creature reacts (ADR 0019) but give it nothing new to
do; the vision's chain ends in techniques, special actions such as a lunge,
learned, invented by a style, or known from the body (an ability). Plan:
`docs/plans/techniques.md`. Whether the game wants stamina is undecided, and
turning it on later must not mean a refactor.

## Decision

- **A technique is a blueprint entity attached to its holder**, like a
  spell (ADR 0014): `Technique { domain, verb, reach, recharge, stamina,
  requires }` says how it is aimed, what it asks and what it costs; payload
  parts (`Blow`, `Dash`) say what it does. A creature's `techniques:` list
  in its blueprint attaches them at spawn, inherited or replaced like
  `spells:`. `Technique` holds skill ids, so it is built from
  `TechniqueDef` at load and saved by name.
- **It shares the spell's effect half, not its payment.** No discipline,
  no `CheckCast`/`PayCost`: a technique costs a turn (`Command::Perform`),
  then recharges (`Recharging`, the spells' type, in ticks so speed does not
  shorten it), and takes `stamina` from a `Stamina` pool.
- **Stamina is dormant.** Every technique costs 0 and no blueprint has a
  pool, so nothing shows it. `Stamina` and `Mana` are the same `Pool`
  (computed on read, ADR 0006), so turning stamina on is a data change. A
  cost with no pool is refused, so a half-done change fails loudly.
- **Requirements are an open list** of `Need`s, each checked in one place in
  `game/src/technique.rs` with its own refusal: `Wielding(skills)` (the blow
  uses them: what is wielded and how), `Skill(skill, level)`.
- **Its blow is a blow.** A bump and a technique both go through one blow in
  `game/src/combat.rs`: the same odds, outcomes, damage and training, and the
  followed style trains when the blow follows it. `Blow` shifts the accuracy
  and the damage; the blow draws the combat stream exactly as a bump does.
- **One check, shared.** Readiness and targeting (reach, view, a straight or
  diagonal line, open and empty ground between, an enemy on the tile) are
  answered by the same functions for the command, refused input, the list
  and, later, aiming and AI.

- **Riders** (`Rider`) are what a full hit does besides its damage:
  `Shove` (straight back until something stops it, no RNG) and `Effect`
  (any timed effect blueprint, such as the trip's `OffBalance`). They are
  applied through `Command::Ride` after the hit's damage, and only if the
  target lived. Grazes carry none.
- **`uses`** names the skills a technique's blow is levelled and trained
  by, instead of the weapon's, for blows that are not weapon work.

- **Styles shape techniques, per technique.** `Need::Following(style)`
  makes one work only under a style; `StyleBonus` entries add accuracy,
  damage, riders and cheaper recharge or stamina while a style is followed,
  from a level of its skill. A style is named by its skill (unique per
  style, ADR 0019). Bonuses apply where the technique is readied, so the
  list and the aim preview show them.

- **Forms are steps of a group.** A `Combo` on a technique places it in a
  group at a step; the performer's `Performed` records the last step and
  when, and the next step continues the run only within `within` ticks of
  it (ADR 0006). Continuing earns the step's bonus and more XP per step of
  the run; `Need::Continuing` makes a step only follow the one before. No
  technique names another, so forms are pure data. A group's first step
  says whether the list shows every step or one row that turns into the
  next.

- **Creatures perform them** with `TechniqueAi`, before a bump or a step:
  the first technique, in the order known, whose shared check passes and
  whose footprint strikes the target and none of its own side. Goblins
  shove; the spirit wolf trips, an ability in the vision's sense.

## Consequences

- A new requirement is a `Need` variant and one match arm; a new payload is
  a part read where the technique is readied and performed.
- `SAVE_VERSION` 31 (`Technique`, `Blow`, `Dash`, `Stamina`; `Mana` is now
  a `Pool`, saved the same); 32 (`Technique` riders and `uses`,
  `OffBalance`); 33 (`Technique` bonuses, `Need::Following`); 34 (`Combo`,
  `Performed`, `Need::Continuing`); 35 (`TechniqueAi`).
- The player knows the lunge from their blueprint until manuals and styles
  teach techniques.
