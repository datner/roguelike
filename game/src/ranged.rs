//! Flight of a thrown or fired thing (ADR 0013): the straight line it
//! travels, the first creature on it, and where it comes to rest. No RNG:
//! an attack always hits whoever is first in line.

use hecs::Entity;
use rand::RngExt;

use crate::ammo;
use crate::combat::{Strike, fire_strike, roll_strike, throw_strike};
use crate::command::{Command, DamageCause};
use crate::effect::BASIS_POINTS;
use crate::entity::{Dead, EquippedOn, Found, Health, Position, Throwable};
use crate::equipment;
use crate::grammar::{Subject, capitalize};
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

/// A missile ready to leave the shooter: what flies, how it hurts and how
/// far it goes. Built the same way for the command and for the aiming
/// preview, so they cannot disagree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ready {
    pub missile: Entity,
    pub strike: Strike,
    pub range: i32,
    /// Where the missile comes from: a launcher or quiver, or `None` for
    /// the shooter's own hands (carried or equipped).
    container: Option<Entity>,
    verb: &'static str,
    /// Chance, out of 10 000, that it is lost when it hits a creature.
    break_bp: Option<u32>,
}

/// `actor` throwing `item`, which they must hold and which must be throwable.
pub(crate) fn ready_throw(state: &GameState, actor: Entity, item: Entity) -> Result<Ready, String> {
    let world = &state.world;
    let held = holding::carried(world, actor).contains(&item)
        || equipment::equipped(world, actor)
            .iter()
            .any(|&(worn, _)| worn == item);
    if !held {
        return Err("You do not have that.".to_string());
    }
    let (Some(strike), Ok(throwable)) = (
        throw_strike(world, actor, item),
        world.get::<&Throwable>(item).map(|t| *t),
    ) else {
        return Err("You can't throw that.".to_string());
    };
    Ok(Ready {
        missile: item,
        strike,
        range: throwable.range,
        container: None,
        verb: "throw",
        break_bp: None,
    })
}

/// `actor` firing the launcher they wield with the arrow it would use: the
/// nocked one, else the next in an equipped quiver.
pub(crate) fn ready_fire(state: &GameState, actor: Entity) -> Result<Ready, String> {
    let world = &state.world;
    let Some((launcher, spec)) = ammo::wielded_launcher(world, actor) else {
        return Err("You have nothing to fire.".to_string());
    };
    let Some((arrow, container)) = ammo::next_arrow(world, actor, launcher, spec.ammo) else {
        return Err("You have no arrows.".to_string());
    };
    let Some((strike, break_bp)) = fire_strike(world, actor, spec, arrow) else {
        return Err("You can't fire that.".to_string());
    };
    Ok(Ready {
        missile: arrow,
        strike,
        range: spec.range,
        container: Some(container),
        verb: "shoot",
        break_bp: Some(break_bp),
    })
}

/// Whether `ready` may be aimed at `at` right now. The player can only aim
/// at tiles in view.
pub(crate) fn check(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    at: (i32, i32),
) -> Result<(), String> {
    let Some(from) = position(state, actor) else {
        return Err("You can't aim from nowhere.".to_string());
    };
    if from == at {
        return Err("You can't aim at yourself.".to_string());
    }
    if distance(from, at) > ready.range {
        return Err("That is out of range.".to_string());
    }
    if actor == state.player && !state.visible.get(at.0, at.1).copied().unwrap_or(false) {
        return Err("You can't see that spot.".to_string());
    }
    Ok(())
}

fn position(state: &GameState, entity: Entity) -> Option<(i32, i32)> {
    let at = state.world.get::<&Position>(entity).ok()?;
    Some((at.x, at.y))
}

/// Why `Throw` is refused, if it is.
pub(crate) fn check_throw(
    state: &GameState,
    actor: Entity,
    item: Entity,
    at: (i32, i32),
) -> Result<(), String> {
    check(state, actor, &ready_throw(state, actor, item)?, at)
}

/// Why `Fire` is refused, if it is.
pub(crate) fn check_fire(state: &GameState, actor: Entity, at: (i32, i32)) -> Result<(), String> {
    check(state, actor, &ready_fire(state, actor)?, at)
}

