//! Dives (docs/plans/dives.md): going down the stairs to a new floor. A floor exists only while you are on it: going down builds the next one from its own seed and forgets this one, keeping only you, what you hold, your effects and your summons.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use rand_pcg::Pcg64;

use crate::entity::{Position, Summoned};
use crate::map::{Grid, Map, Tile};
use crate::rng::Rngs;
use crate::state::GameState;
use crate::zone::{
    CaveParams, ZoneCoord, generate_cave, mix, place_stairs_down, region, start_position, zone_seed,
};

/// Where the player is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Place {
    /// `floor` of the `dive`th dive, both counted from 1, in the dungeon
    /// `data/dungeons/<dungeon>.ron` describes.
    Floor {
        dungeon: String,
        dive: u32,
        floor: u32,
    },
}

/// FNV-1a over a name's bytes: stable across builds and platforms, unlike
/// `std::hash` (ADR 0009).
fn name_seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A floor's seed: a zone's (ADR 0009) in the dungeon's own world,
/// addressed by dive and floor. Every dive rerolls its floors; leaving
/// `dive` out of the address would keep their layouts.
pub(crate) fn floor_seed(world_seed: u64, dungeon: &str, dive: u32, floor: u32) -> u64 {
    let index = |n: u32| i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX);
    zone_seed(
        mix(world_seed, name_seed(dungeon)),
        ZoneCoord {
            x: index(dive),
            y: index(floor),
        },
    )
}

/// A floor, generated but not yet entered.
pub(crate) struct Floor {
    pub map: Map,
    /// The map stream after generation, and a fresh spawn stream, both
    /// from the floor's seed (ADR 0005).
    pub map_stream: Pcg64,
    pub spawn_stream: Pcg64,
    /// The stairs up, where the player arrives.
    pub arrival: (i32, i32),
    /// Floor around the arrival, nearest first: where summons land.
    pub near: Vec<(i32, i32)>,
}

/// Generates `floor` of `dive` into `dungeon`: its cave, the stairs up near
/// the centre where you arrive, the stairs down farthest from them.
pub(crate) fn build(
    world_seed: u64,
    dungeon: &str,
    cave: &CaveParams,
    dive: u32,
    floor: u32,
) -> Floor {
    let seed = floor_seed(world_seed, dungeon, dive, floor);
    let (mut map, map_stream) = generate_cave(seed, cave);
    let arrival = start_position(&map).unwrap_or((map.width / 2, map.height / 2));
    // Before the stairs change any tile: `region` walks plain floor.
    let near = region(&map, arrival, &mut Grid::new(map.width, map.height, false));
    place_stairs_down(&mut map, arrival);
    map.set(arrival.0, arrival.1, Tile::StairsUp);
    Floor {
        map,
        map_stream,
        spawn_stream: Rngs::new(world_seed, seed).spawn,
        arrival,
        near,
    }
}

/// Who goes with the player between floors: the player and their summons.
fn travellers(state: &GameState) -> Vec<Entity> {
    let player = state.player;
    let mut travellers = vec![player];
    travellers.extend(
        state
            .world
            .query::<(Entity, &Summoned, &Position)>()
            .iter()
            .filter(|(_, summoned, _)| summoned.owner == player)
            .map(|(summon, _, _)| summon),
    );
    travellers
}

