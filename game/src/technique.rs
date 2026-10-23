//! Techniques (ADR 0024): special actions done with the body and what it
//! wields, such as a lunge. They share the spell's effect half (shapes,
//! aiming) but not its payment: a technique costs a turn, a recharge and
//! stamina, and its blow is an ordinary blow (`combat::blow`), so it lands,
//! grazes and trains like a bump.

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::blueprint::BlueprintId;
use std::ops::ControlFlow;

use crate::combat::{self, AttackOdds, BlowMods, Outcome, Strike};
use crate::command::Command;
use crate::cost::TICKS_PER_TURN;
use crate::entity::{
    Area, Blow, Dash, Name, Need, OffBalance, Pool, PoolRates, Position, Recharging, Rider, Shape,
    Stamina, Stats, StyleBonus, Technique,
};
use crate::entity::{Combo, ComboDisplay, Loose, PartKind, Performed};
use crate::event::EventRegistry;
use crate::grammar::Subject;
use crate::holding;
use crate::map::Tile;
use crate::prio;
use crate::ranged;
use crate::side;
use crate::skill::{self, Domain, SkillBook, SkillId};
use crate::spell;
use crate::state::GameState;
use crate::time::Tick;
use crate::view::ItemRef;

/// A `Technique` as written in RON and saves: skills and effects by name.
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
    #[serde(default)]
    pub riders: Vec<RiderDef>,
    #[serde(default)]
    pub uses: Option<Vec<String>>,
    #[serde(default)]
    pub bonuses: Vec<StyleBonusDef>,
    #[serde(default)]
    pub combo: Option<ComboDef>,
}

/// A `Combo` as written in RON and saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboDef {
    pub group: String,
    pub step: u32,
    pub within: u64,
    #[serde(default)]
    pub accuracy: i32,
    #[serde(default)]
    pub damage_pct: i32,
    #[serde(default)]
    pub riders: Vec<RiderDef>,
    #[serde(default)]
    pub xp_pct: u32,
    #[serde(default)]
    pub display: ComboDisplay,
}

/// A `StyleBonus` as written in RON and saves: the style by its skill's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleBonusDef {
    pub style: String,
    #[serde(default)]
    pub from_level: u32,
    #[serde(default)]
    pub accuracy: i32,
    #[serde(default)]
    pub damage_pct: i32,
    #[serde(default)]
    pub recharge_pct: i32,
    #[serde(default)]
    pub stamina_pct: i32,
    #[serde(default)]
    pub riders: Vec<RiderDef>,
}

fn one() -> i32 {
    1
}

/// A `Need` as written in RON and saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeedDef {
    Wielding(Vec<String>),
    Skill(String, u32),
    /// A style, by its skill's name.
    Following(String),
    Continuing,
    /// A body part of this kind.
    Part(PartKind),
}

/// A `Rider` as written in RON and saves: an effect by blueprint name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiderDef {
    Shove { tiles: i32 },
    Effect(String),
}

/// A name a `TechniqueDef` gives that the content lacks, or an id saved
/// content no longer has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Missing {
    Skill(String),
    Blueprint(String),
    SkillId(SkillId),
    BlueprintId(BlueprintId),
}

/// Names to ids, while a `TechniqueDef` resolves; ids to names, while a
/// `Technique` is saved.
pub(crate) struct Names<'a> {
    pub skills: &'a SkillBook,
    pub blueprint: &'a dyn Fn(&str) -> Option<BlueprintId>,
    pub blueprint_name: &'a dyn Fn(BlueprintId) -> Option<String>,
}

impl Names<'_> {
    fn skill(&self, name: &str) -> Result<SkillId, Missing> {
        self.skills
            .id(name)
            .ok_or_else(|| Missing::Skill(name.to_string()))
    }

    fn skills(&self, names: &[String]) -> Result<Vec<SkillId>, Missing> {
        let mut ids = names
            .iter()
            .map(|name| self.skill(name))
            .collect::<Result<Vec<_>, _>>()?;
        ids.sort();
        Ok(ids)
    }

    fn skill_name(&self, id: SkillId) -> Result<String, Missing> {
        self.skills
            .get(id)
            .map(|def| def.name.clone())
            .ok_or(Missing::SkillId(id))
    }

    fn skill_names(&self, ids: &[SkillId]) -> Result<Vec<String>, Missing> {
        ids.iter().map(|&id| self.skill_name(id)).collect()
    }

    fn rider(&self, rider: &RiderDef) -> Result<Rider, Missing> {
        Ok(match rider {
            &RiderDef::Shove { tiles } => Rider::Shove { tiles },
            RiderDef::Effect(name) => Rider::Effect(
                (self.blueprint)(name).ok_or_else(|| Missing::Blueprint(name.clone()))?,
            ),
        })
    }

    fn need(&self, need: &NeedDef) -> Result<Need, Missing> {
        Ok(match need {
            NeedDef::Wielding(skills) => Need::Wielding(self.skills(skills)?),
            NeedDef::Skill(skill, level) => Need::Skill(self.skill(skill)?, *level),
            NeedDef::Following(style) => Need::Following(self.skill(style)?),
            NeedDef::Continuing => Need::Continuing,
            &NeedDef::Part(kind) => Need::Part(kind),
        })
    }

    fn need_def(&self, need: &Need) -> Result<NeedDef, Missing> {
        Ok(match need {
            Need::Wielding(skills) => NeedDef::Wielding(self.skill_names(skills)?),
            Need::Skill(skill, level) => NeedDef::Skill(self.skill_name(*skill)?, *level),
            Need::Following(style) => NeedDef::Following(self.skill_name(*style)?),
            Need::Continuing => NeedDef::Continuing,
            &Need::Part(kind) => NeedDef::Part(kind),
        })
    }

    fn bonus(&self, bonus: &StyleBonusDef) -> Result<StyleBonus, Missing> {
        Ok(StyleBonus {
            style: self.skill(&bonus.style)?,
            from_level: bonus.from_level,
            accuracy: bonus.accuracy,
            damage_pct: bonus.damage_pct,
            recharge_pct: bonus.recharge_pct,
            stamina_pct: bonus.stamina_pct,
            riders: bonus
                .riders
                .iter()
                .map(|rider| self.rider(rider))
                .collect::<Result<_, _>>()?,
        })
    }

    fn bonus_def(&self, bonus: &StyleBonus) -> Result<StyleBonusDef, Missing> {
        Ok(StyleBonusDef {
            style: self.skill_name(bonus.style)?,
            from_level: bonus.from_level,
            accuracy: bonus.accuracy,
            damage_pct: bonus.damage_pct,
            recharge_pct: bonus.recharge_pct,
            stamina_pct: bonus.stamina_pct,
            riders: bonus
                .riders
                .iter()
                .map(|rider| self.rider_def(rider))
                .collect::<Result<_, _>>()?,
        })
    }

    fn combo(&self, combo: &ComboDef) -> Result<Combo, Missing> {
        Ok(Combo {
            group: combo.group.clone(),
            step: combo.step,
            within: combo.within,
            accuracy: combo.accuracy,
            damage_pct: combo.damage_pct,
            riders: combo
                .riders
                .iter()
                .map(|rider| self.rider(rider))
                .collect::<Result<_, _>>()?,
            xp_pct: combo.xp_pct,
            display: combo.display,
        })
    }

    fn combo_def(&self, combo: &Combo) -> Result<ComboDef, Missing> {
        Ok(ComboDef {
            group: combo.group.clone(),
            step: combo.step,
            within: combo.within,
            accuracy: combo.accuracy,
            damage_pct: combo.damage_pct,
            riders: combo
                .riders
                .iter()
                .map(|rider| self.rider_def(rider))
                .collect::<Result<_, _>>()?,
            xp_pct: combo.xp_pct,
            display: combo.display,
        })
    }

    fn rider_def(&self, rider: &Rider) -> Result<RiderDef, Missing> {
        Ok(match *rider {
            Rider::Shove { tiles } => RiderDef::Shove { tiles },
            Rider::Effect(id) => {
                RiderDef::Effect((self.blueprint_name)(id).ok_or(Missing::BlueprintId(id))?)
            }
        })
    }
}

