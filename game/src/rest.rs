//! Rest (docs/plans/rest.md): health comes back a point at a time, from a budget per floor, so
//! careful play can heal between fights but a floor is still a budget. A `Regeneration` holder's
//! tick is a timer like bleeding's: it re-checks everything when it fires (ADR 0006). Camp heals in
//! full.

use hecs::Entity;

use crate::dive::Place;
use crate::entity::{Dead, Health, Regeneration, RestBudget};
use crate::explore::visible_hostile;
use crate::grammar::Subject;
use crate::input::PlayerAction;
use crate::state::GameState;
use crate::time::{Timer, take_turn};

/// A new holder's first tick, and a full budget for where it stands.
pub(crate) fn start(state: &mut GameState, entity: Entity) {
    if state.world.satisfies::<&Regeneration>(entity) {
        refill(state, entity);
        schedule(state, entity);
    }
}

fn schedule(state: &mut GameState, entity: Entity) {
    let Ok(interval) = state
        .world
        .get::<&Regeneration>(entity)
        .map(|regeneration| regeneration.interval)
    else {
        return;
    };
    // A zero interval would fire forever within one tick.
    let at = state.now.after(interval.max(1));
    state.timers.schedule(at, Timer::Regenerate(entity));
}

/// `Command::Regenerate` applier: a point of health if the holder is hurt
/// and its budget has any left, then the next tick. A holder that is gone
/// or dead gets no next tick.
pub(crate) fn regenerate(state: &mut GameState, entity: Entity) {
    if !state.world.satisfies::<&Regeneration>(entity) || state.world.satisfies::<&Dead>(entity) {
        return;
    }
    let left = state
        .world
        .get::<&RestBudget>(entity)
        .map_or(0, |budget| budget.left);
    let healed = left > 0
        && state
            .world
            .get::<&mut Health>(entity)
            .is_ok_and(|mut health| {
                let hurt = health.current < health.max;
                if hurt {
                    health.current += 1;
                }
                hurt
            });
    if healed {
        let _ = state
            .world
            .insert_one(entity, RestBudget { left: left - 1 });
    }
    schedule(state, entity);
}

/// The budget a holder gets on a floor: `per_floor` percent of max health.
fn full_budget(state: &GameState, entity: Entity) -> Option<RestBudget> {
    let per_floor = state.world.get::<&Regeneration>(entity).ok()?.per_floor;
    let max = state.world.get::<&Health>(entity).ok()?.max;
    let max = u32::try_from(max).unwrap_or(0);
    Some(RestBudget {
        left: max * per_floor / 100,
    })
}

fn refill(state: &mut GameState, entity: Entity) {
    if let Some(budget) = full_budget(state, entity) {
        let _ = state.world.insert_one(entity, budget);
    }
}

/// Arriving somewhere (`dive::enter`): a new floor refills every traveller's
/// budget; camp heals them in full as well.
pub(crate) fn arrive(state: &mut GameState, travellers: &[Entity]) {
    let at_camp = matches!(state.place, Place::Camp);
    for &traveller in travellers {
        if !state.world.satisfies::<&Regeneration>(traveller) {
            continue;
        }
        refill(state, traveller);
        if at_camp && let Ok(mut health) = state.world.get::<&mut Health>(traveller) {
            health.current = health.max;
        }
    }
}

/// Why resting now would do nothing, or `None` if it would heal.
pub fn rest_refusal(state: &GameState) -> Option<String> {
    let player = state.player;
    if let Some(hostile) = visible_hostile(state) {
        return Some(Subject::of(&state.world, hostile).sentence("be", "in view"));
    }
    let Some(left) = budget(state) else {
        return Some("You can't rest.".into());
    };
    let hurt = state
        .world
        .get::<&Health>(player)
        .is_ok_and(|health| health.current < health.max);
    if !hurt {
        return Some("You are rested.".into());
    }
    if left == 0 {
        return Some("You can rest no more on this floor.".into());
    }
    None
}

/// Takes one turn of rest as the player's and returns whether to keep going,
/// like `practice_step`. Stops without spending time when resting would do
/// nothing (the log says why): rested, the budget spent, an enemy in view.
pub fn rest_step(state: &mut GameState) -> bool {
    if state.game_over {
        return false;
    }
    if let Some(reason) = rest_refusal(state) {
        state.log.push(reason);
        return false;
    }
    take_turn(state, PlayerAction::Wait);
    !state.game_over
}

