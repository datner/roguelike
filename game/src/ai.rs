//! Non-player decisions. AI only chooses a `Command`; the scheduler queues
//! it like any other action, so moving and bump-attacking reuse `TryMove`.

use hecs::Entity;

use crate::command::Command;
use crate::entity::{
    BlocksMovement, Dead, Health, Hostile, Morale, Position, Quarry, Roams, Route, SpawnSeq, Spell,
    SpellAi, Summoned, TechniqueAi,
};
use crate::fov;
use crate::holding;
use crate::input::Dir;
use crate::map::Grid;
use crate::map::Tile;
use crate::ranged::line;
use crate::side;
use crate::spell;
use crate::state::GameState;
use crate::technique;

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
    // Winding up: it holds still until the blow lands.
    if technique::winding_up(world, actor).is_some() {
        return wait;
    }

    if let Some((foe, enemy)) = nearest_enemy(state, actor, me) {
        // Hurt past its morale, or a skirmisher with a foe at its elbow:
        // it backs off while it can.
        let pressed =
            world.satisfies::<&crate::entity::Skirmish>(actor) && chebyshev(me, enemy) == 1;
        if (shaken(state, actor) || pressed)
            && let Some(away) = step_away(state, me, enemy)
        {
            return Command::TryMove(actor, away);
        }
        if world.satisfies::<&SpellAi>(actor)
            && let Some(cast) = spell_at(state, actor, foe, enemy)
        {
            return cast;
        }
        if world.satisfies::<&TechniqueAi>(actor)
            && let Some(perform) = technique_at(state, actor, foe, enemy)
        {
            return perform;
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
    if let Ok(quarry) = world.get::<&Quarry>(actor).map(|q| q.at) {
        let at = Position {
            x: quarry.0,
            y: quarry.1,
        };
        if let Some(dir) = step_toward(state, me, at) {
            return Command::TryMove(actor, dir);
        }
    }
    if let Ok(to) = world.get::<&Route>(actor).map(|route| route.to)
        && let Some(dir) = path_step(state, me, to)
    {
        return Command::TryMove(actor, dir);
    }
    let leader = owner.and_then(|owner| world.get::<&Position>(owner).ok().map(|p| *p));
    match leader {
        Some(leader) if chebyshev(me, leader) > FOLLOW_DISTANCE => step_toward(state, me, leader)
            .map(|dir| Command::TryMove(actor, dir))
            .unwrap_or(wait),
        _ => wait,
    }
}

/// What `actor` remembers before deciding: where it sees its nearest enemy
/// now, or nothing once it stands where it last saw one (or can get no
/// closer) with none in sight. `None` when the memory is already right.
pub(crate) fn notice(state: &GameState, actor: Entity) -> Option<Command> {
    let world = &state.world;
    if !world.satisfies::<&Hostile>(actor) && !world.satisfies::<&Summoned>(actor) {
        return None;
    }
    let me = world.get::<&Position>(actor).ok().map(|p| *p)?;
    let known = world.get::<&Quarry>(actor).ok().map(|q| q.at);
    let at = match nearest_enemy(state, actor, me) {
        Some((_, enemy)) => Some((enemy.x, enemy.y)),
        None => known.filter(|&(x, y)| {
            let goal = Position { x, y };
            goal != me && step_toward(state, me, goal).is_some()
        }),
    };
    (at != known).then_some(Command::Notice { actor, at })
}

/// A roamer with no quarry and no enemy in sight that has arrived, or has no
/// way on, sets off for the farthest open ground it can reach.
pub(crate) fn route(state: &GameState, actor: Entity) -> Option<Command> {
    let world = &state.world;
    if !world.satisfies::<&Roams>(actor) || world.satisfies::<&Quarry>(actor) {
        return None;
    }
    let me = world.get::<&Position>(actor).ok().map(|p| *p)?;
    if nearest_enemy(state, actor, me).is_some() {
        return None;
    }
    // On its way unless it has arrived; a way blocked for now (a creature in
    // a corridor) is waited out, not re-planned.
    let going = world
        .get::<&Route>(actor)
        .ok()
        .is_some_and(|route| route.to != (me.x, me.y));
    if going {
        return None;
    }
    let to = reach(state, me).order.last().copied()?;
    Some(Command::SetRoute { actor, to })
}

/// Open ground reachable from `from`, nearest first (ties in `Dir::ALL`
/// order), each with the first step toward it. Creatures don't block: a
/// patrol's way is the floor's, not today's crowd.
struct Reach {
    order: Vec<(i32, i32)>,
    first: Grid<Option<Dir>>,
}

fn reach(state: &GameState, from: Position) -> Reach {
    let map = &state.map;
    let mut first = Grid::new(map.width, map.height, None);
    let mut seen = Grid::new(map.width, map.height, false);
    seen.set(from.x, from.y, true);
    let mut order = vec![(from.x, from.y)];
    let mut next = 0;
    while let Some(&(x, y)) = order.get(next) {
        next += 1;
        let step = first.get(x, y).copied().flatten();
        for dir in Dir::ALL {
            let (dx, dy) = dir.delta();
            let to = (x + dx, y + dy);
            if seen.get(to.0, to.1) != Some(&false) || !Tile::is_way(map.get(to.0, to.1)) {
                continue;
            }
            seen.set(to.0, to.1, true);
            first.set(to.0, to.1, Some(step.unwrap_or(dir)));
            order.push(to);
        }
    }
    Reach { order, first }
}

/// A step toward `to`: straight when one gets closer (cheap, and right in
/// the open), else the first step on a shortest way, if it is free now.
pub(crate) fn path_step(state: &GameState, from: Position, to: (i32, i32)) -> Option<Dir> {
    if let Some(dir) = step_toward(state, from, Position { x: to.0, y: to.1 }) {
        return Some(dir);
    }
    let dir = reach(state, from)
        .first
        .get(to.0, to.1)
        .copied()
        .flatten()?;
    let (dx, dy) = dir.delta();
    passable(
        state,
        Position {
            x: from.x + dx,
            y: from.y + dy,
        },
    )
    .then_some(dir)
}

/// `Command::Notice` applier.
pub(crate) fn remember(state: &mut GameState, actor: Entity, at: Option<(i32, i32)>) {
    match at {
        Some(at) => {
            let _ = state.world.insert_one(actor, Quarry { at });
        }
        None => {
            let _ = state.world.remove_one::<Quarry>(actor);
        }
    }
}

/// Whether `actor` is a threat to whoever stands at `to`: anything but a
/// creature fleeing from further off. Auto-runs stop for threats only, so a
/// goblin backing off at the edge of sight never blocks exploring; walking
/// up to one (explore paths through it) makes it a threat again.
pub(crate) fn threatens(state: &GameState, actor: Entity, to: Position) -> bool {
    !shaken(state, actor)
        || state
            .world
            .get::<&Position>(actor)
            .is_ok_and(|at| chebyshev(*at, to) <= 1)
}

/// Below its morale: hurt past the point where it would rather not fight.
fn shaken(state: &GameState, actor: Entity) -> bool {
    let world = &state.world;
    let (Ok(morale), Ok(health)) = (world.get::<&Morale>(actor), world.get::<&Health>(actor))
    else {
        return false;
    };
    i64::from(health.current) * 100 < i64::from(health.max) * i64::from(morale.flee_below)
}

/// The passable step that ends farthest from `threat` (ties to the earlier
/// entry in `Dir::ALL`); `None` if no step gets farther: cornered.
fn step_away(state: &GameState, from: Position, threat: Position) -> Option<Dir> {
    let current = chebyshev(from, threat);
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
                let (ex, ey) = (threat.x - next.x, threat.y - next.y);
                (
                    (
                        chebyshev(next, threat),
                        ex * ex + ey * ey,
                        std::cmp::Reverse(order),
                    ),
                    dir,
                )
            })
        })
        .max_by_key(|&(key, _)| key)
        .filter(|&((distance, _, _), _)| distance > current)
        .map(|(_, dir)| dir)
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

