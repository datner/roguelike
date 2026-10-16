//! Auto-explore: the player's next step toward an unfound floor item, then
//! unexplored ground, then the exit, judged only from what the player knows.

use hecs::Entity;

use crate::entity::{BlocksMovement, Found, Hostile, Item, Position};
use crate::grammar::Subject;
use crate::input::{Dir, PlayerAction};
use crate::map::{Grid, Tile};
use crate::state::GameState;
use crate::time::take_turn;

/// Takes one auto-explore step as the player's turn and returns whether the
/// player moved. When it doesn't step, the log says why and no time passes;
/// a frontend repeats this until it returns false.
pub fn explore_step(state: &mut GameState) -> bool {
    if state.game_over {
        return false;
    }
    let Some(before) = player_position(state) else {
        return false;
    };
    match next_step(state, before) {
        Ok(dir) => {
            take_turn(state, PlayerAction::Move(dir));
            // A step that didn't move (something unseen in the way) already
            // logged why; stopping keeps it from repeating forever.
            player_position(state).is_some_and(|after| after != before)
        }
        Err(reason) => {
            state.log.push(reason);
            false
        }
    }
}

fn player_position(state: &GameState) -> Option<Position> {
    state
        .world
        .get::<&Position>(state.player)
        .ok()
        .map(|pos| *pos)
}

/// The first step of a shortest known path to the best target, or why
/// there is none.
fn next_step(state: &GameState, from: Position) -> Result<Dir, String> {
    if let Some(hostile) = visible_hostile(state) {
        return Err(Subject::of(&state.world, hostile).sentence("be", "in view"));
    }
    let paths = paths_from(state, from);
    let step_to = |(x, y): (i32, i32)| paths.first.get(x, y).copied().flatten();

    let unfound: Vec<(i32, i32)> = state
        .world
        .query::<(&Position, &Item, Option<&Found>)>()
        .iter()
        .filter(|(pos, _, found)| found.is_none() && flag(&state.seen, pos.x, pos.y))
        .map(|(pos, _, _)| (pos.x, pos.y))
        .collect();
    let targets: [&dyn Fn((i32, i32)) -> bool; 3] = [
        &|at| unfound.contains(&at),
        &|at| is_frontier(state, at),
        &|(x, y)| state.map.get(x, y) == Some(&Tile::Exit),
    ];
    for is_target in targets {
        if let Some(dir) = paths
            .order
            .iter()
            .copied()
            .find(|&at| is_target(at))
            .and_then(step_to)
        {
            return Ok(dir);
        }
    }
    Err(if state.map.get(from.x, from.y) == Some(&Tile::Exit) {
        "You stand at the exit; there is nothing left to find.".to_string()
    } else {
        "There is nowhere left to explore.".to_string()
    })
}

fn visible_hostile(state: &GameState) -> Option<Entity> {
    state
        .world
        .query::<(Entity, &Position, &Hostile)>()
        .iter()
        .find(|(_, pos, _)| flag(&state.visible, pos.x, pos.y))
        .map(|(entity, _, _)| entity)
}

/// Known open ground next to ground never seen.
fn is_frontier(state: &GameState, (x, y): (i32, i32)) -> bool {
    Dir::ALL.into_iter().any(|dir| {
        let (dx, dy) = dir.delta();
        state.seen.get(x + dx, y + dy) == Some(&false)
    })
}

fn flag(flags: &Grid<bool>, x: i32, y: i32) -> bool {
    flags.get(x, y) == Some(&true)
}

struct Paths {
    /// Reachable tiles in order of distance, the start excluded; ties go
    /// to `Dir::ALL` order, so the choice is deterministic.
    order: Vec<(i32, i32)>,
    /// For each reachable tile, the first step from the start toward it.
    first: Grid<Option<Dir>>,
}

