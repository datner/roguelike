# 0006 — Time model: energy scheduling, timer queue, relative durations

Status: Accepted
Date: 2026-09-27

## Context
The game needs time before monsters can fight back or effects can wear off: actors of different speeds taking turns, and effects that last a while, tick periodically, and have durations that other things can later scale or extend. Two common approaches break easily. One is countdowns decremented every turn: they drift, they're awkward to rescale, and changes can double-apply. The other is timers treated as the source of truth ("the effect is active because its expiry hasn't fired"): once a duration changes, the old timer is wrong and has to be found and cancelled.

## Decision
- **Clock.** `GameState.now: Tick(u64)` is the only notion of time. The frontend shows `now / TICKS_PER_TURN` as the turn.
- **Energy scheduling.** Actors carry `Speed(u32)` (blueprint data, 100 = normal) and `Energy(i64)` (runtime, starts at 0). Each tick grants `speed` energy. An actor may act while energy ≥ 0, and acting subtracts the action's cost. A standard action costs `100 × 100`, so a speed-100 actor acts every 100 ticks. Costs live only in `game/src/cost.rs`; `cost::of(&Command)` charges actions (`TryMove`, `Wait`) inside `apply_commands`, so a bump-attack costs one move.
- **Scheduler** (`time::step`/`advance`). It never iterates tick by tick. Each step does one thing: fire the next due timer; else let the ready actor with the lowest `SpawnSeq` act; else jump the clock (via `Command::AdvanceClock`) to the next tick where a timer is due or an actor becomes ready. So at any tick, timers fire before actors. Ties go to `SpawnSeq` (assigned by `GameState::spawn`), never to hecs `Entity` order. It stops and returns to the frontend when the player is ready or the game is over.
- **Timer queue.** `Timers` is a `BTreeMap<(Tick, seq), Timer>`, so timers due at the same tick fire in insertion order. A timer is an idempotent *check*: it becomes a command whose applier re-reads current state and does nothing if its entity is gone or its deadline has moved. Timers are never cancelled or rescheduled to correct their own timing.
- **Relative durations.** Timed state stores `Timed { start, duration }`. The end is always derived (`start + duration`), never stored, and nothing counts down. "Is this effect active?" is always `now < end`, answered from timestamps, never from whether a timer has run.
- **Effects** are entities attached to their target (`Attachments` + `EffectTarget`). `ApplyEffect` spawns one from a blueprint whose `Duration` becomes `Timed { start: now, .. }`.
- **Schedule on change.** Every duration change goes through `ModifyDuration { effect, change }` (`ScaleTotal`/`ScaleRemaining` in basis points, `AddTicks`; integer math, rounding down). Its applier always schedules an `ExpireEffect` at the new end, or expires the effect in the same step if that end is ≤ now. Stale timers for the old end remain queued and fire as no-ops.

## Consequences
- Changing a duration never requires finding the old timer; extending, shortening and rescaling all follow one path, and correctness never depends on timer bookkeeping.
- Timers target a specific `Entity` (index + generation), so a stale timer can't touch a newer effect of the same kind on the same target.
- Periodic effects (bleeding) reschedule themselves only while active. They keep rescheduling even when the next tick falls past the current end, because the effect may be extended before then.
- The queue can accumulate no-op timers after many changes. That's cheap today; revisit if profiling says otherwise.
- Speed 0 actors never become ready again after acting. An actor with no `Speed` isn't scheduled at all.
- The player's action is applied by `take_turn`, which then runs the world. Anything that should happen "between turns" must be a timer or an actor, never frontend code.
- Out of scope for now: activities/interruption, a reality bubble (every actor everywhere is scheduled), and duration-modifier events (resistances).
