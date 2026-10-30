//! Sprinting (docs/plans/fights.md): a toggle that makes a step cost half the energy and a point of
//! stamina, and rules out attacking. Fleeing a foe as fast as you never works under the energy
//! model; sprinting makes it a choice that runs out. A creature with `SprintAi` sprints while it
//! chases from afar (the spirit wolf's charge).

use hecs::Entity;

use crate::command::Command;
use crate::entity::{Position, SprintAi, Sprinting, Stamina};
use crate::grammar::Subject;
use crate::state::GameState;
use crate::technique;

/// Stamina a sprinting step costs.
const STEP: u32 = 1;
/// A `SprintAi` creature sprints while its quarry is farther than this.
const CHARGE_FROM: i32 = 3;

pub(crate) fn is_sprinting(state: &GameState, actor: Entity) -> bool {
    state.world.satisfies::<&Sprinting>(actor)
}

/// Whether the player is sprinting, for the status line.
pub fn sprinting(state: &GameState) -> bool {
    is_sprinting(state, state.player)
}

fn stamina_left(state: &GameState, actor: Entity) -> u32 {
    technique::stamina(state, actor).map_or(0, |(pool, rates)| pool.current(rates, state.now))
}

/// Why `actor` can't start sprinting now, or `None`.
pub(crate) fn refusal(state: &GameState, actor: Entity) -> Option<&'static str> {
    (!is_sprinting(state, actor) && stamina_left(state, actor) < STEP)
        .then_some("You are too winded to sprint.")
}

/// `Command::Sprint` applier: starts or stops sprinting.
pub(crate) fn toggle(state: &mut GameState, actor: Entity) {
    let runner = Subject::of(&state.world, actor);
    if is_sprinting(state, actor) {
        let _ = state.world.remove_one::<Sprinting>(actor);
        if actor == state.player {
            state.log.push(runner.sentence("slow", "to a walk"));
        }
    } else if refusal(state, actor).is_none() {
        let _ = state.world.insert_one(actor, Sprinting);
        if actor == state.player || crate::ai::can_see_player(state, actor) {
            state.log.push(runner.sentence("break", "into a sprint"));
        }
    }
}

/// A step `actor` is about to pay for: if it is sprinting and has the
/// stamina, the stamina is spent and the step is a sprinting one (half the
/// energy). Out of stamina, the sprint ends and the step is a walk.
pub(crate) fn pay_step(state: &mut GameState, actor: Entity) -> bool {
    if !is_sprinting(state, actor) {
        return false;
    }
    let Some((pool, rates)) = technique::stamina(state, actor) else {
        let _ = state.world.remove_one::<Sprinting>(actor);
        return false;
    };
    if pool.current(rates, state.now) < STEP {
        let _ = state.world.remove_one::<Sprinting>(actor);
        if actor == state.player {
            state.log.push("You are out of breath.");
        }
        return false;
    }
    let spent = pool.spend(STEP, rates, state.now);
    let _ = state.world.insert_one(actor, Stamina(spent));
    true
}

