# Plan: legacy

Handoff document. Read `docs/vision.md` ("The shape of the game", "Legacy"), `docs/plans/origins.md`
(origins are the head start; legacy reads `Origin`), `docs/plans/dives.md` (camp, its follow-ups),
ADR 0005 (RNG streams), ADR 0008 (saves), ADR 0017 (skills, lessons), ADR 0018 (discovery),
ADR 0025 (origins) and `game/CLAUDE.md` first.

## Goal

Permadeath, with legacy. Today a death leaves the last save on disk and nothing else happens. After
this plan a death ends that character for good and leaves a **legacy record**. The next new character
finds the dead one's camp in ruins: a grave and its note, whatever gear survived, a memoir and a
journal. A memoir's lessons are capped below the predecessor's levels and a journal names the
requirements of the styles they found. So the ruins help a new character, but never make them stronger
than their origin did. How long ago the predecessor died is rolled when the new character is made, and
it decides how much survived.

## Decisions taken for this plan

- **Death is final.** On death the character's save file is deleted and a record is appended to the
  lineage. Today the last save stays and can be continued, which undoes permadeath. `saves.rs`'s rule
  "no save after death" becomes "no save after death, and the old one is removed".
- **A record is plain data, not the save**: `legacy::Record { name, origin, turn, cause, place,
  skills, styles, techniques, discoveries, gear, stash }`. Everything is a name (skill, blueprint,
  style, dungeon, phase), never an id, so content changes don't break a record: a name that no longer
  exists is skipped when the ruins are built, never an error. `skills` holds each skill's level, not
  its XP. `techniques` leaves out abilities, which come from what a creature is and can't be passed
  on. `gear` is what the character wore and held.
- **One lineage per saves directory.** `<saves dir>/legacy.ron` holds the records, newest first,
  trimmed to `keep` (data) whenever one is added. Every new character in that directory finds the
  ruins of the last `keep` dead. Living characters don't affect each other. If two are alive at once,
  each found the ruins that existed when it was made.
- **Records are an input to a new game, like the seed and the origin.** `GameState::new_as` takes
  `&[Record]`. Ruins are built once, at creation, from a new `rng::stream::LEGACY` stream seeded from
  the world seed. After that they are ordinary entities in camp and are saved like anything else, and
  the lineage file is never read again for that character. The same seed, origin and records give the
  same game.
- **Time passed is rolled per set of ruins**, at creation: the newest ruins get one roll from the gap
  table, and each older set gets its newer neighbour's gap plus another roll. So older ruins are never
  better kept than newer ones, and no file is changed when a character is made.
- **How much a legacy is worth is data**: `data/legacy.ron` sets `keep` and a weighted gap table. Each
  gap entry has `name` (the words the note uses: "a season", "years", "an age"), `weight`,
  `survives` (the % chance each piece of gear lasts), `lesson` (the % of the predecessor's level that a
  memoir can teach) and `journal` (whether it can still be read). The first numbers are a guess (keep
  3; lesson 25–50%); tuning them is the vision's open question and comes after playing.
- **The camp keeps what's left in it.** Legacy needs a camp stash and nothing persists in camp today.
  Items dropped on the camp floor are still there when you come back from a dive. Their stack is
  `stash` in the record, and in ruins it survives like gear.
