# Plan: play-time calibration

Handoff document. Read `docs/vision.md` (Pacing), `docs/plans/floor-bot.md`, `front/CLAUDE.md` and
`gui/CLAUDE.md` first.

## Goal

The pacing targets are counted in keypresses and converted to time at an assumed 1000 an hour
(`bench/targets.ron`). The vision: "`front` records wall-clock time and actions per life in the
save, outside `game`, so determinism holds. It only tunes the actions-per-hour figure and is never
shown as a score." This plan records it and lets the bench read it back.

## Decisions taken for this plan

- **Keypresses are counted by the session**, the same way the floor bot counts them: every input
  that becomes a turn or starts a run is one; a run's steps are not; menus and panels are not.
  `Session::keypresses()`; the count is part of the session, not the game state.
- **Time is the frontend's**: `front` has no clock (its rule). The gui measures the time a game was
  on screen (`Instant`s in `App`, paused while the window is unfocused) and hands the seconds to
  `title::finish` with the session.
- **One line per life end, in a plain file beside the saves**: `<saves dir>/playtime.csv`
  (`character,keypresses,seconds`), appended by `front::saves::record_play` whenever a game is left
  (saved or dead). Not inside the game's save: `game` never sees it, and it survives a death.
- **The bench reads it**: `bench --calibrate FILE` prints keypresses per hour over every line (and
  per character), so the owner can move `actions_per_hour` in `bench/targets.ron` by hand. Nothing
  changes the targets automatically.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Counting and recording — done
- `Session::keypresses()`; `saves::record_play(dir, character, keypresses, seconds)`; `title::finish`
  takes the seconds and records; the gui measures focused time and passes it.
- Tests: a session counts a move, a run start and a cast as one each, a run's steps and a panel as
  none; `record_play` appends lines; `finish` records on save and on death.
- As built: a session counts an input when time passed or a run started. The gui adds up the gaps
  between frames while a game is open, a gap counting at most 30 s (`IDLE`), rather than tracking
  window focus. `record_play` creates the saves directory if needed (the first save of a new
  character makes it only afterwards); a failure to record never costs the save.

### 2. The bench reads it — done
- `bench --calibrate FILE`: keypresses, hours, keypresses per hour, overall and per character;
  `scripts/balance.sh --calibrate [FILE]` defaults to the gui's saves directory.
- Tests: a fixed file gives the expected rate; an empty one says so.
- As built: `bench/src/calibrate.rs`; lines it can't read are counted and skipped. The script's
  default path is the gui's saves directory per platform (`directories`' data dir).

## Out of scope

- Moving `actions_per_hour` automatically; showing time played to the player.

Exempt: measures real play for the bench; builds no game system.
