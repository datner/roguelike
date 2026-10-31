//! Balance bench (docs/plans/balance.md): a build set before training dolls for a window of ticks,
//! through the real systems (`GameState`, commands, events), so what it reports is the game's, not a
//! model's. The `bench` program drives it; nothing in play calls it.

mod floors;

use std::collections::BTreeMap;

pub use floors::{Dive, DiveRun, End, FloorRun, dive};

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::blueprint::Blueprints;
use crate::command::{self, Command};
use crate::entity::{Ammo, Health, Learner, Pool, PoolRates, Position, Skills, Stats};
use crate::fx::{self, Fx, Outcome};
use crate::input::Dir;
use crate::map::{Grid, Map, Tile};
use crate::rng::Rngs;
use crate::spawn::Spawn;
use crate::state::GameState;
use crate::time::Tick;
use crate::{ai, equipment, holding, skill, spell, style, technique, time};

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
    /// The origin it starts as (a key in `data/origins.ron`); the first if
    /// absent.
    #[serde(default)]
    pub origin: Option<String>,
    /// Levels by skill name, over the origin's; every other skill is 0.
    #[serde(default)]
    pub skills: BTreeMap<String, u32>,
    /// The origin's own if absent.
    #[serde(default)]
    pub stats: Option<Stats>,
    /// Blueprints worn, replacing the origin's kit; the origin's if absent.
    /// A quiver comes stocked.
    #[serde(default)]
    pub equipment: Option<Vec<String>>,
    /// Technique blueprints known, beside the origin's own.
    #[serde(default)]
    pub techniques: Vec<String>,
    /// Spell blueprints known, beside the origin's own.
    #[serde(default)]
    pub spells: Vec<String>,
    /// A style blueprint, followed from the start (the origin's own, if it
    /// knows it).
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
    /// Perform this technique (by blueprint) at the nearest doll.
    Perform(String),
    /// Cast this spell (by blueprint) at the nearest doll.
    Cast(String),
    /// Shoot the wielded launcher at the nearest doll.
    Fire,
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
    pub mana: Spent,
    pub stamina: Spent,
    /// Arrows shot (lost from what the build holds; it never picks one up).
    pub arrows: Spent,
    /// Of those, how many broke instead of landing.
    pub arrows_broken: u32,
    /// The build's own health lost (blood, its own blasts).
    pub health: u32,
}

/// What a run took from one resource.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spent {
    pub total: u32,
    /// Ticks into the run when it first could not pay for the cheapest
    /// thing it paid for in the whole run (a pool), or held none (arrows);
    /// `None` if never.
    pub dry: Option<u64>,
    /// The smallest payment, for `dry`.
    pub cheapest: Option<u32>,
}

/// A resource watched turn by turn; `dry` is judged once the run is over,
/// so a pool is not called dry for want of a price it later stops paying.
#[derive(Default)]
struct Gauge {
    spent: Spent,
    /// Ticks into the run, and what was left then.
    levels: Vec<(u64, u32)>,
}

impl Gauge {
    /// A pool before and after an action taken at `acted`: a spend sets the
    /// pool to what it held then, less the price.
    fn pool(
        &mut self,
        before: Option<(Pool, PoolRates)>,
        after: Option<(Pool, PoolRates)>,
        (acted, now): (Tick, Tick),
        elapsed: u64,
    ) {
        let (Some((old, rates)), Some((new, new_rates))) = (before, after) else {
            return;
        };
        if new != old {
            let price = old.current(rates, acted).saturating_sub(new.value);
            if price > 0 {
                self.spent.total += price;
                let cheapest = self.spent.cheapest.map_or(price, |c| c.min(price));
                self.spent.cheapest = Some(cheapest);
            }
        }
        self.levels.push((elapsed, new.current(new_rates, now)));
    }

    /// A count of things held before and after an action.
    fn stock(&mut self, before: u32, after: u32, elapsed: u64) {
        self.spent.total += before.saturating_sub(after);
        if self.spent.total > 0 {
            self.spent.cheapest = Some(1);
        }
        self.levels.push((elapsed, after));
    }

