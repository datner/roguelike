//! Techniques (ADR 0024): special actions done with the body and what it
//! wields, such as a lunge. They share the spell's effect half (shapes,
//! aiming) but not its payment: a technique costs a turn, a recharge and
//! stamina, and its blow is an ordinary blow (`combat::blow`), so it lands,
//! grazes and trains like a bump.

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::combat::{self, BlowMods, Strike};
use crate::command::Command;
use crate::cost::TICKS_PER_TURN;
use crate::entity::{
    Area, Blow, Dash, Name, Need, Pool, PoolRates, Position, Recharging, Shape, Stamina, Stats,
    Technique,
};
use crate::grammar::Subject;
use crate::holding;
use crate::map::Tile;
use crate::ranged;
use crate::side;
use crate::skill::{self, Domain, SkillBook, SkillId};
use crate::spell;
use crate::state::GameState;
use crate::view::ItemRef;

/// A `Technique` as written in RON and saves: skills by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechniqueDef {
    pub domain: Domain,
    pub verb: String,
    #[serde(default = "one")]
    pub reach: i32,
    /// In ticks.
    #[serde(default)]
    pub recharge: u64,
    #[serde(default)]
    pub stamina: u32,
    #[serde(default)]
    pub requires: Vec<NeedDef>,
}

fn one() -> i32 {
    1
}

/// A `Need` as written in RON and saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeedDef {
    Wielding(Vec<String>),
    Skill(String, u32),
}

impl TechniqueDef {
    /// The skills it names, for checking at load.
    pub(crate) fn names(&self) -> impl Iterator<Item = &String> {
        self.requires.iter().flat_map(|need| match need {
            NeedDef::Wielding(skills) => skills.iter().collect::<Vec<_>>(),
            NeedDef::Skill(skill, _) => vec![skill],
        })
    }

    /// With every skill resolved, or the first name `book` lacks.
    pub(crate) fn resolve(&self, book: &SkillBook) -> Result<Technique, String> {
        let id = |name: &String| book.id(name).ok_or_else(|| name.clone());
        let requires = self
            .requires
            .iter()
            .map(|need| match need {
                NeedDef::Wielding(skills) => {
                    let mut ids = skills.iter().map(id).collect::<Result<Vec<_>, _>>()?;
                    ids.sort();
                    Ok(Need::Wielding(ids))
                }
                NeedDef::Skill(skill, level) => Ok(Need::Skill(id(skill)?, *level)),
            })
            .collect::<Result<_, String>>()?;
        Ok(Technique {
            domain: self.domain,
            verb: self.verb.clone(),
            reach: self.reach,
            recharge: self.recharge,
            stamina: self.stamina,
            requires,
        })
    }

    /// `technique` with its skills named, or the first id `book` lacks.
    pub(crate) fn of(technique: &Technique, book: &SkillBook) -> Result<Self, SkillId> {
        let name = |id: SkillId| book.get(id).map(|def| def.name.clone()).ok_or(id);
        let requires = technique
            .requires
            .iter()
            .map(|need| match need {
                Need::Wielding(skills) => Ok(NeedDef::Wielding(
                    skills
                        .iter()
                        .map(|&id| name(id))
                        .collect::<Result<_, _>>()?,
                )),
                Need::Skill(skill, level) => Ok(NeedDef::Skill(name(*skill)?, *level)),
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            domain: technique.domain,
            verb: technique.verb.clone(),
            reach: technique.reach,
            recharge: technique.recharge,
            stamina: technique.stamina,
            requires,
        })
    }
}

/// The techniques `creature` knows, in the order it learned them.
pub(crate) fn known(world: &World, creature: Entity) -> Vec<Entity> {
    holding::attached(world, creature)
        .into_iter()
        .filter(|&thing| world.satisfies::<&Technique>(thing))
        .collect()
}

/// A technique as it would go now, checked the same way for the command, the
/// aim preview, the technique list and the AI.
#[derive(Debug, Clone)]
pub(crate) struct Ready {
    pub reach: i32,
    /// The blow as it would land, before the target's defences.
    pub strike: Strike,
    pub mods: BlowMods,
    pub dash: Option<Dash>,
    /// The ground it sweeps; `None` strikes the one creature aimed at.
    pub area: Option<Area>,
    pub stamina: u32,
    pub recharge: u64,
    pub verb: String,
}