/// Health still to regain on this floor, for the status line; `None` for a
/// creature that doesn't regenerate.
pub fn budget(state: &GameState) -> Option<u32> {
    let player = state.player;
    if !state.world.satisfies::<&Regeneration>(player) {
        return None;
    }
    Some(
        state
            .world
            .get::<&RestBudget>(player)
            .map_or(0, |budget| budget.left),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::entity::Energy;
    use crate::state::tests::bare_state;
    use crate::time::{Step, step};

    const INTERVAL: u64 = 300;

    /// A bare state whose player regenerates, hurt to `current`, and is
    /// unscheduled so only timers move the clock.
    fn hurt(current: i32) -> GameState {
        let mut state = bare_state();
        let player = state.player;
        state.world.remove_one::<Energy>(player).unwrap();
        state
            .world
            .insert_one(
                player,
                Regeneration {
                    interval: INTERVAL,
                    per_floor: 125,
                },
            )
            .unwrap();
        start(&mut state, player);
        state.world.get::<&mut Health>(player).unwrap().current = current;
        state
    }

    fn health(state: &GameState) -> i32 {
        state.world.get::<&Health>(state.player).unwrap().current
    }

    /// Steps the clock until `ticks` have passed and whatever was due then
    /// has fired (the clock reaches a timer's tick a step before it fires).
    fn wait(state: &mut GameState, ticks: u64) {
        let until = state.now.after(ticks);
        while state.now <= until {
            if step(state) == Step::Idle {
                break;
            }
        }
    }

    #[test]
    fn a_hurt_holder_heals_a_point_per_interval() {
        let mut state = hurt(10);
        assert_eq!(budget(&state), Some(25), "125% of 20");
        wait(&mut state, INTERVAL * 3);
        assert_eq!(health(&state), 13);
        assert_eq!(budget(&state), Some(22));
    }

    #[test]
    fn healing_stops_at_max_and_keeps_the_budget() {
        let mut state = hurt(19);
        wait(&mut state, INTERVAL * 5);
        assert_eq!(health(&state), 20);
        assert_eq!(budget(&state), Some(24));
    }

    #[test]
    fn healing_stops_when_the_budget_is_spent() {
        let mut state = hurt(1);
        let player = state.player;
        state
            .world
            .insert_one(player, RestBudget { left: 2 })
            .unwrap();
        wait(&mut state, INTERVAL * 10);
        assert_eq!(health(&state), 3);
        assert_eq!(budget(&state), Some(0));
    }

    #[test]
    fn arriving_refills_and_camp_heals_in_full() {
        let mut state = hurt(5);
        let player = state.player;
        state
            .world
            .insert_one(player, RestBudget { left: 0 })
            .unwrap();
        arrive(&mut state, &[player]);
        assert_eq!(budget(&state), Some(25));
        assert_eq!(health(&state), 5, "a floor does not heal");
        state.place = Place::Camp;
        arrive(&mut state, &[player]);
        assert_eq!(health(&state), 20);
    }

    #[test]
    fn a_dead_holder_neither_heals_nor_ticks_again() {
        let mut state = hurt(5);
        let player = state.player;
        state.world.insert_one(player, Dead).unwrap();
        regenerate(&mut state, player);
        assert_eq!(health(&state), 5);
        let ticks = state
            .timers
            .queue
            .values()
            .filter(|&&timer| timer == Timer::Regenerate(player))
            .count();
        assert_eq!(ticks, 1, "only the tick scheduled at start");
    }

    #[test]
    fn resting_waits_until_rested_and_then_says_so() {
        let mut state = hurt(18);
        let player = state.player;
        state.world.insert_one(player, Energy(0)).unwrap();
        while rest_step(&mut state) {}
        assert_eq!(health(&state), 20);
        assert_eq!(crate::testkit::last_log(&state), "You are rested.");
    }

    #[test]
    fn resting_is_refused_with_no_budget_or_an_enemy_in_view() {
        let mut state = hurt(10);
        let player = state.player;
        state
            .world
            .insert_one(player, RestBudget { left: 0 })
            .unwrap();
        let before = state.now;
        assert!(!rest_step(&mut state));
        assert_eq!(state.now, before);
        assert_eq!(
            rest_refusal(&state).as_deref(),
            Some("You can rest no more on this floor.")
        );
        state
            .world
            .insert_one(player, RestBudget { left: 5 })
            .unwrap();
        crate::testkit::thing("goblin")
            .with(crate::entity::Hostile)
            .at(2, 1)
            .make(&mut state);
        crate::testkit::see_everything(&mut state);
        assert_eq!(
            rest_refusal(&state).as_deref(),
            Some("The goblin is in view.")
        );
    }

    #[test]
    fn the_tick_runs_through_the_command_queue() {
        let mut state = hurt(5);
        let player = state.player;
        state.run(Command::Regenerate(player));
        assert_eq!(health(&state), 6);
    }
}
