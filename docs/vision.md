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
  See "Phases" below.
- **No classes.** A character starts from an **origin** (what they are: abilities, a few starting skills,
  maybe a style already in progress) and becomes what they do. Origins need not be equally strong:
  there is no difficulty setting, and choosing an origin is the difficulty choice.
- **Permadeath, with legacy.** A life ends by death or by retiring. The next character finds what it
  left, after an unknown amount of time; what is left depends on how the life is remembered. It helps
  (see "Legacy"), but the new character does not start stronger.

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

## Runs and endings

- **Three loops.** A *dive* pushes its luck: deeper is newer and harder, and returning to camp banks what
  you carry. A *life* is an arc of becoming. A *lineage* is many lives, and what it gains is knowledge of
  the world, not power.
- **Every dungeon has a bottom, and it differs by phase.** The Castle's bottom in the Embers is not its
  bottom in the Rime.
- **Many endings.** Most are *character* endings, which close one life's story. A few are *world*
  endings, like finishing the game without reaching 100%. One is the *true* (final) ending.
- **At an ending the character may retire or continue.** Retiring ends that character and forces a new
  one, and it should be very attractive anyway. It is never required: one character can go all the way
  to the true ending as the first of its line.
- **Revelations persist across the lineage**, though maybe not all of them, or not fully. That is a
  direction to decide per revelation, not one rule.

## Interconnection

The experiment of this game is a system as interconnected as possible. A new system earns its place by
reaching into the others (skills, styles, discovery, lore, phases, legacy), and the others read it
back. A system that only adds content beside the rest is not finished.

## Phases

- **A phase leans the whole world one way, for everyone.** Each has an element that is stronger for
  every creature while it lasts, you included, and one rule of the world it bends. It also has natives
  made for it and finds only it has. A phase is a choice that cuts both ways, never simply better: a
  pyromancer in the Embers burns hotter and meets things that burn back.
- **Elements are the first layer, not the definition.** Only the first phases map onto elements (fire,
  cold, …), which makes them look simple. Later phases are stranger and lean the world in other ways:
  Lush, Forgotten, Shining, Regretful, Hungry. A phase is a set of leans on any system (growth, memory,
  light, regret, appetite); an element is just one kind of lean.
- **Phases have opposites**, arranged on a wheel. Styles, discoveries and skills read the phase: a style
  may thrive in one phase, a discovery may need stakes in one, and time spent in a phase attunes you to
  it.
- **Five to start; more are revealed as time goes on.** The **Stillness** looks like the neutral
  baseline, where nothing leans and the game loop is learned. Later it is revealed to be a phase of its
  own, with an opposite (Chaos, or similar), so it is unique and only seems simple at first.
- **How a phase is chosen is not settled.** Today you wait for any phase at camp. That will change in a
  big way, so the choosing lives behind one seam and nothing else assumes how it works.

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

When a life ends, by death or by retiring, a **legacy record** is written: plain data (name, origin,
where and how it ended, best skills, styles, techniques, recipes, notable items, deeds), not the save.
The next character finds what is left, spawned from blueprints through channels that already exist:

- technique books and memoirs are **lessons**, capped at a fraction of the predecessor's level;
- their journal reveals the requirements of the styles they found (the inspiration still needs stakes);
- recipe notes are **recipes**; their gear is worn items; the grave and its note are story.

How long has passed (the **gap**) is rolled when a character is made (its own RNG stream) and decides
how much survived. Ruins can stack over generations; only the last few are kept. Time between lives is
counted in lives, never in turns: nothing ages while you play.

### Judgement: what a life is remembered as

- **Two axes.** *Renown* is how widely a life is known (unknown → local → known → legend); it decides
  how much survives and for how long. *Repute* is what they are known as (hero, wicked, holy, scholar,
  wild, oathbreaker, …); it decides what kind of thing is left. Not every life leaves a legend or even an
  epitaph.
- **What is left depends on repute.** These are examples, all data:
  - *unknown*: bones and whatever lasted;
  - *known local*: a grave and an epitaph;
  - *hero*: a shrine, a relic that does not decay, a tale that works as a lesson, perhaps a blessing
    (a proficiency obtained from outside);
  - *wicked*: haunted ruins, a revenant built from their record that walks a dungeon with their own
    techniques, tainted gear (power at a cost), a journal of dark discoveries;
  - *holy*: relics that heal or bless, a sacred site that may even reveal a phase;
  - *scholar*: a library whose memoirs teach to a higher cap.
