//! The floor bot (docs/plans/floor-bot.md): a build played through real dungeon floors, from camp
//! down the stairs, exploring and fighting what it meets with its policy until it dies or reaches
//! the depth asked for. It is the naive player of the vision's Fights: it never retreats or rests
//! on purpose, so what it reports is what bumping gets you.

use std::collections::BTreeMap;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::{Act, Build, command, dress, health, position};
use crate::blueprint::Blueprints;
use crate::command::Command;
use crate::dive::Place;
use crate::entity::{Dead, Name, Position, Skills};
use crate::explore::{ExploreItems, explore_step, travel_step};
use crate::input::PlayerAction;
use crate::map::Tile;
use crate::rest::{rest_refusal, rest_step};
use crate::state::GameState;
use crate::time::{self, take_turn};

/// Player turns after which a floor is given up as stuck.
const TURNS_PER_FLOOR: u32 = 2000;
/// Waits in a row (an enemy in view the policy can't reach) after which a
/// floor is given up as stuck.
const WAITS_IN_A_ROW: u32 = 50;
/// Steps the bot may take in camp to reach the dungeon's entrance.
const CAMP_STEPS: u32 = 500;

/// A dive to play, as written in `bench/dives.ron`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dive {
    pub name: String,
    /// The dungeon's key (`data/dungeons/<Key>.ron`).
    pub dungeon: String,
    /// The phase waited for at camp first; the starting one if absent.
    #[serde(default)]
    pub phase: Option<String>,
    /// How many floors to play before calling it done.
    #[serde(default = "default_floors")]
    pub floors: u32,
}

fn default_floors() -> u32 {
    6
}

/// One dive, floor by floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiveRun {
    /// XP by skill name as the dive began, for the levels it reaches.
    pub start: BTreeMap<String, u32>,
    pub floors: Vec<FloorRun>,
    pub ended: End,
}

/// Why a dive stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// It played every floor asked for.
    Depth,
    /// The build died, on the last floor listed.
    Died,
    /// The last floor listed could not be finished.
    Stuck,
}

/// What one floor cost and gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FloorRun {
    /// Every action the player took.
    pub turns: u32,
    /// What a person would press: a run of explore steps is one.
    pub keypresses: u32,
    pub ticks: u64,
    /// XP gained, by skill name.
    pub xp: BTreeMap<String, u32>,
    /// Health on arriving less health on leaving: what resting didn't make
    /// up for.
    pub hp_lost: u32,
    /// Health on leaving the floor (0 on a death).
    pub hp_left: u32,
    /// What it died to: the nearest enemy in view at the end, or "unseen".
    pub died_to: Option<String>,
    /// Turns spent resting.
    pub rested: u32,
    /// Each fight's length in keypresses: from an enemy coming into view to
    /// none being in view.
    pub fights: Vec<u32>,
}

impl FloorRun {
    pub fn xp_total(&self) -> u32 {
        self.xp.values().sum()
    }
}

/// `build` played through `dive` on `seed`: a new game in camp, walked to the
/// dungeon's entrance, the phase waited for, then floor after floor.
pub fn dive(
    blueprints: &Blueprints,
    build: &Build,
    dive: &Dive,
    seed: u64,
) -> Result<DiveRun, String> {
    let mut state = GameState::new_as(seed, blueprints.clone(), build.origin.as_deref())
        .map_err(|err| format!("{}: {err}", build.name))?;
    dress(&mut state, build)?;
    state.explore_items = ExploreItems::PickUp;
    if let Some(phase) = &dive.phase {
        wait_for(&mut state, phase).map_err(|err| format!("{}: {err}", dive.name))?;
    }
    enter(&mut state, &dive.dungeon).map_err(|err| format!("{}: {err}", dive.name))?;
    Ok(play(&mut state, build, dive.floors))
}