    fn done(self) -> Spent {
        let dry = self.spent.cheapest.and_then(|cheapest| {
            self.levels
                .iter()
                .find(|&&(_, left)| left < cheapest)
                .map(|&(elapsed, _)| elapsed)
        });
        Spent { dry, ..self.spent }
    }
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
    let mut state = arena(blueprints, build, seed)?;
    dress(&mut state, build)?;
    // The window measures one fixed build, not one that levels as it goes.
    let _ = state.world.remove_one::<Learner>(state.player);
    let dolls = place(&mut state, scenario)?;
    state.run(Command::UpdateFov);
    fx::take_fx(&mut state);

    let start = state.now;
    let end = start.after(window);
    let mut run = Run::default();
    let player = state.player;
    let arrows_at_start = arrows(&state).0;
    let (mut mana_gauge, mut stamina_gauge, mut arrow_gauge) =
        (Gauge::default(), Gauge::default(), Gauge::default());
    while state.now < end && !state.game_over {
        let Some(target) = nearest(&state, &dolls) else {
            break;
        };
        let acted = state.now;
        let mana = spell::pool(&state.world, player);
        let stamina = technique::stamina(&state, player);
        let health = health(&state, player);
        let (held, _) = arrows(&state);
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
        let (now, elapsed) = (state.now, state.now.since(start));
        let pool = spell::pool(&state.world, player);
        mana_gauge.pool(mana, pool, (acted, now), elapsed);
        let pool = technique::stamina(&state, player);
        stamina_gauge.pool(stamina, pool, (acted, now), elapsed);
        run.health += health.saturating_sub(self::health(&state, player));
        arrow_gauge.stock(held, arrows(&state).0, elapsed);
    }
    run.mana = mana_gauge.done();
    run.stamina = stamina_gauge.done();
    run.arrows = arrow_gauge.done();
    run.ticks = state.now.since(start);
    run.dealt = dolls.iter().map(|&doll| lost(&state, doll)).sum();
    let (held, landed) = arrows(&state);
    run.arrows_broken = arrows_at_start.saturating_sub(held + landed);
    Ok((state, run))
}

fn health(state: &GameState, creature: Entity) -> u32 {
    state
        .world
        .get::<&Health>(creature)
        .map_or(0, |health| u32::try_from(health.current).unwrap_or(0))
}

/// Arrows held (nocked or in a quiver: no position) and lying on the floor.
fn arrows(state: &GameState) -> (u32, u32) {
    let mut query = state.world.query::<(&Ammo, Option<&Position>)>();
    query
        .iter()
        .fold((0, 0), |(held, lying), (_, position)| match position {
            Some(_) => (held, lying + 1),
            None => (held + 1, lying),
        })
}

/// A walled, open floor with the player at `START`.
fn arena(blueprints: &Blueprints, build: &Build, seed: u64) -> Result<GameState, String> {
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
        build.origin.as_deref(),
    )
    .map_err(|err| format!("{}: {err}", build.name))
}