/// The first technique `actor` knows, in the order learned, that it could
/// perform at `enemy` now and that would strike it and none of `actor`'s own
/// side: the same checks and footprint the performance uses.
fn technique_at(state: &GameState, actor: Entity, enemy: Entity, at: Position) -> Option<Command> {
    let world = &state.world;
    let target = (at.x, at.y);
    technique::known(world, actor)
        .into_iter()
        .find(|&known| {
            let Ok(ready) = technique::ready(state, actor, known) else {
                return false;
            };
            technique::footprint(state, actor, &ready, target).is_ok_and(|footprint| {
                footprint.targets.contains(&enemy)
                    && footprint
                        .targets
                        .iter()
                        .all(|&struck| side::enemies(world, actor, struck))
            })
        })
        .map(|technique| Command::Perform {
            actor,
            technique,
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
    if crate::fov::in_grass_beyond(state, at, Some(me)) {
        return false;
    }
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
            .all(|(x, y)| Tile::is_transparent(state.map.get(x, y)))
}

/// Whether `actor` perceives the player.
///
/// Assumption: an actor sees the player iff the actor's tile is in the
/// player's visible set (`GameState::visible`). That is only sound while
/// all three hold:
/// - FOV is symmetric (`fov` — Ford's shadowcasting), so "the player sees
///   me" equals "I see the player";
/// - lighting is uniform (no darkness makes one side unseen);
/// - every actor's sight radius equals the player's (`fov::RADIUS`). A
///   held light stretches the player's view past it; the player is then
///   lit, so being seen from as far as they see is the point, not a bug.
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
pub(crate) fn step_toward(state: &GameState, from: Position, goal: Position) -> Option<Dir> {
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
    if !Tile::is_way(state.map.get(at.x, at.y)) {
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
    fn a_goblin_that_loses_sight_goes_where_it_last_saw_you_then_forgets() {
        let (mut state, goblin) = with_goblin(Position { x: 6, y: 1 });
        see_everything(&mut state);
        state.run(notice(&state, goblin).unwrap());
        let quarry = state.world.get::<&Quarry>(goblin).unwrap().at;
        assert_eq!(quarry, (1, 1));
        // The player is gone from its sight: it walks to the spot.
        state.visible = crate::map::Grid::new(state.map.width, state.map.height, false);
        assert_eq!(notice(&state, goblin), None, "still worth chasing");
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::W));
        *state.world.get::<&mut Position>(goblin).unwrap() = Position { x: 1, y: 1 };
        assert_eq!(
            notice(&state, goblin),
            Some(Command::Notice {
                actor: goblin,
                at: None
            })
        );
    }

    #[test]
    fn a_hurt_goblin_backs_off_unless_cornered() {
        let (mut state, goblin) = with_goblin(Position { x: 2, y: 1 });
        see_everything(&mut state);
        state
            .world
            .insert_one(goblin, Morale { flee_below: 25 })
            .unwrap();
        assert_ne!(
            decide(&state, goblin),
            Command::TryMove(goblin, Dir::E),
            "unhurt, it fights"
        );
        state.world.get::<&mut Health>(goblin).unwrap().current = 1;
        let Command::TryMove(_, dir) = decide(&state, goblin) else {
            panic!("it moves");
        };
        let (dx, _) = dir.delta();
        assert_eq!(dx, 1, "away from the player at (1, 1)");
        // Walls on every side but the player's: cornered, it fights.
        for (x, y) in [(3, 0), (3, 1), (3, 2), (2, 0), (2, 2), (1, 0), (1, 2)] {
            state.map.set(x, y, Tile::Wall);
        }
        assert_eq!(decide(&state, goblin), Command::TryMove(goblin, Dir::W));
    }

    #[test]
    fn a_fleeing_goblin_far_off_does_not_stop_a_run_but_an_adjacent_one_does() {
        let (mut state, goblin) = with_goblin(Position { x: 5, y: 1 });
        see_everything(&mut state);
        state
            .world
            .insert_one(goblin, Morale { flee_below: 25 })
            .unwrap();
        state.world.get::<&mut Health>(goblin).unwrap().current = 1;
        let me = Position { x: 1, y: 1 };
        assert!(!threatens(&state, goblin, me));
        assert_eq!(crate::explore::visible_hostile(&state), None);
        assert!(
            threatens(&state, goblin, Position { x: 4, y: 1 }),
            "beside it"
        );
    }

    #[test]
    fn a_roamer_patrols_to_the_far_end_and_back() {
        let (mut state, scout) = with_goblin(Position { x: 5, y: 5 });
        state.world.insert_one(scout, Roams).unwrap();
        // Out of the player's sight, so it has no one to hunt.
        state.visible = Grid::new(state.map.width, state.map.height, false);
        let Some(Command::SetRoute { to, .. }) = route(&state, scout) else {
            panic!("it sets off");
        };
        let far = chebyshev(Position { x: 5, y: 5 }, Position { x: to.0, y: to.1 });
        assert!(far > 20, "the far end of the room, not {to:?}");
        assert_eq!(
            route(&state, scout),
            route(&state, scout),
            "the same from the same state"
        );
        state.run(Command::SetRoute { actor: scout, to });
        assert_eq!(route(&state, scout), None, "on its way");
        assert!(matches!(decide(&state, scout), Command::TryMove(..)));
        *state.world.get::<&mut Position>(scout).unwrap() = Position { x: to.0, y: to.1 };
        let Some(Command::SetRoute { to: back, .. }) = route(&state, scout) else {
            panic!("it turns around");
        };
        assert_ne!(back, to);
    }

    #[test]
    fn a_roamer_that_sees_you_hunts_you() {
        let (mut state, scout) = with_goblin(Position { x: 5, y: 1 });
        state.world.insert_one(scout, Roams).unwrap();
        see_everything(&mut state);
        assert_eq!(route(&state, scout), None);
        assert_eq!(decide(&state, scout), Command::TryMove(scout, Dir::W));
    }

    #[test]
    fn in_tall_grass_you_are_seen_only_from_the_next_tile() {
        let (mut state, goblin) = with_goblin(Position { x: 3, y: 1 });
        see_everything(&mut state);
        state.map.set(1, 1, Tile::Grass);
        assert_eq!(nearest_enemy(&state, goblin, Position { x: 3, y: 1 }), None);
        *state.world.get::<&mut Position>(goblin).unwrap() = Position { x: 2, y: 1 };
        assert!(nearest_enemy(&state, goblin, Position { x: 2, y: 1 }).is_some());
    }

    #[test]
    fn a_goblin_in_grass_is_not_drawn_beyond_the_next_tile() {
        let (mut state, _) = with_goblin(Position { x: 4, y: 1 });
        state.map.set(4, 1, Tile::Grass);
        state.run(Command::UpdateFov);
        let glyph_at = |state: &GameState, x| {
            let view = crate::state::render_view(state, state.map.width, state.map.height);
            let me = *state.world.get::<&Position>(state.player).unwrap();
            let camera = crate::camera::Camera::follow(
                (me.x, me.y),
                (state.map.width, state.map.height),
                (state.map.width, state.map.height),
            );
            let (sx, sy) = camera.world_to_screen((x, 1));
            view.get(sx, sy).map(|cell| cell.glyph)
        };
        assert_eq!(glyph_at(&state, 4), Some('"'));
        thing_moves(&mut state, (3, 1));
        assert_eq!(glyph_at(&state, 4), Some('g'));
    }

    fn thing_moves(state: &mut GameState, to: (i32, i32)) {
        let player = state.player;
        *state.world.get::<&mut Position>(player).unwrap() = Position { x: to.0, y: to.1 };
        state.run(Command::UpdateFov);
    }

    fn archer_at(at: Position) -> (GameState, Entity) {
        let data = crate::blueprint::Blueprints::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap();
        let mut state = crate::state::tests::bare_state_with(data);
        let archer = state
            .spawn(Spawn::blueprint("GoblinArcher").with(at))
            .unwrap();
        let quiver = crate::equipment::equipped(&state.world, archer)
            .into_iter()
            .map(|(item, _)| item)
            .find(|&item| state.world.satisfies::<&crate::entity::Quiver>(item))
            .unwrap();
        for _ in 0..4 {
            state
                .spawn(Spawn::blueprint("Arrow").carried_by(quiver))
                .unwrap();
        }
        see_everything(&mut state);
        (state, archer)
    }

    #[test]
    fn an_archer_steps_away_from_your_elbow_and_shoots_from_afar() {
        let (state, archer) = archer_at(Position { x: 2, y: 1 });
        let Command::TryMove(_, dir) = decide(&state, archer) else {
            panic!("it moves");
        };
        assert_eq!(dir.delta().0, 1, "away from the player at (1, 1)");
        let (state, archer) = archer_at(Position { x: 6, y: 1 });
        assert!(
            matches!(decide(&state, archer), Command::Perform { .. }),
            "it shoots"
        );
    }

    #[test]
    fn a_cornered_archer_fights() {
        let (mut state, archer) = archer_at(Position { x: 2, y: 1 });
        for (x, y) in [(3, 0), (3, 1), (3, 2), (2, 0), (2, 2), (1, 0), (1, 2)] {
            state.map.set(x, y, Tile::Wall);
        }
        assert!(!matches!(decide(&state, archer), Command::TryMove(_, dir) if dir.delta().0 > 0));
    }

    #[test]
    fn a_placed_archer_has_arrows() {
        let data = crate::blueprint::Blueprints::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap()
        .with_camp(None);
        let state = GameState::new(3, data).unwrap();
        let archer = state
            .world
            .query::<(Entity, &crate::entity::Name)>()
            .iter()
            .find(|(_, name)| name.0 == "goblin archer")
            .map(|(entity, _)| entity)
            .expect("an archer on the first floor");
        let arrows = crate::equipment::equipped(&state.world, archer)
            .into_iter()
            .filter_map(|(item, _)| {
                state
                    .world
                    .get::<&crate::entity::Inventory>(item)
                    .ok()
                    .map(|inv| inv.0.len())
            })
            .sum::<usize>();
        assert_eq!(arrows, 8);
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
