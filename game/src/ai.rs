//! Non-player decisions. AI only chooses a `Command`; the scheduler queues
//! it like any other action, so moving and bump-attacking reuse `TryMove`.

use hecs::Entity;

use crate::command::Command;
use crate::entity::{
    BlocksMovement, Dead, Health, Hostile, Position, SpawnSeq, Spell, SpellAi, Summoned,
};
use crate::fov;
use crate::holding;
use crate::input::Dir;
use crate::map::Tile;
use crate::ranged::line;
use crate::side;
use crate::spell;
use crate::state::GameState;

/// How far a summon with nothing to fight lets its owner get ahead.
const FOLLOW_DISTANCE: i32 = 2;

/// A hostile or a summon goes for the nearest enemy it perceives (ties by
/// `SpawnSeq`): with `SpellAi` it casts at it if a spell fits (`spell_at`),
/// else it attacks when adjacent and otherwise steps closer. A summon
/// with no enemy in view keeps near its owner. Anything else waits.
pub(crate) fn decide(state: &GameState, actor: Entity) -> Command {
    let wait = Command::Wait(actor);
    let world = &state.world;
    let owner = world
        .get::<&Summoned>(actor)
        .ok()
        .map(|summoned| summoned.owner);
    if !world.satisfies::<&Hostile>(actor) && owner.is_none() {
        return wait;
    }
    let Ok(me) = world.get::<&Position>(actor).map(|p| *p) else {
        return wait;
    };

    if let Some((foe, enemy)) = nearest_enemy(state, actor, me) {
        if world.satisfies::<&SpellAi>(actor)
            && let Some(cast) = spell_at(state, actor, foe, enemy)
        {
            return cast;
        }
        if chebyshev(me, enemy) == 1 {
            let toward = ((enemy.x - me.x).signum(), (enemy.y - me.y).signum());
            return match Dir::ALL.into_iter().find(|dir| dir.delta() == toward) {
                Some(dir) => Command::TryMove(actor, dir),
                None => wait,
            };
        }
        return step_toward(state, me, enemy)
            .map(|dir| Command::TryMove(actor, dir))
            .unwrap_or(wait);
    }
    let leader = owner.and_then(|owner| world.get::<&Position>(owner).ok().map(|p| *p));
    match leader {
        Some(leader) if chebyshev(me, leader) > FOLLOW_DISTANCE => step_toward(state, me, leader)
            .map(|dir| Command::TryMove(actor, dir))
            .unwrap_or(wait),
        _ => wait,
    }
}

/// Where the nearest living enemy `actor` perceives stands.
fn nearest_enemy(state: &GameState, actor: Entity, me: Position) -> Option<(Entity, Position)> {
    let world = &state.world;
    let candidates: Vec<(Entity, Position, SpawnSeq)> = world
        .query::<(Entity, &Position, &SpawnSeq)>()
        .with::<&Health>()
        .without::<&Dead>()
        .iter()
        .filter(|&(other, _, _)| other != actor && side::enemies(world, actor, other))
        .map(|(other, pos, seq)| (other, *pos, *seq))
        .collect();
    candidates
        .into_iter()
        .filter(|&(other, at, _)| perceives(state, actor, me, other, at))
        .min_by_key(|&(_, at, seq)| (chebyshev(me, at), seq))
        .map(|(other, at, _)| (other, at))
}

/// The first spell `actor` knows, in the order learned, that it could cast
/// at `enemy` now and that would hit it while sparing `actor`'s own side,
/// or a summoning with room to call: the same checks and footprint the
/// cast uses, placement on a clone of the spell stream.
fn spell_at(state: &GameState, actor: Entity, enemy: Entity, at: Position) -> Option<Command> {
    let world = &state.world;
    let target = (at.x, at.y);
    holding::attached(world, actor)
        .into_iter()
        .filter(|&spell| world.satisfies::<&Spell>(spell))
        .find(|&spell| {
            let Ok(ready) = spell::ready_cast(state, actor, spell) else {
                return false;
            };
            if spell::check_target(state, actor, &ready, target).is_err() {
                return false;
            }
            let mut rng = state.rngs.spell.clone();
            let Some(footprint) = spell::footprint(state, actor, &ready, target, &mut rng) else {
                return false;
            };
            let hits = footprint.caught.contains(&enemy)
                && footprint
                    .caught
                    .iter()
                    .all(|&caught| side::enemies(world, actor, caught));
            hits || (ready.summon.is_some() && !footprint.placed.is_empty())
        })
        .map(|spell| Command::Cast {
            actor,
            spell,
            at: target,
        })
}

