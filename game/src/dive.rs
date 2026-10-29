//! Dives (docs/plans/dives.md): going down the stairs to a new floor. A floor exists only while you are on it: going down builds the next one from its own seed and forgets this one, keeping only you, what you hold, your effects and your summons.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use rand_pcg::Pcg64;

use crate::camp::Camp;
use crate::dungeon::Dungeon;
use crate::entity::{Position, Summoned};
use crate::map::{Grid, Map, Tile};
use crate::rng::Rngs;
use crate::state::GameState;
use crate::zone::{ZoneCoord, mix, place_stairs_down, region, start_position, zone_seed};

/// Where the player is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Place {
    /// Between dives (`camp`).
    Camp,
    /// `floor` of the `dive`th dive, both counted from 1, in the dungeon
    /// `data/dungeons/<dungeon>.ron` describes, during `phase` (fixed for
    /// the whole dive).
    Floor {
        dungeon: String,
        phase: String,
        dive: u32,
        floor: u32,
    },
}

/// Where the player is, named for reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceView {
    /// At camp; the phase the next dive will be in.
    Camp { next_phase: String },
    Floor {
        /// "the caves".
        dungeon: String,
        floor: u32,
        /// "the embers".
        phase: String,
    },
}

/// The current place by the names content gives it.
pub fn place_view(state: &GameState) -> PlaceView {
    let phase_name = |key: &str| {
        state
            .blueprints
            .phases()
            .iter()
            .find(|phase| phase.key == key)
            .map_or_else(|| key.to_string(), |phase| phase.name.clone())
    };
    match &state.place {
        Place::Camp => PlaceView::Camp {
            next_phase: phase_name(&state.phase),
        },
        Place::Floor {
            dungeon,
            phase,
            floor,
            ..
        } => PlaceView::Floor {
            dungeon: state
                .blueprints
                .dungeon(dungeon)
                .map_or_else(|| dungeon.clone(), |found| found.name.clone()),
            floor: *floor,
            phase: phase_name(phase),
        },
    }
}

