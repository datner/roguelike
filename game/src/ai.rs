//! Non-player decisions. AI only chooses a `Command`; the scheduler queues
//! it like any other action, so moving and bump-attacking reuse `TryMove`.

use hecs::Entity;

use crate::command::Command;
use crate::entity::{BlocksMovement, Health, Hostile, Position};
use crate::input::Dir;
use crate::map::Tile;
use crate::state::GameState;

/// A hostile that sees the player attacks when adjacent and otherwise steps
/// toward them; anything else waits.
pub(crate) fn decide(state: &GameState, actor: Entity) -> Command {
    let wait = Command::Wait(actor);
    if !state.world.satisfies::<&Hostile>(actor) || !can_see_player(state, actor) {
        return wait;
    }
    let (Ok(me), Ok(player)) = (
        state.world.get::<&Position>(actor).map(|p| *p),
        state.world.get::<&Position>(state.player).map(|p| *p),
    ) else {
        return wait;
    };

    if chebyshev(me, player) == 1 {
        let toward = ((player.x - me.x).signum(), (player.y - me.y).signum());
        return match Dir::ALL.into_iter().find(|dir| dir.delta() == toward) {
            Some(dir) => Command::TryMove(actor, dir),
            None => wait,
        };
    }
    step_toward(state, me, player)
        .map(|dir| Command::TryMove(actor, dir))
        .unwrap_or(wait)
}

/// Whether `actor` perceives the player.
///
/// Assumption: an actor sees the player iff the actor's tile is in the
/// player's visible set (`GameState::visible`). That is only sound while
/// all three hold:
/// - FOV is symmetric (`fov` — Ford's shadowcasting), so "the player sees
///   me" equals "I see the player";
/// - lighting is uniform (no darkness makes one side unseen);
/// - every actor's sight radius equals the player's (`fov::RADIUS`).
///
/// Break any of them and this needs the actor's own view. ADR 0007.
pub(crate) fn can_see_player(state: &GameState, actor: Entity) -> bool {
    state
        .world
        .get::<&Position>(actor)
        .is_ok_and(|pos| state.visible.get(pos.x, pos.y) == Some(&true))
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

/// Open ground with nothing blocking or fightable on it.
fn passable(state: &GameState, at: Position) -> bool {
    if !Tile::is_open(state.map.get(at.x, at.y)) {
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
    use crate::fov::{self, RADIUS};
    use crate::input::PlayerAction;
    use crate::spawn::Spawn;
    use crate::state::tests::bare_state;
    use crate::testkit::last_log;
    use crate::time::{Tick, take_turn};

    /// Player at (1, 1) plus a scheduled goblin at `at`.
    fn with_goblin(at: Position) -> (GameState, Entity) {
        let mut state = bare_state();
        let goblin = state.spawn(Spawn::blueprint("Goblin").with(at)).unwrap();
        (state, goblin)
    }

    fn position(state: &GameState, entity: Entity) -> Position {
        *state.world.get::<&Position>(entity).unwrap()
    }

    fn player_health(state: &GameState) -> i32 {
        state.world.get::<&Health>(state.player).unwrap().current
    }

    fn wait(state: &mut GameState) {
        take_turn(state, PlayerAction::Wait);
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
        let (mut state, goblin) = with_goblin(Position {
            x: 1 + RADIUS,
            y: 1,
        });
        wait(&mut state);
        assert_eq!(position(&state, goblin), Position { x: RADIUS, y: 1 });
    }

    #[test]
    fn a_goblin_out_of_range_waits() {
        let (mut state, goblin) = with_goblin(Position {
            x: 2 + RADIUS,
            y: 1,
        });
        assert_eq!(decide(&state, goblin), Command::Wait(goblin));
        wait(&mut state);
        assert_eq!(
            position(&state, goblin),
            Position {
                x: 2 + RADIUS,
                y: 1
            }
        );
        assert_eq!(state.now, Tick(100));
    }

    #[test]
    fn a_goblin_behind_a_wall_waits_until_it_is_in_view() {
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 1 });
        state.map.set(3, 1, Tile::Wall);
        fov::update(&mut state);
        assert!(!can_see_player(&state, goblin));
        assert_eq!(decide(&state, goblin), Command::Wait(goblin));
        wait(&mut state);
        assert_eq!(position(&state, goblin), Position { x: 5, y: 1 });

        // Stepping out from behind the pillar brings the goblin into view
        // before it acts.
        take_turn(&mut state, PlayerAction::Move(Dir::SE));
        assert!(can_see_player(&state, goblin));
        assert_eq!(position(&state, goblin), Position { x: 4, y: 2 });
    }

    #[test]
    fn a_blocked_goblin_steps_around_in_fixed_order() {
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 1 });
        state.spawn(Spawn::blueprint("Rubble").at(4, 1)).unwrap();
        // W is blocked and N/NW are wall; SW is the only step that gets closer.
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::SW));

        // From (5, 3) to (1, 1), W and NW both end 3 away; NW lines up
        // better (squared distance 10 vs 13)…
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 3 });
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::NW));
        // …and with NW blocked, W wins.
        state.spawn(Spawn::blueprint("Rubble").at(4, 2)).unwrap();
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
        assert_eq!(last_log(&state), "You die.");

        let now = state.now;
        wait(&mut state);
        assert_eq!(state.now, now, "turns are ignored after game over");
    }
}
