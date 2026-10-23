//! Balance bench (docs/plans/balance.md): a build set before training dolls for a window of ticks,
//! through the real systems (`GameState`, commands, events), so what it reports is the game's, not a
//! model's. The `bench` program drives it; nothing in play calls it.

use std::collections::BTreeMap;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::blueprint::Blueprints;
use crate::command::{self, Command};
use crate::entity::{Health, Learner, Position, Skills, Stats};
use crate::fx::{self, Fx, Outcome};
use crate::input::Dir;
use crate::map::{Grid, Map, Tile};
use crate::rng::Rngs;
use crate::spawn::Spawn;
use crate::state::GameState;
use crate::{ai, equipment, holding, skill, style, technique, time};

/// The arena: open floor, walled, with room for dolls on every side.
const ARENA: (i32, i32) = (31, 15);
/// Where the build stands; dolls are placed relative to it.
const START: (i32, i32) = (10, 7);

/// A character to measure, as written in `bench/builds/*.ron`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub name: String,
    /// What the build is, for the report.
    #[serde(default)]
    pub note: String,
    /// Levels by skill name; every other skill is 0.
    pub skills: BTreeMap<String, u32>,
    /// The player's own if absent.
    #[serde(default)]
    pub stats: Option<Stats>,
    /// Blueprints worn, replacing the player's starting kit; a quiver comes
    /// stocked.
    pub equipment: Vec<String>,
    /// Technique blueprints known, beside the player's own.
    #[serde(default)]
    pub techniques: Vec<String>,
    /// A style blueprint, followed from the start.
    #[serde(default)]
    pub style: Option<String>,
    /// What it does each turn: the first entry it can, else it waits.
    pub policy: Vec<Act>,
}

/// One entry of a build's policy, tried every turn in order. One the game
/// refuses (not ready, no target in reach) costs nothing and falls through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Act {
    /// Step toward the nearest doll; attack it when next to it.
    Bump,
}

/// Where the dolls stand, as written in `bench/scenarios.ron`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    /// A doll blueprint and its offset from the build.
    pub dolls: Vec<(String, (i32, i32))>,
}

/// What one run dealt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Run {
    /// Health taken from the dolls.
    pub dealt: i64,
    /// Ticks from the first action to the end of the last.
    pub ticks: u64,
    /// Blows that did damage (full hits and grazes).
    pub landed: u32,
    pub grazes: u32,
    /// Misses and perfect dodges.
    pub misses: u32,
}

impl Run {
    /// Damage per 100 ticks: per standard turn.
    pub fn per_turn(&self) -> f64 {
        if self.ticks == 0 {
            return 0.0;
        }
        self.dealt as f64 * 100.0 / self.ticks as f64
    }
}

/// `build` against `scenario` for `window` ticks on `seed`.
pub fn run(
    blueprints: &Blueprints,
    build: &Build,
    scenario: &Scenario,
    window: u64,
    seed: u64,
) -> Result<Run, String> {
    simulate(blueprints, build, scenario, window, seed).map(|(_, run)| run)
}

fn simulate(
    blueprints: &Blueprints,
    build: &Build,
    scenario: &Scenario,
    window: u64,
    seed: u64,
) -> Result<(GameState, Run), String> {
    let mut state = arena(blueprints, seed)?;
    dress(&mut state, build)?;
    let dolls = place(&mut state, scenario)?;
    state.run(Command::UpdateFov);
    fx::take_fx(&mut state);

    let start = state.now;
    let end = start.after(window);
    let mut run = Run::default();
    while state.now < end && !state.game_over {
        let Some(target) = nearest(&state, &dolls) else {
            break;
        };
        let mut took = false;
        for act in &build.policy {
            if let Some(command) = command(&state, act, target)
                && time::submit(&mut state, command)
            {
                took = true;
                break;
            }
        }
        if !took {
            let wait = Command::Wait(state.player);
            time::submit(&mut state, wait);
        }
        tally(&mut run, &fx::take_fx(&mut state));
    }
    run.ticks = state.now.since(start);
    run.dealt = dolls.iter().map(|&doll| lost(&state, doll)).sum();
    Ok((state, run))
}

/// A walled, open floor with the player at `START`.
fn arena(blueprints: &Blueprints, seed: u64) -> Result<GameState, String> {
    let (width, height) = ARENA;
    let mut map: Map = Grid::new(width, height, Tile::Floor);
    for x in 0..width {
        map.set(x, 0, Tile::Wall);
        map.set(x, height - 1, Tile::Wall);
    }
    for y in 0..height {
        map.set(0, y, Tile::Wall);
        map.set(width - 1, y, Tile::Wall);
    }
    let (x, y) = START;
    GameState::with_map(
        seed,
        Rngs::new(seed, seed),
        map,
        Position { x, y },
        blueprints.clone(),
    )
    .map_err(|err| err.to_string())
}