fn wait_for(state: &mut GameState, phase: &str) -> Result<(), String> {
    let index = state
        .blueprints
        .phases()
        .iter()
        .position(|known| known.key == phase)
        .ok_or_else(|| format!("unknown phase {phase:?}"))?;
    if state.phase != phase {
        let index = u32::try_from(index).map_err(|err| err.to_string())?;
        take_turn(state, PlayerAction::WaitForPhase(index));
    }
    Ok(())
}

/// Walks from wherever the player stands in camp to `dungeon`'s entrance and
/// takes it, exploring camp when the way is not yet known.
fn enter(state: &mut GameState, dungeon: &str) -> Result<(), String> {
    let Some(entrance) = state.blueprints.camp().and_then(|camp| {
        camp.entrances
            .iter()
            .find(|(_, key)| key == dungeon)
            .map(|&(at, _)| at)
    }) else {
        return Err(format!("camp has no entrance to {dungeon:?}"));
    };
    for _ in 0..CAMP_STEPS {
        if position(state, state.player) == Some(entrance) {
            take_turn(state, PlayerAction::Descend);
            return match state.place {
                Place::Floor { .. } => Ok(()),
                Place::Camp => Err(format!("the entrance to {dungeon:?} did not lead down")),
            };
        }
        let before = state.now;
        if !travel_step(state, entrance) && state.now == before && !explore_step(state) {
            break;
        }
    }
    Err(format!("could not reach the entrance to {dungeon:?}"))
}

/// The bot's floors, from the one the player stands on.
pub(super) fn play(state: &mut GameState, build: &Build, floors: u32) -> DiveRun {
    let mut run = DiveRun {
        start: xp(state),
        floors: Vec::new(),
        ended: End::Depth,
    };
    for played in 1..=floors {
        let (floor, end) = floor(state, build);
        run.floors.push(floor);
        if let Some(end) = end {
            run.ended = end;
            break;
        }
        if played < floors {
            take_turn(state, PlayerAction::Descend);
        }
    }
    run
}

/// One floor, played until the bot stands on the stairs down with nowhere
/// left to explore (`None`), or dies or gets stuck.
fn floor(state: &mut GameState, build: &Build) -> (FloorRun, Option<End>) {
    let player = state.player;
    let start = state.now;
    let xp_before = xp(state);
    let hp_before = health(state, player);
    let mut run = FloorRun::default();
    let (mut exploring, mut resting, mut waits, mut fight) = (false, false, 0, None::<u32>);
    let end = loop {
        if state.game_over {
            break Some(End::Died);
        }
        if run.turns >= TURNS_PER_FLOOR || waits >= WAITS_IN_A_ROW {
            break Some(End::Stuck);
        }
        let before = state.now;
        if let Some(target) = nearest_enemy(state) {
            (exploring, resting) = (false, false);
            run.keypresses += 1;
            *fight.get_or_insert(0) += 1;
            if act(state, build, target) {
                waits = 0;
            } else {
                time::submit(state, Command::Wait(player));
                waits += 1;
            }
        } else {
            if let Some(length) = fight.take() {
                run.fights.push(length);
            }
            waits = 0;
            // Below half health, a careful player rests to full before
            // exploring on, while the floor's budget lasts.
            if (resting || below_half(state)) && rest_refusal(state).is_none() {
                if !resting {
                    run.keypresses += 1;
                    (exploring, resting) = (false, true);
                }
                rest_step(state);
                run.rested += 1;
                run.turns += 1;
                continue;
            }
            resting = false;
            if !exploring {
                run.keypresses += 1;
                exploring = true;
            }
            let going = explore_step(state);
            if !going && state.now == before {
                if on_stairs_down(state) {
                    run.keypresses += 1;
                    break None;
                }
                break Some(End::Stuck);
            }
        }
        if state.now != before {
            run.turns += 1;
        }
    };
    if let Some(length) = fight {
        run.fights.push(length);
    }
    if end == Some(End::Died) {
        run.died_to = Some(
            nearest_enemy(state)
                .and_then(|enemy| noun(state, enemy))
                .unwrap_or_else(|| "unseen".to_string()),
        );
    }
    run.ticks = state.now.since(start);
    run.xp = gained(state, &xp_before);
    run.hp_left = if state.game_over {
        0
    } else {
        health(state, player)
    };
    run.hp_lost = hp_before.saturating_sub(run.hp_left);
    (run, end)
}

