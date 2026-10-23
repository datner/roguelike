# Vision

Where the game is going. Not a spec: plans in `docs/plans/` build pieces of it, ADRs record what was
decided while building. When a plan and this document disagree, ask the owner which one moves.

## The shape of the game

- **A camp, and dives into themed dungeons** (a castle, a cave, …). You return to camp between dives.
- **Phases.** The world turns through phases (e.g. a fire-strong phase). At camp you choose which phase
  to wait for, and enter the dungeon in it. The phase is fixed for the whole dive. Each phase has its own
  creatures, floors and finds on top of the dungeon's shared content: entering the castle in the fire
  phase, you never see the floors and enemies of the other phases, and you meet ones only it has.
  Layouts are rolled anew on every dive; a phase decides what you meet, never a map to learn.
- **No classes.** A character starts from an **origin** (what they are: abilities, a few starting skills,
  maybe a style already in progress) and becomes what they do. Origins need not be equally strong:
  there is no difficulty setting, and choosing an origin is the difficulty choice.
- **Permadeath, with legacy.** A dead character's camp is found in ruins by the next one, after an
  unknown amount of time. What they left helps (see below), but the new character does not start stronger.

## Pillars

1. **You become what you do.** Skills grow by use; doing things in combination leads to discoveries.
2. **Progress comes from new and hard things.** Repeating the same thing teaches less and less; variety
   and danger restore it. Grinding is not punished, it is pointless.
3. **Boring-but-useful is one keystroke.** If the best option is dull (practice, waiting for a phase), it
   is a single action that runs until done or interrupted.
4. **Discoveries need stakes.** The moment of inspiration that unlocks a style or proficiency only comes
   from a use with something at risk, never from practice.
5. **Time is neutral.** No hunger, no clocks, no ratchets that punish slow players or reward fast ones.
   The phase you choose at camp is the only thing time decides.
6. **Content is data** (ADR 0004): skills, styles, proficiencies, phases, lessons and legacy contents
   are RON, composed from reusable parts.

## The character

Four systems, linked into one chain; abilities stand apart.

```
skills ──combinations──> proficiencies (passive, permanent)
   │                └──> styles (active practice) ──invent──> techniques / spells / recipes
   └── grow by use, practice and lessons
abilities: from what the creature is (origin, body); never learned, never found on a scroll
```

### Skills

How well a creature does a thing. Flat, overlapping tags, not a tree: stabbing with a knife and an empty
off hand uses Melee, Dagger, Pierce, One-handed and Empty offhand together. An action's level is the
weighted average of its skills, specific ones weighing more, so general skills carry over between tools
and specific ones reward sticking with one.

Each skill pays off in one place. Most just raise a number (damage, fizzle chance, a cost, a duration);
a few unlock information or an action at thresholds (Lore reveals a creature's resistances). A skill
exists only once a system reads it.

Domains, as they come online: martial, defence, magic, disciplines, lore, body, senses, care, handling,
and life skills (scavenging, fletching, smithing, tinkering, alchemy, cooking, scribing).

Attacks can miss: combat skills raise hit chance as well as damage, and Dodge is a skill. A miss can
still **graze** (half damage, and only effects meant for grazes), so missing is less frustrating and
forgiving weapons graze more. Defenders answer in two ways, each an outcome other things react to:
a **perfect dodge** (the body) means no graze is even calculated (a riposte; an ability that makes every
dodge perfect); a **deflection** (the gear: a buckler, a pauldron) turns a graze aside (a shield bash,
wear on the item).

**Only the player learns.** Monsters have fixed skills from their blueprint; they do not live long or act
consistently enough for growth to mean anything.

**How skills grow.** Three sources:

- **Use.** A use counts only if it passes its domain's test: *stakes* (something hostile involved),
  *novelty* (first time with this kind of thing), *odds* (it could have failed), or *consumption* (it
  used up something finite, like materials). How much it is worth decays with repetition against the
  same kind of thing.
- **Practice.** A repeatable, low-yield action (drills, sparring a dummy, meditation, study) that runs
  until its ceiling or an interruption. The ceiling is relative to what real use has reached, so practice
  tops up and never replaces play.
- **Lessons.** Once per kind of thing, a big chunk: dissecting a corpse (first aid, beast lore), reading a
  manual, studying a rune, watching an enemy use a style.

### Discovery

Requirements over skills *and* styles (`Brawling 20 + Berserking 20`) unlock, at a moment of inspiration
during a use with stakes, one of:

- a **proficiency**: passive and permanent (Archery + Athletics: shooting no longer breaks your stride).
  Proficiencies can also be *obtained* from outside: a teacher, an item, a body change.
- a **style**: an active practice;
- a **recipe**.

A journal shows half-formed ideas ("your bare-fisted fury feels like it could become a method…") once
some requirements are met, so discovery is legible, not a hidden recipe hunt.

### Styles

- **One active style per domain** (one martial, one magic, one crafting). A hybrid style fills every slot
  it spans: Blade-singer (Magic + Melee + Channeling + Enchantment + One-handed) is your martial *and*
  magic style. Switching costs an action.
- **A style grows by use while you follow it**: an action conforms when its skills include the style's.
  Fencing pushes you towards a rapier and an empty off hand.
- **A fresh style does nothing**, good or bad: it is an idea being tested, and trying a new discovery must
  never get you killed. **Principles** (bonuses, penalties, changed actions or reactions) switch on at
  style levels. Principles change *how* you fight (a riposte after a dodge, hits that cost HP and add
  damage), not just numbers: skills give numbers, styles give behaviour, proficiencies give permission.
- **Styles invent** in their own domain at milestones: martial styles invent techniques, magic styles
  spells (forms × elements × modifiers, ADR 0016), crafting styles recipes.
- **Advanced styles** need mastered styles: Brawling + Berserking inspire Savage Warfare. These are the
  prestige classes of the game, and the level curve must make them reachable within one life.

### Techniques and abilities

Special actions. They share the spell's *effect* half (payloads, shapes, `area.rs`, the `aim.rs`
preview) but not its *source or payment*: there is no scroll of an ability, and lunge is not cast through
a discipline. Techniques are learned (a teacher, a manual) or invented by a style; abilities come from
what the creature is.

## Legacy

On death, a **legacy record** is written: plain data (name, where and how they fell, best skills, styles,
techniques, recipes, notable items), not the save. A new character finds that camp in ruins, and its
contents are spawned from blueprints through channels that already exist:

- technique books and manuals are **lessons**, capped at a fraction of the predecessor's level;
- their journal reveals the requirements of the styles they found (the inspiration still needs stakes);
- recipe notes are **recipes**; their gear is worn items; the grave and its note are story.

How long has passed is rolled at character creation (its own RNG stream) and decides how much survived.
Ruins can stack over generations; only the last few are kept.

## Platforms

Desktop first. Phones (Android) later, not soon. The seam makes it a frontend's job: `game` and `front`
build as they are, and eframe runs on Android. What it takes: data embedded in the app instead of read
from a path, saves in the app's own storage, a touch layer onto the same `front` flows (tap a tile,
buttons for the action keys; keyboard-first stays the rule on desktop), and a portrait layout with the
map full screen and panels as overlays. The Android SDK and NDK would come from the flake.

## Open questions

- How much a legacy is worth: the lesson cap, how much survives a long gap.
- How many phases, and what each one changes.
