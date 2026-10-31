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
  enemy (`side::enemies`) gives each skill used `BASE_XP × 20 / (20 + n)`,
  where `n` counts earlier uses of that skill against that kind of thing
  (its `Name`, in `Familiarity`). Variety restores the rate; repetition
  can't be ground. No randomness is drawn. (The decay was `10 / (10 + n)`
  until the first pacing pass: a dungeon's handful of kinds then ran dry
  long before the vision's level targets; the bench measures it.)
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
- **How a weapon is held is a skill too** (plan slice 3): a weapon strike
  adds One-handed (every weapon takes one hand for now) and Empty offhand
  when the wielder has another hand and nothing is equipped on any of them.
  They come from equipment state in `skill::melee`, never from data.
- Two-handed weapons and dual wielding need a grip model and the
  dual-wielding proficiency first.

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

## Missiles (plan slice 4)

- A thrown missile uses Thrown, what it `Trains` and its damage kinds; a
  fired one, Archery, what the launcher `Trains` (the short bow: Bow) and
  the arrow's damage kinds (`skill::missile`, with `Launch` on the readied
  shot). Their strikes carry level, accuracy and damage bonus like melee
  (`combat::skilled`), and resolve through the same odds. `Strike.weapon`
  is the missile, so a missile's own `Graze` counts.

## Magic (plan slice 5)

- A cast uses Magic, its school (Pyromancy, Arcane, Hemomancy) and its form:
  Bolt for damage without an area, else the area's shape (Burst, Cone,
  Beam, Nova), plus Chain, Rune or Summoning for those payloads, plus what
  the spell `Trains` (`skill::cast`). A scroll is its spell, so it trains
  the same.
- The effective level adds to a damaging spell's strike like a blow's
  (every 4 levels, +1) and lowers the fizzle chance by 2% a level
  (`skill::steadied`), both in `spell::inspect_cast`, so the spell list,
  the aim preview and the cast agree. **Departs from the plan**: it is not a
  `CastRisk` handler, because handlers can't see the skill book; `cast`
  itself still learns nothing about skills.
- A cast trains when it catches an enemy (named by the first one), and so
  does the discipline that paid for it: Channeling (mana), Bloodletting
  (blood; not "Hemomancy", which is the blood school's skill) or Scroll
  lore. A cooldown trains none. Summonings and runes catch nobody when
  cast, so they don't train.
- **Discipline skills ease their price**: every 4 levels of Channeling or
  Bloodletting take a point off a mana or HP price (never below 1); Scroll
  lore lowers a scroll's fizzle chance like magic skill does. The
  claiming discipline records how it is paid (`CheckCast.way`), and
  `CheckCast`/`PayCost` carry the caster's levels (`Mastery`), because
  handlers can't see the skill book.

## Lessons and practice (plan slice 8)

- **A lesson** is an item part (`Lesson({"Sword": 200})`): studying it
  (`Command::Study`, five turns) grants that XP once per kind of thing (its
  `Name`, recorded in `Studied`). A second copy teaches nothing.
- **Practice** (`Command::Practice`, a turn; `training::practice_step`
  repeats it like auto-explore) drills the skills of the current melee
  strike, 2 XP a turn each, up to a ceiling: half the learner's best level
  in that skill's domain, never below 2. **Departs from the plan**, which
  asked for a fraction of the best level reached through counted use: that
  needs tracking every skill's best counted level; the domain's best level
  is the same idea without new state.
- **Neither has stakes**: both go through `skill::gain`, which skips
  familiarity, styles and inspiration (only `skill::train` has them).

## Lore (plan slice 9)

- **Beast lore** (domain Lore) is trained by novelty: the first sight of
  each kind of creature (by `Name`, recorded in `Sighted`) when the
  player's view is refreshed teaches it once, through `skill::gain` (no
  stakes, so no inspiration).
- **It decides what looking reveals** (`lore::inspect`, the `;` cursor):
  level 1 a creature's health, level 3 its armor by kind. Below that the
  game says you know too little of its kind.
- **Arcana** (domain Lore) is trained by novelty too: the first cast of
  each spell (by name, recorded in `Attuned`). It reads what other
  creatures cast when looking: their spells from level 1, their damage
  from level 3.
