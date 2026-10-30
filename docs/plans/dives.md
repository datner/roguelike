# Plan: camp and dives

Handoff document. Read `docs/vision.md` ("The shape of the game"), ADR 0005 (RNG streams), ADR 0008
(saves), ADR 0009 (zones and seeds) and `game/CLAUDE.md` first.

## Goal

The game's outer loop: a character starts at a **camp**, chooses a **phase** to wait for, and **dives**
into a themed dungeon, floor after floor, until they come back up to camp or die. Today a game is one
generated cave whose exit leads nowhere.

## Decisions taken for this plan

- **Dungeons reroll every dive** (the vision's first open question). A floor's seed mixes the world seed,
  the dungeon, the phase, the dive number and the floor; dropping the dive number from the mix would make
  each phase keep its layout instead, a one-line change if the answer changes.
- **Only the current place exists.** Leaving a floor forgets it: going down makes a new floor, going up
  ends the dive. Camp is rebuilt from data on every visit; nothing left there persists yet (a stash is a
  follow-up, needed by legacy).
- **What comes along:** the player, everything they hold, their timed effects, and their summons (placed
  next to them on arrival). Everything else on the floor is despawned.
- **Keys:** `>` goes down stairs, `<` goes up (from floor 1 that is back to camp), `W` waits for a phase
  (camp only). All three are free in both key maps.
- **`term` is frozen** (`term/CLAUDE.md`): it gets the `>`/`<` bindings so it stays playable, and the
  phase chooser's one-line fallback. Everything else is `front` + `gui`.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Floors: going down — done
- `Tile::Exit` becomes `Tile::StairsDown` (`>`) and a new `Tile::StairsUp` (`<`) marks where you arrive.
- `GameState.place: Place` with `Place::Floor { dive: u32, floor: u32 }` for now (camp comes in slice
  3); saved, `SAVE_VERSION` bumped.
- `PlayerAction::Descend` → `Command::Descend`: refused off stairs ("There are no stairs here."). Builds
  the next floor in `game/src/dive.rs`: a seed from `zone::mix` over the parts above, the map and spawn
  streams reseeded from it (combat stays on the world seed), the cave generated, arrival on `<` and
  stairs down placed far from it, then `populate`. Keeps what comes along, despawns the rest through
  `despawn_with_holdings`, resets `visible`/`seen`/`remembered_items`, logs "You descend to floor N.".
- Explore treats `>` as its last target, as the exit today, and says "Nothing left to find. `>` goes
  deeper." when standing on it.
- Tests: same seed and inputs give the same floor; the player's items, effects and summons arrive, other
  creatures don't; memory is reset; stale timers of despawned entities no-op; a save mid-dive loads on the
  same floor.

### 2. Dungeons are data — done
- `data/dungeons/<Name>.ron`: a dungeon's name ("the caves"), arrival line ("Goblins prowl these
  caves."), generator (the cave parameters, moving `data/cave.ron` in), and a spawn table of creatures
  (`blueprint`, `count`, `per_floor` extra for each floor down) and floor items (`count`).
- Loaded and checked with the blueprints (unknown names fail at load). `GOBLINS`, `SHAMANS`,
  `FLOOR_ITEMS` and the hardcoded arrival line go; `Place::Floor` gains the dungeon's name.
- Tests: a table spawns what it lists, deeper floors spawn more, a bad name fails at load; the shipped
  caves give today's start on seed 3 (same counts, so seeded tests barely move).

### 3. Camp — done
- `data/camp.ron`: an ASCII layout (walls, floor, the player's start) with entrances, each a tile that
  names a dungeon. No hostiles, no floor items.
- A new game starts at camp (`Place::Camp`). `>` on an entrance starts a dive (dive number + 1, floor 1
  of that dungeon); `<` on floor 1's arrival stairs returns to camp and ends the dive ("You climb back to
  camp.").
- Tests: new game at camp, entrance → floor 1 → camp round trip, dive numbers count up and reroll the
  floor, save and load at camp, practice works at camp.

### 4. Phases — done
- `data/phases.ron`: each phase's name and the line said when you wait for it. A dungeon's spawn entries
  may name the phases they belong to (none = every phase). Two phases to start, using the creatures that
  exist: a quiet phase (goblins) and an ember phase (goblins and shamans, more items).
- `GameState.phase`: chosen at camp, fixed for the dive, part of the floor seed. `Command::WaitForPhase`
  (camp only; elsewhere refused with a reason) costs one action, as pillar 5 asks: time is neutral.
- `front`: `W` opens `Mode::ChoosePhase` (a chooser like the spell list); `gui` draws it with a shot;
  `term` shows the fallback line.
- Tests: the phase filters spawns, is fixed through a dive, is saved; the chooser flow in `front`.

### 5. Knowing where you are — done
- Side panel and status: "Camp", or "The caves, floor 2 (embers)". Log lines on every move between
  places. README Controls (`>`, `<`, `W`) and Structure. Shots for camp and a floor; references updated.

### 6. A second dungeon: the castle — done
- A rooms-and-corridors generator chosen by the dungeon's data (`generator: Rooms(...)` beside
  `Cave(...)`), with its own spawn table; camp gets its entrance. Proves themes are data plus a generator.
- Tests: generator determinism, every room reachable, stairs placed apart.

## Follow-ups (not in this plan)

- A camp stash that persists, and camp ruins: both part of legacy.
- Origins: starting kit and skills chosen at character creation instead of the fixed player blueprint.
- Phase-only creatures and finds (new content per phase).
- Deleting `term` after this plan, if it hasn't been played (ask).

## Connections

*TODO* (interconnection.md, slices 2–3)