/// Whether `actor`, standing at `me`, perceives `other` at `at`. The player
/// is perceived through the player's view (`can_see_player`). Anyone else
/// only when both are in the player's view, within sight radius of each
/// other, with open ground on the line between: an approximation that needs
/// no view per actor, so creatures out of the player's sight never fight
/// each other (ADR 0015).
fn perceives(state: &GameState, actor: Entity, me: Position, other: Entity, at: Position) -> bool {
    if other == state.player {
        return can_see_player(state, actor);
    }
    let shown = |pos: Position| state.visible.get(pos.x, pos.y) == Some(&true);
    let (dx, dy) = (at.x - me.x, at.y - me.y);
    shown(me)
        && shown(at)
        && dx * dx + dy * dy <= fov::RADIUS * fov::RADIUS
        && line((me.x, me.y), (at.x, at.y))
            .into_iter()
            .all(|(x, y)| Tile::is_open(state.map.get(x, y)))
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
    use crate::testkit::{last_log, see_everything, sure_hits, thing};
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
        let player = state.player;
        sure_hits(&mut state, player);
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
        let player = state.player;
        sure_hits(&mut state, player);
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

    fn wolf(state: &mut GameState, at: (i32, i32)) -> Entity {
        let player = state.player;
        thing("wolf")
            .at(at.0, at.1)
            .health(8)
            .summoned_by(player)
            .make(state)
    }

    fn goblin_at(state: &mut GameState, at: (i32, i32)) -> Entity {
        thing("goblin")
            .at(at.0, at.1)
            .health(8)
            .hostile()
            .make(state)
    }

    #[test]
    fn a_hostile_never_bumps_into_its_own_side() {
        let mut state = bare_state();
        goblin_at(&mut state, (2, 1));
        let behind = goblin_at(&mut state, (3, 1));
        see_everything(&mut state);
        assert_ne!(decide(&state, behind), Command::TryMove(behind, Dir::W));
    }

    #[test]
    fn a_summon_goes_for_the_nearest_enemy() {
        let mut state = bare_state();
        let wolf = wolf(&mut state, (4, 1));
        goblin_at(&mut state, (5, 1));
        goblin_at(&mut state, (4, 6));
        see_everything(&mut state);
        assert_eq!(decide(&state, wolf), Command::TryMove(wolf, Dir::E));
    }

    #[test]
    fn a_hostile_goes_for_a_summon_nearer_than_the_player() {
        let mut state = bare_state();
        wolf(&mut state, (5, 1));
        let goblin = goblin_at(&mut state, (6, 1));
        see_everything(&mut state);
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::W));
    }

    #[test]
    fn out_of_the_players_sight_creatures_do_not_fight_and_summons_follow() {
        let mut state = bare_state();
        let wolf = wolf(&mut state, (4, 1));
        goblin_at(&mut state, (5, 1));
        see_everything(&mut state);
        state.visible.set(5, 1, false);
        assert_eq!(
            decide(&state, wolf),
            Command::TryMove(wolf, Dir::W),
            "back toward its owner"
        );
        let close = self::wolf(&mut state, (2, 2));
        assert_eq!(decide(&state, close), Command::Wait(close));
    }

    #[test]
    fn the_player_trades_places_with_their_own_summon() {
        let mut state = bare_state();
        let wolf = wolf(&mut state, (2, 1));
        state.run(Command::TryMove(state.player, Dir::E));
        assert_eq!(position(&state, state.player), Position { x: 2, y: 1 });
        assert_eq!(position(&state, wolf), Position { x: 1, y: 1 });
        assert_eq!(state.world.get::<&Health>(wolf).unwrap().current, 8);
    }
}
