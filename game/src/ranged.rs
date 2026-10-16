//! Flight of a thrown or fired thing (ADR 0013): the straight line it
//! travels, the first creature on it, and where it comes to rest. No RNG:
//! an attack always hits whoever is first in line.

use hecs::Entity;

use crate::combat::{roll_strike, throw_strike};
use crate::command::{Command, DamageCause};
use crate::entity::{Dead, EquippedOn, Found, Health, Position, Throwable};
use crate::equipment;
use crate::grammar::Subject;
use crate::holding;
use crate::map::Tile;
use crate::state::GameState;

/// The tiles from `from` (not included) to `to` (included): Bresenham's
/// line, integer only. Empty when the two are the same tile.
pub fn line(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((to.0 - from.0).abs(), -(to.1 - from.1).abs());
    let (step_x, step_y) = ((to.0 - from.0).signum(), (to.1 - from.1).signum());
    let (mut x, mut y) = from;
    let mut error = dx + dy;
    let mut tiles = Vec::new();
    while (x, y) != to {
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += step_x;
        }
        if doubled <= dx {
            error += dx;
            y += step_y;
        }
        tiles.push((x, y));
    }
    tiles
}

/// Tiles between two points as a flying thing counts them: diagonals cost
/// one, like a step.
pub fn distance(from: (i32, i32), to: (i32, i32)) -> i32 {
    (to.0 - from.0).abs().max((to.1 - from.1).abs())
}

/// Where a missile aimed at a tile goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flight {
    /// Every tile it passes, the landing tile last. Empty if it can't move.
    pub path: Vec<(i32, i32)>,
    /// Where it comes to rest: the creature's tile, the last open tile
    /// before a wall, or the end of its range.
    pub landing: (i32, i32),
    /// The first creature on the way, if any.
    pub hit: Option<Entity>,
}

/// `shooter`'s missile aimed at `to`, stopping after `range` tiles, at a
/// wall (it lands before it) or at the first living creature. The shooter's
/// own tile is never a target.
pub(crate) fn flight(
    state: &GameState,
    shooter: Entity,
    from: (i32, i32),
    to: (i32, i32),
    range: i32,
) -> Flight {
    let creatures: Vec<(Entity, (i32, i32))> = state
        .world
        .query::<(Entity, &Position, &Health)>()
        .without::<&Dead>()
        .iter()
        .filter(|&(entity, _, _)| entity != shooter)
        .map(|(entity, pos, _)| (entity, (pos.x, pos.y)))
        .collect();
    let mut path = Vec::new();
    let mut landing = from;
    for tile in line(from, to).into_iter().take(range.max(0) as usize) {
        if !Tile::is_open(state.map.get(tile.0, tile.1)) {
            break;
        }
        path.push(tile);
        landing = tile;
        if let Some(&(hit, _)) = creatures.iter().find(|&&(_, at)| at == tile) {
            return Flight {
                path,
                landing,
                hit: Some(hit),
            };
        }
    }
    Flight {
        path,
        landing,
        hit: None,
    }
}

/// Whether `actor` may throw `item` at `at` right now. The player can only
/// aim at tiles in view. Shared by `reject_input` and the aiming view, so
/// what the preview allows is what the command allows.
pub(crate) fn check_throw(
    state: &GameState,
    actor: Entity,
    item: Entity,
    at: (i32, i32),
) -> Result<(), String> {
    let held = holding::carried(&state.world, actor).contains(&item)
        || equipment::equipped(&state.world, actor)
            .iter()
            .any(|&(worn, _)| worn == item);
    if !held {
        return Err("You do not have that.".to_string());
    }
    let Ok(throwable) = state.world.get::<&Throwable>(item).map(|t| *t) else {
        return Err("You can't throw that.".to_string());
    };
    let Some(from) = state.world.get::<&Position>(actor).ok().map(|p| (p.x, p.y)) else {
        return Err("You can't throw from nowhere.".to_string());
    };
    if from == at {
        return Err("You can't throw at yourself.".to_string());
    }
    if distance(from, at) > throwable.range {
        return Err("That is out of range.".to_string());
    }
    if actor == state.player && !state.visible.get(at.0, at.1).copied().unwrap_or(false) {
        return Err("You can't see that spot.".to_string());
    }
    Ok(())
}

