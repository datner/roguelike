# Plan: legacy

Handoff document. Read `docs/vision.md` ("The shape of the game", "Runs and endings", "Legacy",
"Camp"), `docs/plans/origins.md` (origins are the head start; legacy reads `Origin`),
`docs/plans/dives.md` (camp, its follow-ups), ADR 0005 (RNG streams), ADR 0008 (saves), ADR 0017
(skills, lessons), ADR 0018 (discovery), ADR 0025 (origins) and `game/CLAUDE.md` first.

## Goal

Permadeath, with legacy: the first stage, **the camp that is yours alone**. Today a death leaves the
last save on disk and nothing else happens. After this plan a death ends that character for good and
leaves a **legacy record**. The next character finds the dead one's camp in ruins: what they left in
the stash, a memoir and a journal. A memoir's lessons are capped below the predecessor's levels, and a
journal names the requirements of the styles they found. So the ruins help a new character, but never
make them stronger than their origin did. How long ago the predecessor died is rolled when the new
character is made, and it decides how much survived.

The vision's later stages (judgement, perception and tellers, retiring and mentors, heirs, settlers and
places) all need the first retirement, so they are out of scope here. This plan must not close doors
on them; the decisions below say where each one will attach.

## Decisions taken for this plan

- **Death is final.** On death the character's save file is deleted and a record is appended to the
  lineage. Today the last save stays and can be continued, which undoes permadeath. `saves.rs`'s rule
  "no save after death" becomes "no save after death, and the old one is removed".
- **Deleting a living character counts as a death.** It writes a record (cause: abandoned) like any
  death, so the ruins and gap rolled at its creation stand. Making and deleting characters can't
  reroll the ruins.
- **A record is plain data, not the save**: `legacy::Record { name, origin, turn, ending, place, skills,
  styles, techniques, discoveries, gear, stash }`. Everything is a name (skill, blueprint, style,
  dungeon, phase), never an id, so content changes don't break a record: a name that no longer exists
  is skipped when the ruins are built, never an error. `skills` holds each skill's level, not its XP.
  `techniques` leaves out abilities, which come from what a creature is and can't be passed on.
- **A life ends in an `Ending`, not only a death**: `Ending::Died(Cause)` is the only variant this plan
  builds. Retiring will be another variant, so nothing may assume a record means a death; text says
  "how it ended". Deeds and judgement will be new record fields (records are versioned), not a
  reworking of these.
- **Nobody judges or buries anyone yet.** No one lives at camp before the first retirement, so there is
  no grave, no epitaph and no tale. The dead are unknown. What they left is their own: the camp they
  kept, their writings, and whatever was in it. Their **gear died with them in the dungeon** and is
  recorded but not spawned (a later remains or revenant may use it). If they died in camp it is with
  the rest.
- **One lineage per saves directory.** `<saves dir>/legacy.ron` holds the records, newest first,
  trimmed to `keep` (data) whenever one is added. Every new character in that directory finds the
  ruins of the last `keep` lives. **One living character per lineage**: a new character can be made
  only once the last one has died (later, retired), so ruins, gaps and mentors always follow one line.
  Another lineage is another saves directory.
- **Records are an input to a new game, like the seed and the origin.** `GameState::new_as` takes
  `&[Record]`. Ruins are built once, at creation, from a new `rng::stream::LEGACY` stream seeded from
  the world seed. After that they are ordinary entities in camp and are saved like anything else, and
  the lineage file is never read again for that character. The same seed, origin and records give the
  same game. Later lineage state (understandings, recognized origins, places) joins the records as the
  same kind of input.
- **The gap is counted in lives and rolled per set of ruins**, at creation: the newest ruins get one
  roll from the gap table, and each older set gets its newer neighbour's gap plus another roll. So older
  ruins are never better kept than newer ones, no file is changed when a character is made, and nothing
  ages during play.
- **How much a legacy is worth is data**: `data/legacy.ron` sets `keep` and a weighted gap table. Each
  gap entry has `name` (the words used for it: "a season", "years", "an age"), `weight`, `survives`
  (the % chance each stashed item lasts), `lesson` (the % of the predecessor's level that a memoir can
  teach) and `journal` (whether it can still be read). The first numbers are a guess (keep 3; lesson
  25–50%); tuning them is the vision's open question and comes after playing. Renown will later scale
  `survives` and how long ruins last; the table is where it will hook in.
- **The camp keeps what's left in it.** Legacy needs a camp stash and nothing persists in camp today.
  Items dropped on the camp floor are still there when you come back from a dive. They are `stash` in
  the record, and in ruins each item survives by the gap's `survives`.
