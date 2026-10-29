# Vision

Where the game is going. Not a spec: plans in `docs/plans/` build pieces of it, ADRs record what was
decided while building. When a plan and this document disagree, ask the owner which one moves.

## The shape of the game

- **A camp, and dives into themed dungeons** (a castle, a cave, …). You return to camp between dives.
- **Phases.** The world turns through phases (e.g. a fire-strong phase). You enter a dungeon in one
  phase, and it is fixed for the whole dive; how a phase is reached is still open. Each phase has its own
  creatures, floors and finds on top of the dungeon's shared content: entering the castle in the fire
  phase, you never see the floors and enemies of the other phases, and you meet ones only it has.
  Layouts are rolled anew on every dive; a phase decides what you meet, never a map to learn.
  See "Phases" below.
- **No classes.** A character starts from an **origin** (what they are: abilities, a few starting skills,
  maybe a style already in progress) and becomes what they do. Origins need not be equally strong:
  there is no difficulty setting, and choosing an origin is the difficulty choice.
- **Permadeath, with legacy.** A life ends by death or by retiring. The next character finds what it
  left, after an unknown amount of time; what is left depends on how the life is remembered.
- **An origin is the same start on every life.** Picking an origin on life 3, 30 or 80 gives the same
  starting point. What the lineage changes is what waits at camp: the options, teachers, relics and
  power available to earn and learn in play, never the start itself.

## Pillars

1. **You become what you do.** Skills grow by use; doing things in combination leads to discoveries.
2. **Progress comes from new and hard things.** Repeating the same thing teaches less and less; variety
   and danger restore it. Grinding is not punished, it is pointless.
3. **Boring-but-useful is one keystroke.** If the best option is dull (practice, waiting for a phase), it
   is a single action that runs until done or interrupted.
4. **Discoveries need stakes.** The moment of inspiration that unlocks a style, proficiency or recipe
   only comes from a use with something at risk, never from practice. What is at risk differs by
   domain: your life in a fight, a rare material in a craft.
5. **Time is neutral.** No hunger, no clocks, no ratchets that punish slow players or reward fast ones.
   Nothing decays or ages during play; the world's phase is the one thing that turns, and only when
   you choose to wait for it. Time passes only between lives (the gap), and that is where decay,
   ageing and succession happen.
6. **Content is data** (ADR 0004): skills, styles, proficiencies, phases, lessons, legacy contents,
   factions and their beliefs, situations, materials, recipes and endings are RON, composed from
   reusable parts.
7. **Every "why?" has an answer on screen.** Each system has one place where it is shown, and a system
   is not finished until it has one. Legible is not omniscient: what you don't know is shown as unknown,
   never left out (see Legibility).

## Runs and endings

- **Three loops.** A *dive* pushes its luck: deeper is newer and harder, and returning to camp banks what
  you carry. A *life* is an arc of becoming. A *lineage* is many lives, and what it gains is knowledge of
  the world and what waits at camp, never a stronger start.
- **Returning costs commitment.** A dungeon's data says which floors have a **way-landing**, a way back to
  camp (say every third floor and the bottom). Between landings the only way is down, so each landing is
  a point to bank or push on. A **recall** takes you home from anywhere, at a price: crafted (scribing,
  alchemy) or bought from a trader, so it is rare until camp has a maker or a trader.
- **Every dungeon has a bottom, and it differs by phase.** The Castle's bottom in the Embers is not its
  bottom in the Rime.
- **Many endings.** Most are *character* endings, which close one life's story. A few are *world*
  endings, like finishing the game without reaching 100%. One is the *true* (final) ending.
- **The first ending is simple to reach.** Retiring, and everything it unlocks (settlers, renown,
  perception, creeds, tellers, heirs), waits for an ending, so the first one must come within the first
  few lives, not after mastering the game.
- **At an ending the character may retire or continue.** Retiring ends that character and forces a new
  one, and it should be very attractive anyway. It is never required: one character can go all the way
  to the true ending as the first of its line.
- **A retirement is worth what the life was.** What it gives (the mentor's strength, renown, which
  settlers come) is measured from the life itself: how deep it went, what it discovered, what it did.
  Retiring a short life at the easy ending is allowed and gives little, so retiring early every life is
  never the best line.
- **Revelations belong to the lineage.** They persist across its lives, though maybe not all of them,
  or not fully (decided per revelation). A new lineage starts from scratch, revelations included: a
  fresh lineage is a fresh world, not a new chapter.

## Fights

Everything else stands on dives being fun in themselves. A fight is a decision when the obvious move
(bump the nearest thing) is sometimes wrong and you can see why before you act. Sometimes, not always:
bumping can overpower an encounter, and steamrolling or facetanking one is a tactical choice of its
own, paid for in HP.

