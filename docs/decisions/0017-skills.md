# 0017 — Skills

Status: Accepted
Date: 2026-10-17

## Context

Characters have no classes (`docs/vision.md`): they become what they do.
That needs a measure of how well a creature does each thing, one that
grows by doing it, overlaps (a knife fighter is also a melee fighter) and
cannot be ground out by repetition. The build plan is
`docs/plans/skills.md`.

## Decision

- **Skills are flat, data-defined tags.** `data/skills.ron` lists each
  skill with a domain and a weight; there is no tree, because skills cut
  across each other (Pierce spans daggers and spears). Load gives each a
  runtime-only `SkillId` (file order); saves and blueprints use names, and
  every name in a blueprint is checked at load. The skill list travels
  inside `Blueprints` (`Blueprints::load(data_dir)`), so everything that
  holds content holds it.
- **The code names a few core skills** (`skill::Core`: Melee, Unarmed,
  Pierce, Slash, Blunt): the ones an action or circumstance implies rather
  than a part. A skill file without them fails to load. An empty
  `SkillBook` (tests) has no core, and then no action uses or trains a skill.
- **An action's skills** are the action's own (Melee), the tool's
  (`Trains([..])` on the item; Unarmed for natural attacks) and one per
  kind of damage dealt (`Weapon.types`: Slashing → Slash), so data never
  repeats what the weapon already says. `skill::melee` collects them, sorted.
- **The level at an action is a weighted average** of the creature's levels
  in its skills, rounded down. A sum would make the tool with the most
  skills the best one. Specific skills weigh more than general ones.
- **XP is stored; levels are derived** (`skill::level`, `50 × n²` XP for
  level `n`, at most 20). Changing the curve needs no save migration.
- **Use trains, only with stakes, less each time.** A melee hit on an
  enemy (`side::enemies`) gives each skill used `BASE_XP × 10 / (10 + n)`,
  where `n` counts earlier uses of that skill against that kind of thing
  (its `Name`, in `Familiarity`). Variety restores the rate; repetition
  can't be ground. No randomness is drawn.
- **Training happens inside the attack's applier** (`skill::train`, called
  from `command::attack`), not through its own `Command`: it is part of
  applying the attack, and the skill list it carries isn't `Copy`.
- **Only `Learner` creatures gain XP**: the player. Monsters have fixed
  `Skills` from their blueprint (`Skills({"Dagger": 2})` in levels).
- **What a level does, for now: damage.** Every 4 levels of the effective
  level add 1 to `Strike.bonus`; `Strike.level` carries the level for the
  hit chance to come. Preview and attack share `combat::strike`.

## Consequences

- New skills are data; a new *kind* of action adds its own core skill or
  `Trains` reading in one function like `skill::melee`.
- `Familiarity` keys on names, so two creatures sharing a name count as one
  kind. Fine while names are kinds; revisit if names become personal.
- Two-handed weapons and dual wielding need a grip model first; one-handed
  and empty-offhand skills come from equipment state (plan slice 3).