/// Applies `Command::Throw`: the item leaves its holder and comes to rest
/// where `flight` says; whoever is first in line takes `DealDamage`. The
/// damage roll is one combat-stream draw, made only when something is hit.
pub(crate) fn throw(state: &mut GameState, actor: Entity, item: Entity, at: (i32, i32)) {
    let Some(from) = state.world.get::<&Position>(actor).ok().map(|p| (p.x, p.y)) else {
        return;
    };
    let (Some(strike), Ok(throwable)) = (
        throw_strike(&state.world, actor, item),
        state.world.get::<&Throwable>(item).map(|t| *t),
    ) else {
        return;
    };
    let flight = flight(state, actor, from, at, throwable.range);

    if state.world.satisfies::<&EquippedOn>(item) {
        holding::take_off(&mut state.world, actor, item);
    } else {
        holding::unstow(&mut state.world, actor, item);
    }
    let _ = state.world.insert(
        item,
        (
            Position {
                x: flight.landing.0,
                y: flight.landing.1,
            },
            Found,
        ),
    );

    let what = Subject::of(&state.world, item);
    let aimed = flight.hit.map_or_else(String::new, |target| {
        format!(" at {}", Subject::of(&state.world, target).noun())
    });
    let message =
        Subject::of(&state.world, actor).sentence("throw", &format!("{}{aimed}", what.noun()));
    state.log.push(message);

    if let Some(target) = flight.hit {
        let hit = roll_strike(&mut state.rngs.combat, strike);
        state.commands.push_back(Command::DealDamage {
            target,
            amount: hit.amount,
            cause: DamageCause::Ranged {
                attacker: actor,
                types: hit.types,
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Armor, DamageType, OnHit, OnHitEntry, Stat};
    use crate::input::PlayerAction;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};
    use crate::time::take_turn;
    use crate::view::ItemRef;

    #[test]
    fn a_line_ends_on_its_target_and_skips_its_start() {
        assert_eq!(line((1, 1), (4, 1)), [(2, 1), (3, 1), (4, 1)]);
        assert_eq!(line((1, 1), (3, 3)), [(2, 2), (3, 3)]);
        assert_eq!(line((1, 1), (1, 1)), []);
    }

    #[test]
    fn a_line_has_one_tile_per_step_of_the_longer_axis() {
        for to in [(9, 4), (-3, 7), (0, -5), (6, -6), (2, 9)] {
            let tiles = line((0, 0), to);
            assert_eq!(tiles.len() as i32, distance((0, 0), to), "{to:?}");
            assert_eq!(tiles.last(), Some(&to));
        }
    }

    #[test]
    fn a_line_is_connected_and_deterministic() {
        let tiles = line((2, 2), (11, 6));
        assert_eq!(tiles, line((2, 2), (11, 6)));
        let mut previous = (2, 2);
        for tile in tiles {
            assert_eq!(distance(previous, tile), 1);
            previous = tile;
        }
    }

    #[test]
    fn a_missile_lands_at_the_end_of_its_range() {
        let state = bare_state();
        let flight = flight(&state, state.player, (1, 1), (9, 1), 4);
        assert_eq!((flight.landing, flight.hit), ((5, 1), None));
        assert_eq!(flight.path.len(), 4);
    }

    #[test]
    fn a_missile_stops_at_the_first_creature() {
        let mut state = bare_state();
        let near = thing("near").at(3, 1).health(5).make(&mut state);
        thing("far").at(5, 1).health(5).make(&mut state);
        let flight = flight(&state, state.player, (1, 1), (5, 1), 8);
        assert_eq!((flight.landing, flight.hit), ((3, 1), Some(near)));
    }

    #[test]
    fn a_missile_lands_before_a_wall() {
        let mut state = bare_state();
        state.map.set(4, 1, Tile::Wall);
        let flight = flight(&state, state.player, (1, 1), (7, 1), 8);
        assert_eq!((flight.landing, flight.hit), ((3, 1), None));
    }

    #[test]
    fn the_dead_and_the_shooter_are_not_in_the_way() {
        let mut state = bare_state();
        let corpse = thing("corpse").at(3, 1).health(5).make(&mut state);
        state.world.insert_one(corpse, Dead).unwrap();
        let flight = flight(&state, state.player, (1, 1), (4, 1), 8);
        assert_eq!((flight.landing, flight.hit), ((4, 1), None));
    }

    const CUTS_AND_PIERCES: [DamageType; 2] = [DamageType::Slashing, DamageType::Piercing];

    /// A knife in the player's pack that always rolls 3.
    fn knife(state: &mut GameState) -> Entity {
        thing("throwing knife")
            .item()
            .throwable(6, (3, 3), Stat::Agility, &CUTS_AND_PIERCES)
            .carried_by(state.player)
            .make(state)
    }

    fn goblin(state: &mut GameState, x: i32) -> Entity {
        thing("goblin").at(x, 1).health(10).hostile().make(state)
    }

    fn throw_at(state: &mut GameState, item: Entity, at: (i32, i32)) {
        state.run(Command::Throw {
            actor: state.player,
            item,
            at,
        });
    }

    fn health(state: &GameState, entity: Entity) -> i32 {
        state.world.get::<&Health>(entity).unwrap().current
    }

    fn position(state: &GameState, entity: Entity) -> Option<(i32, i32)> {
        let at = state.world.get::<&Position>(entity).ok()?;
        Some((at.x, at.y))
    }

    #[test]
    fn a_thrown_knife_hits_the_first_creature_and_lands_there() {
        let mut state = bare_state();
        let knife = knife(&mut state);
        let near = goblin(&mut state, 4);
        let far = goblin(&mut state, 6);
        throw_at(&mut state, knife, (6, 1));
        assert_eq!((health(&state, near), health(&state, far)), (7, 10));
        assert_eq!(position(&state, knife), Some((4, 1)));
        assert!(holding::carried(&state.world, state.player).is_empty());
        assert!(state.world.satisfies::<&Found>(knife));
        let log: Vec<_> = state.log.recent(2).cloned().collect();
        assert_eq!(
            log,
            [
                "The goblin takes 3 damage.",
                "You throw the throwing knife at the goblin."
            ]
        );
    }

    #[test]
    fn a_throw_at_an_empty_tile_lands_there_and_hurts_nobody() {
        let mut state = bare_state();
        let knife = knife(&mut state);
        let bystander = goblin(&mut state, 9);
        throw_at(&mut state, knife, (5, 1));
        assert_eq!(position(&state, knife), Some((5, 1)));
        assert_eq!(health(&state, bystander), 10);
        assert_eq!(last_log(&state), "You throw the throwing knife.");
    }

    #[test]
    fn a_throw_into_a_wall_lands_before_it() {
        let mut state = bare_state();
        let knife = knife(&mut state);
        state.map.set(4, 1, Tile::Wall);
        throw_at(&mut state, knife, (6, 1));
        assert_eq!(position(&state, knife), Some((3, 1)));
    }

    #[test]
    fn an_equipped_item_can_be_thrown() {
        let mut state = bare_state();
        let knife = thing("throwing knife")
            .item()
            .throwable(6, (3, 3), Stat::Agility, &CUTS_AND_PIERCES)
            .equipped_on(state.player, 3)
            .make(&mut state);
        throw_at(&mut state, knife, (4, 1));
        assert!(equipment::equipped(&state.world, state.player).is_empty());
        assert!(!state.world.satisfies::<&EquippedOn>(knife));
        assert_eq!(position(&state, knife), Some((4, 1)));
    }

    #[test]
    fn armor_resists_the_weakest_kind_of_a_thrown_blow() {
        let target = |slashing, piercing| {
            let mut state = bare_state();
            let knife = knife(&mut state);
            let goblin = goblin(&mut state, 4);
            thing("mail")
                .with(Armor {
                    slashing,
                    piercing,
                    blunt: 0,
                })
                .attached_to(goblin)
                .make(&mut state);
            throw_at(&mut state, knife, (4, 1));
            10 - health(&state, goblin)
        };
        // Both kinds armored: the lesser counts. One kind open: it all gets in.
        assert_eq!((target(2, 2), target(2, 1), target(2, 0)), (1, 2, 3));
    }

    #[test]
    fn a_thrown_blow_never_triggers_the_throwers_on_hit_parts() {
        let mut state = bare_state();
        let knife = knife(&mut state);
        let target = goblin(&mut state, 4);
        let bleed = OnHit(vec![OnHitEntry {
            effect: state.blueprints.id("Bleeding").unwrap(),
            chance_bp: 10_000,
        }]);
        state.world.insert_one(state.player, bleed).unwrap();
        throw_at(&mut state, knife, (4, 1));
        assert!(holding::attached(&state.world, target).is_empty());
    }

    #[test]
    fn the_player_cannot_throw_what_is_not_throwable_and_loses_no_time() {
        let mut state = bare_state();
        let rock = thing("lump")
            .item()
            .carried_by(state.player)
            .make(&mut state);
        let item = ItemRef::of(&state.world, rock).unwrap();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Throw { item, at: (3, 1) });
        assert_eq!(last_log(&state), "You can't throw that.");
        assert_eq!(state.now, before);
        assert_eq!(holding::carried(&state.world, state.player), [rock]);
    }

    #[test]
    fn throwing_takes_a_turn_and_is_refused_out_of_range_or_out_of_sight() {
        let mut state = bare_state();
        let knife = knife(&mut state);
        let item = ItemRef::of(&state.world, knife).unwrap();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Throw { item, at: (10, 1) });
        assert_eq!(last_log(&state), "That is out of range.");
        take_turn(&mut state, PlayerAction::Throw { item, at: (1, 1) });
        assert_eq!(last_log(&state), "You can't throw at yourself.");
        assert_eq!(state.now, before);

        take_turn(&mut state, PlayerAction::Throw { item, at: (4, 1) });
        assert!(state.now > before);
        assert_eq!(position(&state, knife), Some((4, 1)));
    }

    #[test]
    fn a_throw_is_deterministic_per_seed() {
        let run = |seed| {
            let mut state = crate::state::tests::bare_state_seeded(seed);
            let item = thing("knife")
                .item()
                .throwable(6, (1, 9), Stat::Agility, &CUTS_AND_PIERCES)
                .carried_by(state.player)
                .make(&mut state);
            let target = goblin(&mut state, 4);
            throw_at(&mut state, item, (4, 1));
            health(&state, target)
        };
        assert_eq!(run(7), run(7));
        assert!((1..30).map(run).any(|health| health != run(7)));
    }
}