impl TechniqueDef {
    /// The skills it names, for checking at load.
    pub(crate) fn names(&self) -> impl Iterator<Item = &String> {
        let needs = self.requires.iter().flat_map(|need| match need {
            NeedDef::Wielding(skills) => skills.iter().collect::<Vec<_>>(),
            NeedDef::Skill(skill, _) | NeedDef::Following(skill) => vec![skill],
            NeedDef::Continuing | NeedDef::Part(_) => Vec::new(),
        });
        let styles = self.bonuses.iter().map(|bonus| &bonus.style);
        needs.chain(self.uses.iter().flatten()).chain(styles)
    }

    /// The effect blueprints it applies, for checking at load.
    pub(crate) fn effects(&self) -> impl Iterator<Item = &String> {
        let bonuses = self.bonuses.iter().flat_map(|bonus| &bonus.riders);
        let combo = self.combo.iter().flat_map(|combo| &combo.riders);
        self.riders
            .iter()
            .chain(bonuses)
            .chain(combo)
            .filter_map(|rider| match rider {
                RiderDef::Effect(name) => Some(name),
                RiderDef::Shove { .. } => None,
            })
    }

    /// With every name resolved, or the first one `names` lacks.
    pub(crate) fn resolve(&self, names: &Names<'_>) -> Result<Technique, Missing> {
        let requires = self
            .requires
            .iter()
            .map(|need| names.need(need))
            .collect::<Result<_, Missing>>()?;
        Ok(Technique {
            domain: self.domain,
            verb: self.verb.clone(),
            reach: self.reach,
            recharge: self.recharge,
            stamina: self.stamina,
            requires,
            riders: self
                .riders
                .iter()
                .map(|rider| names.rider(rider))
                .collect::<Result<_, _>>()?,
            uses: self
                .uses
                .as_ref()
                .map(|uses| names.skills(uses))
                .transpose()?,
            bonuses: self
                .bonuses
                .iter()
                .map(|bonus| names.bonus(bonus))
                .collect::<Result<_, _>>()?,
            combo: self
                .combo
                .as_ref()
                .map(|combo| names.combo(combo))
                .transpose()?,
        })
    }

