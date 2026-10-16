//! Timed effects (ADR 0006): entities attached to a target, carrying
//! `Timed { start, duration }`. Whether an effect is active is always read
//! from those timestamps; timers only perform end-of-effect side effects and
//! periodic ticks, and each one re-checks the timestamps before acting.

use hecs::Entity;

use crate::blueprint::BlueprintId;
use crate::command::{Command, DamageCause};
use crate::entity::{Bleeding, Duration, EffectTarget, Name, Timed};
use crate::grammar::Subject;
use crate::holding;
use crate::state::GameState;
use crate::time::{Tick, Timer};

/// 10 000 basis points = 100%.
pub const BASIS_POINTS: u64 = 10_000;

/// A change to an effect's duration. Integer math, rounding down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationChange {
    /// Scale the whole duration, measured from the start.
    ScaleTotal(u32),
    /// Scale only what is left at the current tick.
    ScaleRemaining(u32),
    /// Lengthen (or, negative, shorten) by a number of ticks; floors at 0.
    AddTicks(i64),
}

impl DurationChange {
    /// The new total duration of `timed` when this change is applied at `now`.
    pub fn apply(self, timed: Timed, now: Tick) -> u64 {
        match self {
            DurationChange::ScaleTotal(bp) => scale(timed.duration, bp),
            DurationChange::ScaleRemaining(bp) => {
                let elapsed = now.since(timed.start).min(timed.duration);
                elapsed + scale(timed.duration - elapsed, bp)
            }
            DurationChange::AddTicks(ticks) if ticks >= 0 => {
                timed.duration.saturating_add(ticks.unsigned_abs())
            }
            DurationChange::AddTicks(ticks) => timed.duration.saturating_sub(ticks.unsigned_abs()),
        }
    }
}