/// Applies `Command::Throw`.
pub(crate) fn throw(state: &mut GameState, actor: Entity, item: Entity, at: (i32, i32)) {
    if let Ok(ready) = ready_throw(state, actor, item) {
        launch(state, actor, ready, at);
    }
}

/// Applies `Command::Fire`.
pub(crate) fn fire(state: &mut GameState, actor: Entity, at: (i32, i32)) {
    if let Ok(ready) = ready_fire(state, actor) {
        launch(state, actor, ready, at);
    }
}

/// Sends `ready` toward `at`: it leaves its holder and comes to rest where
/// `flight` says, and whoever is first in line takes `DealDamage`. The
/// damage roll is one combat-stream draw, made only when something is hit;
/// ammunition then makes a second, for breaking.
fn launch(state: &mut GameState, actor: Entity, ready: Ready, at: (i32, i32)) {
    let Some(from) = position(state, actor) else {
        return;
    };
    let item = ready.missile;
    let flight = flight(state, actor, from, at, ready.range);

    match ready.container {
        Some(container) => holding::unstow(&mut state.world, container, item),
        None if state.world.satisfies::<&EquippedOn>(item) => {
            holding::take_off(&mut state.world, actor, item);
        }
        None => holding::unstow(&mut state.world, actor, item),
    }

    let what = Subject::of(&state.world, item);
    let aimed = flight.hit.map_or_else(String::new, |target| {
        format!(" at {}", Subject::of(&state.world, target).noun())
    });
    let message =
        Subject::of(&state.world, actor).sentence(ready.verb, &format!("{}{aimed}", what.noun()));
    state.log.push(message);

    let Some(target) = flight.hit else {
        land(state, item, flight.landing);
        return;
    };
    let hit = roll_strike(&mut state.rngs.combat, ready.strike);
    let breaks = ready
        .break_bp
        .is_some_and(|chance| state.rngs.combat.random_range(0..BASIS_POINTS) < u64::from(chance));
    if breaks {
        let _ = state.world.despawn(item);
        state
            .log
            .push(format!("{} breaks.", capitalize(what.noun())));
    } else {
        land(state, item, flight.landing);
    }
    state.commands.push_back(Command::DealDamage {
        target,
        amount: hit.amount,
        cause: DamageCause::Ranged {
            attacker: actor,
            types: hit.types,
        },
    });
}