    /// `technique` with every id named, or the first one `names` lacks.
    pub(crate) fn of(technique: &Technique, names: &Names<'_>) -> Result<Self, Missing> {
        let requires = technique
            .requires
            .iter()
            .map(|need| names.need_def(need))
            .collect::<Result<_, Missing>>()?;
        Ok(Self {
            domain: technique.domain,
            verb: technique.verb.clone(),
            reach: technique.reach,
            recharge: technique.recharge,
            stamina: technique.stamina,
            requires,
            riders: technique
                .riders
                .iter()
                .map(|rider| names.rider_def(rider))
                .collect::<Result<_, _>>()?,
            uses: technique
                .uses
                .as_ref()
                .map(|uses| names.skill_names(uses))
                .transpose()?,
            bonuses: technique
                .bonuses
                .iter()
                .map(|bonus| names.bonus_def(bonus))
                .collect::<Result<_, _>>()?,
            combo: technique
                .combo
                .as_ref()
                .map(|combo| names.combo_def(combo))
                .transpose()?,
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

/// Whether `creature` knows a technique called `name` (its bare noun).
pub(crate) fn knows(world: &World, creature: Entity, name: &str) -> bool {
    known(world, creature).into_iter().any(|technique| {
        world
            .get::<&Name>(technique)
            .is_ok_and(|known| known.0 == name)
    })
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
    /// What each full hit does besides its damage.
    pub riders: Vec<Rider>,
    pub stamina: u32,
    pub recharge: u64,
    pub verb: String,
    /// The styles (by skill) whose bonuses it has now.
    pub styled: Vec<SkillId>,
    /// What performing it now records, for a step of a form.
    pub performed: Option<Performed>,
    /// It shoots (`Loose`) rather than strikes: `strike` is the shot's, from
    /// where the performer stands now.
    pub loose: bool,
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
    let bonuses: Vec<&StyleBonus> = data
        .bonuses
        .iter()
        .filter(|bonus| {
            following(world, actor, bonus.style)
                && skill::level(skill::xp(world, actor, bonus.style)) >= bonus.from_level
        })
        .collect();
    let sum = |part: fn(&StyleBonus) -> i32| bonuses.iter().map(|&bonus| part(bonus)).sum::<i32>();
    // How long the run is that this would continue, if it continues one.
    let run = data
        .combo
        .as_ref()
        .and_then(|combo| continues(world, actor, combo, state.now));
    let combo = data.combo.as_ref().filter(|_| run.is_some());
    let loose = world.get::<&Loose>(technique).ok().map(|loose| *loose);
    let shot = loose.unwrap_or_default();
    let mods = BlowMods {
        accuracy: blow.accuracy
            + shot.accuracy
            + sum(|bonus| bonus.accuracy)
            + combo.map_or(0, |combo| combo.accuracy),
        damage_pct: blow.damage_pct
            + shot.damage_pct
            + sum(|bonus| bonus.damage_pct)
            + combo.map_or(0, |combo| combo.damage_pct),
        uses: data.uses.clone(),
        xp_pct: combo.map_or(0, |combo| combo.xp_pct) * run.unwrap_or(0),
        unarmed: blow.unarmed,
    };
    let book = state.blueprints.skills();
    // A shot's strike is the launcher's; why it can't shoot is a refusal.
    let fired = loose.map(|_| ranged::ready_fire(state, actor));
    let (strike, used, unfired) = match fired {
        Some(Ok(fired)) => {
            let used = skill::missile(world, book, fired.launch, &fired.strike);
            let mut strike = fired.strike;
            strike.accuracy += mods.accuracy;
            (strike, used, None)
        }
        Some(Err(reason)) => {
            let Some((strike, used)) = combat::blow_strike(world, book, actor, &mods) else {
                return Err(reason);
            };
            (strike, used, Some(reason))
        }
        None => {
            let Some((strike, used)) = combat::blow_strike(world, book, actor, &mods) else {
                return Err("You have nothing to strike with.".to_string());
            };
            (strike, used, None)
        }
    };
    let mut riders = data.riders.clone();
    riders.extend(
        bonuses
            .iter()
            .flat_map(|bonus| bonus.riders.iter().copied()),
    );
    riders.extend(combo.iter().flat_map(|combo| combo.riders.iter().copied()));
    let ready = Ready {
        reach: data.reach,
        strike,
        mods,
        dash: world.get::<&Dash>(technique).ok().map(|dash| *dash),
        area: world.get::<&Area>(technique).ok().map(|area| *area),
        riders,
        stamina: scaled(data.stamina.into(), sum(|bonus| bonus.stamina_pct))
            .try_into()
            .unwrap_or(u32::MAX),
        recharge: scaled(data.recharge, sum(|bonus| bonus.recharge_pct)),
        verb: data.verb.clone(),
        styled: bonuses.iter().map(|bonus| bonus.style).collect(),
        performed: data.combo.as_ref().map(|combo| Performed {
            group: combo.group.clone(),
            step: combo.step,
            at: state.now,
            run: run.unwrap_or(0) + 1,
        }),
        loose: loose.is_some(),
    };
    let blocked = unmet(
        state,
        actor,
        technique,
        &data,
        &used,
        ready.stamina,
        run.is_some(),
    )
    .or(unfired);
    Ok((ready, blocked))
}

/// `base` changed by `pct` percent; never below 0.
fn scaled(base: u64, pct: i32) -> u64 {
    let pct = u64::try_from(100 + i64::from(pct)).unwrap_or(0);
    base * pct / 100
}

/// How many steps the run `combo` would continue has had, if `actor`'s last
/// step of its form was the one before it, within its time.
fn continues(world: &World, actor: Entity, combo: &Combo, now: Tick) -> Option<u32> {
    let last = world.get::<&Performed>(actor).ok()?;
    (last.group == combo.group && last.step + 1 == combo.step && now.since(last.at) <= combo.within)
        .then_some(last.run)
}

/// Whether `actor` follows the style whose skill is `style`.
fn following(world: &World, actor: Entity, style: SkillId) -> bool {
    crate::style::followed(world, actor)
        .into_iter()
        .any(|thing| {
            world
                .get::<&crate::entity::Style>(thing)
                .is_ok_and(|followed| followed.skill == style)
        })
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
    stamina_cost: u32,
    continuing: bool,
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
            Need::Continuing => {
                if !continuing {
                    return Some(format!("Your {name} only follows the step before it."));
                }
            }
            &Need::Part(kind) => {
                let has = world
                    .get::<&crate::entity::Body>(actor)
                    .is_ok_and(|body| body.0.iter().any(|part| part.kind == kind));
                if !has {
                    return Some(format!("Your {name} needs {}.", kind.noun()));
                }
            }
            &Need::Following(style) => {
                if !following(world, actor, style) {
                    return Some(format!(
                        "Your {name} needs you to follow {}.",
                        named(&[style])
                    ));
                }
            }
        }
    }
    if stamina_cost > 0 {
        let Some((pool, rates)) = stamina(world, actor) else {
            return Some("You have no stamina for that.".to_string());
        };
        if pool.current(rates, state.now) < stamina_cost {
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
    if ready.loose {
        return loosed(state, actor, ready, from, at);
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

/// A shot's footprint: aimed within reach and view, stepping back first if it
/// dashes away (open, empty ground behind), then flying as a plain shot does
/// from there (`ranged::flight`): whoever is first in line must be an enemy.
fn loosed(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    from: (i32, i32),
    at: (i32, i32),
) -> Result<Footprint, String> {
    if from == at {
        return Err("You can't aim at yourself.".to_string());
    }
    if ranged::distance(from, at) > ready.reach {
        return Err("That is out of reach.".to_string());
    }
    if !in_view(state, actor, at) {
        return Err("You can't see that spot.".to_string());
    }
    let mut stand = from;
    if let Some(dash) = ready.dash.filter(|dash| dash.away) {
        let back = ((from.0 - at.0).signum(), (from.1 - at.1).signum());
        for _ in 0..dash.tiles {
            let next = (stand.0 + back.0, stand.1 + back.1);
            if !Tile::is_open(state.map.get(next.0, next.1)) || spell::occupied(&state.world, next)
            {
                return Err("There is no room behind you.".to_string());
            }
            stand = next;
        }
    }
    let shot = fired(state, actor, ready, stand)?;
    let flight = ranged::flight(state, actor, stand, at, shot.range);
    let target = flight
        .hit
        .filter(|&target| side::enemies(&state.world, actor, target))
        .ok_or_else(|| "There is no enemy in line.".to_string())?;
    Ok(Footprint {
        targets: vec![target],
        stand,
        area: Vec::new(),
    })
}

/// The shot a `Loose` technique fires from `stand`: the launcher's next
/// arrow (`ranged::ready_fire_from`, so point blank is judged there), surer
/// and harder by its mods. Its damage is scaled here, since the shot rolls
/// it itself; `ready.strike` stays unscaled, as a blow's does.
pub(crate) fn fired(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    stand: (i32, i32),
) -> Result<ranged::Ready, String> {
    let mut shot = ranged::ready_fire_from(state, actor, Some(stand))?;
    let (low, high) = shot.strike.range();
    shot.strike.accuracy += ready.mods.accuracy;
    shot.strike.min = ready.mods.scale(low);
    shot.strike.max = ready.mods.scale(high);
    shot.strike.bonus = 0;
    Ok(shot)
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
    if let Some(performed) = ready.performed.clone() {
        let _ = state.world.insert_one(actor, performed);
    }
    if let Ok(mut pos) = state.world.get::<&mut Position>(actor) {
        (pos.x, pos.y) = footprint.stand;
    }
    if actor == state.player && ranged::position(state, actor) != Some(at) {
        state.commands.push_back(Command::FindItems(actor));
    }
    if ready.loose {
        // The shot logs, lands, breaks and trains as a plain shot does.
        if let Ok(shot) = fired(state, actor, &ready, footprint.stand) {
            ranged::launch(state, actor, shot, at);
        }
        return;
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
        // Riders follow a full hit's damage in the queue, so a target the
        // blow kills is neither shoved nor affected.
        if combat::blow(state, actor, target, &ready.mods) == Some(Outcome::Hit) {
            for rider in &ready.riders {
                state.commands.push_back(Command::Ride {
                    actor,
                    target,
                    rider: *rider,
                });
            }
        }
    }
}

/// Applies `Command::Ride`: `rider` on `target`, unless the hit killed it (its
/// `Die` is queued after the rider, so health is what tells).
pub(crate) fn ride(state: &mut GameState, actor: Entity, target: Entity, rider: Rider) {
    let alive = state
        .world
        .get::<&crate::entity::Health>(target)
        .is_ok_and(|health| health.current > 0);
    if !alive {
        return;
    }
    match rider {
        Rider::Shove { tiles } => shove(state, actor, target, tiles),
        Rider::Effect(blueprint) => crate::effect::apply_effect(state, target, blueprint),
    }
}

/// `target` moves straight away from `pusher`, a tile at a time, up to
/// `tiles`, until a wall or a creature stops it. No RNG and no damage from
/// what it meets.
fn shove(state: &mut GameState, pusher: Entity, target: Entity, tiles: i32) {
    let (Some(from), Some(start)) = (
        ranged::position(state, pusher),
        ranged::position(state, target),
    ) else {
        return;
    };
    let step = ((start.0 - from.0).signum(), (start.1 - from.1).signum());
    let mut at = start;
    for _ in 0..tiles {
        let next = (at.0 + step.0, at.1 + step.1);
        if step == (0, 0)
            || !Tile::is_open(state.map.get(next.0, next.1))
            || spell::occupied(&state.world, next)
        {
            break;
        }
        at = next;
    }
    let shoved = Subject::of(&state.world, target);
    if at == start {
        state.log.push(shoved.sentence("hold", "firm"));
        return;
    }
    if let Ok(mut pos) = state.world.get::<&mut Position>(target) {
        (pos.x, pos.y) = at;
    }
    state.log.push(shoved.sentence("be", "shoved back"));
    if target == state.player {
        state.commands.push_back(Command::FindItems(target));
    }
}

pub(crate) fn register(registry: &mut EventRegistry) {
    // A reeling defender is easier to hit and can't dodge perfectly.
    registry.on::<AttackOdds, OffBalance>(prio::OFF_BALANCE, |_, reeling, mut ev| {
        if ev.asking == ev.defender {
            ev.hit_bp = ev.hit_bp.saturating_add(reeling.hit_bp);
            ev.perfect_bp = 0;
        }
        ControlFlow::Continue(ev)
    });
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
    /// The styles whose bonuses it has now, by name.
    pub styled: Vec<String>,
    /// For a step of a form: which, of how many the player knows; `None`
    /// unless they know two steps or more of it.
    pub step: Option<(u32, u32)>,
    /// Why it can't be performed now, if it can't.
    pub blocked: Option<String>,
}

/// What the player knows, in the order they learned it.
pub fn techniques(state: &GameState) -> Vec<TechniqueView> {
    let world = &state.world;
    let known = known(world, state.player);
    let combos: Vec<Option<Combo>> = known
        .iter()
        .map(|&technique| {
            world
                .get::<&Technique>(technique)
                .ok()
                .and_then(|data| data.combo.clone())
        })
        .collect();
    known
        .iter()
        .zip(&combos)
        .filter(|(_, combo)| {
            combo
                .as_ref()
                .is_none_or(|combo| shown(state, combo, &combos))
        })
        .filter_map(|(&technique, combo)| {
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
                styled: ready
                    .styled
                    .iter()
                    .filter_map(|&style| state.blueprints.skills().get(style))
                    .map(|def| def.name.clone())
                    .collect(),
                // A lone known step of a form (the trip, before the rest of
                // the bow dance) is no form yet: no step is shown.
                step: combo.as_ref().and_then(|combo| {
                    let group: Vec<u32> = combos
                        .iter()
                        .flatten()
                        .filter(|other| other.group == combo.group)
                        .map(|other| other.step)
                        .collect();
                    let steps = group.iter().copied().max().unwrap_or(combo.step);
                    (group.len() > 1).then_some((combo.step, steps))
                }),
                blocked,
            })
        })
        .collect()
}

/// Whether the list shows this step of a form: every step shows, unless its
/// form shows as `Turns`, in which case only the step that would come next
/// does: the one after the player's last while the run lasts, else the
/// first. `known` holds the combos of every technique the player knows.
fn shown(state: &GameState, combo: &Combo, known: &[Option<Combo>]) -> bool {
    let steps: Vec<&Combo> = known
        .iter()
        .flatten()
        .filter(|other| other.group == combo.group)
        .collect();
    let Some(first) = steps.iter().min_by_key(|other| other.step) else {
        return true;
    };
    if first.display == ComboDisplay::Steps {
        return true;
    }
    let next = steps
        .iter()
        .find(|&&other| continues(&state.world, state.player, other, state.now).is_some())
        .map_or(first.step, |other| other.step);
    combo.step == next
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
            riders: Vec::new(),
            uses: None,
            bonuses: Vec::new(),
            combo: None,
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
            .with(Dash {
                tiles: 1,
                away: false,
            })
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
                unarmed: false,
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

    /// The player knowing a one-tile technique with `riders`, wielding a
    /// thrusting blade, every blow landing.
    fn rider(riders: Vec<Rider>) -> (GameState, Entity) {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        sure_hits(&mut state, player);
        wield(&mut state, &[DamageType::Piercing]);
        let technique = thing("shove")
            .with(Technique {
                verb: "shove".to_string(),
                reach: 1,
                riders,
                ..lunge(0, Vec::new())
            })
            .attached_to(player)
            .make(&mut state);
        (state, technique)
    }

    fn tough_goblin(state: &mut GameState, at: (i32, i32)) -> Entity {
        thing("goblin")
            .at(at.0, at.1)
            .health(100)
            .hostile()
            .make(state)
    }

    #[test]
    fn a_shove_drives_the_target_straight_back() {
        let (mut state, technique) = rider(vec![Rider::Shove { tiles: 2 }]);
        let goblin = tough_goblin(&mut state, (2, 2));
        perform(&mut state, technique, (2, 2));
        assert_eq!(at(&state, goblin), (4, 4));
        assert!(
            state
                .log
                .recent(10)
                .any(|line| line == "The goblin is shoved back.")
        );
    }

    #[test]
    fn a_shove_stops_at_what_is_in_the_way() {
        let (mut state, technique) = rider(vec![Rider::Shove { tiles: 3 }]);
        let goblin = tough_goblin(&mut state, (2, 1));
        thing("rubble")
            .at(4, 1)
            .with(crate::entity::BlocksMovement)
            .make(&mut state);
        perform(&mut state, technique, (2, 1));
        assert_eq!(at(&state, goblin), (3, 1));

        let (mut state, technique) = rider(vec![Rider::Shove { tiles: 3 }]);
        let goblin = tough_goblin(&mut state, (2, 1));
        tough_goblin(&mut state, (3, 1));
        perform(&mut state, technique, (2, 1));
        assert_eq!(at(&state, goblin), (2, 1));
        assert_eq!(last_log(&state), "The goblin holds firm.");
    }

    #[test]
    fn a_blow_that_kills_shoves_no_one() {
        let (mut state, technique) = rider(vec![Rider::Shove { tiles: 2 }]);
        let goblin = thing("goblin")
            .at(2, 1)
            .health(1)
            .hostile()
            .make(&mut state);
        perform(&mut state, technique, (2, 1));
        let gone =
            !state.world.contains(goblin) || state.world.satisfies::<&crate::entity::Dead>(goblin);
        assert!(gone);
        assert!(!state.log.recent(10).any(|line| line.contains("shoved")));
    }

    #[test]
    fn an_effect_rider_applies_its_effect_on_a_hit() {
        let bleeding = bare_state().blueprints.id("Bleeding").unwrap();
        let (mut state, technique) = rider(vec![Rider::Effect(bleeding)]);
        let goblin = tough_goblin(&mut state, (2, 1));
        perform(&mut state, technique, (2, 1));
        let effects = holding::attached(&state.world, goblin);
        assert!(
            effects
                .iter()
                .any(|&effect| state.world.satisfies::<&crate::entity::Bleeding>(effect))
        );
    }

    #[test]
    fn a_trip_leaves_the_target_reeling_until_its_time_runs_out() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = crate::blueprint::Blueprints::load(&data).unwrap();
        let mut state = crate::state::tests::bare_state_with(blueprints);
        see_everything(&mut state);
        let player = state.player;
        let trip = known(&state.world, player)
            .into_iter()
            .find(|&t| name(&state.world, t) == "trip")
            .unwrap();
        let goblin = tough_goblin(&mut state, (2, 1));
        let odds = |state: &GameState| {
            let strike = combat::strike(&state.world, state.blueprints.skills(), player).unwrap();
            combat::odds(state, &strike, player, goblin)
        };
        let before = odds(&state);

        sure_hits(&mut state, player);
        perform(&mut state, trip, (2, 1));
        let _ = state.world.remove_one::<crate::testkit::SureHits>(player);
        assert!(
            state
                .log
                .recent(10)
                .any(|line| line == "The goblin starts reeling.")
        );
        let reeling = odds(&state);
        assert_eq!(reeling.hit_bp, before.hit_bp + 2_000);
        assert_eq!(reeling.perfect_bp, 0);

        for _ in 0..3 {
            take_turn(&mut state, PlayerAction::Wait);
        }
        assert_eq!(odds(&state), before);
    }

    #[test]
    fn a_technique_that_names_its_skills_is_levelled_and_trained_by_them() {
        let book = crate::skill::tests::book();
        let unarmed = book.id("Unarmed").unwrap();
        let (mut state, technique) = rider(Vec::new());
        let player = state.player;
        let _ = state.world.insert_one(player, Learner);
        let mut skills = crate::entity::Skills::default();
        skills.set(unarmed, skill::xp_for(8));
        let _ = state.world.insert_one(player, skills);
        let _ = state.world.get::<&mut Technique>(technique).map(|mut t| {
            t.uses = Some(vec![unarmed]);
        });
        goblin(&mut state, (2, 1));

        let (ready, _) = inspect(&state, player, technique).unwrap();
        assert_eq!(ready.strike.level, 8);
        perform(&mut state, technique, (2, 1));

        let pierce = pierce(&state);
        assert_eq!(skill::xp(&state.world, player, pierce), 0);
        assert!(skill::xp(&state.world, player, unarmed) > skill::xp_for(8));
    }

    /// Gives the player a style whose skill is Dagger, at `level`, followed
    /// or only known.
    fn style(state: &mut GameState, level: u32, followed: bool) -> Entity {
        let player = state.player;
        let dagger = state.blueprints.skills().id("Dagger").unwrap();
        let mut skills = crate::entity::Skills::default();
        skills.set(dagger, skill::xp_for(level));
        let _ = state.world.insert_one(player, skills);
        let style = thing("knifework").with(crate::entity::Style {
            domains: vec![Domain::Martial],
            skill: dagger,
            follows: Vec::new(),
            wielding: Vec::new(),
            principles: Vec::new(),
            invents: Vec::new(),
        });
        if followed {
            style.attached_to(player).make(state)
        } else {
            style.carried_by(player).make(state)
        }
    }

    fn with_bonus(state: &mut GameState, technique: Entity, from_level: u32) {
        let dagger = state.blueprints.skills().id("Dagger").unwrap();
        let bleeding = state.blueprints.id("Bleeding").unwrap();
        let _ = state.world.get::<&mut Technique>(technique).map(|mut t| {
            t.bonuses.push(StyleBonus {
                style: dagger,
                from_level,
                accuracy: 3,
                damage_pct: 100,
                recharge_pct: -40,
                stamina_pct: 0,
                riders: vec![Rider::Effect(bleeding)],
            });
        });
    }

    #[test]
    fn a_style_bonus_applies_only_while_following_from_its_level() {
        let (mut state, technique) = lunger();
        let player = state.player;
        with_bonus(&mut state, technique, 3);
        let (plain, _) = inspect(&state, player, technique).unwrap();

        let known = style(&mut state, 5, false);
        assert_eq!(
            inspect(&state, player, technique).unwrap().0.recharge,
            plain.recharge
        );

        let _ = state.world.despawn(known);
        style(&mut state, 2, true);
        assert_eq!(
            inspect(&state, player, technique).unwrap().0.recharge,
            plain.recharge
        );

        let (mut state, technique) = lunger();
        with_bonus(&mut state, technique, 3);
        style(&mut state, 3, true);
        let (styled, _) = inspect(&state, player, technique).unwrap();
        assert_eq!(styled.recharge, 300);
        assert_eq!(styled.strike.accuracy, plain.strike.accuracy + 3);
        assert_eq!((styled.mods.damage_pct, styled.riders.len()), (100, 1));
        assert_eq!(techniques(&state)[0].styled, ["Dagger"]);
    }

    #[test]
    fn the_preview_shows_a_style_bonus_as_the_blow_lands_it() {
        let (mut state, technique) = lunger();
        let player = state.player;
        with_bonus(&mut state, technique, 0);
        // Only the numbers: a bleeding rider would tick into the damage.
        let _ = state.world.get::<&mut Technique>(technique).map(|mut t| {
            t.bonuses[0].riders.clear();
        });
        style(&mut state, 1, true);
        sure_hits(&mut state, player);
        let goblin = tough_goblin(&mut state, (3, 1));
        let shot = crate::aim::Shot::Technique(ItemRef::of(&state.world, technique).unwrap());
        let aim = crate::aim::aim(&state, shot, (3, 1)).unwrap();

        perform(&mut state, technique, (3, 1));

        let dealt = 100 - health(&state, goblin);
        assert!((aim.damage.0..=aim.damage.1).contains(&dealt));
        assert_eq!(aim.damage, (4, 4));
    }

    #[test]
    fn a_technique_of_a_style_is_refused_once_the_style_is_set_aside() {
        let (mut state, technique) = lunger();
        let dagger = state.blueprints.skills().id("Dagger").unwrap();
        let _ = state.world.get::<&mut Technique>(technique).map(|mut t| {
            t.requires.push(Need::Following(dagger));
        });
        goblin(&mut state, (3, 1));
        refused(
            &mut state,
            technique,
            (3, 1),
            "Your lunge needs you to follow Dagger.",
        );
        style(&mut state, 0, true);
        perform(&mut state, technique, (3, 1));
        assert_eq!(at(&state, state.player), (2, 1));
    }

    /// The player, holding a blade, knowing the three steps of a form shown
    /// `display`; steps 2 and 3 only follow the one before.
    fn form(display: ComboDisplay) -> (GameState, [Entity; 3]) {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        sure_hits(&mut state, player);
        wield(&mut state, &[DamageType::Piercing]);
        let steps = [1, 2, 3].map(|step| {
            let requires = if step == 1 {
                Vec::new()
            } else {
                vec![Need::Continuing]
            };
            thing(&format!("step {step}"))
                .with(Technique {
                    verb: "cut".to_string(),
                    reach: 1,
                    recharge: 0,
                    combo: Some(Combo {
                        group: "form".to_string(),
                        step,
                        within: 200,
                        accuracy: 1,
                        damage_pct: 0,
                        riders: Vec::new(),
                        xp_pct: 50,
                        display,
                    }),
                    ..lunge(0, requires)
                })
                .attached_to(player)
                .make(&mut state)
        });
        (state, steps)
    }

    #[test]
    fn the_next_step_of_a_form_is_boosted_only_right_after_the_one_before() {
        let (mut state, [first, second, third]) = form(ComboDisplay::Steps);
        let player = state.player;
        tough_goblin(&mut state, (2, 1));
        refused(
            &mut state,
            second,
            (2, 1),
            "Your step 2 only follows the step before it.",
        );
        let plain = inspect(&state, player, first).unwrap().0.strike.accuracy;

        perform(&mut state, first, (2, 1));
        let (ready, _) = inspect(&state, player, second).unwrap();
        assert_eq!(ready.strike.accuracy, plain + 1);
        assert_eq!(ready.mods.xp_pct, 50);
        refused(
            &mut state,
            third,
            (2, 1),
            "Your step 3 only follows the step before it.",
        );

        perform(&mut state, second, (2, 1));
        assert_eq!(inspect(&state, player, third).unwrap().0.mods.xp_pct, 100);
        perform(&mut state, third, (2, 1));
        let performed = state.world.get::<&Performed>(player).unwrap().clone();
        assert_eq!((performed.step, performed.run), (3, 3));
    }

    #[test]
    fn a_missed_window_ends_the_run() {
        let (mut state, [first, second, _]) = form(ComboDisplay::Steps);
        tough_goblin(&mut state, (2, 1));
        perform(&mut state, first, (2, 1));
        take_turn(&mut state, PlayerAction::Wait);
        take_turn(&mut state, PlayerAction::Wait);
        refused(
            &mut state,
            second,
            (2, 1),
            "Your step 2 only follows the step before it.",
        );

        perform(&mut state, first, (2, 1));
        let player = state.player;
        assert_eq!(state.world.get::<&Performed>(player).unwrap().run, 1);
    }

    #[test]
    fn a_form_shown_as_turns_is_one_row_that_advances() {
        let (mut state, [first, second, _]) = form(ComboDisplay::Turns);
        let names = |state: &GameState| {
            techniques(state)
                .into_iter()
                .map(|view| (view.name, view.step))
                .collect::<Vec<_>>()
        };
        tough_goblin(&mut state, (2, 1));
        assert_eq!(names(&state), [("step 1".to_string(), Some((1, 3)))]);
        perform(&mut state, first, (2, 1));
        assert_eq!(names(&state), [("step 2".to_string(), Some((2, 3)))]);
        perform(&mut state, second, (2, 1));
        assert_eq!(names(&state)[0].0, "step 3");
        take_turn(&mut state, PlayerAction::Wait);
        take_turn(&mut state, PlayerAction::Wait);
        assert_eq!(names(&state)[0].0, "step 1");
    }

    #[test]
    fn the_same_inputs_run_a_form_the_same_way() {
        let run = || {
            let (mut state, steps) = form(ComboDisplay::Steps);
            let player = state.player;
            let _ = state.world.remove_one::<crate::testkit::SureHits>(player);
            let goblin = tough_goblin(&mut state, (2, 1));
            for step in steps {
                perform(&mut state, step, (2, 1));
            }
            (health(&state, goblin), state.rngs.combat.clone())
        };
        assert_eq!(run(), run());
    }

    /// A hostile at `at` that knows `technique` (with `area` if given),
    /// wields a thrusting blade and performs techniques.
    fn fighter(
        state: &mut GameState,
        at: (i32, i32),
        technique: Technique,
        area: Option<Area>,
    ) -> (Entity, Entity) {
        let fighter = thing("duelist")
            .at(at.0, at.1)
            .health(10)
            .hostile()
            .with(crate::entity::TechniqueAi)
            .with(test_body())
            .make(state);
        thing("blade")
            .item()
            .with(Weapon {
                min: 2,
                max: 2,
                stat: Stat::Agility,
                types: DamageTypes::of(&[DamageType::Piercing]),
            })
            .equipped_on(fighter, 2)
            .make(state);
        let mut known = thing("lunge").with(technique).with(Dash {
            tiles: 1,
            away: false,
        });
        if let Some(area) = area {
            known = known.with(area);
        }
        let known = known.attached_to(fighter).make(state);
        (fighter, known)
    }

    fn test_body() -> crate::entity::Body {
        let crate::blueprint::ComponentDef::Body(parts) = crate::state::tests::test_body() else {
            unreachable!("test_body is a body");
        };
        crate::entity::Body(parts)
    }

    #[test]
    fn a_creature_lunges_from_two_tiles_and_bumps_while_it_recharges() {
        let mut state = bare_state();
        see_everything(&mut state);
        let (duelist, known) = fighter(&mut state, (3, 1), lunge(0, Vec::new()), None);
        assert!(matches!(
            crate::ai::decide(&state, duelist),
            Command::Perform { technique, at: (1, 1), .. } if technique == known
        ));
        spell::recharge(&mut state, known, 500);
        assert!(matches!(
            crate::ai::decide(&state, duelist),
            Command::TryMove(..)
        ));
    }

    #[test]
    fn a_creature_never_sweeps_its_own_side() {
        let mut state = bare_state();
        see_everything(&mut state);
        let all = Area {
            shape: Shape::Nova { radius: 1 },
            hits: crate::entity::Hits::All,
            falloff: 0,
        };
        let (duelist, _) = fighter(&mut state, (2, 1), lunge(0, Vec::new()), Some(all));
        thing("goblin")
            .at(3, 1)
            .health(5)
            .hostile()
            .make(&mut state);
        assert!(matches!(
            crate::ai::decide(&state, duelist),
            Command::TryMove(..)
        ));
    }

    /// A bare room with the real content: the real player, who learns.
    fn real() -> GameState {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = crate::blueprint::Blueprints::load(&data).unwrap();
        let mut state = crate::state::tests::bare_state_with(blueprints);
        see_everything(&mut state);
        state
    }

    fn treatise(state: &mut GameState) -> ItemRef {
        let player = state.player;
        let id = state.blueprints.id("LungeTreatise").unwrap();
        let treatise = state
            .spawn(crate::spawn::Spawn::id(id).carried_by(player))
            .unwrap();
        ItemRef::of(&state.world, treatise).unwrap()
    }

    #[test]
    fn a_manual_teaches_its_technique_once() {
        let mut state = real();
        let player = state.player;
        assert!(!knows(&state.world, player, "lunge"));
        let first = treatise(&mut state);
        take_turn(&mut state, PlayerAction::Study(first));
        assert!(knows(&state.world, player, "lunge"));
        assert_eq!(last_log(&state), "You learn the lunge.");

        let second = treatise(&mut state);
        take_turn(&mut state, PlayerAction::Study(second));
        assert_eq!(
            last_log(&state),
            "You have nothing more to learn from the treatise on the lunge."
        );
        assert_eq!(
            known(&state.world, player)
                .into_iter()
                .filter(|&t| name(&state.world, t) == "lunge")
                .count(),
            1
        );
    }

    #[test]
    fn inspiration_can_bring_a_technique() {
        let mut state = real();
        let player = state.player;
        let book = state.blueprints.skills();
        let mut skills = crate::entity::Skills::default();
        for skill in ["Melee", "Slash"] {
            skills.set(book.id(skill).unwrap(), skill::xp_for(3));
        }
        let _ = state.world.insert_one(player, skills);
        crate::discovery::inspire(&mut state, player);
        assert!(knows(&state.world, player, "cleave"));
        assert!(
            state
                .log
                .recent(5)
                .any(|line| line == "Inspiration! You discover the cleave.")
        );
    }

    /// The real player (a short bow in hand) knowing the push kick.
    fn kicker() -> (GameState, Entity) {
        let mut state = real();
        let player = state.player;
        let id = state.blueprints.id("PushKick").unwrap();
        let kick = state
            .spawn(crate::spawn::Spawn::id(id).attached_to(player))
            .unwrap();
        (state, kick)
    }

    #[test]
    fn a_kick_strikes_with_the_leg_whatever_is_in_hand() {
        let (mut state, kick) = kicker();
        let player = state.player;
        let book = state.blueprints.skills().clone();
        let bump = combat::strike(&state.world, &book, player).unwrap();
        assert!(bump.weapon.is_some(), "a bump hits with the bow");
        let (ready, blocked) = inspect(&state, player, kick).unwrap();
        assert_eq!(blocked, None);
        assert_eq!(ready.strike.weapon, None);
        assert_eq!(
            (ready.strike.min, ready.strike.max),
            (2, 4),
            "the player's own"
        );

        let goblin = thing("goblin")
            .at(2, 1)
            .health(1000)
            .hostile()
            .make(&mut state);
        crate::testkit::sure_hits(&mut state, player);
        perform(&mut state, kick, (2, 1));
        assert_eq!(at(&state, goblin), (3, 1), "kicked back a step");
        let xp = |name| skill::xp(&state.world, player, book.id(name).unwrap());
        assert!(xp("Unarmed") > 0);
        assert_eq!((xp("Bow"), xp("Short bow")), (0, 0), "the bow did nothing");
    }

    #[test]
    fn a_kick_needs_feet() {
        let (mut state, kick) = kicker();
        let player = state.player;
        goblin(&mut state, (2, 1));
        let _ = state
            .world
            .insert_one(player, crate::entity::Body(Vec::new()));
        refused(&mut state, kick, (2, 1), "Your push kick needs feet.");
    }

    /// The real player (short bow, stocked quiver) at (5, 5) knowing the
    /// parting shot, with a sturdy goblin at `goblin`.
    fn parting(goblin: (i32, i32)) -> (GameState, Entity, Entity) {
        let mut state = real();
        let player = state.player;
        let _ = state.world.insert_one(player, Position { x: 5, y: 5 });
        let id = state.blueprints.id("PartingShot").unwrap();
        let shot = state
            .spawn(crate::spawn::Spawn::id(id).attached_to(player))
            .unwrap();
        let target = thing("goblin")
            .at(goblin.0, goblin.1)
            .health(1000)
            .hostile()
            .make(&mut state);
        see_everything(&mut state);
        (state, shot, target)
    }

    fn arrows_held(state: &GameState) -> usize {
        state
            .world
            .query::<(&crate::entity::Ammo, Option<&Position>)>()
            .iter()
            .filter(|(_, position)| position.is_none())
            .count()
    }

    #[test]
    fn a_parting_shot_steps_back_and_shoots_without_point_blank() {
        let (mut state, shot, goblin) = parting((6, 5));
        let player = state.player;
        let technique = ItemRef::of(&state.world, shot).unwrap();
        let aim = crate::aim::aim(&state, crate::aim::Shot::Technique(technique), (6, 5)).unwrap();
        assert_eq!(aim.problem, None);
        assert!(!aim.point_blank, "it shoots from a step back");
        let plain = crate::aim::aim(&state, crate::aim::Shot::Fire, (6, 5)).unwrap();
        assert!(plain.point_blank);
        assert!(aim.chances.unwrap().hit_bp > plain.chances.unwrap().hit_bp);

        let before = arrows_held(&state);
        crate::testkit::sure_hits(&mut state, player);
        perform(&mut state, shot, (6, 5));
        assert_eq!(at(&state, player), (4, 5));
        assert_eq!(arrows_held(&state), before - 1);
        assert!(health(&state, goblin) < 1000);
        let book = state.blueprints.skills();
        let xp = |name| skill::xp(&state.world, player, book.id(name).unwrap());
        assert!(xp("Archery") > 0 && xp("Bow") > 0, "it trains as a shot");
        assert_eq!(xp("Unarmed"), 0);
    }

    #[test]
    fn a_parting_shot_needs_room_behind_arrows_and_reach() {
        let (mut state, shot, _) = parting((6, 5));
        thing("crate")
            .at(4, 5)
            .with(crate::entity::BlocksMovement)
            .make(&mut state);
        let refuse = |state: &mut GameState, at, why: &str| {
            let now = state.now;
            perform(state, shot, at);
            assert_eq!(last_log(state), why);
            assert_eq!(state.now, now);
        };
        refuse(&mut state, (6, 5), "There is no room behind you.");

        let (mut state, shot, _) = parting((12, 5));
        let refuse = |state: &mut GameState, at, why: &str| {
            let now = state.now;
            perform(state, shot, at);
            assert_eq!(last_log(state), why);
            assert_eq!(state.now, now);
        };
        refuse(&mut state, (12, 5), "That is out of reach.");

        let (mut state, shot, _) = parting((8, 5));
        // The quiver goes, and its arrows with it.
        let player = state.player;
        for (item, _) in crate::equipment::equipped(&state.world, player) {
            if state.world.satisfies::<&crate::entity::Quiver>(item) {
                holding::take_off(&mut state.world, player, item);
                crate::command::despawn_with_holdings(&mut state, item);
            }
        }
        let now = state.now;
        perform(&mut state, shot, (8, 5));
        assert_eq!(last_log(&state), "You have no arrows.");
        assert_eq!(state.now, now);
    }

    #[test]
    fn learned_techniques_survive_a_save() {
        let mut state = real();
        let first = treatise(&mut state);
        take_turn(&mut state, PlayerAction::Study(first));
        let text = crate::save::to_ron(&state).unwrap();
        let loaded = crate::save::from_ron(&text, real().blueprints).unwrap();
        assert!(knows(&loaded.world, loaded.player, "lunge"));
        assert!(knows(&loaded.world, loaded.player, "trip"));
    }

    /// The real player following Fencing at `level`, and a goblin to have
    /// fought.
    fn fencer(level: u32) -> (GameState, Entity, Entity) {
        let mut state = real();
        let player = state.player;
        let id = state.blueprints.id("Fencing").unwrap();
        let fencing = state
            .spawn(crate::spawn::Spawn::id(id).attached_to(player))
            .unwrap();
        let skill = state.blueprints.skills().id("Fencing").unwrap();
        let mut skills = crate::entity::Skills::default();
        skills.set(skill, skill::xp_for(level));
        let _ = state.world.insert_one(player, skills);
        let goblin = tough_goblin(&mut state, (5, 5));
        (state, fencing, goblin)
    }

    #[test]
    fn a_followed_style_invents_at_its_level_after_a_use_with_stakes() {
        let (mut state, fencing, goblin) = fencer(3);
        let player = state.player;
        let melee = state.blueprints.skills().id("Melee").unwrap();
        assert!(!knows(&state.world, player, "lunge"));

        skill::train(&mut state, player, &[melee], goblin, skill::BASE_XP);
        assert!(knows(&state.world, player, "lunge"));
        assert!(
            state
                .log
                .recent(10)
                .any(|line| line == "Fencing teaches you something new: the lunge.")
        );

        skill::train(&mut state, player, &[melee], goblin, skill::BASE_XP);
        let lunges = known(&state.world, player)
            .into_iter()
            .filter(|&t| name(&state.world, t) == "lunge")
            .count();
        assert_eq!(lunges, 1);

        // Set aside, the style keeps what it invented.
        state.run(Command::FollowStyle {
            actor: player,
            style: fencing,
        });
        assert!(crate::style::followed(&state.world, player).is_empty());
        assert!(knows(&state.world, player, "lunge"));
    }

    #[test]
    fn practice_never_invents() {
        let (mut state, _, _) = fencer(2);
        let player = state.player;
        let fencing = state.blueprints.skills().id("Fencing").unwrap();
        skill::gain(&mut state, player, &[(fencing, skill::xp_for(5))]);
        assert!(skill::level(skill::xp(&state.world, player, fencing)) >= 3);
        assert!(!knows(&state.world, player, "lunge"));
    }

    #[test]
    fn a_style_close_to_inventing_shows_in_the_journal() {
        let (state, _, _) = fencer(1);
        let idea = crate::discovery::IdeaView {
            name: "lunge".to_string(),
            met: vec![("Fencing".to_string(), 1)],
            unmet: 1,
        };
        assert!(crate::discovery::ideas(&state).contains(&idea));
        let (state, _, _) = fencer(0);
        assert!(
            !crate::discovery::ideas(&state)
                .iter()
                .any(|idea| idea.name == "lunge")
        );
    }

    /// The real player following bow dancing at `level`, a goblin beside.
    fn bow_dancer(level: u32) -> (GameState, Entity) {
        let mut state = real();
        let player = state.player;
        let id = state.blueprints.id("BowDancing").unwrap();
        let style = state
            .spawn(crate::spawn::Spawn::id(id).carried_by(player))
            .unwrap();
        crate::style::follow(&mut state, player, style);
        let skill = state.blueprints.skills().id("Bow dancing").unwrap();
        let mut skills = state
            .world
            .get::<&crate::entity::Skills>(player)
            .map(|skills| (*skills).clone())
            .unwrap_or_default();
        skills.set(skill, skill::xp_for(level));
        let _ = state.world.insert_one(player, skills);
        let goblin = thing("goblin")
            .at(2, 1)
            .health(1000)
            .hostile()
            .make(&mut state);
        (state, goblin)
    }

    fn named(state: &GameState, name: &str) -> SkillId {
        state.blueprints.skills().id(name).unwrap()
    }

    #[test]
    fn a_kick_follows_bow_dancing_with_a_short_bow_in_hand_only() {
        let (mut state, _) = bow_dancer(1);
        let player = state.player;
        let book = state.blueprints.skills().clone();
        let kick = [named(&state, "Melee"), named(&state, "Unarmed")];
        let dancing = named(&state, "Bow dancing");
        let follows = |state: &GameState, used: &[SkillId]| {
            crate::style::following(&state.world, &book, player, used)
        };
        assert_eq!(follows(&state, &kick), [dancing]);
        let spell = [named(&state, "Magic"), named(&state, "Arcane")];
        assert!(follows(&state, &spell).is_empty(), "a spell is not martial");

        for (item, _) in crate::equipment::equipped(&state.world, player) {
            holding::take_off(&mut state.world, player, item);
        }
        let sabre = state
            .spawn(crate::spawn::Spawn::blueprint("Sabre").carried_by(player))
            .unwrap();
        crate::equipment::equip(&mut state, player, sabre, None);
        assert!(
            follows(&state, &kick).is_empty(),
            "not with a sabre in hand"
        );
    }

    #[test]
    fn bow_dancing_steadies_a_shot_up_close_from_its_level_2() {
        let accuracy = |level| {
            let (state, _) = bow_dancer(level);
            ranged::ready_fire(&state, state.player)
                .unwrap()
                .strike
                .accuracy
        };
        let (state, goblin) = bow_dancer(1);
        let mut calm = state;
        let _ = calm.world.despawn(goblin);
        let open = ranged::ready_fire(&calm, calm.player)
            .unwrap()
            .strike
            .accuracy;
        assert_eq!(accuracy(1), open - ranged::POINT_BLANK);
        // Level 2 adds its own skill nowhere (styles never count toward a
        // use's level), so the shot is exactly as sure as with room.
        assert_eq!(accuracy(2), open);
    }

    #[test]
    fn bow_dancing_invents_the_push_kick_at_2_once() {
        let (mut state, goblin) = bow_dancer(1);
        let player = state.player;
        let dancing = named(&state, "Bow dancing");
        let mut skills = state
            .world
            .get::<&crate::entity::Skills>(player)
            .map(|skills| (*skills).clone())
            .unwrap();
        skills.set(dancing, skill::xp_for(2) - 1);
        let _ = state.world.insert_one(player, skills);
        assert!(!knows(&state.world, player, "push kick"));
        sure_hits(&mut state, player);
        let bash = [named(&state, "Melee"), named(&state, "Bow")];
        skill::train(&mut state, player, &bash, goblin, 10);
        assert!(knows(&state.world, player, "push kick"));
        skill::train(&mut state, player, &bash, goblin, 10);
        let kicks = known(&state.world, player)
            .into_iter()
            .filter(|&t| name(&state.world, t) == "push kick")
            .count();
        assert_eq!(kicks, 1);
    }

    #[test]
    fn a_lone_step_of_a_form_shows_no_step() {
        let (state, _) = bow_dancer(1);
        let trip = techniques(&state)
            .into_iter()
            .find(|view| view.name == "trip")
            .unwrap();
        assert_eq!(trip.step, None);
    }

    #[test]
    fn the_bow_dance_is_surer_along_a_run() {
        let (mut state, _) = bow_dancer(1);
        let player = state.player;
        let id = state.blueprints.id("PushKick").unwrap();
        let kick = state
            .spawn(crate::spawn::Spawn::id(id).attached_to(player))
            .unwrap();
        let trip = known(&state.world, player)
            .into_iter()
            .find(|&t| name(&state.world, t) == "trip")
            .unwrap();
        let alone = inspect(&state, player, trip).unwrap().0.mods.accuracy;
        let step = |state: &GameState| {
            techniques(state)
                .into_iter()
                .find(|view| view.name == "trip")
                .unwrap()
                .step
        };
        assert_eq!(step(&state), Some((2, 2)), "with the push kick known");
        sure_hits(&mut state, player);
        perform(&mut state, kick, (2, 1));
        let (ready, blocked) = inspect(&state, player, trip).unwrap();
        assert_eq!(blocked, None);
        assert_eq!(ready.mods.accuracy, alone + 1, "the form's step 2");
        assert!(ready.performed.is_some_and(|p| p.run == 2));
    }
}
