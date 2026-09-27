//! Non-player decisions. AI only chooses a `Command`; the scheduler queues
//! it like any other action, so moving and bump-attacking reuse `TryMove`.

use hecs::Entity;

use crate::command::Command;
use crate::entity::{BlocksMovement, Health, Hostile, Position};
use crate::input::Dir;
use crate::map::Tile;
use crate::state::GameState;

/// How far (Chebyshev) a hostile notices the player. There is no field of
/// view yet, so this sees through walls.
pub const SIGHT: i32 = 8;

pub(crate) fn decide(state: &GameState, actor: Entity) -> Command {
    let wait = Command::Wait(actor);
    if !state.world.satisfies::<&Hostile>(actor) {
        return wait;
    }
    let (Ok(me), Ok(player)) = (
        state.world.get::<&Position>(actor).map(|p| *p),
        state.world.get::<&Position>(state.player).map(|p| *p),
    ) else {
        return wait;
    };

    let distance = chebyshev(me, player);
    if distance == 1 {
        let toward = ((player.x - me.x).signum(), (player.y - me.y).signum());
        return match Dir::ALL.into_iter().find(|dir| dir.delta() == toward) {
            Some(dir) => Command::TryMove(actor, dir),
            None => wait,
        };
    }
    if distance > SIGHT {
        return wait;
    }
    step_toward(state, me, player)
        .map(|dir| Command::TryMove(actor, dir))
        .unwrap_or(wait)
}

/// The passable direction that ends closest to `goal` (Chebyshev, then
/// squared Euclidean so it prefers lining up), ties going to the earlier
/// entry in `Dir::ALL`. `None` if no step gets closer.
fn step_toward(state: &GameState, from: Position, goal: Position) -> Option<Dir> {
    let current = chebyshev(from, goal);
    Dir::ALL
        .into_iter()
        .enumerate()
        .filter_map(|(order, dir)| {
            let (dx, dy) = dir.delta();
            let next = Position {
                x: from.x + dx,
                y: from.y + dy,
            };
            passable(state, next).then(|| {
                let (ex, ey) = (goal.x - next.x, goal.y - next.y);
                ((chebyshev(next, goal), ex * ex + ey * ey, order), dir)
            })
        })
        .min_by_key(|&(key, _)| key)
        .filter(|&((distance, _, _), _)| distance < current)
        .map(|(_, dir)| dir)
}

fn chebyshev(a: Position, b: Position) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Floor with nothing blocking or fightable on it.
fn passable(state: &GameState, at: Position) -> bool {
    if state.map.get(at.x, at.y) != Some(&Tile::Floor) {
        return false;
    }
    let occupied = |pos: &Position| *pos == at;
    !state
        .world
        .query::<(&Position, &BlocksMovement)>()
        .iter()
        .any(|(pos, _)| occupied(pos))
        && !state
            .world
            .query::<(&Position, &Health)>()
            .iter()
            .any(|(pos, _)| occupied(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Dead, Melee};
    use crate::state::tests::bare_state;
    use crate::time::{Tick, take_turn};

    /// Player at (1, 1) plus a scheduled goblin at `at`.
    fn with_goblin(at: Position) -> (GameState, Entity) {
        let mut state = bare_state();
        let goblin = state.spawn_at("Goblin", at).unwrap();
        (state, goblin)
    }

    fn position(state: &GameState, entity: Entity) -> Position {
        *state.world.get::<&Position>(entity).unwrap()
    }

    fn player_health(state: &GameState) -> i32 {
        state.world.get::<&Health>(state.player).unwrap().current
    }

    fn wait(state: &mut GameState) {
        let player = state.player;
        take_turn(state, Command::Wait(player));
    }

    #[test]
    fn an_adjacent_goblin_attacks() {
        let (mut state, goblin) = with_goblin(Position { x: 2, y: 2 });
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::NW));
        wait(&mut state);
        assert_eq!(position(&state, goblin), Position { x: 2, y: 2 });
        assert!(player_health(&state) < 20);
    }

    #[test]
    fn a_goblin_in_range_steps_closer() {
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 1 });
        wait(&mut state);
        assert_eq!(position(&state, goblin), Position { x: 4, y: 1 });
        assert_eq!(player_health(&state), 20);
    }

    #[test]
    fn a_goblin_at_the_edge_of_sight_steps_closer() {
        let (mut state, goblin) = with_goblin(Position { x: 1 + SIGHT, y: 1 });
        wait(&mut state);
        assert_eq!(position(&state, goblin), Position { x: SIGHT, y: 1 });
    }

    #[test]
    fn a_goblin_out_of_range_waits() {
        let (mut state, goblin) = with_goblin(Position { x: 2 + SIGHT, y: 1 });
        assert_eq!(decide(&state, goblin), Command::Wait(goblin));
        wait(&mut state);
        assert_eq!(position(&state, goblin), Position { x: 2 + SIGHT, y: 1 });
        assert_eq!(state.now, Tick(100));
    }

    #[test]
    fn a_blocked_goblin_steps_around_in_fixed_order() {
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 1 });
        state.spawn_at("Rubble", Position { x: 4, y: 1 }).unwrap();
        // W is blocked and N/NW are wall; SW is the only step that gets closer.
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::SW));

        // From (5, 3) to (1, 1), W and NW both end 3 away; NW lines up
        // better (squared distance 10 vs 13)…
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 3 });
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::NW));
        // …and with NW blocked, W wins.
        state.spawn_at("Rubble", Position { x: 4, y: 2 }).unwrap();
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::W));
    }

    #[test]
    fn non_hostile_actors_wait() {
        let (mut state, goblin) = with_goblin(Position { x: 2, y: 1 });
        state.world.remove_one::<Hostile>(goblin).unwrap();
        assert_eq!(decide(&state, goblin), Command::Wait(goblin));
    }

    #[test]
    fn the_player_dying_ends_the_game() {
        let (mut state, goblin) = with_goblin(Position { x: 2, y: 1 });
        state
            .world
            .insert_one(goblin, Melee { min: 50, max: 50 })
            .unwrap();
        wait(&mut state);
        assert!(state.game_over);
        assert!(state.world.satisfies::<&Dead>(state.player));
        assert_eq!(state.log.recent(1).next().unwrap(), "You die.");

        let now = state.now;
        wait(&mut state);
        assert_eq!(state.now, now, "turns are ignored after game over");
    }
}