/// FNV-1a over a name's bytes: stable across builds and platforms, unlike
/// `std::hash` (ADR 0009).
fn name_seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A floor's seed: a zone's (ADR 0009) in the world of this dungeon in this
/// phase, addressed by dive and floor. Every dive rerolls its floors;
/// leaving `dive` out of the address would keep their layouts.
pub(crate) fn floor_seed(
    world_seed: u64,
    dungeon: &str,
    phase: &str,
    dive: u32,
    floor: u32,
) -> u64 {
    let index = |n: u32| i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX);
    zone_seed(
        mix(mix(world_seed, name_seed(dungeon)), name_seed(phase)),
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

/// Generates `floor` of `dive` into `dungeon` (`found`, keyed `dungeon`): its
/// layout, where you arrive near the centre (stairs up to camp on a
/// way-landing), and the stairs down farthest from it, unless this is the
/// bottom (docs/plans/depth.md).
pub(crate) fn build(
    world_seed: u64,
    found: &Dungeon,
    dungeon: &str,
    phase: &str,
    dive: u32,
    floor: u32,
) -> Floor {
    let seed = floor_seed(world_seed, dungeon, phase, dive, floor);
    let (mut map, map_stream, suggested) = found.generator.generate(seed);
    let arrival = suggested
        .or_else(|| start_position(&map))
        .unwrap_or((map.width / 2, map.height / 2));
    // Before the stairs change any tile: `region` walks plain floor.
    let near = region(&map, arrival, &mut Grid::new(map.width, map.height, false));
    // No RNG in either, so the depth never changes a layout.
    if !found.is_bottom(floor) {
        place_stairs_down(&mut map, arrival);
    }
    if found.is_landing(floor) {
        map.set(arrival.0, arrival.1, Tile::StairsUp);
    }
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

/// The camp as a place to enter: its map, arriving on its `@`. Nothing
/// there draws randomness, but its streams are seeded like any place's.
fn camp_floor(world_seed: u64, camp: &Camp) -> Floor {
    let near = region(
        &camp.map,
        camp.arrival,
        &mut Grid::new(camp.map.width, camp.map.height, false),
    );
    let rngs = Rngs::new(world_seed, mix(world_seed, name_seed("camp")));
    Floor {
        map: camp.map.clone(),
        map_stream: rngs.map,
        spawn_stream: rngs.spawn,
        arrival: camp.arrival,
        near,
    }
}

/// Moves the player and who travels with them into `next`, now `place`,
/// forgetting everything else about the place they leave.
fn enter(state: &mut GameState, next: Floor, place: Place) {
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
    state.place = place;

    let arrival = next.arrival;
    let mut spots =
        std::iter::once(arrival).chain(next.near.into_iter().filter(|&at| at != arrival));
    for &traveller in &travellers {
        let Some((x, y)) = spots.next() else {
            break;
        };
        if let Ok(mut position) = state.world.get::<&mut Position>(traveller) {
            *position = Position { x, y };
        }
    }
    crate::rest::arrive(state, &travellers);
}

/// `Command::Descend` applier: from camp, the first floor of a new dive
/// into the dungeon this entrance leads to; from a floor, the next one.
/// `reject_input` has checked the player stands on stairs down.
pub(crate) fn descend(state: &mut GameState, actor: Entity) {
    if actor != state.player {
        return;
    }
    let (dungeon, phase, dive, floor) = match &state.place {
        Place::Camp => {
            let at = state
                .world
                .get::<&Position>(actor)
                .map(|pos| (pos.x, pos.y))
                .ok();
            let entrance = state
                .blueprints
                .camp()
                .zip(at)
                .and_then(|(camp, at)| camp.entrance_at(at))
                .map(str::to_string);
            let Some(dungeon) = entrance else {
                state.log.push("These stairs lead nowhere.");
                return;
            };
            // A new dive, in the phase waited for.
            (dungeon, state.phase.clone(), state.dives + 1, 1)
        }
        Place::Floor {
            dungeon,
            phase,
            dive,
            floor,
        } => (dungeon.clone(), phase.clone(), *dive, floor + 1),
    };
    let Some(found) = state.blueprints.dungeon(&dungeon) else {
        state.log.push("The stairs lead nowhere.");
        return;
    };
    let found = found.clone();
    let next = build(state.seed, &found, &dungeon, &phase, dive, floor);
    let name = found.name.clone();
    state.dives = dive;
    enter(
        state,
        next,
        Place::Floor {
            dungeon,
            phase,
            dive,
            floor,
        },
    );
    state.log.push(if floor == 1 {
        format!("You go down into {name}.")
    } else {
        format!("You descend to floor {floor}.")
    });
    if found.is_bottom(floor) {
        state.log.push(format!("This is the bottom of {name}."));
    } else if found.is_landing(floor) {
        state.log.push("Stairs lead back up to camp from here.");
    }
    if let Err(err) = state.populate() {
        state.log.push(format!("The floor is empty: {err}."));
    }
}

/// `Command::Ascend` applier: back up to camp, which ends the dive; the
/// floors below are gone. `reject_input` has checked for stairs up.
pub(crate) fn ascend(state: &mut GameState, actor: Entity) {
    if actor != state.player {
        return;
    }
    let Some(camp) = state.blueprints.camp().cloned() else {
        state.log.push("There is nowhere to climb back to.");
        return;
    };
    enter(state, camp_floor(state.seed, &camp), Place::Camp);
    state.log.push("You climb back to camp.");
    if let Err(err) = state.populate() {
        state.log.push(format!("The camp is empty: {err}."));
    }
}

/// Where a new game begins: camp if the content has one.
pub(crate) fn start(world_seed: u64, camp: &Camp) -> Floor {
    camp_floor(world_seed, camp)
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
                phase: "Still".into(),
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
        let trip = thing("trip")
            .with(crate::entity::Technique {
                domain: crate::skill::Domain::Martial,
                reach: 1,
                recharge: 0,
                stamina: 0,
                requires: Vec::new(),
                riders: Vec::new(),
                uses: None,
                bonuses: Vec::new(),
                combo: None,
            })
            .attached_to(player)
            .make(&mut state);

        state.run(Command::Descend(player));
        assert!(state.world.contains(knife));
        assert!(state.world.contains(effect));
        assert_eq!(crate::technique::known(&state.world, player), [trip]);
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
                phase: "Still".into(),
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

    /// The test dungeon, four floors deep with a landing every third.
    fn deep() -> Dungeon {
        Dungeon {
            floors: 4,
            landing_every: 3,
            ..crate::dungeon::tests::test_dungeon()
        }
    }

    fn has(map: &Map, tile: Tile) -> bool {
        (0..map.height).any(|y| (0..map.width).any(|x| map.get(x, y) == Some(&tile)))
    }

    #[test]
    fn only_landings_have_a_way_back_and_the_bottom_no_way_down() {
        let dungeon = deep();
        let floor = |n| build(9, &dungeon, "Test", "Still", 1, n);
        for (n, up, down) in [
            (1, false, true),
            (2, false, true),
            (3, true, true),
            (4, true, false),
        ] {
            let map = floor(n).map;
            assert_eq!(has(&map, Tile::StairsUp), up, "floor {n} up");
            assert_eq!(has(&map, Tile::StairsDown), down, "floor {n} down");
        }
        let arrival = floor(3).arrival;
        assert_eq!(
            floor(3).map.get(arrival.0, arrival.1),
            Some(&Tile::StairsUp)
        );
    }

    #[test]
    fn depth_changes_no_layout_but_the_stairs() {
        let shallow = crate::dungeon::tests::test_dungeon();
        let (a, b) = (
            build(9, &deep(), "Test", "Still", 1, 1),
            build(9, &shallow, "Test", "Still", 1, 1),
        );
        let mut expected = b.map.clone();
        expected.set(b.arrival.0, b.arrival.1, Tile::Floor);
        assert_eq!(a.map, expected);
    }

    #[test]
    fn a_dungeon_without_depth_fails_to_load() {
        let dungeon = Dungeon {
            floors: 0,
            ..crate::dungeon::tests::test_dungeon()
        };
        assert!(crate::dungeon::check("Test", &dungeon, &test_blueprints()).is_err());
    }

    fn at_camp() -> GameState {
        let blueprints = test_blueprints().with_camp(Some(crate::camp::tests::test_camp()));
        GameState::new(9, blueprints).unwrap()
    }

    fn walk_to_the_entrance(state: &mut GameState) {
        // The test camp's entrance is two steps east of the arrival.
        for _ in 0..2 {
            take_turn(state, PlayerAction::Move(crate::input::Dir::E));
        }
    }

    #[test]
    fn a_new_game_starts_at_a_quiet_camp() {
        let state = at_camp();
        assert_eq!(state.place, Place::Camp);
        assert_eq!(player_position(&state), Position { x: 2, y: 1 });
        assert_eq!(crate::testkit::last_log(&state), "The fire is warm.");
        let others = state.world.query::<&Position>().iter().count();
        assert_eq!(others, 1, "only the player");
    }

    #[test]
    fn camp_to_a_dive_and_back() {
        let mut state = at_camp();
        walk_to_the_entrance(&mut state);
        take_turn(&mut state, PlayerAction::Descend);
        assert_eq!(
            state.place,
            Place::Floor {
                dungeon: "Test".into(),
                phase: "Still".into(),
                dive: 1,
                floor: 1
            }
        );
        assert!(
            state
                .log
                .recent(5)
                .any(|line| line == "You go down into the test caves.")
        );
        let first_dive = state.map.clone();

        take_turn(&mut state, PlayerAction::Ascend);
        assert_eq!(state.place, Place::Camp);
        assert_eq!(player_position(&state), Position { x: 2, y: 1 });
        assert!(
            state
                .log
                .recent(5)
                .any(|line| line == "You climb back to camp.")
        );

        walk_to_the_entrance(&mut state);
        take_turn(&mut state, PlayerAction::Descend);
        assert_eq!(state.dives, 2);
        assert_ne!(state.map, first_dive, "every dive rerolls its floors");
    }

    #[test]
    fn going_up_off_the_stairs_is_refused() {
        let mut state = at_camp();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Ascend);
        assert_eq!(state.now, before);
        assert_eq!(
            crate::testkit::last_log(&state),
            "There are no stairs up here."
        );
    }

    #[test]
    fn a_save_at_camp_loads_at_camp() {
        let mut state = at_camp();
        walk_to_the_entrance(&mut state);
        take_turn(&mut state, PlayerAction::Descend);
        take_turn(&mut state, PlayerAction::Ascend);
        let text = crate::save::to_ron(&state).unwrap();
        let blueprints = test_blueprints().with_camp(Some(crate::camp::tests::test_camp()));
        let loaded = crate::save::from_ron(&text, blueprints).unwrap();
        assert_eq!(loaded.place, Place::Camp);
        assert_eq!(loaded.dives, 1);
    }

    #[test]
    fn practice_works_at_camp() {
        let mut state = at_camp();
        let before = state.now;
        take_turn(&mut state, PlayerAction::Practice);
        assert!(state.now > before, "a turn of drills");
    }

    /// The test dungeon with three more goblins in the embers.
    fn ember_goblins_at_camp() -> GameState {
        let mut dungeon = crate::dungeon::tests::test_dungeon();
        dungeon.creatures.push(crate::dungeon::Inhabitant {
            blueprint: "Goblin".into(),
            count: 3,
            per_floor: 0,
            phases: vec!["Embers".into()],
        });
        let blueprints = test_blueprints()
            .with_dungeons(std::collections::BTreeMap::from([(
                "Test".to_string(),
                dungeon,
            )]))
            .unwrap()
            .with_camp(Some(crate::camp::tests::test_camp()));
        GameState::new(9, blueprints).unwrap()
    }

    fn goblins(state: &GameState) -> usize {
        state
            .world
            .query::<&crate::entity::Name>()
            .iter()
            .filter(|name| name.0 == "goblin")
            .count()
    }

    #[test]
    fn the_phase_decides_what_lives_below_and_holds_for_the_dive() {
        let mut still = ember_goblins_at_camp();
        walk_to_the_entrance(&mut still);
        take_turn(&mut still, PlayerAction::Descend);

        let mut embers = ember_goblins_at_camp();
        take_turn(&mut embers, PlayerAction::WaitForPhase(1));
        walk_to_the_entrance(&mut embers);
        take_turn(&mut embers, PlayerAction::Descend);
        assert!(matches!(&embers.place, Place::Floor { phase, .. } if phase == "Embers"));
        assert!(goblins(&embers) > goblins(&still), "the embers bring more");

        // Waiting is for camp; the dive keeps its phase all the way down.
        take_turn(&mut embers, PlayerAction::WaitForPhase(0));
        assert_eq!(embers.phase, "Embers");
        let stairs = stairs_down(&embers);
        let player = embers.player;
        *embers.world.get::<&mut Position>(player).unwrap() = Position {
            x: stairs.0,
            y: stairs.1,
        };
        take_turn(&mut embers, PlayerAction::Descend);
        assert!(matches!(&embers.place, Place::Floor { phase, floor: 2, .. } if phase == "Embers"));
    }

    #[test]
    fn every_dive_rerolls_its_floors() {
        let seed = |dungeon, phase, dive, floor| floor_seed(9, dungeon, phase, dive, floor);
        assert_ne!(seed("Caves", "Still", 1, 2), seed("Caves", "Still", 2, 2));
        assert_ne!(seed("Caves", "Still", 1, 1), seed("Castle", "Still", 1, 1));
        assert_ne!(seed("Caves", "Still", 1, 1), seed("Caves", "Embers", 1, 1));
    }
}
