# Plan: depth (way-landings and a bottom)

Summary: Way-landings back to camp every few floors, and a bottom to each dungeon.

Read first: `docs/vision.md` (Runs and endings; Pacing), `docs/plans/dives.md`, ADR 0009
(zones) and `game/CLAUDE.md` (`dive.rs`, `dungeon.rs`, `zone.rs`).

## Goal

The vision: "Returning costs commitment. A dungeon's data says which floors have a **way-landing**, a
way back to camp (say every third floor and the bottom). Between landings the only way is down, so
each landing is a point to bank or push on." And: "Every dungeon has a bottom." Today every floor has
stairs up to camp where you arrive, and no dungeon ends. This plan gives each dungeon a depth and its
landings, and makes both visible.

## Decisions

- **Data**: a dungeon gains `floors` (its bottom: Caves 9, Castle 12, inside the Pacing target of
  9–12) and `landing_every` (3). A floor is a **landing** when its number is a multiple of
  `landing_every`, or it is the bottom. Checked at load: both at least 1.
- **The way back is where you arrive**: on a landing, the arrival tile is stairs up to camp, as today;
  elsewhere it is plain floor, so the only way is down. Floor 1 is not a landing (with
  `landing_every: 3`): a dive commits you to three floors. That is the vision's "returning costs
  commitment", and a recall (crafted or bought) is the planned way out between landings.
- **The bottom has no stairs down.** Its arrival line adds "This is the bottom of the caves." What
  waits there (and how it differs by phase) is the endings' plan, not this one. `place_stairs_down`
  draws no RNG, so leaving it out changes no seeded layout above the bottom.
- **Legible**: the status line reads "The caves, floor 2 of 9 (stillness)"; the side panel says where
  the next way back is ("Way back: floor 3", or "Way back: here, on the stairs up").
- **The floor bot** ends a dive that reaches the bottom as `Bottom` (explored, no stairs to take),
  not as stuck.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Depth in the data and the floors — done
- `dungeon.rs`: `floors`, `landing_every`, `Dungeon::is_landing(floor)`, `is_bottom(floor)`; checks.
- `dive.rs`: `build` takes the dungeon's depth; stairs up only on landings, no stairs down on the bottom;
  the bottom's arrival line.
- `data/dungeons/Caves.ron` (`floors: 9`), `Castle.ron` (`floors: 12`), `landing_every: 3` in both.
- Tests: floors 1 and 2 have no stairs up, floor 3 does; the bottom has stairs up and none down; going
  down from the bottom is refused; a dungeon with `floors: 0` fails to load; layouts above the bottom
  are unchanged by the depth (same seed, same map but for the stairs up).
- As built: `dive::build` takes the `Dungeon`. Arriving on a landing logs "Stairs lead back up to
  camp from here."; the bottom "This is the bottom of the caves.". The test dungeon makes every floor
  a landing so the older camp tests still climb back from floor 1; the depth tests use their own. The
  bench did not move.

### 2. Seeing it — done
- `PlaceView::Floor` gains `of` (the bottom) and `way_back` (the next landing's floor, or this one);
  `front::text::place` and `whereabouts` say them.
- Shots: `floor` and `castle` updated; README (Structure: dungeons have a bottom and landings;
  Controls: `<` only on a landing).
- Tests: the status line and the side panel on a landing and between landings.
- As built: the side panel's last row names the keys that work there ("> deeper", "> deeper  < back
  to camp", "The bottom  < back to camp").

### 3. The bot at the bottom — done
- `balance::End::Bottom`; the bench reports it beside depth, died and stuck.
- Tests: a bot dropped on the bottom floor with nothing left ends `Bottom`.
- As built: the report says "n explored the bottom" only when some did; the JSON row gains `bottom`.
  No dive asks for more floors than its dungeon has yet, so the report did not move but for that key.

## Out of scope

- What lies at the bottom, bosses, endings, and the bottom differing by phase (the endings' plan).
- Recall items (crafting, traders).
- The floor bot banking at landings (it plays one dive down).

Exempt: extends dives; its connections are listed in dives.md.