- **Ruins are spawned from blueprints through channels that already exist:**
  - **Abandoned camp** (`AbandonedCamp.ron`): a ruined tent feature at the ruin spot, with a `Note` part
    giving only what can be seen ("An abandoned camp, left years ago."). Whose it was is learned by
    reading what is in it, not given.
  - **Memoir** (`Memoir.ron` plus a `Memoir` part filled from the record): studied with the existing
    `Study`, once per memoir (the kind is "Ana's memoir"). It raises each recorded skill to at most
    `lesson`% of the predecessor's level, through `skill::gain`, and never lowers one. It also teaches
    the recorded techniques. Studying it tells who they were and how they ended. Because it is a
    lesson, it has no stakes and gives no inspiration (ADR 0017).
  - **Journal** (`Journal.ron` plus a `Reveals` part): studying it reveals the recorded discoveries.
    `Revealed(names)` on the player makes `discovery::ideas` list each one with all of its
    requirements, met or not. The inspiration still needs stakes. If the gap makes the journal
    unreadable, it spawns as a ruined page that teaches nothing.
  - **Stash**: each surviving item is spawned by blueprint name around the ruin spot.
- **Where ruins go is camp data**: `camp.ron` marks ruin spots, one per kept record, newest nearest the
  fire. A camp with no records is the camp of today.
- **Cause of death is recorded where it happens**: `Command::Die` gains `cause: Cause` (`By(Entity)`,
  meaning a killer whose name is read at death, or `Of(String)`, for "burned" or "bled out"). Every
  path that pushes `Die` says why. Later, when tales are told, this is the fact a teller reads.
- New ADR **0026 — Legacy**, written in slice 1 and amended in each slice.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every new
component: blueprint `ComponentDef` where content sets it, `SavedComponent` + round-trip test,
`SAVE_VERSION` bump.

### 1. The record — done
- As built: no `Command::Die { cause }`. The death chain (`state.death`, legibility.md) already
  names the killer at the moment of death, so `Ending::Died { by }` takes it, and `Ending::Abandoned`
  covers the menu's Delete; `legacy::record(state, name)` always builds a record (the name is the
  frontend's), abandoned unless the life ended in death. Every spawn from a blueprint now carries
  `Made(name)` so gear, styles and techniques are recorded by blueprint name (save version 58).
  `KEEP` is a constant (3) until slice 3's `data/legacy.ron`. `saves::lineage` reads the file;
  `title::perform` takes the `Setup` so Delete can load the save it abandons. The name `legacy` is
  reserved. Gear lists what was worn and carried, not what a container holds.