/// `actor`'s stamina pool and its size and pace for their stats.
pub(crate) fn stamina(world: &World, actor: Entity) -> Option<(Pool, PoolRates)> {
    let pool = world.get::<&Stamina>(actor).ok()?.0.clone();
    let stats = world.get::<&Stats>(actor).ok().map(|stats| *stats);
    let rates = pool.rates(stats);
    Some((pool, rates))
}

/// `ready`, except that a technique that can't be performed now still comes
/// back, with the reason, so a list can show it; `Err` only if `actor`
/// doesn't know it or has nothing to strike with.
pub(crate) fn inspect(
    state: &GameState,
    actor: Entity,
    technique: Entity,
) -> Result<(Ready, Option<String>), String> {
    let world = &state.world;
    let data = match world.get::<&Technique>(technique) {
        Ok(data) if known(world, actor).contains(&technique) => (*data).clone(),
        _ => return Err("You don't know that technique.".to_string()),
    };
    let blow = world
        .get::<&Blow>(technique)
        .map_or_else(|_| Blow::default(), |blow| *blow);
    let mods = BlowMods {
        accuracy: blow.accuracy,
        damage_pct: blow.damage_pct,
        uses: None,
    };
    let Some((strike, used)) = combat::blow_strike(world, state.blueprints.skills(), actor, &mods)
    else {
        return Err("You have nothing to strike with.".to_string());
    };
    let ready = Ready {
        reach: data.reach,
        strike,
        mods,
        dash: world.get::<&Dash>(technique).ok().map(|dash| *dash),
        area: world.get::<&Area>(technique).ok().map(|area| *area),
        stamina: data.stamina,
        recharge: data.recharge,
        verb: data.verb.clone(),
    };
    let blocked = unmet(state, actor, technique, &data, &used);
    Ok((ready, blocked))
}

/// `actor` performing `technique`: they know it, it has recharged, and every
/// need is met.
pub(crate) fn ready(state: &GameState, actor: Entity, technique: Entity) -> Result<Ready, String> {
    match inspect(state, actor, technique)? {
        (_, Some(reason)) => Err(reason),
        (ready, None) => Ok(ready),
    }
}

/// Why `technique` can't be performed now, if it can't: still recharging, a
/// need unmet (`used` is the skills its blow would use), or too little
/// stamina.
fn unmet(
    state: &GameState,
    actor: Entity,
    technique: Entity,
    data: &Technique,
    used: &[SkillId],
) -> Option<String> {
    let world = &state.world;
    let name = name(world, technique);
    let recharging = world
        .get::<&Recharging>(technique)
        .is_ok_and(|recharging| recharging.0.is_active(state.now));
    if recharging {
        return Some(format!("Your {name} is not ready."));
    }
    let book = state.blueprints.skills();
    let named = |ids: &[SkillId]| {
        let names: Vec<String> = ids
            .iter()
            .filter_map(|&id| book.get(id).map(|def| def.name.clone()))
            .collect();
        names.join(" and ")
    };
    for need in &data.requires {
        match need {
            Need::Wielding(skills) => {
                if !skills.iter().all(|skill| used.contains(skill)) {
                    return Some(format!("Your {name} needs {}.", named(skills)));
                }
            }
            &Need::Skill(skill, level) => {
                if skill::level(skill::xp(world, actor, skill)) < level {
                    return Some(format!("Your {name} needs {} {level}.", named(&[skill])));
                }
            }
        }
    }
    if data.stamina > 0 {
        let Some((pool, rates)) = stamina(world, actor) else {
            return Some("You have no stamina for that.".to_string());
        };
        if pool.current(rates, state.now) < data.stamina {
            return Some("Not enough stamina.".to_string());
        }
    }
    None
}

fn name(world: &World, technique: Entity) -> String {
    world
        .get::<&Name>(technique)
        .map_or_else(|_| "technique".to_string(), |name| name.0.clone())
}

