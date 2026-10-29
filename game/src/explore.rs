//! Auto-explore: the player's next step toward an unfound floor item, then
//! unexplored ground, then the stairs down, judged only from what the player knows.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::entity::{BlocksMovement, Dead, Found, Item, Position};
use crate::grammar::Subject;
use crate::input::{Dir, PlayerAction};
use crate::map::{Grid, Tile};
use crate::side;
use crate::state::GameState;
use crate::time::take_turn;

/// What auto-explore does when a step finds floor items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExploreItems {
    /// Note them and keep going.
    #[default]
    Find,
    /// Stop on them, so the player decides.
    Stop,
    /// Pick them up (a turn) and keep going.
    PickUp,
}

impl ExploreItems {
    fn next(self) -> Self {
        match self {
            ExploreItems::Find => ExploreItems::Stop,
            ExploreItems::Stop => ExploreItems::PickUp,
            ExploreItems::PickUp => ExploreItems::Find,
        }
    }
}

/// Switches `state.explore_items` to the next setting and says so. Takes no
/// time.
pub fn cycle_explore_items(state: &mut GameState) {
    state.explore_items = state.explore_items.next();
    state.log.push(match state.explore_items {
        ExploreItems::Find => "Auto-explore will note items and walk on.",
        ExploreItems::Stop => "Auto-explore will stop at items.",
        ExploreItems::PickUp => "Auto-explore will pick items up.",
    });
}

/// Takes one auto-explore step as the player's turn and returns whether to
/// keep going. It stops without spending time when there is nowhere to go
/// (the log says why), after a step that didn't move, and, per
/// `state.explore_items`, after a step that found items.
pub fn explore_step(state: &mut GameState) -> bool {
    if state.game_over {
        return false;
    }
    let Some(before) = player_position(state) else {
        return false;
    };
    match next_step(state, before) {
        Ok(dir) => {
            let unfound = unfound_items(state);
            take_turn(state, PlayerAction::Move(dir));
            // A step that didn't move (something unseen in the way) already
            // logged why; stopping keeps it from repeating forever.
            let moved = player_position(state).is_some_and(|after| after != before);
            if !moved || unfound_items(state) == unfound {
                return moved;
            }
            match state.explore_items {
                ExploreItems::Find => true,
                ExploreItems::Stop => false,
                ExploreItems::PickUp => {
                    take_turn(state, PlayerAction::PickUp);
                    !state.game_over
                }
            }
        }
        Err(reason) => {
            state.log.push(reason);
            false
        }
    }
}

/// Takes one step toward the known tile `to` as the player's turn, along
/// the same known ground auto-explore walks, and returns whether to keep
/// going. It stops on arrival, after a step that didn't move, and without
/// spending time when an enemy is in view or there is no known way (the
/// log says why).
pub fn travel_step(state: &mut GameState, to: (i32, i32)) -> bool {
    if state.game_over {
        return false;
    }
    let Some(before) = player_position(state) else {
        return false;
    };
    if (before.x, before.y) == to {
        return false;
    }
    if let Some(hostile) = visible_hostile(state) {
        let reason = Subject::of(&state.world, hostile).sentence("be", "in view");
        state.log.push(reason);
        return false;
    }
    let Some(dir) = paths_from(state, before)
        .first
        .get(to.0, to.1)
        .copied()
        .flatten()
    else {
        state.log.push("You know no way there.");
        return false;
    };
    take_turn(state, PlayerAction::Move(dir));
    player_position(state).is_some_and(|after| after != before && (after.x, after.y) != to)
}