/// The first entry of the policy the game takes, at `target`.
fn act(state: &mut GameState, build: &Build, target: Entity) -> bool {
    build.policy.iter().any(|act: &Act| {
        command(state, act, target).is_some_and(|command| time::submit(state, command))
    })
}

/// The nearest living enemy in the player's view; the first spawned on a tie.
fn nearest_enemy(state: &GameState) -> Option<Entity> {
    let from = position(state, state.player)?;
    let mut query = state
        .world
        .query::<(Entity, &Position)>()
        .without::<&Dead>();
    let mut enemies: Vec<(i32, crate::entity::SpawnSeq, Entity)> = query
        .iter()
        .filter(|&(entity, pos)| {
            state.visible.get(pos.x, pos.y) == Some(&true)
                && crate::side::enemies(&state.world, state.player, entity)
        })
        .filter_map(|(entity, pos)| {
            let seq = *state.world.get::<&crate::entity::SpawnSeq>(entity).ok()?;
            Some((crate::ranged::distance(from, (pos.x, pos.y)), seq, entity))
        })
        .collect();
    enemies.sort_by_key(|&(distance, seq, _)| (distance, seq));
    enemies.first().map(|&(_, _, entity)| entity)
}

fn below_half(state: &GameState) -> bool {
    state
        .world
        .get::<&crate::entity::Health>(state.player)
        .is_ok_and(|health| health.current * 2 < health.max)
}

fn on_stairs_down(state: &GameState) -> bool {
    position(state, state.player)
        .is_some_and(|(x, y)| state.map.get(x, y) == Some(&Tile::StairsDown))
}

fn noun(state: &GameState, entity: Entity) -> Option<String> {
    state
        .world
        .get::<&Name>(entity)
        .ok()
        .map(|name| name.0.clone())
}

fn xp(state: &GameState) -> BTreeMap<String, u32> {
    let Ok(skills) = state.world.get::<&Skills>(state.player) else {
        return BTreeMap::new();
    };
    let book = state.blueprints.skills();
    skills
        .0
        .iter()
        .filter_map(|&(id, xp)| Some((book.get(id)?.name.clone(), xp)))
        .collect()
}