fn scale(ticks: u64, bp: u32) -> u64 {
    let scaled = u128::from(ticks) * u128::from(bp) / u128::from(BASIS_POINTS);
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// `ApplyEffect` applier: spawns `blueprint`, starts it now, and attaches it
/// to `target`. A target that is gone (e.g. killed by the hit that applied
/// the effect) is a silent no-op.
pub(crate) fn apply_effect(state: &mut GameState, target: Entity, blueprint: BlueprintId) {
    if !state.world.contains(target) {
        return;
    }
    let effect = match state.spawn_id(blueprint) {
        Ok(effect) => effect,
        Err(err) => {
            state.log.push(format!("Error: cannot apply effect: {err}"));
            return;
        }
    };
    // On-hit effects are checked for a `Duration` at load.
    debug_assert!(
        state.world.satisfies::<&Duration>(effect),
        "effect blueprint {blueprint:?} has no Duration"
    );
    let Ok(Duration(duration)) = state.world.remove_one::<Duration>(effect) else {
        let _ = state.world.despawn(effect);
        return;
    };
    let timed = Timed {
        start: state.now,
        duration,
    };
    if state
        .world
        .insert(effect, (timed, EffectTarget(target)))
        .is_err()
    {
        return;
    }

    holding::attach(&mut state.world, target, effect);

    // The name completes the sentence, as in the expiry message.
    let name = state.world.get::<&Name>(effect).ok().map(|n| n.0.clone());
    if let Some(name) = name {
        let subject = Subject::of(&state.world, target);
        state.log.push(subject.sentence("start", &name));
    }

    let interval = state
        .world
        .get::<&Bleeding>(effect)
        .ok()
        .map(|bleeding| bleeding.interval);
    if let Some(interval) = interval {
        schedule_bleed(state, effect, interval);
    }
    schedule_expiry(state, effect);
}

/// `ModifyDuration` applier. Always schedules an expiry check at the new
/// end; any timer for the old end stays queued and fires as a no-op.
pub(crate) fn modify_duration(state: &mut GameState, effect: Entity, change: DurationChange) {
    let now = state.now;
    {
        let Ok(mut timed) = state.world.get::<&mut Timed>(effect) else {
            return;
        };
        timed.duration = change.apply(*timed, now);
    }
    schedule_expiry(state, effect);
}

/// Expire at the effect's current end, or right now if that has passed.
fn schedule_expiry(state: &mut GameState, effect: Entity) {
    let Ok(timed) = state.world.get::<&Timed>(effect).map(|t| *t) else {
        return;
    };
    if timed.is_active(state.now) {
        state
            .timers
            .schedule(timed.end(), Timer::ExpireEffect(effect));
    } else {
        state.commands.push_front(Command::ExpireEffect(effect));
    }
}

/// `ExpireEffect` applier: a no-op unless `effect` still exists and its end
/// has passed.
pub(crate) fn expire_effect(state: &mut GameState, effect: Entity) {
    let Some((timed, target)) = timed_target(state, effect) else {
        return;
    };
    if timed.is_active(state.now) {
        return;
    }

    let name = state.world.get::<&Name>(effect).ok().map(|n| n.0.clone());
    if let Some(name) = name {
        let subject = Subject::of(&state.world, target);
        state.log.push(subject.sentence("stop", &name));
    }
    holding::detach(&mut state.world, target, effect);
    let _ = state.world.despawn(effect);
}

/// `BleedTick` applier: damages the target and queues the next tick, but
/// only while the effect is active. Rescheduling even when the next tick
/// falls past the current end keeps bleeding going if the effect is extended.
pub(crate) fn bleed_tick(state: &mut GameState, effect: Entity) {
    let Some((timed, target)) = timed_target(state, effect) else {
        return;
    };
    let Ok(bleeding) = state.world.get::<&Bleeding>(effect).map(|b| *b) else {
        return;
    };
    if !timed.is_active(state.now) {
        return;
    }
    state.commands.push_back(Command::DealDamage {
        target,
        amount: bleeding.damage,
        cause: DamageCause::Effect { effect },
    });
    schedule_bleed(state, effect, bleeding.interval);
}

fn schedule_bleed(state: &mut GameState, effect: Entity, interval: u64) {
    // A zero interval would fire forever within one tick.
    let at = state.now.after(interval.max(1));
    state.timers.schedule(at, Timer::BleedTick(effect));
}

fn timed_target(state: &GameState, effect: Entity) -> Option<(Timed, Entity)> {
    let timed = *state.world.get::<&Timed>(effect).ok()?;
    let target = state.world.get::<&EffectTarget>(effect).ok()?.0;
    Some((timed, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Energy, Health, Melee, OnHit, OnHitEntry};
    use crate::state::tests::bare_state;
    use crate::testkit::thing;
    use crate::time::{Step, step};

    /// Bare state with an unscheduled player, so only timers move the clock.
    fn quiet_state() -> GameState {
        let mut state = bare_state();
        state.world.remove_one::<Energy>(state.player).unwrap();
        state
    }

    fn apply_bleeding(state: &mut GameState) -> Entity {
        let target = state.player;
        apply_bleeding_to(state, target)
    }

    fn apply_bleeding_to(state: &mut GameState, target: Entity) -> Entity {
        let blueprint = state.blueprints.id("Bleeding").unwrap();
        state.run(Command::ApplyEffect { target, blueprint });
        *holding::attached(&state.world, target).last().unwrap()
    }

    /// Fires every timer due up to and including `tick`, then sets the
    /// clock there.
    fn run_until(state: &mut GameState, tick: u64) {
        while state.timers.next_due().is_some_and(|due| due <= Tick(tick)) {
            assert_ne!(step(state), Step::GameOver, "the scheduler has stopped");
        }
        state.now = state.now.max(Tick(tick));
    }

    fn modify(state: &mut GameState, effect: Entity, change: DurationChange) {
        state.run(Command::ModifyDuration { effect, change });
    }

    fn timed(state: &GameState, effect: Entity) -> Timed {
        *state.world.get::<&Timed>(effect).unwrap()
    }

    fn health(state: &GameState) -> i32 {
        state.world.get::<&Health>(state.player).unwrap().current
    }

    fn last_log(state: &GameState) -> &str {
        state.log.recent(1).next().map(String::as_str).unwrap()
    }

    #[test]
    fn applying_an_effect_starts_it_now_and_attaches_it() {
        let mut state = quiet_state();
        state.now = Tick(40);
        let effect = apply_bleeding(&mut state);
        assert_eq!(
            timed(&state, effect),
            Timed {
                start: Tick(40),
                duration: 500
            }
        );
        assert_eq!(
            state.world.get::<&EffectTarget>(effect).unwrap().0,
            state.player
        );
        assert!(!state.world.satisfies::<&Duration>(effect));
    }

    #[test]
    fn applying_an_effect_to_a_despawned_target_does_nothing() {
        let mut state = quiet_state();
        let target = thing("dummy").spawn(&mut state);
        state.world.despawn(target).unwrap();
        let (entities, log) = (state.world.len(), state.log.clone());
        let blueprint = state.blueprints.id("Bleeding").unwrap();
        state.run(Command::ApplyEffect { target, blueprint });
        assert_eq!(state.world.len(), entities);
        assert_eq!(state.log, log);
        assert!(state.timers.is_empty());
    }

    #[test]
    fn applying_an_effect_is_announced() {
        let mut state = quiet_state();
        apply_bleeding(&mut state);
        assert_eq!(last_log(&state), "You start bleeding.");
    }

    #[test]
    fn an_effect_expires_at_its_end_with_a_message() {
        let mut state = quiet_state();
        let effect = apply_bleeding(&mut state);
        run_until(&mut state, 499);
        assert!(state.world.contains(effect));
        run_until(&mut state, 500);
        assert!(!state.world.contains(effect));
        assert!(holding::attached(&state.world, state.player).is_empty());
        assert!(state.log.recent(3).any(|m| m == "You stop bleeding."));
    }

    #[test]
    fn activity_is_read_from_timestamps_before_the_expiry_timer_runs() {
        let mut state = quiet_state();
        let effect = apply_bleeding(&mut state);
        state.now = Tick(500);
        assert!(state.world.contains(effect), "no timer has run");
        assert!(!timed(&state, effect).is_active(state.now));
        state.now = Tick(499);
        assert!(timed(&state, effect).is_active(state.now));
    }

    #[test]
    fn an_extended_effect_outlives_its_old_end() {
        let mut state = quiet_state();
        let effect = apply_bleeding(&mut state);
        run_until(&mut state, 200);
        modify(&mut state, effect, DurationChange::AddTicks(300));
        run_until(&mut state, 799);
        assert!(state.world.contains(effect), "old end (500) passed");
        run_until(&mut state, 800);
        assert!(!state.world.contains(effect));
    }

    #[test]
    fn a_shortened_effect_expires_early_and_the_old_timer_is_a_no_op() {
        let mut state = quiet_state();
        let effect = apply_bleeding(&mut state);
        run_until(&mut state, 100);
        modify(&mut state, effect, DurationChange::AddTicks(-200));
        run_until(&mut state, 299);
        assert!(state.world.contains(effect));
        run_until(&mut state, 300);
        assert!(!state.world.contains(effect));

        let log = state.log.clone();
        assert_eq!(state.timers.next_due(), Some(Tick(500)));
        run_until(&mut state, 500);
        assert_eq!(state.log, log);
        assert!(state.timers.is_empty());
    }

    #[test]
    fn shortening_past_elapsed_time_expires_in_the_same_step() {
        let mut state = quiet_state();
        let effect = apply_bleeding(&mut state);
        run_until(&mut state, 300);
        modify(&mut state, effect, DurationChange::ScaleTotal(5_000));
        assert!(!state.world.contains(effect));
        assert_eq!(last_log(&state), "You stop bleeding.");
    }

    #[test]
    fn a_stale_timer_does_not_expire_a_newer_effect_of_the_same_kind() {
        let mut state = quiet_state();
        let old = apply_bleeding(&mut state);
        run_until(&mut state, 100);
        modify(&mut state, old, DurationChange::ScaleTotal(0));
        assert!(!state.world.contains(old));

        let new = apply_bleeding(&mut state);
        run_until(&mut state, 500);
        assert!(state.world.contains(new), "old effect's timer fired at 500");
        run_until(&mut state, 600);
        assert!(!state.world.contains(new));
    }

    #[test]
    fn scale_total_and_scale_remaining_round_down() {
        // Started at 0, 999 long, applied at 400: 400 elapsed, 599 left.
        let timed = Timed {
            start: Tick(0),
            duration: 999,
        };
        let now = Tick(400);
        // 999 × 1/3 = 332.9…
        assert_eq!(DurationChange::ScaleTotal(3_333).apply(timed, now), 332);
        // 400 + 599 × 1/3 = 400 + 199.6…
        assert_eq!(DurationChange::ScaleRemaining(3_333).apply(timed, now), 599);
        assert_eq!(DurationChange::ScaleTotal(20_000).apply(timed, now), 1998);
        assert_eq!(
            DurationChange::ScaleRemaining(20_000).apply(timed, now),
            1598
        );
        assert_eq!(DurationChange::AddTicks(-5_000).apply(timed, now), 0);
    }

    #[test]
    fn scale_total_and_scale_remaining_mid_effect() {
        let mut state = quiet_state();
        let total = apply_bleeding(&mut state);
        let remaining = apply_bleeding(&mut state);
        run_until(&mut state, 200);
        modify(&mut state, total, DurationChange::ScaleTotal(15_000));
        modify(
            &mut state,
            remaining,
            DurationChange::ScaleRemaining(15_000),
        );
        assert_eq!(timed(&state, total).end(), Tick(750));
        assert_eq!(timed(&state, remaining).end(), Tick(650));

        run_until(&mut state, 650);
        assert!(!state.world.contains(remaining));
        assert!(state.world.contains(total));
        run_until(&mut state, 750);
        assert!(!state.world.contains(total));
    }

    #[test]
    fn bleeding_hurts_every_interval_and_stops_exactly_at_the_end() {
        let mut state = quiet_state();
        apply_bleeding(&mut state);
        let mut damage_ticks = Vec::new();
        let mut last = health(&state);
        for tick in 1..=1000 {
            run_until(&mut state, tick);
            if health(&state) < last {
                damage_ticks.push(tick);
                last = health(&state);
            }
        }
        assert_eq!(damage_ticks, [100, 200, 300, 400]);
        assert_eq!(health(&state), 16);
        assert!(state.timers.is_empty());
    }

    #[test]
    fn extended_bleeding_keeps_hurting() {
        let mut state = quiet_state();
        let effect = apply_bleeding(&mut state);
        run_until(&mut state, 450);
        modify(&mut state, effect, DurationChange::AddTicks(200));
        run_until(&mut state, 2000);
        // Ticks at 100..=600; the end is 700.
        assert_eq!(health(&state), 14);
    }

    #[test]
    fn bleeding_dies_with_its_target() {
        let mut state = quiet_state();
        let dummy = thing("dummy").health(10).spawn(&mut state);
        let effect = apply_bleeding_to(&mut state, dummy);
        state.run(Command::Die { entity: dummy });
        assert!(!state.world.contains(effect));
        let log = state.log.clone();
        run_until(&mut state, 1000);
        assert_eq!(state.log, log, "orphaned timers are no-ops");
        assert!(state.timers.is_empty());
    }

    /// A 100-HP dummy, and a player whose every damaging hit (exactly 1)
    /// makes the victim bleed.
    fn bleeding_striker() -> (GameState, Entity) {
        let mut state = quiet_state();
        let dummy = thing("dummy").health(100).spawn(&mut state);
        let certain_bleed = OnHit(vec![OnHitEntry {
            effect: state.blueprints.id("Bleeding").unwrap(),
            chance_bp: 10_000,
        }]);
        let player = state.player;
        state
            .world
            .insert(player, (Melee { min: 1, max: 1 }, certain_bleed))
            .unwrap();
        (state, dummy)
    }

    fn player_hits(state: &mut GameState, target: Entity) {
        let attacker = state.player;
        state.run(Command::Attack { attacker, target });
    }

    fn attachments(state: &GameState, entity: Entity) -> Vec<Entity> {
        holding::attached(&state.world, entity)
    }

    #[test]
    fn each_successful_hit_applies_an_independent_instance() {
        let (mut state, dummy) = bleeding_striker();
        player_hits(&mut state, dummy);
        run_until(&mut state, 200);
        player_hits(&mut state, dummy);

        let [first, second] = attachments(&state, dummy)[..] else {
            panic!("expected two bleeds");
        };
        assert_eq!(timed(&state, first).start, Tick(0));
        assert_eq!(timed(&state, second).start, Tick(200));

        run_until(&mut state, 500);
        assert_eq!(attachments(&state, dummy), [second]);
        run_until(&mut state, 700);
        assert!(attachments(&state, dummy).is_empty());
        // Two hits, then four ticks from each bleed.
        assert_eq!(state.world.get::<&Health>(dummy).unwrap().current, 90);
    }

    #[test]
    fn bleed_damage_does_not_trigger_on_hit_effects() {
        let (mut state, dummy) = bleeding_striker();
        // On the victim too, so neither side of a bleed tick may fire it.
        let on_hit = OnHit::clone(&state.world.get::<&OnHit>(state.player).unwrap());
        state.world.insert_one(dummy, on_hit).unwrap();
        player_hits(&mut state, dummy);
        for tick in 1..=1000 {
            run_until(&mut state, tick);
            assert!(attachments(&state, dummy).len() <= 1, "tick {tick}");
        }
        assert_eq!(state.world.get::<&Health>(dummy).unwrap().current, 95);
    }
}