/// The origin's player made into `build`: its skills, stats, gear,
/// techniques and style.
fn dress(state: &mut GameState, build: &Build) -> Result<(), String> {
    let player = state.player;
    let mut skills = state
        .world
        .get::<&Skills>(player)
        .map(|skills| (*skills).clone())
        .unwrap_or_default();
    for (name, &level) in &build.skills {
        let id = state
            .blueprints
            .skills()
            .id(name)
            .ok_or_else(|| format!("{}: unknown skill {name:?}", build.name))?;
        skills.set(id, skill::xp_for(level));
    }
    let _ = state.world.insert_one(player, skills);
    // Toughness set by hand raises max health as if earned.
    crate::health::refresh(state, player);
    if let Some(stats) = build.stats {
        let _ = state.world.insert_one(player, stats);
    }

    if let Some(kit) = &build.equipment {
        for (item, _) in equipment::equipped(&state.world, player) {
            holding::take_off(&mut state.world, player, item);
            command::despawn_with_holdings(state, item);
        }
        for name in kit {
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
    }

    for name in &build.techniques {
        let noun = noun(state, name).ok_or_else(|| format!("{}: unknown {name:?}", build.name))?;
        if !technique::knows(&state.world, player, &noun) {
            spawn(state, build, Spawn::blueprint(name).attached_to(player))?;
        }
    }
    for name in &build.spells {
        if named(state, crate::view::castable(state), name).is_none() {
            spawn(state, build, Spawn::blueprint(name).attached_to(player))?;
        }
    }
    for act in &build.policy {
        let known = match act {
            Act::Perform(name) => named(state, technique::known(&state.world, player), name),
            Act::Cast(name) => named(state, crate::view::castable(state), name),
            Act::Bump | Act::Fire => Some(player),
        };
        if known.is_none() {
            return Err(format!(
                "{}: its policy names {act:?}, unknown to it",
                build.name
            ));
        }
    }
    if let Some(name) = &build.style {
        let noun = noun(state, name).ok_or_else(|| format!("{}: unknown {name:?}", build.name))?;
        let known = match named_style(state, &noun) {
            Some(known) => known,
            None => spawn(state, build, Spawn::blueprint(name).carried_by(player))?,
        };
        if !style::followed(&state.world, player).contains(&known) {
            style::follow(state, player, known);
        }
    }
    Ok(())
}

fn spawn(state: &mut GameState, build: &Build, spawn: Spawn) -> Result<Entity, String> {
    state
        .spawn(spawn)
        .map_err(|err| format!("{}: {err}", build.name))
}

/// The style the player knows by `noun`, followed or not.
fn named_style(state: &GameState, noun: &str) -> Option<Entity> {
    style::known(&state.world, state.player)
        .into_iter()
        .find(|&known| {
            state
                .world
                .get::<&crate::entity::Name>(known)
                .is_ok_and(|name| name.0 == noun)
        })
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
                ai::path_step(
                    state,
                    Position {
                        x: from.0,
                        y: from.1,
                    },
                    at,
                )?
            };
            Some(Command::TryMove(player, dir))
        }
        Act::Perform(name) => Some(Command::Perform {
            actor: player,
            technique: named(state, technique::known(&state.world, player), name)?,
            at,
        }),
        Act::Cast(name) => Some(Command::Cast {
            actor: player,
            spell: named(state, crate::view::castable(state), name)?,
            at,
        }),
        Act::Fire => Some(Command::Fire { actor: player, at }),
    }
}

/// The one of `things` made from `blueprint`, by its name.
fn named(state: &GameState, things: Vec<Entity>, blueprint: &str) -> Option<Entity> {
    let noun = noun(state, blueprint)?;
    things.into_iter().find(|&thing| {
        state
            .world
            .get::<&crate::entity::Name>(thing)
            .is_ok_and(|name| name.0 == noun)
    })
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
    pub mana: Use,
    pub stamina: Use,
    pub arrows: Use,
    /// Mean arrows broken and own health lost per run.
    pub arrows_broken: f64,
    pub health: f64,
}

/// One resource over the runs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Use {
    /// Mean spent per run.
    pub spent: f64,
    /// The share of runs that ran dry, and when, on average, they did.
    pub dry: f64,
    pub dry_at: Option<f64>,
}

fn usage(runs: &[Run], of: impl Fn(&Run) -> Spent) -> Use {
    let count = runs.len().max(1) as f64;
    let dried: Vec<u64> = runs.iter().filter_map(|run| of(run).dry).collect();
    Use {
        spent: runs.iter().map(|run| f64::from(of(run).total)).sum::<f64>() / count,
        dry: dried.len() as f64 / count,
        dry_at: (!dried.is_empty()).then(|| dried.iter().sum::<u64>() as f64 / dried.len() as f64),
    }
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
        mana: usage(runs, |run| run.mana),
        stamina: usage(runs, |run| run.stamina),
        arrows: usage(runs, |run| run.arrows),
        arrows_broken: mean(runs, |run| run.arrows_broken),
        health: mean(runs, |run| run.health),
    }
}

fn mean(runs: &[Run], of: impl Fn(&Run) -> u32) -> f64 {
    runs.iter().map(|run| f64::from(of(run))).sum::<f64>() / runs.len().max(1) as f64
}

/// What a build takes to reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cost {
    /// XP its skills hold, summed over the curve: how much use it took.
    pub xp: u32,
    /// Everything it holds or knows, with where each comes from.
    pub needs: Vec<Source>,
}

/// One thing a build needs and the ways to come by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Its blueprint.
    pub thing: String,
    /// "origin: Wanderer", "found lying about, 1 in 23 finds", "taught by the
    /// lunge treatise", …; empty if nothing in the content gives it.
    pub from: Vec<String>,
}

