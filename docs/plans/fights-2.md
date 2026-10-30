# Plan: fights 2 — a warning, rubble and deep water

Handoff document. Read `docs/vision.md` (Fights, Legibility), `docs/plans/fights.md` (what this builds
on: telegraphs, chasms, tall grass), ADR 0002 (commands), ADR 0006 (time) and `game/CLAUDE.md` first.

## Goal

The vision's Fights section lists what `fights.md` left: "deep water, rubble, darkness as a trade,
warnings before stepping into a telegraphed tile, and a smarter floor bot". This plan builds the three
cheap ones, each reusing what exists: the telegraph's marked tiles, the chasm's shove-to-kill path and
the terrain patches `dive::build` already digs. Darkness (it needs non-uniform light in FOV, ADR 0007)
and the smarter bot are their own plans.

## Decisions taken for this plan

- **The warning is a keyboard flow, the danger is the game's.** `game::step_warning(state, dir)` says
  whether the player's step in `dir` would end on a tile a wind-up in view will strike (a bump at a
  creature is no step, so never warned). `front::Session` refuses the first such step with the warning
  logged ("A blow is about to land there. Step again to go anyway.") and lets the same step through on
  the next key; any other key forgets it. No time passes on the refused key. Runs and travel already
  stop for threats, so only the keyboard step is warned.
- **Rubble slows.** `Tile::Rubble` (`,`) is walked, seen and shot across, but a step onto it costs
  double (`cost::RUBBLE_STEP`). It's the destination that counts: a bump at a creature standing in
  rubble is an ordinary attack. Sprinting halves it as it halves any step. Creatures pay it too, so it
  slows a chase both ways: a retreat across rubble gives ground, and a foe crossing it gives you time.
- **Deep water drowns what is forced into it.** `Tile::DeepWater` (`~`) is seen and shot across but
  never walked ("You stop at the water's edge."). A shove that would carry a creature into it sinks it
  (`Command::Sink`): a creature drowns, which is a kill like the chasm's. The player hauls out where
  they went in and loses a turn (`cost::FLOUNDER`), so being shoved in costs tempo, not a floor:
  that's what separates water from a chasm. Chasms and deep water are both "drops" to a shove
  (`Tile::is_drop`).
- **Generated like the others.** `Dungeon.water` and `Dungeon.rubble` count patches, dug in
  `dive::build` on the map stream: water after the chasms, with the same keep-clear and
  stairs-reachable rules (a pool that cuts the stairs off is filled back in), on any floor including
  the bottom; rubble after the grass, on plain floor only (it is walked, so it cuts nothing off). A
  dungeon with neither draws nothing new. The caves get water and rubble, the castle rubble (ruined
  halls).
- **Tiles and sprites.** Glyphs in `state.rs`; `tiles.ron` may name `water` and `rubble` sprites like
  the other terrain. Saves store tiles by name, so adding variants is no format change: no
  `SAVE_VERSION` bump.
- **The bench reports, the owner tunes.** The slices that change the dungeons rerun
  `scripts/balance.sh` and note what moved.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. A warning before a telegraphed tile — done
- `game::step_warning` (`technique.rs` beside `telegraphed`, re-exported from `lib.rs`); `Session`
  holds the warned direction.
- Tests (game): a step onto a marked tile is warned; a step elsewhere, a marked tile out of view and
  a bump at a creature on a marked tile are not. Tests (front): the first step is refused with the
  warning and no turn passes; the same step again goes; another key in between resets it.
- As built: the game tests as planned. The front flow (`Session.warned`, taken at the start of each
  `handle`, so any key forgets it) is not tested in `front`: a session can't place a brute, as with
  the outnumbered status in `fights.md`.

### 2. Rubble
- `Tile::Rubble`, `is_open`; the doubled step cost in `apply_commands`; glyph, sprite name;
  `Dungeon.rubble` and `grow_rubble`; caves 4, castle 5.
- Tests: a step onto rubble costs double, a sprinting one the normal step; a bump at a creature on
  rubble costs one move; a goblin pays it too; rubble is seen and shot across; a dungeon without it
  draws nothing new; same seed, same rubble.
- Bench rerun.

### 3. Deep water
- `Tile::DeepWater`, `is_clear`, `Tile::is_drop`; `shove` ends in `Fall` or `Sink` by the tile;
  `Command::Sink`; the edge message; glyph, sprite name; `Dungeon.water` and the generalised digging;
  caves 3.
- Tests: a shoved goblin drowns; the shoved player stays on the bank, logged, and pays a turn;
  walking in is refused with the message; water is seen and shot across; the stairs stay reachable;
  a dungeon without it draws nothing new.
- Bench rerun.

### 4. The vision
- `docs/vision.md` Fights: mark the three as built; README Structure/Controls if they changed.

## Out of scope

- Darkness as a trade (FOV and light), a smarter floor bot, swimming creatures, wet gear, burning
  rubble or boiling water.
- A warning for the AI: creatures step where they like.
- Tuning: the bench reports, the owner decides.