- **Ruins are spawned from blueprints through channels that already exist:**
  - **Grave** (`Grave.ron`): an impassable feature with a `Note` part, built from the record ("Here
    lies Ana, a pit fighter, slain by a goblin on the third floor of the Caves, in the Embers, years
    ago."). Looking at it reads the note.
  - **Memoir** (`Memoir.ron` plus a `Memoir` part filled from the record): studied with the existing
    `Study`, once per memoir (the kind is "Ana's memoir"). It raises each recorded skill to at most
    `lesson`% of the predecessor's level, through `skill::gain`, and never lowers one. It also teaches
    the recorded techniques. Because it is a lesson, it has no stakes and gives no inspiration
    (ADR 0017).
  - **Journal** (`Journal.ron` plus a `Reveals` part): studying it reveals the recorded discoveries.
    `Revealed(names)` on the player makes `discovery::ideas` list each one with all of its
    requirements, met or not. The inspiration still needs stakes. If the gap makes the journal
    unreadable, it spawns as a ruined page that teaches nothing.
  - **Gear and stash**: each surviving piece is spawned by blueprint name near the grave.
- **Where ruins go is camp data**: `camp.ron` marks ruin spots, one per kept record, newest nearest the
  fire. A camp with no records is the camp of today.
- **Cause of death is recorded where it happens**: `Command::Die` gains `cause: Cause` (`By(Entity)`,
  meaning a killer whose name is read at death, or `Of(String)`, for "burned" or "bled out"). Every
  path that pushes `Die` says why.
- New ADR **0026 — Legacy**, written in slice 1 and amended in each slice.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every new
component: blueprint `ComponentDef` where content sets it, `SavedComponent` + round-trip test,
`SAVE_VERSION` bump.

### 1. The record
- `Command::Die { entity, cause }`; every producer passes its cause. `die` keeps it on the player
  (`DiedOf(Cause)`, with the killer's name resolved at that moment) when the game ends.
- `game/src/legacy.rs` (new): `Record`, `legacy::record(&GameState) -> Option<Record>` (`Some` only
  after death), and `to_ron`/`from_ron` for a list of records with its own `version`. A file of
  another version is ignored with a message and never overwritten until the next death replaces it.
- `front/saves.rs`: `bury(dir, character, record)` deletes the save and appends the record to
  `legacy.ron` (written atomically, trimmed to `keep`). `title::finish` calls it after a death and
  returns "Ana died on turn 812." as it does today.
- ADR 0026.
- Tests: a death's record (cause, place, top skills, no abilities, gear); a record round-trips; bury
  removes the save and trims the lineage; an outdated lineage is left alone; a living quit saves as
  before.

### 2. The camp keeps what's left in it
- Leaving camp moves its floor items, with their positions, into a camp store. This is an inert holder
  that crosses floors the way the player's things do, so it is saved with the game. Coming back puts
  them where they were. Nothing hostile is ever stored.
- `Record.stash` is read from the store, plus what is on the camp floor if the player dies in camp.
- Tests: drop at camp, dive, return, it's there; a save round-trip in between keeps it; the record
  lists the stash.

### 3. Ruins
- `data/legacy.ron` (`keep`, gap table), loaded and checked with the blueprints (weights > 0,
  percentages ≤ 100). `rng::stream::LEGACY` with an `Rngs` field.
- `camp.ron` gets ruin spots; load fails if it has fewer than `keep`.
- `GameState::new_as(seed, blueprints, origin, records)`. The current callers pass `&[]`. Building the
  ruins rolls the gaps, then for each record spawns a grave with its `Note`, then the surviving gear
  and stash, from that record's spot. The order of draws is fixed: gap first, then gear in record
  order.
- `look`/`inspect` shows a `Note`'s text.
- `title::start` reads the lineage and passes it in.
- Tests: no records give today's camp; the same seed and records give the same ruins; a longer gap
  never keeps more than a shorter one; unknown names are skipped; the note's wording; two records
  stack with the older one more decayed.

### 4. Memoirs and journals
- `Memoir { skills: Vec<(SkillId, u32 cap)>, teaches }` (saved by name). `Study` of a memoir raises
  each skill to its cap through `skill::gain` (`skill::xp_for(cap)` minus what is already there,
  never negative) and teaches its techniques with the existing `learn`.
- `Reveals(Vec<String>)` on a journal; studying it adds to `Revealed` on the learner;
  `discovery::ideas` lists revealed discoveries with their full requirements, marked as revealed.
- Ruins spawn one memoir and one journal per record (or a ruined page when `journal` is false).
- Tests: the cap is a % of the recorded level; a skill already above the cap is untouched; studied once
  only; a revealed idea lists its requirements before any are met; studying gives no inspiration.

### 5. Seeing it
- Death: the status line and a death panel show the record's summary from a `game` view
  (`legacy::summary`), built in `front/text.rs`, so the session and the menu say the same thing.
- Title menu: a **Fallen** list (newest first, read-only) with each record's summary, from
  `title::lineage`.
- gui: draws the death panel and the Fallen list; `death` and `fallen` shots, plus `ruins` (a camp
  with two records).
- README Controls and Structure.
- Tests: a `Session` death shows the summary and any key leaves; a `Menu` flow into Fallen and back;
  the shots.

## Out of scope

- Family trees, inheritance between relatives, or choosing whose ruins to visit.
- Tuning a legacy's worth beyond the first numbers (vision's open question).
- Recipes and recipe notes: there are no recipes yet. The record leaves room for them by name.
- Ruins in dungeons (a predecessor's body where they fell).
- A legacy bench scenario.