/// Where a technique aimed at a tile goes: whom it strikes and where its
/// performer ends up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Footprint {
    /// Who the blows fall on, in `SpawnSeq` order for an area.
    pub targets: Vec<Entity>,
    /// Where the performer stands when the blows land.
    pub stand: (i32, i32),
    /// The ground an area technique sweeps; empty for one blow.
    pub area: Vec<(i32, i32)>,
}

/// What `ready` aimed at `at` would strike from where `actor` stands, or why
/// it can't. One blow needs an enemy on `at`, in reach and view, on a
/// straight or diagonal line with open, empty ground between. An area is
/// aimed like a spell of its shape (ADR 0014) and must catch an enemy.
pub(crate) fn footprint(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    at: (i32, i32),
) -> Result<Footprint, String> {
    let Some(from) = ranged::position(state, actor) else {
        return Err("You can't act from nowhere.".to_string());
    };
    if let Some(area) = ready.area {
        return swept(state, actor, ready, area, from, at);
    }
    if from == at {
        return Err("You can't aim at yourself.".to_string());
    }
    let (dx, dy) = (at.0 - from.0, at.1 - from.1);
    if dx != 0 && dy != 0 && dx.abs() != dy.abs() {
        return Err("Only in a straight line.".to_string());
    }
    let distance = dx.abs().max(dy.abs());
    if distance > ready.reach {
        return Err("That is out of reach.".to_string());
    }
    if !in_view(state, actor, at) {
        return Err("You can't see that spot.".to_string());
    }
    let step = (dx.signum(), dy.signum());
    let between = (1..distance).map(|k| (from.0 + step.0 * k, from.1 + step.1 * k));
    for tile in between {
        if !Tile::is_open(state.map.get(tile.0, tile.1)) || spell::occupied(&state.world, tile) {
            return Err("Something is in the way.".to_string());
        }
    }
    let target = creature_at(&state.world, at)
        .filter(|&target| side::enemies(&state.world, actor, target))
        .ok_or_else(|| "There is no enemy there.".to_string())?;
    let moves = ready
        .dash
        .map_or(0, |dash| dash.tiles.clamp(0, distance - 1));
    Ok(Footprint {
        targets: vec![target],
        stand: (from.0 + step.0 * moves, from.1 + step.1 * moves),
        area: Vec::new(),
    })
}

fn in_view(state: &GameState, actor: Entity, at: (i32, i32)) -> bool {
    actor != state.player || state.visible.get(at.0, at.1).copied().unwrap_or(false)
}

/// An area technique's footprint: the shape's ground (`spell::cover`, so a
/// spell and a technique of one shape catch the same creatures), aimed as a
/// spell of that shape is, catching at least one enemy.
fn swept(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    area: Area,
    from: (i32, i32),
    at: (i32, i32),
) -> Result<Footprint, String> {
    match area.shape {
        Shape::Nova { .. } => {}
        Shape::Cone | Shape::Beam if from == at => {
            return Err("You can't aim at yourself.".to_string());
        }
        Shape::Cone | Shape::Beam => {}
        Shape::Burst { .. } => {
            if ranged::distance(from, at) > ready.reach {
                return Err("That is out of reach.".to_string());
            }
            if !in_view(state, actor, at) {
                return Err("You can't see that spot.".to_string());
            }
        }
    }
    let spell::Cover {
        tiles, mut caught, ..
    } = spell::cover(state, actor, area, from, at, ready.reach);
    // A blow never falls on its own performer.
    caught.retain(|&target| target != actor);
    if !caught
        .iter()
        .any(|&target| side::enemies(&state.world, actor, target))
    {
        return Err("No enemy in reach.".to_string());
    }
    Ok(Footprint {
        targets: caught,
        stand: from,
        area: tiles,
    })
}

/// The living creature standing on `at`, if any.
fn creature_at(world: &World, at: (i32, i32)) -> Option<Entity> {
    world
        .query::<(Entity, &Position, &crate::entity::Health)>()
        .without::<&crate::entity::Dead>()
        .iter()
        .find(|&(_, pos, _)| (pos.x, pos.y) == at)
        .map(|(entity, _, _)| entity)
}

