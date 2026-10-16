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

## Blows can miss (plan slice 1b)

- **Five outcomes.** A blow rolls to hit; a miss then rolls the defender's
  perfect dodge, which skips the graze roll entirely; otherwise the
  weapon's graze chance (`Graze`, 30% by default) makes it a graze or a
  miss; a graze then meets the best `Deflect` the defender wears, which
  turns it aside. A graze deals half the rolled damage and fires `AfterHit`
  with `graze: true`; an `OnHit` entry applies on one only if `on_graze`.
- **Hit chance** is 75% between equals, ±4% per point of the attacker's
  accuracy (effective level + weapon stat modifier) over the defender's
  Dodge level + Agility modifier, clamped to 5–95%. Poise gives 1% perfect
  dodge per level; Deflection adds 1% per level to worn `Deflect`.
- **Parts adjust the odds through `AttackOdds`**, fired at the attacker,
  then the defender, after the base chances are set (an ability making
  every dodge perfect sets `perfect_bp` to certain). `combat::odds` runs it
  on a cloned RNG with a scratch buffer, so asking changes nothing; the
  attack resolves exactly those odds.
- **A roll is drawn only when its chance is neither 0 nor certain**, so
  creatures without the parts involved leave the combat stream untouched.
  This changes the combat stream's draw order (seeded fights differ).
- **Perfect dodges and deflections are outcomes** with their own events
  (`PerfectDodge`, `Deflected { by }`) and log lines, for reactions to come
  (a riposte, a shield bash). Nothing reacts yet.
- **Learning from a blow.** The attacker's skills gain `BASE_XP × (2 − hit
  chance)` on a hit and half `BASE_XP` otherwise. A learning defender
  trains Dodge and Poise (plus Deflection when wearing a deflector):
  `BASE_XP`, or `BASE_XP × (2 − perfect chance)` for a perfect dodge.
- Tests about what a hit does give the attacker (or defender) the test-only
  `testkit::SureHits` part, registered only under `cfg(test)`.