/// A `SprintAi` creature's toggle before it decides: on while chasing a
/// quarry farther than `CHARGE_FROM`, off once close.
pub(crate) fn decide(state: &GameState, actor: Entity) -> Option<Command> {
    let world = &state.world;
    if !world.satisfies::<&SprintAi>(actor) {
        return None;
    }
    let me = world.get::<&Position>(actor).ok().map(|p| *p)?;
    let far = world
        .get::<&crate::entity::Quarry>(actor)
        .ok()
        .map(|quarry| quarry.at)
        .is_some_and(|(x, y)| (x - me.x).abs().max((y - me.y).abs()) > CHARGE_FROM);
    let sprinting = is_sprinting(state, actor);
    let want = far && (sprinting || refusal(state, actor).is_none());
    (want != sprinting).then_some(Command::Sprint(actor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Energy, Pool};
    use crate::input::{Dir, PlayerAction};
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};
    use crate::time::take_turn;

    fn with_stamina(value: u32) -> GameState {
        let mut state = bare_state();
        let player = state.player;
        state
            .world
            .insert_one(
                player,
                Stamina(Pool {
                    value,
                    max: 10,
                    as_of: state.now,
                    regen_ticks: 0,
                    growth: Vec::new(),
                }),
            )
            .unwrap();
        state
    }

    fn energy(state: &GameState) -> i64 {
        state.world.get::<&Energy>(state.player).unwrap().0
    }

    #[test]
    fn a_sprinting_step_costs_half_and_a_point_of_stamina() {
        let mut state = with_stamina(5);
        let player = state.player;
        state.run(Command::Sprint(player));
        assert!(sprinting(&state));
        state.world.insert_one(player, Energy(0)).unwrap();
        state.run(Command::TryMove(player, Dir::E));
        assert_eq!(energy(&state), -crate::cost::MOVE / 2);
        assert_eq!(stamina_left(&state, player), 4);
    }

    #[test]
    fn sprinting_onto_rubble_costs_a_plain_step() {
        let mut state = with_stamina(5);
        let player = state.player;
        state.map.set(2, 1, crate::map::Tile::Rubble);
        state.run(Command::Sprint(player));
        state.world.insert_one(player, Energy(0)).unwrap();
        state.run(Command::TryMove(player, Dir::E));
        assert_eq!(energy(&state), -crate::cost::MOVE);
    }

    #[test]
    fn out_of_stamina_the_sprint_ends_and_the_step_is_a_walk() {
        let mut state = with_stamina(1);
        let player = state.player;
        state.run(Command::Sprint(player));
        state.run(Command::TryMove(player, Dir::E));
        state.world.insert_one(player, Energy(0)).unwrap();
        state.run(Command::TryMove(player, Dir::E));
        assert!(!sprinting(&state));
        assert_eq!(energy(&state), -crate::cost::MOVE);
        assert_eq!(last_log(&state), "You are out of breath.");
    }

    #[test]
    fn winded_you_cannot_start_and_sprinting_you_cannot_attack() {
        let mut state = with_stamina(0);
        let before = state.now;
        take_turn(&mut state, PlayerAction::Sprint);
        assert_eq!(state.now, before);
        assert_eq!(last_log(&state), "You are too winded to sprint.");

        let mut state = with_stamina(5);
        thing("goblin")
            .health(5)
            .hostile()
            .at(2, 1)
            .make(&mut state);
        let player = state.player;
        state.run(Command::Sprint(player));
        let before = state.now;
        take_turn(&mut state, PlayerAction::Move(Dir::E));
        assert_eq!(state.now, before);
        assert_eq!(last_log(&state), "You can't attack while sprinting.");
    }

    #[test]
    fn a_charging_creature_sprints_from_afar_and_stops_close() {
        let mut state = bare_state();
        let wolf = thing("wolf").health(5).hostile().at(10, 1).make(&mut state);
        state.world.insert_one(wolf, SprintAi).unwrap();
        state
            .world
            .insert_one(wolf, crate::entity::Quarry { at: (1, 1) })
            .unwrap();
        state
            .world
            .insert_one(
                wolf,
                Stamina(Pool {
                    value: 5,
                    max: 5,
                    as_of: state.now,
                    regen_ticks: 0,
                    growth: Vec::new(),
                }),
            )
            .unwrap();
        assert_eq!(decide(&state, wolf), Some(Command::Sprint(wolf)));
        state.run(Command::Sprint(wolf));
        assert_eq!(decide(&state, wolf), None, "still far");
        *state.world.get::<&mut Position>(wolf).unwrap() = Position { x: 3, y: 1 };
        assert_eq!(
            decide(&state, wolf),
            Some(Command::Sprint(wolf)),
            "close: stop"
        );
    }
}