/// Why `Perform` is refused, if it is.
pub(crate) fn check_perform(
    state: &GameState,
    actor: Entity,
    technique: Entity,
    at: (i32, i32),
) -> Result<(), String> {
    footprint(state, actor, &ready(state, actor, technique)?, at).map(|_| ())
}

/// Applies `Command::Perform`: pays, moves the performer if it dashes, strikes,
/// and starts the recharge.
pub(crate) fn perform(state: &mut GameState, actor: Entity, technique: Entity, at: (i32, i32)) {
    let Ok(ready) = ready(state, actor, technique) else {
        return;
    };
    let Ok(footprint) = footprint(state, actor, &ready, at) else {
        return;
    };
    if ready.stamina > 0
        && let Some((pool, rates)) = stamina(&state.world, actor)
    {
        let spent = pool.spend(ready.stamina, rates, state.now);
        let _ = state.world.insert_one(actor, Stamina(spent));
    }
    if ready.recharge > 0 {
        state.commands.push_back(Command::Recharge {
            spell: technique,
            duration: ready.recharge,
        });
    }
    if let Ok(mut pos) = state.world.get::<&mut Position>(actor) {
        (pos.x, pos.y) = footprint.stand;
    }
    if actor == state.player && ranged::position(state, actor) != Some(at) {
        state.commands.push_back(Command::FindItems(actor));
    }
    let aimed = match footprint.targets.as_slice() {
        [target] if ready.area.is_none() => {
            format!("at {}", Subject::of(&state.world, *target).noun())
        }
        _ => String::new(),
    };
    let message = Subject::of(&state.world, actor).sentence(&ready.verb, &aimed);
    state.log.push(message);
    for target in footprint.targets {
        let _ = combat::blow(state, actor, target, &ready.mods);
    }
}

/// One technique the player knows, for the technique list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechniqueView {
    pub technique: ItemRef,
    /// Bare noun, e.g. "lunge".
    pub name: String,
    pub reach: i32,
    /// The ground it sweeps; `None` for one blow.
    pub shape: Option<Shape>,
    /// Damage range before the target's defences.
    pub damage: (i32, i32),
    /// Turns until it can be performed again; 0 when ready.
    pub recharging: u64,
    /// Turns it recharges for once performed.
    pub recharge: u64,
    /// Why it can't be performed now, if it can't.
    pub blocked: Option<String>,
}