/// XP gained since `before`, by skill, leaving out skills that gained none.
fn gained(state: &GameState, before: &BTreeMap<String, u32>) -> BTreeMap<String, u32> {
    xp(state)
        .into_iter()
        .filter_map(|(name, now)| {
            let gain = now.saturating_sub(before.get(&name).copied().unwrap_or(0));
            (gain > 0).then_some((name, gain))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::entity::Health;

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    fn bumper() -> Build {
        Build {
            name: "bumper".into(),
            note: String::new(),
            origin: Some("PitFighter".into()),
            skills: BTreeMap::new(),
            stats: None,
            equipment: None,
            techniques: vec![],
            spells: vec![],
            style: None,
            policy: vec![Act::Bump],
        }
    }

    fn caves(floors: u32) -> Dive {
        Dive {
            name: "caves".into(),
            dungeon: "Caves".into(),
            phase: None,
            floors,
        }
    }

    /// The game as `dive` leaves it on the first floor, before playing.
    fn on_the_first_floor(build: &Build, seed: u64) -> GameState {
        let mut state = GameState::new_as(seed, data(), build.origin.as_deref()).unwrap();
        dress(&mut state, build).unwrap();
        state.explore_items = ExploreItems::PickUp;
        enter(&mut state, "Caves").unwrap();
        state
    }

    fn despawn_enemies(state: &mut GameState) {
        let player = state.player;
        let enemies: Vec<Entity> = state
            .world
            .query::<(Entity, &Position)>()
            .iter()
            .map(|(entity, _)| entity)
            .filter(|&entity| crate::side::enemies(&state.world, player, entity))
            .collect();
        for enemy in enemies {
            crate::command::despawn_with_holdings(state, enemy);
        }
    }

    #[test]
    fn a_dive_is_the_same_per_seed() {
        let (build, dive) = (bumper(), caves(2));
        let first = super::dive(&data(), &build, &dive, 3).unwrap();
        assert_eq!(first, super::dive(&data(), &build, &dive, 3).unwrap());
    }

    #[test]
    fn the_bot_walks_from_camp_into_the_dungeon() {
        let state = on_the_first_floor(&bumper(), 1);
        assert!(
            matches!(&state.place, Place::Floor { dungeon, floor: 1, .. } if dungeon == "Caves")
        );
    }

    #[test]
    fn with_no_enemies_the_bot_reaches_the_floor_asked_for() {
        let build = bumper();
        let mut state = on_the_first_floor(&build, 1);
        despawn_enemies(&mut state);
        let run = play(&mut state, &build, 1);
        assert_eq!(run.ended, End::Depth);
        assert_eq!(run.floors.len(), 1);
        assert!(run.floors[0].fights.is_empty());
        assert!(run.floors[0].turns > 0 && run.floors[0].keypresses < run.floors[0].turns);
        state.run(Command::Descend(state.player));
        assert!(matches!(state.place, Place::Floor { floor: 2, .. }));
    }

    #[test]
    fn the_bot_fights_and_learns() {
        let run = super::dive(&data(), &bumper(), &caves(2), 1).unwrap();
        let fights: usize = run.floors.iter().map(|floor| floor.fights.len()).sum();
        assert!(fights > 0, "goblins prowl the caves: {run:?}");
        assert!(run.floors.iter().any(|floor| floor.xp_total() > 0));
    }

    #[test]
    fn a_bot_that_dies_ends_the_dive_there() {
        let build = bumper();
        let mut state = on_the_first_floor(&build, 1);
        let player = state.player;
        *state.world.get::<&mut Health>(player).unwrap() = Health { current: 1, max: 1 };
        let run = play(&mut state, &build, 3);
        assert_eq!(run.ended, End::Died);
        assert_eq!(run.floors.len(), 1);
        assert_eq!(run.floors[0].hp_left, 0);
        assert!(run.floors[0].died_to.is_some());
    }

    #[test]
    fn a_hurt_bot_rests_before_exploring_on() {
        let build = bumper();
        let mut state = on_the_first_floor(&build, 1);
        despawn_enemies(&mut state);
        let player = state.player;
        state.world.get::<&mut Health>(player).unwrap().current = 5;
        let run = play(&mut state, &build, 1);
        assert!(run.floors[0].rested > 0);
        assert_eq!(run.floors[0].hp_left, 20, "rested to full");
    }

    #[test]
    fn a_bot_with_no_budget_left_explores_on() {
        let build = bumper();
        let mut state = on_the_first_floor(&build, 1);
        despawn_enemies(&mut state);
        let player = state.player;
        state.world.get::<&mut Health>(player).unwrap().current = 5;
        state
            .world
            .insert_one(player, crate::entity::RestBudget { left: 0 })
            .unwrap();
        let run = play(&mut state, &build, 1);
        assert_eq!(run.floors[0].rested, 0);
        assert_eq!(run.floors[0].hp_left, 5);
    }

    #[test]
    fn a_floor_without_stairs_down_is_stuck() {
        let build = bumper();
        let mut state = on_the_first_floor(&build, 1);
        despawn_enemies(&mut state);
        for y in 0..state.map.height {
            for x in 0..state.map.width {
                if state.map.get(x, y) == Some(&Tile::StairsDown) {
                    state.map.set(x, y, Tile::Floor);
                }
            }
        }
        let run = play(&mut state, &build, 2);
        assert_eq!(run.ended, End::Stuck);
        assert_eq!(run.floors.len(), 1);
    }
}