- **Reputation is perception, not truth.** Deeds are facts with tags (blood magic, a rescue, present at
  a necromantic ritual). People judge the tags through **beliefs** ("blood magic is evil"), so the same
  deed reads differently to different people. A deed big enough can outweigh a bias and reframe a whole
  story: the feared hemomancer who saves someone important is remembered for redemption and sacrifice.
  The holy paladin seen at a necromantic ritual who never comes back to explain is remembered as an
  oathbreaker.
- **Tales are told at camp.** Coming back lets you tell your side; dying mid-dive leaves only how things
  looked. So a dive risks being misremembered as well as dying.
- **Lineages unlock understandings.** A legacy can split a belief: thanks to a past scholar people now
  tell sorcery from magic, a mystic from an archwizard, psionics from arcana, or stop seeing hemomancy
  as evil. Understandings are lineage knowledge, like phase revelations, and they change how every
  later life is judged.
- **Judgement is legible.** While you live, camp lets you know what is said of you and why ("They say
  you bled a man to save yourself"); the legacy screen names who told the tale. Never a surprise score.
- **One language of conditions.** Judgements, endings, discoveries, settlers' arrivals and phase
  revelations are all written as requirements over the same things (skills, styles, deeds, phases,
  places, the lineage), so every system can be a condition for every other.

### Retiring, mentors and heirs

- **Retiring is the best way to end a life** (offered at an ending, never required). A retired
  character stays alive at camp as a **mentor**: a teacher whose lessons go higher than any memoir's
  cap. Their legacy does not decay while they live.
- **Mentors age by lives, not turns.** They decline visibly one life ahead ("Ana is growing frail"),
  never as a surprise, and dying of age gives the best legacy there is: full renown, nothing decayed.
  Nothing is lost when they go; their tale and memoir carry it on. Only two or three generations are
  alive in camp at once.
- **Heirs are recognized origins.** A known or heroic life can leave an origin ("Child of Ana", "Squire
  of Ana"), and so can other lineage events (a revealed phase, a settler's trade). The world recognizes
  only a fixed number of origins beyond the base ones: recognizing a new one means forgetting one, chosen
  at character creation. An heir starts differently, never stronger (a style in progress, a relic).

## Camp

- **Camp starts as yours alone.** Before anyone retires, no one lives there: a death leaves the camp in
  ruins and no one judges the life. Renown, perception and settlers all unlock with the **first
  retirement**, the way the Stillness looks simple until it isn't.
- **Settlers come for reasons**, each a condition over the lineage: they married the retired one, their
  child went into the dungeon, they heard the stories and lost their nerve, they saw people living here
  and came to trade. A handful of people, each with a role (teacher, trader, healer, storyteller), a
  reason and a few beliefs. Camp opinion is what its people think. No needs and no economy beyond trade.
- **Places last; people change.** A settler founds a **place** (an inn, a shrine, a healer's hut, a
  library, a forge) and the place outlives them. The gap decides what happened in between: after a
  short one the same person, older; after a medium one a named successor ("Mira, granddaughter of Tova
  the healer"); after a long one the place is abandoned, or a newcomer drawn by its ruin. The same
  person across many generations happens only when the content explains it (an elf, a ghost, a relic).
- **Beliefs drift with succession.** A successor knows the old tales second-hand: things decay with the
  gap, but a legend can grow in the retelling.
- **"Since Ana's time…"**: a new character arrives to a short summary of what changed hands, fell
  silent or arrived. After that, looking at a settler shows their tie in one line.
- **Tales need a teller, and the teller is a place.** With no teller, an unwitnessed death is just
  "vanished" and nothing is judged. An inn's rumor mill tells the worst reading; a storyteller a random
  one; a bard paid before you leave tells the spin you chose, all your life. It's decided before a dive
  in at most one action, never a chore during one.
- **A death no longer ruins everything once there is a village.** Only the character's own things (their
  tent, their stash) fall to ruin; the village carries on.
- **Camp grows by lives.** Retirements found places, gaps age them, renown draws people. Nothing changes
  while you are in a dive.

## Platforms

Desktop first. Phones (Android) later, not soon. The seam makes it a frontend's job: `game` and `front`
build as they are, and eframe runs on Android. What it takes: data embedded in the app instead of read
from a path, saves in the app's own storage, a touch layer onto the same `front` flows (tap a tile,
buttons for the action keys; keyboard-first stays the rule on desktop), and a portrait layout with the
map full screen and panels as overlays. The Android SDK and NDK would come from the flake.

## Open questions

- How much a legacy is worth: the lesson cap, how much survives a long gap.
- How a phase is chosen or reached (waiting, a cycle, a ritual, a roll, something earned).
- What the Stillness really is, and how it is revealed.
- Which revelations persist across the lineage, and how fully.
- Whether settlers can leave or turn against you when camp opinion sours.
- Where moral weight comes from in play (neutral creatures, oaths, desecration, factions).