/// What the player knows, in the order they learned it.
pub fn techniques(state: &GameState) -> Vec<TechniqueView> {
    let world = &state.world;
    known(world, state.player)
        .into_iter()
        .filter_map(|technique| {
            let (ready, blocked) = inspect(state, state.player, technique).ok()?;
            let left = world
                .get::<&Recharging>(technique)
                .map_or(0, |recharging| recharging.0.end().since(state.now));
            let (low, high) = ready.strike.range();
            Some(TechniqueView {
                technique: ItemRef::of(world, technique)?,
                name: name(world, technique),
                reach: ready.reach,
                shape: ready.area.map(|area| area.shape),
                damage: (ready.mods.scale(low), ready.mods.scale(high)),
                recharging: left.div_ceil(TICKS_PER_TURN),
                recharge: ready.recharge.div_ceil(TICKS_PER_TURN),
                blocked,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{DamageType, DamageTypes, Health, Learner, Stat, Weapon};
    use crate::input::PlayerAction;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, see_everything, sure_hits, thing};
    use crate::time::take_turn;

    /// A lunge as `Lunge.ron` has it, needing what `needs` names.
    fn lunge(stamina: u32, needs: Vec<Need>) -> Technique {
        Technique {
            domain: Domain::Martial,
            verb: "lunge".to_string(),
            reach: 2,
            recharge: 500,
            stamina,
            requires: needs,
        }
    }

    fn pierce(state: &GameState) -> SkillId {
        state.blueprints.skills().id("Pierce").unwrap()
    }

    /// A bare room, everything in view, the player holding a thrusting blade
    /// in the right hand and knowing a lunge that needs Pierce.
    fn lunger() -> (GameState, Entity) {
        let mut state = bare_state();
        see_everything(&mut state);
        wield(&mut state, &[DamageType::Piercing]);
        let needs = vec![Need::Wielding(vec![pierce(&state)])];
        let technique = learn(&mut state, lunge(0, needs), Blow::default());
        (state, technique)
    }

    fn wield(state: &mut GameState, kinds: &[DamageType]) -> Entity {
        let player = state.player;
        thing("blade")
            .item()
            .with(Weapon {
                min: 2,
                max: 2,
                stat: Stat::Agility,
                types: DamageTypes::of(kinds),
            })
            .equipped_on(player, 2)
            .make(state)
    }

    fn learn(state: &mut GameState, technique: Technique, blow: Blow) -> Entity {
        let player = state.player;
        thing("lunge")
            .with(technique)
            .with(Dash { tiles: 1 })
            .with(blow)
            .attached_to(player)
            .make(state)
    }

    fn goblin(state: &mut GameState, at: (i32, i32)) -> Entity {
        thing("goblin")
            .at(at.0, at.1)
            .health(10)
            .hostile()
            .make(state)
    }

    fn perform(state: &mut GameState, technique: Entity, at: (i32, i32)) {
        let technique = ItemRef::of(&state.world, technique).unwrap();
        take_turn(state, PlayerAction::Perform { technique, at });
    }

    fn at(state: &GameState, who: Entity) -> (i32, i32) {
        ranged::position(state, who).unwrap()
    }

    fn health(state: &GameState, who: Entity) -> i32 {
        state.world.get::<&Health>(who).unwrap().current
    }

    #[test]
    fn a_lunge_steps_in_and_strikes_from_two_tiles() {
        let (mut state, technique) = lunger();
        let player = state.player;
        sure_hits(&mut state, player);
        let goblin = goblin(&mut state, (3, 1));

        perform(&mut state, technique, (3, 1));

        assert_eq!(at(&state, player), (2, 1));
        assert!(health(&state, goblin) < 10);
        assert!(
            state
                .log
                .recent(10)
                .any(|line| line == "You lunge at the goblin.")
        );
    }

    #[test]
    fn a_lunge_from_beside_its_target_strikes_without_moving() {
        let (mut state, technique) = lunger();
        let player = state.player;
        sure_hits(&mut state, player);
        let goblin = goblin(&mut state, (2, 1));

        perform(&mut state, technique, (2, 1));

        assert_eq!(at(&state, player), (1, 1));
        assert!(health(&state, goblin) < 10);
    }

    /// Refused input logs why and leaves the world as it was.
    fn refused(state: &mut GameState, technique: Entity, aim: (i32, i32), why: &str) {
        let player = state.player;
        let now = state.now;
        perform(state, technique, aim);
        assert_eq!(last_log(state), why);
        assert_eq!(at(state, player), (1, 1));
        assert_eq!(state.now, now);
    }

    #[test]
    fn a_lunge_needs_an_enemy_in_reach_on_a_clear_line() {
        let (mut state, technique) = lunger();
        goblin(&mut state, (4, 1));
        refused(&mut state, technique, (4, 1), "That is out of reach.");
        goblin(&mut state, (3, 2));
        refused(&mut state, technique, (3, 2), "Only in a straight line.");
        refused(&mut state, technique, (1, 3), "There is no enemy there.");
        goblin(&mut state, (3, 3));
        thing("rubble")
            .at(2, 2)
            .with(crate::entity::BlocksMovement)
            .make(&mut state);
        refused(&mut state, technique, (3, 3), "Something is in the way.");
        refused(&mut state, technique, (1, 1), "You can't aim at yourself.");
    }

    #[test]
    fn a_lunge_with_a_slashing_blade_is_refused() {
        let mut state = bare_state();
        see_everything(&mut state);
        wield(&mut state, &[DamageType::Slashing]);
        let needs = vec![Need::Wielding(vec![pierce(&state)])];
        let technique = learn(&mut state, lunge(0, needs), Blow::default());
        goblin(&mut state, (3, 1));
        refused(&mut state, technique, (3, 1), "Your lunge needs Pierce.");
    }

    #[test]
    fn a_skill_need_asks_for_its_level() {
        let mut state = bare_state();
        see_everything(&mut state);
        wield(&mut state, &[DamageType::Piercing]);
        let dagger = state.blueprints.skills().id("Dagger").unwrap();
        let technique = learn(
            &mut state,
            lunge(0, vec![Need::Skill(dagger, 3)]),
            Blow::default(),
        );
        goblin(&mut state, (3, 1));
        refused(&mut state, technique, (3, 1), "Your lunge needs Dagger 3.");

        let mut skills = crate::entity::Skills::default();
        skills.set(dagger, skill::xp_for(3));
        let player = state.player;
        let _ = state.world.insert_one(player, skills);
        perform(&mut state, technique, (3, 1));
        assert_eq!(at(&state, player), (2, 1));
    }

    #[test]
    fn performing_starts_a_recharge_read_from_the_timestamps() {
        let (mut state, technique) = lunger();
        let player = state.player;
        goblin(&mut state, (3, 1));
        perform(&mut state, technique, (3, 1));
        let _ = state.world.insert_one(player, Position { x: 1, y: 1 });

        refused(&mut state, technique, (3, 1), "Your lunge is not ready.");
        assert_eq!(techniques(&state)[0].recharging, 4);

        state.now = state.now.after(500);
        assert!(ready(&state, player, technique).is_ok());
        assert_eq!(techniques(&state)[0].recharging, 0);
    }

    fn pool(value: u32) -> Stamina {
        Stamina(Pool {
            value,
            max: 10,
            as_of: crate::time::Tick::default(),
            regen_ticks: 0,
            growth: Vec::new(),
        })
    }

    #[test]
    fn stamina_is_paid_from_a_pool_and_refused_without_enough() {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        wield(&mut state, &[DamageType::Piercing]);
        let technique = learn(&mut state, lunge(3, Vec::new()), Blow::default());
        goblin(&mut state, (3, 1));
        refused(
            &mut state,
            technique,
            (3, 1),
            "You have no stamina for that.",
        );

        let _ = state.world.insert_one(player, pool(2));
        refused(&mut state, technique, (3, 1), "Not enough stamina.");

        let _ = state.world.insert_one(player, pool(5));
        perform(&mut state, technique, (3, 1));
        let (left, rates) = stamina(&state.world, player).unwrap();
        assert_eq!(left.current(rates, state.now), 2);
    }

    #[test]
    fn a_technique_costing_nothing_needs_no_pool() {
        let (mut state, technique) = lunger();
        let player = state.player;
        goblin(&mut state, (3, 1));
        perform(&mut state, technique, (3, 1));
        assert_eq!(at(&state, player), (2, 1));
        assert!(!state.world.satisfies::<&Stamina>(player));
    }

    #[test]
    fn a_plain_lunge_rolls_like_a_bump_from_the_same_stream() {
        // One lunges from two tiles, the other bumps from beside: the same
        // draws, the same damage, and the same stream after.
        let (mut lunging, technique) = lunger();
        let (mut bumping, _) = lunger();
        let player = lunging.player;
        let _ = bumping.world.insert_one(player, Position { x: 2, y: 1 });
        let a = goblin(&mut lunging, (3, 1));
        let b = goblin(&mut bumping, (3, 1));

        perform(&mut lunging, technique, (3, 1));
        take_turn(&mut bumping, PlayerAction::Move(crate::input::Dir::E));

        assert_eq!(health(&lunging, a), health(&bumping, b));
        assert_eq!(lunging.rngs.combat, bumping.rngs.combat);
    }

    #[test]
    fn a_blow_shifts_accuracy_and_damage() {
        let (mut state, _) = lunger();
        let player = state.player;
        let plain = combat::blow_strike(
            &state.world,
            state.blueprints.skills(),
            player,
            &BlowMods::default(),
        )
        .unwrap()
        .0;
        let technique = learn(
            &mut state,
            lunge(0, Vec::new()),
            Blow {
                accuracy: 2,
                damage_pct: 50,
            },
        );
        let (ready, _) = inspect(&state, player, technique).unwrap();
        assert_eq!(ready.strike.accuracy, plain.accuracy + 2);
        let view = techniques(&state);
        assert_eq!(view[1].damage, (3, 3));
    }

    #[test]
    fn a_lunge_trains_what_the_blow_uses() {
        let (mut state, technique) = lunger();
        let player = state.player;
        let _ = state.world.insert_one(player, Learner);
        sure_hits(&mut state, player);
        goblin(&mut state, (3, 1));
        perform(&mut state, technique, (3, 1));
        let pierce = pierce(&state);
        assert!(skill::xp(&state.world, player, pierce) > 0);
    }

    #[test]
    fn the_aim_preview_agrees_with_the_performance() {
        let (mut state, technique) = lunger();
        goblin(&mut state, (3, 1));
        goblin(&mut state, (3, 2));
        goblin(&mut state, (5, 1));
        let shot = crate::aim::Shot::Technique(ItemRef::of(&state.world, technique).unwrap());

        assert_eq!(crate::aim::aim_targets(&state, shot), [(3, 1)]);
        let aim = crate::aim::aim(&state, shot, (3, 1)).unwrap();
        assert_eq!(aim.target.as_deref(), Some("goblin"));
        assert_eq!((aim.problem, aim.damage), (None, (2, 2)));
        assert!(aim.chances.is_some());
        let off_line = crate::aim::aim(&state, shot, (3, 2)).unwrap();
        assert_eq!(
            off_line.problem.as_deref(),
            Some("Only in a straight line.")
        );
        assert_eq!(off_line.target, None);
    }

    /// The player, holding a slashing blade, knowing an area technique.
    fn sweeper(shape: Shape) -> (GameState, Entity) {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        sure_hits(&mut state, player);
        wield(&mut state, &[DamageType::Slashing]);
        let technique = thing("cleave")
            .with(Technique {
                verb: "cleave".to_string(),
                ..lunge(0, Vec::new())
            })
            .with(Area {
                shape,
                hits: crate::entity::Hits::Enemies,
                falloff: 0,
            })
            .attached_to(player)
            .make(&mut state);
        (state, technique)
    }

    #[test]
    fn a_cleave_strikes_every_enemy_around_and_spares_allies() {
        let (mut state, technique) = sweeper(Shape::Nova { radius: 1 });
        let player = state.player;
        let east = goblin(&mut state, (2, 1));
        let south = goblin(&mut state, (1, 2));
        let far = goblin(&mut state, (3, 1));
        let wolf = thing("wolf")
            .at(2, 2)
            .health(10)
            .summoned_by(player)
            .make(&mut state);

        perform(&mut state, technique, (1, 1));

        assert!(health(&state, east) < 10 && health(&state, south) < 10);
        assert_eq!((health(&state, far), health(&state, wolf)), (10, 10));
        assert!(state.log.recent(10).any(|line| line == "You cleave."));
    }

    #[test]
    fn an_area_with_no_enemy_in_it_is_refused() {
        let (mut state, technique) = sweeper(Shape::Nova { radius: 1 });
        goblin(&mut state, (3, 1));
        refused(&mut state, technique, (1, 1), "No enemy in reach.");
    }

    #[test]
    fn a_wall_stops_a_sweep_as_it_stops_a_cone_spell() {
        let (mut state, technique) = sweeper(Shape::Cone);
        let behind = goblin(&mut state, (3, 1));
        let open = (state.map.clone(), behind);
        if let Some(tile) = state.map.get_mut(2, 1) {
            *tile = Tile::Wall;
        }
        refused(&mut state, technique, (3, 1), "No enemy in reach.");

        state.map = open.0;
        perform(&mut state, technique, (3, 1));
        assert!(health(&state, open.1) < 10);
    }

    #[test]
    fn an_area_preview_counts_everyone_the_blows_reach() {
        let (mut state, technique) = sweeper(Shape::Nova { radius: 1 });
        goblin(&mut state, (2, 1));
        goblin(&mut state, (2, 2));
        let shot = crate::aim::Shot::Technique(ItemRef::of(&state.world, technique).unwrap());
        let aim = crate::aim::aim(&state, shot, (1, 1)).unwrap();
        // In the room's corner, walls leave four of the nine tiles.
        assert_eq!((aim.caught, aim.problem, aim.area.len()), (2, None, 4));
        assert_eq!(aim.chances, None);
        assert_eq!(techniques(&state)[0].shape, Some(Shape::Nova { radius: 1 }));
    }
}
