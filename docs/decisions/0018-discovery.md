# 0018 — Discovery and proficiencies

Status: Accepted
Date: 2026-10-17

## Context

Without classes (`docs/vision.md`), new capabilities have to come from what
a character does: reaching combinations of skills should inspire something
new. The first thing inspiration grants is a proficiency, a passive
qualification. Plan: `docs/plans/skills.md`, slice 6.

## Decision

- **Discoveries are data.** `data/discoveries.ron` lists each discovery's
  name, its requirements (`Skill(name, level)` for now; styles later) and
  what it grants (`Proficiency(blueprint)`). They load with the blueprints
  (`Blueprints::with_discoveries`), and every skill and blueprint named is
  checked then; the granted blueprint must carry `Proficiency`.
- **Inspiration needs stakes.** Discoveries are checked only at the end of
  `skill::train`, which only uses with stakes reach (blows and casts
  against enemies). Practice, when it comes, will train without it.
  Discoveries are checked in file order; each is made once.
- **A proficiency is a blueprint attached to the learner**, marked
  `Proficiency`, whose other parts take part in the learner's events like
  equipment's do. The first, `Footwork`, adds perfect-dodge chance through
  an `AttackOdds` handler.
- **`AttackOdds` says who is being asked** (`asking`), since it is fired at
  the attacker and then the defender: a part learns whether its holder gives
  or takes the blow.
- **What has been discovered is saved by name** (`Discovered`), so the
  discovery file can be reordered or grow.
- **The journal is legible but keeps a surprise.** `discovery::ideas`
  lists undiscovered entries with at least half their requirements met,
  naming only the met requirements; the skills panel shows them without
  the discovery's name.

## Consequences

- New discoveries and proficiencies are data plus, at most, one new part.
- A proficiency part that needs the holder's side reads `asking`; one that
  needs more context than an event carries needs the event to carry it.
- Discoveries granting styles or recipes extend `Grant`; requirements on
  styles extend `Requirement`.