- `Command::Die { entity, cause }`; every producer passes its cause. `die` keeps it on the player
  (`DiedOf(Cause)`, with the killer's name resolved at that moment) when the game ends.
- `game/src/legacy.rs` (new): `Record`, `Ending`, `legacy::record(&GameState) -> Option<Record>`
  (`Some` only once the life has ended), and `to_ron`/`from_ron` for a list of records with its own
  `version`. A file of another version is ignored with a message and never overwritten until the next
  ending replaces it.
- `front/saves.rs`: `bury(dir, character, record)` deletes the save and appends the record to
  `legacy.ron` (written atomically, trimmed to `keep`). `title::finish` calls it after a death and
  returns "Ana died on turn 812." as it does today. The menu's Delete on a living character buries it
  too, with an abandoned record.
- ADR 0026.
- Tests: a death's record (cause, place, top skills, no abilities, gear); a record round-trips; bury
  removes the save and trims the lineage; an outdated lineage is left alone; a living quit saves as
  before.

### 2. The camp keeps what's left in it — done
- As built: no holder entity. Leaving camp swaps each floor item's `Position` for `Stashed(Position)`;
  `dive::enter` only forgets what has a `Position`, and nothing on a floor queries an item without
  one, so stashed items are inert and saved as they are. Arriving at camp swaps them back. Save
  version 59.
- Leaving camp moves its floor items, with their positions, into a camp store. This is an inert holder
  that crosses floors the way the player's things do, so it is saved with the game. Coming back puts
  them where they were. Nothing hostile is ever stored.
- `Record.stash` is read from the store, plus what is on the camp floor if the player dies in camp.
- Tests: drop at camp, dive, return, it's there; a save round-trip in between keeps it; the record
  lists the stash.

### 3. Ruins — done
- As built: `GameState::new_in_lineage(seed, blueprints, origin, records)` beside `new_as` (which
  passes `&[]`), so the existing callers are untouched. The legacy stream is a local `Pcg64` on
  `rng::stream::LEGACY`, built once in `legacy::build_ruins`, not an `Rngs` field: ruins are drawn
  once, at creation. Ruin spots are `x` in `camp.ron`, ordered nearest the arrival. A gap rolls an
  index into the table; an older record adds its roll to its newer neighbour's, capped at the last.
  Surviving items go on the floor tiles around the spot, in a fixed order. `Note(text)` is the
  ruin's language part; `inspect` reads it on anything that is not a creature. Features draw between
  items and creatures. `bury` takes the `GameState` (the record and `keep` both come from it); `KEEP`
  is gone. Save version 60.
- `data/legacy.ron` (`keep`, gap table), loaded and checked with the blueprints (weights > 0,
  percentages ≤ 100). `rng::stream::LEGACY` with an `Rngs` field.
- `camp.ron` gets ruin spots; load fails if it has fewer than `keep`.
- `GameState::new_as(seed, blueprints, origin, records)`. The current callers pass `&[]`. Building the
  ruins rolls the gaps, then for each record spawns the abandoned camp with its `Note` at that record's
  spot, then the surviving stash around it. The order of draws is fixed: gap first, then stash items
  in record order.
- `look`/`inspect` shows a `Note`'s text.
- `title::start` reads the lineage and passes it in.
- Tests: no records give today's camp; the same seed and records give the same ruins; a longer gap
  never keeps more than a shorter one; unknown names are skipped; gear is not spawned unless the death
  was in camp; two records stack with the older one more decayed.

### 4. Memoirs and journals — done
- As built: `Memoir(Vec<(String, u32)>)` holds caps by skill *name* (plain saved, like the record;
  an unknown name is skipped at study), and techniques reuse `Teaches`. The story is the memoir's
  `Note`, logged by `study` after "You study…" (any studied thing's note is). Named "memoir of Ana"
  and "journal of Ana" so `Subject` reads "the memoir of Ana". `IdeaView.missing` names the unmet
  requirements of a revealed discovery (the panel shows "Footwork: needs Dodge 4, Poise 2"). The
  memoir and the journal (or a `RuinedPage`) lie first around the ruin spot, before the stash; they
  draw nothing. Save version 61.
- `Memoir { skills: Vec<(SkillId, u32 cap)>, teaches, story }` (saved by name; `story` in its own
  language part per "one concern per part"). `Study` of a memoir raises each skill to its cap through
  `skill::gain` (`skill::xp_for(cap)` minus what is already there, never negative), teaches its
  techniques with the existing `learn`, and logs who they were and how they ended.
- `Reveals(Vec<String>)` on a journal; studying it adds to `Revealed` on the learner;
  `discovery::ideas` lists revealed discoveries with their full requirements, marked as revealed.
- Ruins spawn one memoir and one journal per record (or a ruined page when `journal` is false).
- Tests: the cap is a % of the recorded level; a skill already above the cap is untouched; studied once
  only; a revealed idea lists its requirements before any are met; studying gives no inspiration.

### 5. Seeing it
- Death: the status line and a death panel show the record's summary from a `game` view
  (`legacy::summary`), built in `front/text.rs`, so the session and the menu say the same thing. This is
  what the *player* sees; within the world no one tells it yet.
- Title menu: a **Fallen** list (newest first, read-only) with each record's summary, from
  `title::lineage`. **New game** is offered only when no character is alive; otherwise the menu offers
  Continue.
- gui: draws the death panel and the Fallen list; `death` and `fallen` shots, plus `ruins` (a camp
  with two records).
- README Controls and Structure.
- Tests: a `Session` death shows the summary and any key leaves; a `Menu` flow into Fallen and back;
  the shots.

## Out of scope

- Everything that needs the first retirement: endings and retiring, judgement (renown, repute, deeds,
  beliefs, tellers), mentors, heirs and recognized origins, settlers and places, the "Since Ana's
  time…" summary. Each gets its own plan.
- Graves and epitaphs: they need someone to bury and remember the dead.
- Remains or revenants in dungeons (a predecessor's gear where they fell).
- Recipes and recipe notes: there are no recipes yet. The record leaves room for them by name.
- Tuning a legacy's worth beyond the first numbers (vision's open question).
- A legacy bench scenario.

## Connections

**Reads**
- origins: `Record.origin` names the origin the life began as *(planned: slice 1)*.
- skills: `Record.skills` holds each skill's level, the memoir's cap *(planned: slice 1)*.
- techniques: `Record.techniques`, abilities left out *(planned: slice 1)*.
- discovery: `Record.discoveries` from `Discovered`, a journal's contents *(planned: slice 1)*.
- dives: the camp store becomes `Record.stash`; `Record.place` names the dungeon, floor and phase
  *(planned: slices 1, 2)*.
- combat: `Command::Die`'s `cause` becomes the record's ending *(planned: slice 1)*.

**Read by**
- dives: `GameState::new_as` builds ruins at camp's ruin spots from the records *(planned: slice 3)*.
- training: a memoir is a `Study` lesson raising skills to a cap through `skill::gain`
  *(planned: slice 4)*.
- techniques: a memoir teaches its recorded techniques through `learn` *(planned: slice 4)*.
- discovery: `Revealed` makes `discovery::ideas` list a journal's discoveries with their requirements
  *(planned: slice 4)*.
