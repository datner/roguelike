# Plan: rest

Handoff document. Read `docs/vision.md` (Fights: attrition; Pacing), ADR 0006 (time), ADR 0008
(saves), `docs/plans/floor-bot.md` and `game/CLAUDE.md` first.

## Goal

The vision: "HP regenerates slowly, from a **rest budget per floor**: waiting is one keystroke and
careful play can heal between fights, but only so much per floor, so a floor is a budget." Today
health never comes back, and the floor bot shows it: every melee start dies by floor 2. This plan
adds the regeneration, the budget, one key to rest, and the bot resting the way a person would.

## Decisions taken for this plan

- **Regeneration is a part with a periodic tick, like bleeding.** `Regeneration { interval, per_floor }`
  (data, on `Player`): every `interval` ticks a `Timer::Regenerate(entity)` fires; its applier heals
  one point if the holder is hurt and its budget has any left, takes one from the budget, and always
  schedules the next tick. The applier re-checks everything when it fires (ADR 0006: timers are
  idempotent checks), so a dead or despawned holder simply stops. Health stays a stored value: every
  reader of `Health` keeps working.
- **The budget is a count, not a clock.** `RestBudget { left }` is points of health still to regain on
  this floor. Entering a floor sets it to `per_floor` percent of max health (125: about one and a
  quarter full heals). Nothing counts down with time; only healing spends it.
- **Camp heals.** Arriving at camp (a new game, or climbing back) restores full health: camp is where
  a dive banks, and nothing hostile is there. The budget matters on floors only.
- **Slow on purpose**: `interval: 300` (a point every three standard turns, 20 HP in 60 turns), too
  slow to matter inside a fight, fast enough that one rest between fights does it.
- **Resting is one key**, `R`, an auto-run like practice: wait a turn at a time until health is full,
  the budget is spent, or an enemy comes into view. `game::rest_step` (beside `practice_step`) is the
  rule; `front`'s session runs it; the refusals say why ("You are not hurt.", "You can rest no more on
  this floor.", "The goblin is in view.").
- **Legible**: the status line shows the budget left on a floor ("Rest 18"), per the vision's
  Legibility table.
- **The floor bot rests**: with no enemy in view and health below half, it rests (one keypress) before
  exploring on. That is what a careful person does, and it is what the budget is for.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Regeneration and the budget — done
- `entity.rs`: `Regeneration`, `RestBudget`; blueprint `ComponentDef` for `Regeneration`; save kinds for
  both, `Timer::Regenerate` saved; `SAVE_VERSION` bumped.
- `game/src/rest.rs` (new): the tick's applier (`Command::Regenerate`), scheduling a holder's first
  tick when it is spawned, refilling the budget on entering a floor (`dive::enter`), healing in full at
  camp.
- `data/entities/Player.ron`: `Regeneration(interval: 300, per_floor: 125)`.
- Tests: a hurt holder heals a point per interval; never above max; stops when the budget is spent;
  a new floor refills it; camp heals in full; a dead holder's tick does nothing and is not
  rescheduled; a save round-trips the budget and the pending tick.
- As built: `rest::start` runs from `GameState::spawn` (first tick and a full budget); `rest::arrive`
  from `dive::enter`. Saves v46. Bench: with regeneration alone the level-10 builds now reach floor 6
  (15/20 and 19/20, from 0); the starting melee builds still die early, since the bot never rests.

### 2. One key to rest — done
- `game::rest_step` and `rest_refusal`; `front` `Auto::Rest` on `InputEvent::Rest`; `gui` binds `R`.
- The status line's rest budget (`view` exposes it; `Session::status` or the sheet shows it).
- README Controls.
- Tests: a `Session` rest runs until full and stops; refused when unhurt, when the budget is spent and
  when an enemy is in view, each with its message; the status line shows the budget.
- As built: `rest_step` and `rest_refusal` live in `rest.rs` (re-exported); "You are rested." ends a
  run as well as refusing one. The status line reads "… floor 1 (stillness)  Rest: 25  Turn: …" on a
  floor and leaves it out at camp. The session tests cover the refusal and the status line; the run
  to full health is tested in `game` (a session can't hurt its player).

### 3. The bot rests
- `balance::floors`: rest when no enemy is in view and health is below half, counted as one keypress.
- Bench report regenerated; this plan's as-built notes say what moved.
- Tests: a hurt bot with no enemy in view rests before exploring; a bot with no budget left explores on.

## Out of scope

- Healing items, potions, a healer at camp, regeneration for creatures other than the player.
- Way-landings and the bottom (their own plan); roamers that make resting risky (the fights plan).
- Tuning beyond the first numbers: the bench reports, the owner decides.