- **Each creature asks one question.** A new kind of creature earns its place by needing a different
  answer, not by having more HP: the shaman asks who dies first, the wolf says you can't outrun it. Next
  are the **brute** (a heavy blow it winds up), the **skirmisher** (backs off and shoots: corner it or
  use cover) and the **swarm** (find a chokepoint or use an area). Strength is always one answer: a
  stronger character can ignore the question.
- **Telegraphs, never Simon says.** A heavy blow is announced a turn ahead and its tiles are shown.
  Stepping back must not be the free answer, so a telegraph matters because of what else is true: the
  blow covers more than one step, the step back gives the skirmisher a free shot or lets the swarm
  around you, there is a wall behind you. Answers trade against each other: step away (lose tempo),
  interrupt it (a hit during the wind-up staggers it, a miss eats the blow), or take it on armor.
- **Terrain does one thing each**, and gives existing techniques a target. **Doors** break sight and
  make chokepoints. **Chasms** and **deep water** turn a shove, push kick or trip into a kill.
  **Rubble** slows. **Tall grass** hides: in it you are unseen beyond the next tile (and so is anyone
  else), and it burns. Hiding is not stealth: it is only being out of sight, and a foe that loses
  sight of you goes to where it last saw you. That is a lure: show yourself, step into the grass, and
  meet it on your tile. **Darkness** makes a light a trade: you see, and you are seen.
- **Outnumbered is one rule.** Each foe adjacent beyond the first makes you easier to hit, so corridors
  and corners stop being neutral. There is no facing, so no flanking: being surrounded is the
  positional rule, and being off balance is the opening.