/// What `build` costs, read from the content: origins' starts, what lies
/// about in dungeons, what their creatures wear, manuals, discoveries and
/// styles' inventions. XP is what it takes beyond its origin's skills.
pub fn cost(blueprints: &Blueprints, build: &Build) -> Cost {
    let origin = match &build.origin {
        Some(key) => blueprints.origin(key),
        None => blueprints.origins().first(),
    };
    let start = origin
        .and_then(|origin| blueprints.get(&origin.blueprint))
        .and_then(|bp| bp.skills.clone())
        .unwrap_or_default();
    let xp = build
        .skills
        .iter()
        .map(|(name, &level)| {
            let had = blueprints
                .skills()
                .id(name)
                .and_then(|id| start.0.iter().find(|&&(skill, _)| skill == id))
                .map_or(0, |&(_, xp)| xp);
            skill::xp_for(level).saturating_sub(had)
        })
        .sum();
    let needs = build
        .equipment
        .iter()
        .flatten()
        .chain(&build.techniques)
        .chain(&build.spells)
        .chain(&build.style)
        .map(|thing| Source {
            thing: thing.clone(),
            from: sources(blueprints, thing),
        })
        .collect();
    Cost { xp, needs }
}

fn sources(blueprints: &Blueprints, name: &str) -> Vec<String> {
    let Some(id) = blueprints.id(name) else {
        return Vec::new();
    };
    let noun = |id| {
        blueprints
            .by_id(id)
            .and_then(|bp| bp.noun())
            .unwrap_or("something")
            .to_string()
    };
    let skill_name = |skill| {
        blueprints
            .skills()
            .get(skill)
            .map_or_else(String::new, |def| def.name.clone())
    };
    let mut from = Vec::new();
    let origins: Vec<&str> = blueprints
        .origins()
        .iter()
        .filter(|origin| {
            let starts_with = blueprints.get(&origin.blueprint).is_some_and(|player| {
                [&player.equipment, &player.techniques, &player.spells]
                    .iter()
                    .any(|list| list.contains(&id))
            });
            starts_with || origin.styles.iter().any(|style| style == name)
        })
        .map(|origin| origin.name.as_str())
        .collect();
    if !origins.is_empty() {
        from.push(format!("origin: {}", origins.join(", ")));
    }
    if blueprints.items().contains(&id) {
        let finds = blueprints.items().len();
        from.push(format!("found lying about, 1 in {finds} finds"));
    }
    for (_, dungeon) in blueprints.dungeons() {
        for inhabitant in &dungeon.creatures {
            if let Some(creature) = blueprints.get(&inhabitant.blueprint)
                && creature.equipment.contains(&id)
            {
                let source = format!(
                    "worn by the {} in {}",
                    creature.noun().unwrap_or("?"),
                    dungeon.name
                );
                if !from.contains(&source) {
                    from.push(source);
                }
            }
        }
    }
    for (other, blueprint) in blueprints.all() {
        if blueprint
            .teaches
            .as_ref()
            .is_some_and(|teaches| teaches.0.contains(&id))
        {
            from.push(format!("taught by the {}", noun(other)));
        }
        if let Some(style) = &blueprint.style {
            for &(level, invented) in &style.invents {
                if invented == id {
                    from.push(format!(
                        "invented by {} at {level}",
                        skill_name(style.skill)
                    ));
                }
            }
        }
    }
    for discovery in blueprints.discoveries() {
        if matches!(discovery.grants, crate::discovery::Granted::Blueprint { id: granted, .. } if granted == id)
        {
            let needs: Vec<String> = discovery
                .requires
                .iter()
                .map(|&(skill, level)| format!("{} {level}", skill_name(skill)))
                .collect();
            from.push(format!("discovered from {}", needs.join(" and ")));
        }
    }
    from
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::entity::{Energy, Name};

    fn data() -> Blueprints {
        Blueprints::load(&crate::testkit::data()).unwrap()
    }

    fn build(equipment: &[&str], skills: &[(&str, u32)]) -> Build {
        Build {
            name: "test".into(),
            note: String::new(),
            origin: None,
            skills: skills.iter().map(|&(s, l)| (s.to_string(), l)).collect(),
            stats: None,
            equipment: Some(equipment.iter().map(|s| s.to_string()).collect()),
            techniques: vec![],
            spells: vec![],
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
        odd.equipment = Some(vec!["Nothing".into()]);
        assert!(run(&data(), &odd, &one("Doll"), 100, 1).is_err());
        let mut odd = fencer();
        odd.skills.insert("Juggling".into(), 3);
        assert!(run(&data(), &odd, &one("Doll"), 100, 1).is_err());
        let mut odd = fencer();
        odd.policy = vec![Act::Perform("Lunge".into())];
        assert!(
            run(&data(), &odd, &one("Doll"), 100, 1).is_err(),
            "not known"
        );
    }

    fn dealt(build: &Build, scenario: &Scenario, window: u64) -> i64 {
        let data = data();
        (0..10)
            .map(|seed| run(&data, build, scenario, window, seed).unwrap().dealt)
            .sum()
    }

    #[test]
    fn a_policy_falls_through_a_recharging_technique() {
        let mut lunger = fencer();
        lunger.techniques = vec!["Lunge".into()];
        lunger.policy = vec![Act::Perform("Lunge".into())];
        let far = Scenario {
            name: "far".into(),
            dolls: vec![("Doll".into(), (2, 0))],
        };
        let alone = dealt(&lunger, &far, 3000);
        assert!(alone > 0, "it lunges");
        lunger.policy.push(Act::Bump);
        assert!(
            dealt(&lunger, &far, 3000) > alone,
            "and bumps between lunges"
        );
    }

    #[test]
    fn an_area_technique_strikes_every_doll_it_catches() {
        let mut cleaver = build(&["Sabre"], &[("Melee", 5), ("Slash", 5), ("Sword", 5)]);
        cleaver.techniques = vec!["Cleave".into()];
        cleaver.policy = vec![Act::Perform("Cleave".into())];
        let pack = Scenario {
            name: "pack".into(),
            dolls: vec![
                ("Doll".into(), (1, -1)),
                ("Doll".into(), (1, 0)),
                ("Doll".into(), (1, 1)),
            ],
        };
        assert!(dealt(&cleaver, &pack, 3000) > 2 * dealt(&cleaver, &one("Doll"), 3000));
    }

    #[test]
    fn a_shot_without_arrows_falls_through_to_a_bump() {
        let mut archer = build(&["ShortBow"], &[("Archery", 5), ("Bow", 5)]);
        archer.policy = vec![Act::Fire];
        assert_eq!(dealt(&archer, &one("Doll"), 1000), 0, "no quiver");
        archer.policy.push(Act::Bump);
        assert!(dealt(&archer, &one("Doll"), 1000) > 0);
    }

    #[test]
    fn spells_are_cast_at_the_doll() {
        let mut caster = build(&["Torch"], &[("Magic", 5), ("Arcane", 5), ("Bolt", 5)]);
        caster.spells = vec!["MagicMissile".into()];
        caster.policy = vec![Act::Cast("MagicMissile".into())];
        let far = Scenario {
            name: "far".into(),
            dolls: vec![("Doll".into(), (5, 0))],
        };
        assert!(dealt(&caster, &far, 1000) > 0);
    }

    #[test]
    fn a_caster_spends_mana_until_it_cannot_pay() {
        let mut caster = build(&["Torch"], &[("Magic", 5), ("Arcane", 5), ("Bolt", 5)]);
        caster.spells = vec!["MagicMissile".into()];
        caster.policy = vec![Act::Cast("MagicMissile".into())];
        let run = run(&data(), &caster, &one("Doll"), 3000, 1).unwrap();
        assert!(run.mana.total > 0);
        assert_eq!(
            run.mana.total % run.mana.cheapest.unwrap(),
            0,
            "whole casts"
        );
        assert!(run.mana.dry.is_some_and(|tick| tick < 3000), "{run:?}");
        assert_eq!((run.stamina.total, run.arrows.total, run.health), (0, 0, 0));

        caster.policy = vec![Act::Bump];
        assert_eq!(run_of(&caster).mana, Spent::default(), "bumping is free");
    }

    fn run_of(build: &Build) -> Run {
        run(&data(), build, &one("Doll"), 3000, 1).unwrap()
    }

    #[test]
    fn an_archer_runs_out_of_arrows_and_bashes_on() {
        let mut archer = build(&["ShortBow", "Quiver"], &[("Archery", 5), ("Bow", 5)]);
        archer.policy = vec![Act::Fire, Act::Bump];
        let run = run(&data(), &archer, &one("Doll"), 3000, 1).unwrap();
        let stocked = u32::try_from(crate::state::STARTING_ARROWS).unwrap();
        assert_eq!(run.arrows.total, stocked);
        assert!(run.arrows.dry.is_some_and(|tick| tick < 3000));
        assert!(run.arrows_broken <= stocked);
        let shots = run.landed + run.misses;
        assert!(shots > stocked, "it bashed once the quiver was empty");
    }

    #[test]
    fn a_build_costs_the_xp_of_its_levels() {
        let fencer = fencer();
        let cost = cost(&data(), &fencer);
        assert_eq!(cost.xp, 4 * skill::xp_for(5), "four skills at 5");
    }

    #[test]
    fn where_things_come_from_is_read_from_the_content() {
        let mut lunger = fencer();
        lunger.equipment = Some(vec!["ShortBow".into()]);
        lunger.techniques = vec!["Lunge".into(), "Cleave".into()];
        lunger.style = Some("Brawling".into());
        let cost = cost(&data(), &lunger);
        let from = |thing: &str| {
            cost.needs
                .iter()
                .find(|need| need.thing == thing)
                .unwrap()
                .from
                .clone()
        };
        let bow = from("ShortBow");
        assert_eq!(bow[0], "origin: Wanderer");
        assert!(bow[1].starts_with("found lying about, 1 in "), "{bow:?}");
        let lunge = from("Lunge");
        assert!(
            lunge.contains(&"taught by the treatise on the lunge".to_string()),
            "{lunge:?}"
        );
        assert!(
            lunge.contains(&"invented by Fencing at 3".to_string()),
            "{lunge:?}"
        );
        assert!(from("Cleave").contains(&"discovered from Melee 3 and Slash 3".to_string()));
        assert_eq!(
            from("Brawling"),
            [
                "origin: Pit fighter",
                "discovered from Melee 3 and Unarmed 3 and Dodge 3"
            ]
        );
    }

    #[test]
    fn a_build_starts_from_its_origin() {
        let mut duelist = build(&[], &[("Pierce", 5)]);
        duelist.origin = Some("Duelist".into());
        duelist.equipment = None;
        duelist.style = Some("Fencing".into());
        let (state, done) = simulate(&data(), &duelist, &one("Doll"), 3000, 1).unwrap();
        assert!(done.dealt > 0);
        let player = state.player;
        // What the origin declares, read from the data so its numbers can move.
        let blueprints = data();
        let origin = blueprints.get("Duelist").unwrap();
        let declared = |name| {
            let id = blueprints.skills().id(name).unwrap();
            origin
                .skills
                .iter()
                .flat_map(|skills| &skills.0)
                .find(|&&(skill, _)| skill == id)
                .map_or(0, |&(_, xp)| xp)
        };
        assert!(
            declared("Pierce") < skill::xp_for(5),
            "the build adds to it"
        );
        let held: Vec<String> = equipment::equipped(&state.world, player)
            .into_iter()
            .map(|(item, _)| state.world.get::<&Name>(item).unwrap().0.clone())
            .collect();
        let kit: Vec<&str> = origin
            .equipment
            .iter()
            .map(|&id| blueprints.by_id(id).unwrap().noun().unwrap())
            .collect();
        assert_eq!(held, kit, "the origin's kit");
        assert_eq!(style::known(&state.world, player).len(), 1, "fencing once");
        let xp = |name| {
            let id = state.blueprints.skills().id(name).unwrap();
            skill::xp(&state.world, player, id)
        };
        assert_eq!(skill::level(xp("Pierce")), 5, "the build's level");
        assert_eq!(xp("Sword"), declared("Sword"), "the origin's, untouched");

        // What the origin gave is not paid for; only the rest is.
        let cost = cost(&data(), &duelist);
        assert_eq!(cost.xp, skill::xp_for(5) - declared("Pierce"));
        assert_eq!(cost.needs[0].from[0], "origin: Duelist");

        duelist.origin = Some("Nobody".into());
        let refused = run(&data(), &duelist, &one("Doll"), 100, 1).unwrap_err();
        assert_eq!(refused, "test: no origin \"Nobody\"");
    }
}