/// The player made into `build`: its skills (and no learning), stats, gear,
/// techniques and style.
fn dress(state: &mut GameState, build: &Build) -> Result<(), String> {
    let player = state.player;
    let _ = state.world.remove_one::<Learner>(player);
    let mut skills = Skills::default();
    for (name, &level) in &build.skills {
        let id = state
            .blueprints
            .skills()
            .id(name)
            .ok_or_else(|| format!("{}: unknown skill {name:?}", build.name))?;
        skills.set(id, skill::xp_for(level));
    }
    let _ = state.world.insert_one(player, skills);
    if let Some(stats) = build.stats {
        let _ = state.world.insert_one(player, stats);
    }

    for (item, _) in equipment::equipped(&state.world, player) {
        holding::take_off(&mut state.world, player, item);
        command::despawn_with_holdings(state, item);
    }
    for name in &build.equipment {
        let item = spawn(state, build, Spawn::blueprint(name).carried_by(player))?;
        equipment::equip(state, player, item, None);
        if !equipment::equipped(&state.world, player)
            .iter()
            .any(|&(worn, _)| worn == item)
        {
            return Err(format!("{}: {name} does not fit", build.name));
        }
    }
    state.stock_quivers().map_err(|err| err.to_string())?;

    for name in &build.techniques {
        let noun = noun(state, name).ok_or_else(|| format!("{}: unknown {name:?}", build.name))?;
        if !technique::knows(&state.world, player, &noun) {
            spawn(state, build, Spawn::blueprint(name).attached_to(player))?;
        }
    }
    if let Some(name) = &build.style {
        let known = spawn(state, build, Spawn::blueprint(name).carried_by(player))?;
        style::follow(state, player, known);
    }
    Ok(())
}

fn spawn(state: &mut GameState, build: &Build, spawn: Spawn) -> Result<Entity, String> {
    state
        .spawn(spawn)
        .map_err(|err| format!("{}: {err}", build.name))
}

/// The bare noun a blueprint's things are named by ("lunge").
fn noun(state: &GameState, blueprint: &str) -> Option<String> {
    let id = state.blueprints.id(blueprint)?;
    Some(state.blueprints.by_id(id)?.noun()?.to_string())
}

fn place(state: &mut GameState, scenario: &Scenario) -> Result<Vec<Entity>, String> {
    let (x, y) = START;
    scenario
        .dolls
        .iter()
        .map(|(name, (dx, dy))| {
            state
                .spawn(Spawn::blueprint(name).at(x + dx, y + dy))
                .map_err(|err| format!("{}: {err}", scenario.name))
        })
        .collect()
}

fn position(state: &GameState, entity: Entity) -> Option<(i32, i32)> {
    state
        .world
        .get::<&Position>(entity)
        .ok()
        .map(|pos| (pos.x, pos.y))
}

/// The doll closest to the player; the first placed on a tie.
fn nearest(state: &GameState, dolls: &[Entity]) -> Option<Entity> {
    let from = position(state, state.player)?;
    dolls
        .iter()
        .filter_map(|&doll| Some((crate::ranged::distance(from, position(state, doll)?), doll)))
        .min_by_key(|&(distance, _)| distance)
        .map(|(_, doll)| doll)
}

/// What `act` asks of the game this turn, aimed at `target`; `None` if it
/// has no meaning now (the game itself refuses the rest).
fn command(state: &GameState, act: &Act, target: Entity) -> Option<Command> {
    let player = state.player;
    let from = position(state, player)?;
    let at = position(state, target)?;
    match act {
        Act::Bump => {
            let (dx, dy) = ((at.0 - from.0).signum(), (at.1 - from.1).signum());
            let dir = if crate::ranged::distance(from, at) == 1 {
                Dir::ALL.into_iter().find(|dir| dir.delta() == (dx, dy))?
            } else {
                let pos = |(x, y)| Position { x, y };
                ai::step_toward(state, pos(from), pos(at))?
            };
            Some(Command::TryMove(player, dir))
        }
    }
}

fn tally(run: &mut Run, effects: &[Fx]) {
    for effect in effects {
        match effect {
            Fx::Damage {
                to_player: false, ..
            } => run.landed += 1,
            Fx::Outcome { kind, .. } => match kind {
                Outcome::Graze => run.grazes += 1,
                Outcome::Miss | Outcome::Dodge => run.misses += 1,
                Outcome::Deflect | Outcome::Absorbed => {}
            },
            _ => {}
        }
    }
}