/// Breadth-first search over seen open tiles, avoiding anything visibly in
/// the way, in 8-way steps (how things move).
fn paths_from(state: &GameState, from: Position) -> Paths {
    let blocked: Vec<(i32, i32)> = state
        .world
        .query::<(Entity, &Position, &BlocksMovement)>()
        .iter()
        .filter(|&(entity, pos, _)| entity != state.player && flag(&state.visible, pos.x, pos.y))
        .map(|(_, pos, _)| (pos.x, pos.y))
        .collect();
    let mut first = Grid::new(state.map.width, state.map.height, None);
    let mut reached = Grid::new(state.map.width, state.map.height, false);
    reached.set(from.x, from.y, true);
    let mut order = Vec::new();
    let mut next = 0;
    let mut frontier = vec![(from.x, from.y)];
    while let Some(&(x, y)) = frontier.get(next) {
        next += 1;
        let step: Option<Dir> = first.get(x, y).copied().flatten();
        for dir in Dir::ALL {
            let (dx, dy) = dir.delta();
            let to = (x + dx, y + dy);
            if reached.get(to.0, to.1) != Some(&false)
                || !flag(&state.seen, to.0, to.1)
                || !Tile::is_open(state.map.get(to.0, to.1))
                || blocked.contains(&to)
            {
                continue;
            }
            reached.set(to.0, to.1, true);
            first.set(to.0, to.1, Some(step.unwrap_or(dir)));
            order.push(to);
            frontier.push(to);
        }
    }
    Paths { order, first }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Name;
    use crate::state::tests::{bare_state, spawn_with};

    fn item(state: &mut GameState, at: (i32, i32)) -> Entity {
        spawn_with(
            state,
            (Name("knife".into()), Item, Position { x: at.0, y: at.1 }),
        )
    }

    fn at(state: &GameState) -> (i32, i32) {
        let pos = player_position(state).unwrap();
        (pos.x, pos.y)
    }

    fn explore_until_stopped(state: &mut GameState) -> usize {
        let mut steps = 0;
        while explore_step(state) {
            steps += 1;
            assert!(steps < 2_000, "auto-explore did not stop");
        }
        steps
    }

    #[test]
    fn it_finds_every_item_then_stops_at_the_exit() {
        let mut state = bare_state();
        state.map.set(30, 15, Tile::Exit);
        let near = item(&mut state, (5, 2));
        let far = item(&mut state, (35, 17));

        explore_until_stopped(&mut state);

        assert!(state.world.satisfies::<&Found>(near));
        assert!(state.world.satisfies::<&Found>(far));
        assert_eq!(at(&state), (30, 15));
        assert!(state.seen.cells().iter().filter(|&&s| s).count() > 0);
        let log: Vec<&String> = state.log.recent(10).collect();
        assert_eq!(
            log[0],
            "You stand at the exit; there is nothing left to find."
        );
        assert!(log.iter().any(|m| *m == "You find the knife."));
    }

    #[test]
    fn items_come_before_unexplored_ground() {
        let mut state = bare_state();
        let knife = item(&mut state, (1, 4));
        // Everything in view at the start, the knife among it.
        assert!(flag(&state.seen, 1, 4));
        for _ in 0..3 {
            assert!(explore_step(&mut state));
        }
        assert_eq!(at(&state), (1, 4));
        assert!(state.world.satisfies::<&Found>(knife));
    }

    #[test]
    fn a_hostile_in_view_stops_it_before_any_time_passes() {
        let mut state = bare_state();
        spawn_with(
            &mut state,
            (Name("goblin".into()), Hostile, Position { x: 3, y: 1 }),
        );
        let now = state.now;

        assert!(!explore_step(&mut state));
        assert_eq!(at(&state), (1, 1));
        assert_eq!(state.now, now);
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("The goblin is in view.")
        );
    }

    #[test]
    fn with_no_exit_it_stops_when_everything_is_seen() {
        let mut state = bare_state();
        explore_until_stopped(&mut state);
        assert!(
            (0..state.map.height)
                .flat_map(|y| (0..state.map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| Tile::is_open(state.map.get(x, y)))
                .all(|(x, y)| flag(&state.seen, x, y))
        );
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("There is nowhere left to explore.")
        );
    }
}