/// Comes to rest on the floor, found: whoever threw it knows where it is.
fn land(state: &mut GameState, item: Entity, (x, y): (i32, i32)) {
    let _ = state.world.insert(item, (Position { x, y }, Found));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{AmmoKind, Armor, DamageType, OnHit, OnHitEntry, Stat};
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
        assert_eq!(last_log(&state), "You can't aim at yourself.");
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
    fn bow(state: &mut GameState) -> Entity {
        thing("bow")
            .item()
            .launcher(8, AmmoKind::Arrow, Stat::Agility)
            .equipped_on(state.player, 3)
            .make(state)
    }

    fn quiver(state: &mut GameState) -> Entity {
        thing("quiver")
            .item()
            .quiver(AmmoKind::Arrow, 10)
            .equipped_on(state.player, 2)
            .make(state)
    }

    /// An arrow that always rolls `damage` and breaks with chance `break_bp`.
    fn arrow(damage: i32, break_bp: u32) -> crate::spawn::Spawn {
        thing("arrow").item().ammo(AmmoKind::Arrow).projectile(
            damage,
            damage,
            &[DamageType::Piercing],
            break_bp,
        )
    }

    fn stocked_quiver(state: &mut GameState, arrows: usize) -> (Entity, Vec<Entity>) {
        let quiver = quiver(state);
        let loaded = (0..arrows)
            .map(|_| {
                let arrow = arrow(2, 0).make(state);
                holding::stow(&mut state.world, quiver, arrow);
                arrow
            })
            .collect();
        (quiver, loaded)
    }

    fn fire_at(state: &mut GameState, at: (i32, i32)) {
        state.run(Command::Fire {
            actor: state.player,
            at,
        });
    }

    #[test]
    fn firing_uses_the_quiver_and_the_arrow_lands_where_it_hit() {
        let mut state = bare_state();
        bow(&mut state);
        let (quiver, arrows) = stocked_quiver(&mut state, 2);
        let target = goblin(&mut state, 4);
        fire_at(&mut state, (4, 1));
        assert_eq!(health(&state, target), 8);
        assert_eq!(position(&state, arrows[0]), Some((4, 1)));
        assert_eq!(holding::carried(&state.world, quiver), [arrows[1]]);
        let log: Vec<_> = state.log.recent(2).cloned().collect();
        assert_eq!(
            log,
            [
                "The goblin takes 2 damage.",
                "You shoot the arrow at the goblin."
            ]
        );
    }

    #[test]
    fn a_nocked_arrow_overrides_the_quiver_and_is_used_first() {
        let mut state = bare_state();
        let bow = bow(&mut state);
        let (quiver, arrows) = stocked_quiver(&mut state, 2);
        let special = arrow(5, 0).carried_by(state.player).make(&mut state);
        state.run(Command::Nock {
            actor: state.player,
            arrow: special,
        });
        let target = goblin(&mut state, 4);

        fire_at(&mut state, (4, 1));
        assert_eq!(health(&state, target), 5);
        assert!(holding::carried(&state.world, bow).is_empty());
        assert_eq!(holding::carried(&state.world, quiver), arrows);

        fire_at(&mut state, (4, 1));
        assert_eq!(health(&state, target), 3);
    }

    #[test]
    fn the_shooters_stat_adds_to_the_arrows_dice() {
        let mut state = bare_state();
        bow(&mut state);
        stocked_quiver(&mut state, 1);
        state
            .world
            .insert_one(
                state.player,
                crate::entity::Stats {
                    strength: 16,
                    agility: 20,
                },
            )
            .unwrap();
        let target = goblin(&mut state, 4);
        fire_at(&mut state, (4, 1));
        assert_eq!(health(&state, target), 6, "2 + 2 Agility");
    }

    #[test]
    fn an_arrow_that_hits_may_break_and_one_that_misses_never_does() {
        let mut state = bare_state();
        bow(&mut state);
        let quiver = quiver(&mut state);
        let doomed = arrow(2, 10_000).make(&mut state);
        let spared = arrow(2, 10_000).make(&mut state);
        holding::stow(&mut state.world, quiver, doomed);
        holding::stow(&mut state.world, quiver, spared);
        let target = goblin(&mut state, 4);

        fire_at(&mut state, (4, 1));
        assert!(!state.world.contains(doomed));
        assert_eq!(health(&state, target), 8, "a breaking arrow still hurts");
        assert!(state.log.recent(3).any(|line| line == "The arrow breaks."));

        fire_at(&mut state, (1, 5));
        assert_eq!(position(&state, spared), Some((1, 5)));
    }

    #[test]
    fn arrows_are_resisted_by_piercing_armor() {
        let mut state = bare_state();
        bow(&mut state);
        stocked_quiver(&mut state, 1);
        let target = goblin(&mut state, 4);
        thing("plate")
            .with(Armor {
                piercing: 1,
                ..Armor::default()
            })
            .attached_to(target)
            .make(&mut state);
        fire_at(&mut state, (4, 1));
        assert_eq!(health(&state, target), 9);
    }

    #[test]
    fn firing_is_refused_without_a_bow_or_arrows_and_costs_no_time() {
        let mut state = bare_state();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Fire { at: (4, 1) });
        assert_eq!(last_log(&state), "You have nothing to fire.");

        bow(&mut state);
        take_turn(&mut state, PlayerAction::Fire { at: (4, 1) });
        assert_eq!(last_log(&state), "You have no arrows.");

        stocked_quiver(&mut state, 1);
        take_turn(&mut state, PlayerAction::Fire { at: (20, 1) });
        assert_eq!(last_log(&state), "That is out of range.");
        assert_eq!(state.now, before);

        take_turn(&mut state, PlayerAction::Fire { at: (4, 1) });
        assert!(state.now > before);
    }

    #[test]
    fn nocking_costs_a_turn_even_with_a_full_quiver() {
        let mut state = bare_state();
        bow(&mut state);
        stocked_quiver(&mut state, 3);
        let special = arrow(5, 0).carried_by(state.player).make(&mut state);
        let item = ItemRef::of(&state.world, special).unwrap();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Nock(item));
        assert!(state.now > before);
        assert_eq!(last_log(&state), "You nock the arrow.");
    }
}