/// `Command::Descend` applier: the next floor of this dive. `reject_input`
/// has checked the player stands on stairs down.
pub(crate) fn descend(state: &mut GameState, actor: Entity) {
    if actor != state.player {
        return;
    }
    let Place::Floor {
        dungeon,
        dive,
        floor,
    } = state.place.clone();
    let Some(cave) = state.blueprints.dungeon(&dungeon).map(|d| d.cave.clone()) else {
        state.log.push("The stairs lead nowhere.");
        return;
    };
    let floor = floor + 1;
    let next = build(state.seed, &dungeon, &cave, dive, floor);

    let travellers = travellers(state);
    let left_behind: Vec<Entity> = state
        .world
        .query::<(Entity, &Position)>()
        .iter()
        .map(|(entity, _)| entity)
        .filter(|entity| !travellers.contains(entity))
        .collect();
    for entity in left_behind {
        crate::command::despawn_with_holdings(state, entity);
    }

    let (width, height) = (next.map.width, next.map.height);
    state.map = next.map;
    state.rngs.map = next.map_stream;
    state.rngs.spawn = next.spawn_stream;
    state.visible = Grid::new(width, height, false);
    state.seen = Grid::new(width, height, false);
    state.remembered_items = Grid::new(width, height, None);
    state.place = Place::Floor {
        dungeon,
        dive,
        floor,
    };

    let arrival = next.arrival;
    let mut spots =
        std::iter::once(arrival).chain(next.near.into_iter().filter(|&at| at != arrival));
    for traveller in travellers {
        let Some((x, y)) = spots.next() else {
            break;
        };
        if let Ok(mut position) = state.world.get::<&mut Position>(traveller) {
            *position = Position { x, y };
        }
    }
    state.log.push(format!("You descend to floor {floor}."));
    if let Err(err) = state.populate() {
        state.log.push(format!("The floor is empty: {err}."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::entity::Timed;
    use crate::input::PlayerAction;
    use crate::state::tests::{bare_state, player_position, test_blueprints};
    use crate::testkit::thing;
    use crate::time::take_turn;

    /// A new game whose content can build floors, standing on the stairs.
    fn at_the_stairs(seed: u64) -> GameState {
        let state = GameState::new(seed, test_blueprints()).unwrap();
        let stairs = stairs_down(&state);
        let player = state.player;
        *state.world.get::<&mut Position>(player).unwrap() = Position {
            x: stairs.0,
            y: stairs.1,
        };
        state
    }

    fn stairs_down(state: &GameState) -> (i32, i32) {
        (0..state.map.height)
            .flat_map(|y| (0..state.map.width).map(move |x| (x, y)))
            .find(|&(x, y)| state.map.get(x, y) == Some(&Tile::StairsDown))
            .expect("every floor has stairs down")
    }

    #[test]
    fn going_down_off_the_stairs_is_refused() {
        let mut state = bare_state();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Descend);
        assert_eq!(state.now, before, "no time passed");
        assert_eq!(
            crate::testkit::last_log(&state),
            "There are no stairs down here."
        );
    }

    #[test]
    fn the_stairs_lead_to_a_new_floor_the_same_from_the_same_seed() {
        let mut a = at_the_stairs(9);
        let mut b = at_the_stairs(9);
        let first = a.map.clone();
        take_turn(&mut a, PlayerAction::Descend);
        take_turn(&mut b, PlayerAction::Descend);
        assert_eq!(
            a.place,
            Place::Floor {
                dungeon: "Test".into(),
                dive: 1,
                floor: 2
            }
        );
        assert_ne!(a.map, first, "a new floor");
        assert_eq!(a.map, b.map);
        let arrived = player_position(&a);
        assert_eq!(a.map.get(arrived.x, arrived.y), Some(&Tile::StairsUp));
        assert!(
            a.log
                .recent(5)
                .any(|line| line == "You descend to floor 2.")
        );
    }

    #[test]
    fn the_player_brings_what_they_hold_and_their_summons_and_nothing_else() {
        let mut state = at_the_stairs(9);
        let player = state.player;
        let knife = thing("knife").item().carried_by(player).make(&mut state);
        let at = player_position(&state);
        let wolf = thing("wolf")
            .at(at.x + 1, at.y)
            .with(Summoned { owner: player })
            .make(&mut state);
        let stranger = thing("rat").at(at.x - 1, at.y).make(&mut state);
        let effect = thing("haste")
            .with(Timed {
                start: state.now,
                duration: 10_000,
            })
            .attached_to(player)
            .make(&mut state);

        state.run(Command::Descend(player));
        assert!(state.world.contains(knife));
        assert!(state.world.contains(effect));
        assert!(!state.world.contains(stranger));
        let wolf_at = *state.world.get::<&Position>(wolf).unwrap();
        let player_at = player_position(&state);
        assert!(
            (wolf_at.x - player_at.x).abs() <= 1 && (wolf_at.y - player_at.y).abs() <= 1,
            "the summon arrives beside the player"
        );
        assert_ne!(wolf_at, player_at);
    }

    #[test]
    fn a_save_on_a_lower_floor_loads_there() {
        let mut state = at_the_stairs(9);
        take_turn(&mut state, PlayerAction::Descend);
        let text = crate::save::to_ron(&state).unwrap();
        let loaded = crate::save::from_ron(&text, test_blueprints()).unwrap();
        assert_eq!(
            loaded.place,
            Place::Floor {
                dungeon: "Test".into(),
                dive: 1,
                floor: 2
            }
        );
        assert_eq!(loaded.map, state.map);
    }

    #[test]
    fn a_new_floor_is_unknown_ground() {
        let mut state = at_the_stairs(9);
        take_turn(&mut state, PlayerAction::Descend);
        let at = player_position(&state);
        assert_eq!(state.seen.get(at.x, at.y), Some(&true), "seen on arrival");
        let seen = (0..state.map.height)
            .flat_map(|y| (0..state.map.width).map(move |x| (x, y)))
            .filter(|&(x, y)| state.seen.get(x, y) == Some(&true))
            .count();
        let visible = (0..state.map.height)
            .flat_map(|y| (0..state.map.width).map(move |x| (x, y)))
            .filter(|&(x, y)| state.visible.get(x, y) == Some(&true))
            .count();
        assert_eq!(seen, visible, "nothing remembered from the floor above");
    }

    #[test]
    fn deeper_floors_hold_what_the_dungeon_adds() {
        let goblins = |state: &GameState| {
            state
                .world
                .query::<&crate::entity::Name>()
                .iter()
                .filter(|name| name.0 == "goblin")
                .count()
        };
        let mut state = at_the_stairs(9);
        let first = goblins(&state);
        take_turn(&mut state, PlayerAction::Descend);
        assert_eq!(goblins(&state), first + 1, "one more goblin per floor");
    }

    #[test]
    fn every_dive_rerolls_its_floors() {
        assert_ne!(floor_seed(9, "Caves", 1, 2), floor_seed(9, "Caves", 2, 2));
        assert_ne!(floor_seed(9, "Caves", 1, 1), floor_seed(9, "Castle", 1, 1));
    }
}