- **Retreat works.** Fleeing a foe as fast as you never works under the energy model, so it never
  happens. **Sprinting** makes it a choice: moves cost less energy and drain stamina, and you can't
  attack while sprinting. Creatures sprint too (a wolf's charge). Doors you can close, a parting shot,
  way-landings and foes with **morale**, who break and flee in turn, make retreat a plan, not a hope.
- **Attrition makes a fight worth asking about.** HP regenerates slowly, from a **rest budget per
  floor**: waiting is one keystroke (pillar 3) and careful play can heal between fights, but only so
  much per floor, so a floor is a budget and an avoided fight is HP kept. Careful play has its own
  risk: **roamers** wander the floor, so waiting in the open can be found and a dead end behind a door
  is safer. Roamers wander from the start and never arrive because time passed (pillar 5: no clock
  that punishes the slow).
- **Not every fight is mandatory.** Going around, hiding and talking skip a fight. Fighting gives loot
  and martial XP; skipping keeps HP and the rest budget.
- **Checked by the bench.** Each creature's question gets a scenario with a **naive build** (bump only)
  and an **answer build**. At par, the naive build should lose and the answer win; well above par, the
  naive build should win too. If naive wins at par, the creature asks nothing.

## Legibility

Perception, beliefs, attunement, discovery and judgement are each fine alone; together they risk the
game feeling random. Pillar 7 holds them all to the standard discovery and judgement already meet.

- **What you don't know is shown as unknown** ("?", "you can't tell"), never silently left out. Lore,
  perception and hearsay hide things on purpose; that is allowed, hiding that something is hidden is
  not.
- **The game never lies; people do.** Numbers are true. Only people mislead, and their words are always
  marked as theirs ("They say…", "The innkeeper claims…"): perception is hearsay.
- **A surprise is fair once.** The first meeting with a kind of creature may surprise you; after that
  lore records it and it is shown.
- **Numbers explain themselves.** Bonuses add up in one place (Interconnection), and that place also
  gives the breakdown: inspecting any number lists its sources ("Fire damage +35%: Embers +20, Pyre
  attunement +10, meal +5 (cap 50)").
- **The log answers "why?"** A key on a log line expands its cause ("You miss" → "deflection 42%: base
  52, outnumbered −10"). Determinism means the cause is always known, so this is display, not a model.
- **See it before you commit.** Aiming previews what a shot does (ADR 0013); the same goes for stepping
  into a telegraphed tile (a warning), how outnumbered a step leaves you, and a phase's leans and your
  attunement before you wait for it.
- **A death names its chain**, not just the last blow: "Killed by the brute's swing; outnumbered by 3;
  out of rest on this floor." It is the same cause data the why-key reads.
- **Each system's place**, each with a shot in `scripts/shots.sh` so it can be checked:

  | System | Where it is shown |
  | --- | --- |
  | Skills, attunement | Skills tab |
  | Styles and principles | Styles tab |
  | Discovery | Journal (half-formed ideas and their requirements) |
  | Techniques, spells | Chooser, then the aim preview |
  | Lore | Look cursor (`inspect`) |
  | Bonuses and caps | Inspecting a number: its breakdown |
  | Fights | Telegraphed tiles on the map; outnumbered count; the log's why-key |
  | Rest budget | Status line |
  | Phases | Camp's phase choice (leans, natives, attunement); the current phase in the status line |
  | Crafting | Recipe list: what you can make now from what you carry |
  | Judgement, renown | Camp: what is said of you and why |
  | Beliefs, standing | Looking at a person: their beliefs and what they think you did |
  | Legacy | "Since Ana's time…" summary; the Fallen list |
  | Death | Death panel: the cause chain |

## Pacing

Styles must be reachable within one life, the first ending within the first few lives, mentors must
span two lives. Targets make that checkable instead of hoped for.

- **Counted in actions, not hours.** The bench can count player actions, not time. One assumption,
  kept in data, converts them: about **1000 actions an hour**, where an auto-explore or practice run
  is one action.
- **Targets** (first draft; a moved target is a design change, like a moved number in the bench; the
  ones the bench checks live in `bench/targets.ron`):

  | Thing | Target | ≈ time |
  | --- | --- | --- |
  | A normal fight | 3–8 actions; a dangerous one 10–20 | — |
  | A floor | 150–300 actions | 10–20 min |
  | Rest budget | about 1–1.5 full heals per floor | — |
  | Way-landings | every 3 floors | ~45 min between chances to bank |
  | A dungeon's bottom | 9–12 floors | — |
  | First style principle (level 5) | within 1–2 dives of following the style | ~1 h |
  | First discovery | within the first life's first 2 dives | ≤ 2 h |
  | An advanced style | by the middle of a full life | — |
  | A full life | 8–15 dives | 6–10 h |
  | A typical early death | after 1–3 dives | 1–2 h |
  | First ending | within the first 3 lives | 5–8 h in all |
  | Mastery | one life masters one or two domains, never all | — |
  | Mentors | useful for the next life, faded by the one after | — |

  A life sits between Brogue's hour and Qud's tens: legacy needs many lives, so one can't be huge.
- **The bench measures floors.** A **floor bot** (auto-explore and bump, the naive build from Fights)
  plays real dungeon floors through the real game over many seeds, and reports actions, XP, HP and rest
  budget spent, and deaths per floor. From XP per floor and the skill curve it derives how many floors
  a build takes to reach levels 5, 10 and 15, which checks the style targets without simulating a life.
  Lives, endings and mentors stay design-review targets until legacy exists.
- **Real play calibrates the assumption.** `front` records wall-clock time and actions per life in the
  save, outside `game`, so determinism holds. It only tunes the actions-per-hour figure and is never
  shown as a score.

## Interconnection

The experiment of this game is a system as interconnected as possible. A new system earns its place by
reaching into the others (skills, styles, discovery, lore, phases, legacy), and the others read it
back. A system that only adds content beside the rest is not finished.

- **Bonuses add, in one place, under one cap.** The more systems reach into a number (a phase's lean,
  attunement, a style thriving in its phase, a meal, a relic), the more ways they can multiply into one
  that breaks it. So every bonus to a number is an additive percentage, summed in one place, and each
  kind of number has a cap in data. The balance bench reports when a build reaches a cap.

## Phases

- **A phase leans the whole world one way, for everyone.** Its leans apply to every creature while it
  lasts, you included: an element that is stronger, a rule of the world it bends. It also has natives
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
attunement (one skill per phase), social, and life skills (scavenging, fletching, smithing, tinkering,
alchemy, cooking, scribing).

**Social** skills each pay off in one place: *Persuasion* (whether a parley lands), *Insight* (telling a
truce from a trap), *Deceit* (your own lies and bluffs; a deed if found out), *Haggling* (prices when
paying or trading), *Intimidation* (making those who talk only when losing believe they will).
**Tongues** are lore-like skills, one per language, grown by novelty (hearing it spoken, a first
parley) and lessons (a phrasebook, a captive or settler who teaches). Thresholds turn noise into
understood speech, then parley, then hearing their lies. An origin can start fluent.

Attacks can miss: combat skills raise hit chance as well as damage, and Dodge is a skill. A miss can
still **graze** (half damage, and only effects meant for grazes), so missing is less frustrating and
forgiving weapons graze more. Defenders answer in two ways, each an outcome other things react to:
a **perfect dodge** (the body) means no graze is even calculated (a riposte; an ability that makes every
dodge perfect); a **deflection** (the gear: a buckler, a pauldron) turns a graze aside (a shield bash).

**Only the player learns.** Monsters have fixed skills from their blueprint; they do not live long or act
consistently enough for growth to mean anything.

**How skills grow.** Three sources:

- **Use.** A use counts only if it passes its domain's test: *stakes* (something at risk: your life, a
  truce, a rare material; not merely a hostile nearby),
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

## Crafting and life skills

- **Crafting is options, never upkeep.** Time is neutral: no hunger, no wear clock. Crafting makes
  things you could not otherwise have, from what the dives gave you, and every build may ignore it.
- **Materials have properties** (hard, light, flexible, conductive, cold, holy, rotten, …). Creatures,
  ruins and phases supply them (goblin bone, wraith dust, Rime crystal, Ember slag), so each phase and
  faction feeds crafting through its materials, with no rules of its own.
- **Two kinds of recipe.** Most are **rules over properties** ("a blade from anything hard, a grip from
  anything flexible"), and the product takes the properties of what went in: a Rime-crystal blade deals
  cold, a bone one is light and brittle. One rule times many materials gives variety without a
  catalogue, the way runes compose spells (ADR 0016). Some are **unique, authored recipes** that make
  one specific product.
- **Most crafting is done at camp**, in its places. The basics are yours to build from the first life
  (a fire, a workbench, a cooking pot), so crafting never waits for a retirement; settlers bring the
  specialist places (a forge, an alchemist's table, a library) and the recipes that need them. Only
  trivial or essential crafts can be done during a dive (a field dressing, fletching an arrow). Materials
  are part of what a dive banks by coming home.
- **Growth.** Crafting passes the *consumption* test (it uses up something finite); novelty applies, so
  a new material teaches and the hundredth identical arrow does not; low skill can make a worse product
  (*odds*). Batches are one keystroke, running until done or interrupted.
- **Recipe inspiration comes both ways.** At camp, the stakes of a craft are what it could waste: working
  a rare material can inspire, common ones never do. In a dive, the stakes are the dive's: tasting
  something new under threat, taking apart a weapon wrested from an enemy.
- **Where recipes come from**: discovery (combinations inspire recipes and crafting styles), crafting
  styles inventing at milestones, **taking a thing apart** (a lesson, once per kind: break down a
  goblin's crossbow to learn crossbows), teachers, faction secrets, a predecessor's recipe notes.
- **Each life skill pays off in one place**: scavenging (what bodies, aftermaths and ruins yield),
  fletching (arrows), smithing (weapons, armor, repair; acid makes repair matter), tinkering (devices,
  traps), alchemy (potions, oils, reagents for a reagent discipline), cooking (meals), scribing (scrolls
  and manuals).
- **Cooking** is one of the game's joys, not a survival chore. A meal is composed like any recipe: its
  ingredients' properties decide its effects, which last the dive ahead and end on returning to camp (a
  state bounded by the dive, not a duration in ticks: a new kind of timing to name), so the meal
  before a dive is a choice like the phase and the teller. Phases bring their own ingredients; a dish can
  be discovered and become a signature; a cook's table is a camp place, and feeding people is a deed
  that settlers remember.
- **Scribing lets you write your own legacy.** A scribe can write manuals of their own techniques and
  skills for whoever comes next, still capped as lessons. It balances itself: writing one costs
  resources you would otherwise spend on yourself, and needs real expertise in what it teaches.
- **Things carry their maker.** A crafted item bears a maker's mark; a masterwork survives the gap
  better, and a renowned maker's work becomes a relic ("Ana's blade"). Crafts carry tags for
  perception too: poisons, or bone-craft from the wrong bones, make a reputation.
- **Against tedium**: few kinds of material with many properties, no weight juggling; the recipe list
  shows what you can make now from what you carry; wear only where a phase makes it a choice.

## Legacy

When a life ends, by death or by retiring, a **legacy record** is written: plain data (name, origin,
where and how it ended, best skills, styles, techniques, recipes, notable items, deeds), not the save.
The next character finds what is left, spawned from blueprints through channels that already exist:

- technique books and memoirs are **lessons**, capped at a fraction of the predecessor's level;
- their journal reveals the requirements of the styles they found (the inspiration still needs stakes);
- recipe notes are **recipes**; what they kept at camp is items.

What they carried died with them in the dungeon, unless they died at camp or retired. A grave, an
epitaph or a tale needs someone to make it, so those come only once people live at camp (see "Camp").

How long has passed (the **gap**) is rolled when a character is made (its own RNG stream) and decides
how much survived. Ruins can stack over generations; only the last few are kept. Time between lives is
counted in lives, never in turns: nothing ages while you play.

### Judgement: what a life is remembered as

- **Two axes.** *Renown* is how widely a life is known (unknown → local → known → legend); it decides
  how much survives and for how long. *Repute* is what they are known as (hero, wicked, holy, scholar,
  wild, oathbreaker, …); it decides what kind of thing is left. Not every life leaves a legend or even an
  epitaph.
- **What is left depends on repute.** These are examples, all data:
  - *unknown*: whatever lasted, and no one to say whose it was;
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
- **A retirement bounds the next two gaps.** The two lives after a retirement get short gaps, so you are
  guaranteed to meet the mentor at least twice. After that the gap is rolled as usual. Chaining
  retirements keeps gaps short and the camp in good shape; a death lets time run.
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

## Factions

- **Factions are the groups that hold beliefs.** A faction has beliefs, wants, relations with other
  factions and members. Two kinds:
  - **Dungeon factions**: who lives below (goblins, the restless dead, phase natives). Beasts are no
    faction: territorial, not hostile on principle. Relations between factions are data and replace
    today's two sides (ADR 0015): who fights whom is a lookup, never a check for `Hostile`.
  - **Creeds**: who lives above, carried by settlers (an order of oath-sworn paladins, a persecuted
    circle of hemomancers, scholars through whom understandings arrive, wardens of the Stillness who
    know about Chaos, traders). Creeds wait for settlers, so they arrive with the first retirement.
- **Standing is perception, not a number.** What a faction thinks of you is what it believes you did,
  judged by its own beliefs: the same tale of blood magic shames you with the oath-sworn and raises you
  with the hemomancers. There is no absolute good and evil; "wicked" is always wicked in someone's eyes.
  **Moral weight comes from factions**: killing what was not threatening you, breaking a truce or
  robbing a grave is a deed someone judges.
- **Ways to belong differ by faction**, as data: some you *join* (vows, obligations), some you
  *affiliate* with (favour without vows), some you must be *born* into (only an origin starts you
  inside), and some *adopt* you because of how they perceive you, wanted or not. Their enemies then
  treat you as one of them.
- **Factions reach everything.** Their styles are taught only to those they favour, discoveries can need
  standing, origins can start inside one and heirs inherit part of it, their strength and mood shift with
  the phase, and some world endings run through them. They remember across generations: standing fades
  with the gap, legend does not.

### Situations: the world reads as alive without being simulated

- **The player's view is the reality bubble.** A floor exists only while you are on it, and nothing is
  simulated beyond it.
- **A dungeon places situations, not just creatures**: data saying who is there, how they are arranged
  and what state they are in. Examples: a skirmish between goblins and the dead at a chokepoint, the
  aftermath of one (bodies, a wounded survivor, loot), a camp at rest with captives, a border between
  two territories.
- **A situation's participants are engaged**: they know where their foes are, so they perceive each
  other regardless of the player's view, until the situation ends. This is the one exception to
  perceiving through the player's view (ADR 0007, 0015).
- **The whole scene wakes when the player first perceives any part of it.** No rooms or trigger tiles;
  it works in any generator. If you leave, the fight carries on and you come back to the aftermath it
  really produced. Fighting out of sight is heard ("You hear fighting to the east").
- **Walking away means on the same floor.** A floor you leave is gone (`dive.rs`); a situation you
  never finished is gone with it.

### Talking

- **Parley happens at a distance.** Once you are adjacent, the time for talk has passed. Parley is
  aimed like a throw, at a creature in view within shouting range, and is off with a faction once blood
  is drawn between you on that floor. A creature willing to talk may hail you first: it halts at a
  distance and calls out, and a short chooser opens (parley, pay, refuse). No dialogue trees.
- **Whether a creature talks at all is a condition, not a given**, written in the shared language of
  conditions. Goblins may talk only when they expect to lose or are alone; only the clever ones (a
  shaman) may talk at all; only if you speak their tongue (see "Skills"); only if you wear or hold
  something they recognize; or they may simply hate your kind and never talk. The restless dead never
  do.
- **Talk is not truth.** A creature may lie: a truce can be a trap, a promised passage an ambush. How
  often is part of what its faction is; Insight is how you tell.
- **Every outcome costs something, and no path is the grind path.** Paying costs items; a truce binds
  you for the floor and breaking it is a deed; a failed parley gives them the distance you had; sparing
  gives no loot. Fighting gives loot and martial XP; talk gives information, passage or trade, and as a
  use with stakes it trains the social skills and can inspire (a diplomat's style).

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
- Which deeds carry moral weight first (killing the unthreatening, breaking a truce, robbing a grave).
- What endings are, concretely, and what the first, simple one is.