fn lost(state: &GameState, doll: Entity) -> i64 {
    state
        .world
        .get::<&Health>(doll)
        .map_or(0, |health| i64::from(health.max - health.current))
}

/// Runs summed up: the mean and the spread of damage per turn, and the
/// share of blows that landed or grazed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub mean: f64,
    /// 10th and 90th percentile of damage per turn.
    pub low: f64,
    pub high: f64,
    /// Of every blow, the share that landed (grazes included).
    pub landed: f64,
    pub grazed: f64,
}

pub fn summarize(runs: &[Run]) -> Summary {
    if runs.is_empty() {
        return Summary::default();
    }
    let mut per_turn: Vec<f64> = runs.iter().map(Run::per_turn).collect();
    per_turn.sort_by(f64::total_cmp);
    let at = |share: f64| per_turn[((per_turn.len() - 1) as f64 * share).round() as usize];
    let (landed, grazes, misses) = runs.iter().fold((0, 0, 0), |(l, g, m), run| {
        (l + run.landed, g + run.grazes, m + run.misses)
    });
    let blows = f64::from((landed + misses).max(1));
    Summary {
        mean: per_turn.iter().sum::<f64>() / per_turn.len() as f64,
        low: at(0.1),
        high: at(0.9),
        landed: f64::from(landed) / blows,
        grazed: f64::from(grazes) / blows,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::entity::{Energy, Name};

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    fn build(equipment: &[&str], skills: &[(&str, u32)]) -> Build {
        Build {
            name: "test".into(),
            note: String::new(),
            skills: skills.iter().map(|&(s, l)| (s.to_string(), l)).collect(),
            stats: None,
            equipment: equipment.iter().map(|s| s.to_string()).collect(),
            techniques: vec![],
            style: None,
            policy: vec![Act::Bump],
        }
    }

    fn one(doll: &str) -> Scenario {
        Scenario {
            name: doll.into(),
            dolls: vec![(doll.into(), (1, 0))],
        }
    }

    fn fencer() -> Build {
        build(
            &["Rapier"],
            &[("Melee", 5), ("Sword", 5), ("Pierce", 5), ("One-handed", 5)],
        )
    }

    #[test]
    fn a_doll_never_acts_and_never_dies() {
        let (state, run) = simulate(&data(), &fencer(), &one("Doll"), 10_000, 1).unwrap();
        assert!(run.dealt > 0);
        let doll = state
            .world
            .query::<&Name>()
            .iter()
            .find(|name| name.0 == "training doll")
            .is_some();
        assert!(doll, "the doll is still there");
        let scheduled = state
            .world
            .query::<(&Name, &Energy)>()
            .iter()
            .any(|(name, _)| name.0.contains("doll"));
        assert!(!scheduled, "a doll never gets a turn");
    }

    #[test]
    fn a_run_is_the_same_per_seed() {
        let data = data();
        let a = run(&data, &fencer(), &one("Doll"), 3000, 7).unwrap();
        let b = run(&data, &fencer(), &one("Doll"), 3000, 7).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn the_build_does_not_learn() {
        let (state, _) = simulate(&data(), &fencer(), &one("Doll"), 3000, 1).unwrap();
        let sword = state.blueprints.skills().id("Sword").unwrap();
        assert_eq!(
            skill::xp(&state.world, state.player, sword),
            skill::xp_for(5)
        );
    }

    #[test]
    fn armor_takes_from_a_fencer_and_dodging_makes_it_miss() {
        let data = data();
        let runs = |doll| -> Vec<Run> {
            (0..20)
                .map(|seed| run(&data, &fencer(), &one(doll), 3000, seed).unwrap())
                .collect()
        };
        let bare = summarize(&runs("Doll"));
        let armored = summarize(&runs("ArmoredDoll"));
        let nimble = summarize(&runs("NimbleDoll"));
        assert!(armored.mean < bare.mean, "{armored:?} vs {bare:?}");
        assert!(nimble.landed < bare.landed, "{nimble:?} vs {bare:?}");
    }

    #[test]
    fn a_build_walks_to_a_distant_doll() {
        let scenario = Scenario {
            name: "far".into(),
            dolls: vec![("Doll".into(), (5, 0))],
        };
        let run = run(&data(), &fencer(), &scenario, 1000, 1).unwrap();
        assert!(run.dealt > 0);
    }

    #[test]
    fn a_build_that_names_nothing_real_is_refused() {
        let mut odd = fencer();
        odd.equipment = vec!["Nothing".into()];
        assert!(run(&data(), &odd, &one("Doll"), 100, 1).is_err());
        let mut odd = fencer();
        odd.skills.insert("Juggling".into(), 3);
        assert!(run(&data(), &odd, &one("Doll"), 100, 1).is_err());
    }
}