/// Floor items the player has not found yet.
fn unfound_items(state: &GameState) -> usize {
    state
        .world
        .query::<(&Position, &Item, Option<&Found>)>()
        .iter()
        .filter(|(_, _, found)| found.is_none())
        .count()
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
        .filter(|(pos, _, found)| {
            found.is_none()
                && state
                    .remembered_items
                    .get(pos.x, pos.y)
                    .is_some_and(Option::is_some)
        })
        .map(|(pos, _, _)| (pos.x, pos.y))
        .collect();
    let targets: [&dyn Fn((i32, i32)) -> bool; 3] = [
        &|at| unfound.contains(&at),
        &|at| is_frontier(state, at),
        &|(x, y)| state.map.get(x, y) == Some(&Tile::StairsDown),
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
    Err(
        if state.map.get(from.x, from.y) == Some(&Tile::StairsDown) {
            "Nothing left to find here; > goes deeper.".to_string()
        } else {
            "There is nowhere left to explore.".to_string()
        },
    )
}

/// An enemy of the player in view that threatens them (`ai::threatens`):
/// hostiles and their summons, never the player's own, and not one fleeing
/// from further off.
pub(crate) fn visible_hostile(state: &GameState) -> Option<Entity> {
    state
        .world
        .query::<(Entity, &Position)>()
        .without::<&Dead>()
        .iter()
        .find(|&(entity, pos)| {
            flag(&state.visible, pos.x, pos.y)
                && side::enemies(&state.world, state.player, entity)
                && player_position(state).is_some_and(|me| crate::ai::threatens(state, entity, me))
        })
        .map(|(entity, _)| entity)
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
        // A foe running away is no wall: routing around it while it slips in
        // and out of view would turn the path back and forth.
        .filter(|&(entity, pos, _)| {
            entity != state.player
                && flag(&state.visible, pos.x, pos.y)
                && !(side::enemies(&state.world, state.player, entity)
                    && !crate::ai::threatens(state, entity, from))
        })
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
    use crate::cell::Rgb;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};

    /// Spawned after the starting view, so the view is refreshed to let the
    /// player remember it.
    fn item(state: &mut GameState, at: (i32, i32)) -> Entity {
        let knife = thing("knife")
            .look('/', Rgb(200, 200, 200))
            .item()
            .at(at.0, at.1)
            .make(state);
        crate::fov::update(state);
        knife
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
        state.map.set(30, 15, Tile::StairsDown);
        let near = item(&mut state, (5, 2));
        let far = item(&mut state, (35, 17));

        explore_until_stopped(&mut state);

        assert!(state.world.satisfies::<&Found>(near));
        assert!(state.world.satisfies::<&Found>(far));
        assert_eq!(at(&state), (30, 15));
        assert!(state.seen.cells().iter().filter(|&&s| s).count() > 0);
        let log: Vec<&String> = state.log.recent(10).collect();
        assert_eq!(log[0], "Nothing left to find here; > goes deeper.");
        assert!(log.iter().any(|m| *m == "You find the knife."));
    }

    #[test]
    fn cycling_the_item_setting_wraps_and_says_so() {
        let mut state = bare_state();
        assert_eq!(state.explore_items, ExploreItems::Find);
        let mut seen = Vec::new();
        for _ in 0..3 {
            cycle_explore_items(&mut state);
            seen.push(state.explore_items);
        }
        assert_eq!(
            seen,
            [ExploreItems::Stop, ExploreItems::PickUp, ExploreItems::Find]
        );
        assert_eq!(
            last_log(&state),
            "Auto-explore will note items and walk on."
        );
    }

    #[test]
    fn with_stop_it_halts_on_the_step_that_finds_an_item() {
        let mut state = bare_state();
        state.explore_items = ExploreItems::Stop;
        let knife = item(&mut state, (3, 1));
        assert!(explore_step(&mut state));
        assert!(!explore_step(&mut state), "the arriving step stops");
        assert_eq!(at(&state), (3, 1));
        assert!(state.world.satisfies::<&Found>(knife));
        // Found now, so the next run walks past it.
        assert!(explore_step(&mut state));
    }

    #[test]
    fn with_pick_up_it_takes_what_it_finds_and_walks_on() {
        let mut state = bare_state();
        state.explore_items = ExploreItems::PickUp;
        let knife = item(&mut state, (3, 1));
        assert!(explore_step(&mut state));
        assert!(explore_step(&mut state));
        assert!(state.world.get::<&Position>(knife).is_err());
        assert_eq!(crate::view::inventory_view(&state).len(), 1);
        assert_eq!(last_log(&state), "You pick up the knife.");
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
        thing("goblin").hostile().at(3, 1).make(&mut state);
        let now = state.now;

        assert!(!explore_step(&mut state));
        assert_eq!(at(&state), (1, 1));
        assert_eq!(state.now, now);
        assert_eq!(last_log(&state), "The goblin is in view.");
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
        assert_eq!(last_log(&state), "There is nowhere left to explore.");
    }

    #[test]
    fn travel_walks_known_ground_to_the_tile_and_stops_there() {
        let mut state = bare_state();
        let mut steps = 0;
        while travel_step(&mut state, (6, 3)) {
            steps += 1;
            assert!(steps < 100, "travel did not stop");
        }
        assert_eq!(at(&state), (6, 3));
        // Diagonals first: five steps cover five columns and two rows.
        assert_eq!(steps + 1, 5);
        let now = state.now;
        assert!(!travel_step(&mut state, (6, 3)), "already there");
        assert_eq!(state.now, now);
    }

    #[test]
    fn travel_refuses_unknown_ground_and_stops_for_enemies() {
        let mut state = bare_state();
        let now = state.now;
        assert!(!travel_step(&mut state, (38, 18)));
        assert_eq!(last_log(&state), "You know no way there.");
        assert_eq!(state.now, now);

        thing("goblin").hostile().at(3, 1).make(&mut state);
        assert!(!travel_step(&mut state, (2, 2)));
        assert_eq!(last_log(&state), "The goblin is in view.");
        assert_eq!(state.now, now);
    }
}
