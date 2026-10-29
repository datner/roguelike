//! Spells (ADR 0014): what makes a cast possible and what it costs lives in
//! the caster's disciplines, what it does lives in the spell. A cast fires
//! `CheckCast` and `PayCost` at the caster, so a discipline is a part with
//! handlers and `cast` never learns which resources exist.

use std::ops::ControlFlow;

use hecs::{Entity, World};
use rand::RngExt;
use rand_pcg::Pcg64;

use crate::area;
use crate::cell::Rgb;
use crate::combat::{Strike, roll_strike};
use crate::command::{Command, DamageCause, dispatch};
use crate::cost::TICKS_PER_TURN;
use crate::effect;
use crate::entity::{
    Appearance, Area, Blast, BlocksMovement, BloodCasting, Chain, CooldownCasting, DamageTypes,
    Dead, Delay, EffectTarget, Fizzle, Focus, Health, Hits, Mana, ManaCasting, Name, Pool,
    PoolRates, Position, Recharging, School, Scroll, Shape, SpawnSeq, Spell, SpellDamage, Stat,
    Stats, Summon, SummonLimit, Summoned, Timed,
};
use crate::event::{Ctx, EventRegistry, settle};
use crate::fx::Fx;
use crate::grammar::{Subject, capitalize};
use crate::holding;
use crate::map::Tile;
use crate::prio;
use crate::ranged::{self, Trail, check_range, flight};
use crate::side;
use crate::skill::{self, SkillId};
use crate::spawn::Spawn;
use crate::state::GameState;
use crate::time::Tick;
use crate::view::{CostLine, total_armor};

/// Fired at the caster first, so worn foci and perks can change a cast
/// before anyone prices it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModifyCast {
    pub school: School,
    pub range: i32,
    pub power: i32,
    /// Added to the damage roll.
    pub damage: i32,
}

/// Asked before a cast: first of the spell itself (a scroll pays its own
/// way), then of the caster. A discipline for the spell's school sets
/// `claimed`, the `stat` the spell scales with, and describes its price in
/// `costs` (even when it then refuses, so a list can show the price); one
/// that cannot pay sets `blocked` to the reason. A cast already claimed is
/// left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckCast {
    pub caster: Entity,
    pub spell: Entity,
    pub school: School,
    pub power: i32,
    pub now: Tick,
    pub claimed: bool,
    /// Claimed by the spell's own parts: `PayCost` goes to the spell.
    pub self_paid: bool,
    pub stat: Option<Stat>,
    pub blocked: Option<String>,
    pub costs: Vec<CostLine>,
    /// How the claiming discipline takes its price.
    pub way: Option<Way>,
    pub mastery: Mastery,
}

/// How a cast is paid for, set by the discipline that claims it. Decides
/// which discipline skill paying trains (ADR 0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    Cooldown,
    Mana,
    Blood,
    Scroll,
}

/// The caster's levels at paying, carried by the events because a
/// discipline's handlers can't see the skill book: every 4 levels take a
/// point off that discipline's price (never below 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Mastery {
    pub channeling: u32,
    pub bloodletting: u32,
}

/// Fired at the caster once a cast is allowed: what could make it fizzle,
/// as a chance out of 10 000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CastRisk {
    pub caster: Entity,
    pub school: School,
    pub fizzle_bp: u32,
}

/// Fired at the caster as the cast happens: every discipline that claimed
/// the school pays its price by emitting commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayCost {
    pub caster: Entity,
    pub spell: Entity,
    pub school: School,
    pub power: i32,
    pub now: Tick,
    pub mastery: Mastery,
}

pub(crate) fn register(registry: &mut EventRegistry) {
    registry.on::<CheckCast, CooldownCasting>(prio::COOLDOWN, |ctx, rule, mut ev| {
        if rule.school != ev.school || ev.claimed {
            return ControlFlow::Continue(ev);
        }
        ev.way = Some(Way::Cooldown);
        ev.claimed = true;
        ev.stat = Some(rule.stat);
        ev.costs.push(CostLine {
            resource: "turn recharge".to_string(),
            amount: recharge_ticks(rule, ev.power) / TICKS_PER_TURN,
        });
        let recharging = ctx
            .world
            .get::<&Recharging>(ev.spell)
            .is_ok_and(|recharging| recharging.0.is_active(ev.now));
        if recharging {
            let name = ctx
                .world
                .get::<&Name>(ev.spell)
                .map_or_else(|_| "spell".to_string(), |name| name.0.clone());
            ev.blocked = Some(format!("Your {name} is not ready."));
            return ControlFlow::Break(ev);
        }
        ControlFlow::Continue(ev)
    });
    registry.on::<PayCost, CooldownCasting>(prio::COOLDOWN, |ctx: &mut Ctx<'_>, rule, ev| {
        if rule.school == ev.school {
            ctx.out.push(Command::Recharge {
                spell: ev.spell,
                duration: recharge_ticks(rule, ev.power),
            });
        }
        ControlFlow::Continue(ev)
    });

    registry.on::<CheckCast, ManaCasting>(prio::MANA, |ctx, rule, mut ev| {
        if rule.school != ev.school || ev.claimed {
            return ControlFlow::Continue(ev);
        }
        ev.way = Some(Way::Mana);
        ev.claimed = true;
        ev.stat = Some(rule.stat);
        let price = mana_price(rule, ev.power, ev.mastery);
        let pool =
            pool(ctx.world, ev.caster).map_or(0, |(mana, rates)| mana.current(rates, ev.now));
        ev.costs.push(CostLine {
            resource: "mana".to_string(),
            amount: u64::from(price),
        });
        if pool < price {
            ev.blocked = Some("Not enough mana.".to_string());
            return ControlFlow::Break(ev);
        }
        ControlFlow::Continue(ev)
    });
    registry.on::<PayCost, ManaCasting>(prio::MANA, |ctx: &mut Ctx<'_>, rule, ev| {
        if rule.school == ev.school {
            ctx.out.push(Command::SpendMana {
                caster: ev.caster,
                amount: mana_price(rule, ev.power, ev.mastery),
            });
        }
        ControlFlow::Continue(ev)
    });

    registry.on::<CheckCast, SummonLimit>(prio::SUMMON_LIMIT, |ctx, limit, mut ev| {
        let Ok(summon) = ctx.world.get::<&Summon>(ev.spell).map(|summon| *summon) else {
            return ControlFlow::Continue(ev);
        };
        let held = ctx
            .world
            .query::<&Summoned>()
            .iter()
            .filter(|summoned| summoned.owner == ev.caster)
            .count();
        if held + summon.count as usize > limit.max as usize {
            ev.blocked = Some("You can't hold any more summons.".to_string());
            return ControlFlow::Break(ev);
        }
        ControlFlow::Continue(ev)
    });

    registry.on::<CheckCast, BloodCasting>(prio::BLOOD, |ctx, rule, mut ev| {
        if rule.school != ev.school || ev.claimed {
            return ControlFlow::Continue(ev);
        }
        ev.way = Some(Way::Blood);
        ev.claimed = true;
        ev.stat = Some(rule.stat);
        let price = blood_price(rule, ev.power, ev.mastery);
        ev.costs.push(CostLine {
            resource: "HP".to_string(),
            amount: u64::try_from(price).unwrap_or(0),
        });
        let health = ctx
            .world
            .get::<&Health>(ev.caster)
            .map_or(0, |health| health.current);
        if health <= price {
            ev.blocked = Some("That would kill you.".to_string());
            return ControlFlow::Break(ev);
        }
        ControlFlow::Continue(ev)
    });
    registry.on::<PayCost, BloodCasting>(prio::BLOOD, |ctx: &mut Ctx<'_>, rule, ev| {
        if rule.school == ev.school {
            ctx.out.push(Command::DealDamage {
                target: ev.caster,
                amount: blood_price(rule, ev.power, ev.mastery),
                cause: DamageCause::Spell {
                    caster: ev.caster,
                    types: DamageTypes::default(),
                },
            });
        }
        ControlFlow::Continue(ev)
    });

    // Fired at the scroll itself, which is carried and so not reached
    // through the caster's events.
    registry.on::<CheckCast, Scroll>(prio::SCROLL, |_, _, mut ev| {
        if ev.claimed {
            return ControlFlow::Continue(ev);
        }
        ev.claimed = true;
        ev.self_paid = true;
        ev.way = Some(Way::Scroll);
        ev.costs.push(CostLine {
            resource: "scroll".to_string(),
            amount: 1,
        });
        ControlFlow::Continue(ev)
    });
    registry.on::<PayCost, Scroll>(prio::SCROLL, |ctx: &mut Ctx<'_>, _, ev| {
        ctx.out.push(Command::UseUp {
            holder: ev.caster,
            item: ev.spell,
        });
        ControlFlow::Continue(ev)
    });

    registry.on::<ModifyCast, Focus>(prio::FOCUS, |_, focus, mut ev| {
        if focus.school == ev.school {
            ev.range += focus.range;
            ev.power = (ev.power + focus.power).max(0);
            ev.damage += focus.damage;
        }
        ControlFlow::Continue(ev)
    });

    registry.on::<CastRisk, Fizzle>(prio::FIZZLE, |ctx, rule, mut ev| {
        let armor = total_armor(ctx.world, ev.caster);
        let worn = i64::from(armor.weight()).max(0);
        let modifier = ctx
            .world
            .get::<&Stats>(ev.caster)
            .map_or(0, |stats| stats.modifier(rule.stat));
        let chance = i64::from(rule.base_bp) + i64::from(rule.per_armor_bp) * worn
            - i64::from(rule.per_modifier_bp) * i64::from(modifier);
        let chance = u32::try_from(chance.max(0)).unwrap_or(u32::MAX);
        ev.fizzle_bp = ev.fizzle_bp.saturating_add(chance).min(FIZZLE_CERTAIN);
        ControlFlow::Continue(ev)
    });
}

/// A fizzle chance of 10 000 out of 10 000.
const FIZZLE_CERTAIN: u32 = 10_000;

/// The skill paying `way` trains: none for a cooldown.
fn discipline_skill(state: &GameState, way: Way) -> Option<SkillId> {
    let core = state.blueprints.skills().core()?;
    match way {
        Way::Cooldown => None,
        Way::Mana => Some(core.channeling),
        Way::Blood => Some(core.bloodletting),
        Way::Scroll => Some(core.scroll_lore),
    }
}

fn discipline_level(state: &GameState, caster: Entity, way: Way) -> u32 {
    discipline_skill(state, way).map_or(0, |id| skill::level(skill::xp(&state.world, caster, id)))
}

fn mastery(state: &GameState, caster: Entity) -> Mastery {
    Mastery {
        channeling: discipline_level(state, caster, Way::Mana),
        bloodletting: discipline_level(state, caster, Way::Blood),
    }
}

fn blood_price(rule: &BloodCasting, power: i32, mastery: Mastery) -> i32 {
    let price = power.max(0) * rule.hp_per_power;
    let eased = i32::try_from(mastery.bloodletting / 4).unwrap_or(0);
    if price > 0 {
        (price - eased).max(1)
    } else {
        price
    }
}

fn recharge_ticks(rule: &CooldownCasting, power: i32) -> u64 {
    u64::try_from(power).unwrap_or(0) * rule.ticks_per_power
}

fn mana_price(rule: &ManaCasting, power: i32, mastery: Mastery) -> u32 {
    let price = u32::try_from(power).unwrap_or(0) * rule.mana_per_power;
    if price > 0 {
        price.saturating_sub(mastery.channeling / 4).max(1)
    } else {
        price
    }
}

/// A cast as it would go now, checked the same way for the command, the aim
/// preview and the spell list.
#[derive(Debug, Clone)]
pub(crate) struct Ready {
    pub range: i32,
    /// `None` for a spell that does no damage, such as a pure summoning.
    pub strike: Option<Strike>,
    pub summon: Option<Summon>,
    /// A bolt that leaps on; ignored for an area.
    pub chain: Option<Chain>,
    /// Goes off later, from a rune; needs an `Area` and damage.
    pub delay: Option<Delay>,
    /// What the claiming disciplines will take, as they describe it.
    pub costs: Vec<CostLine>,
    /// The ground it covers; `None` for a bolt, which hits the first
    /// creature on its line.
    pub area: Option<Area>,
    /// Chance, out of 10 000, that it fizzles once paid for.
    pub fizzle_bp: u32,
    school: School,
    power: i32,
    /// Who `PayCost` goes to: the caster, or the spell for a scroll.
    payer: Entity,
    /// The skills it uses (ADR 0017), trained when it catches an enemy.
    skills: Vec<SkillId>,
    /// How it is paid for, and the caster's skill at paying.
    way: Option<Way>,
    mastery: Mastery,
}

/// What `spell` hits for when `caster` casts it: its dice plus the caster's
/// modifier for `stat` (the claiming discipline's) and `extra` (from
/// `ModifyCast`), untyped. `None` if it does no damage.
fn strike(
    world: &World,
    caster: Entity,
    spell: Entity,
    stat: Option<Stat>,
    extra: i32,
) -> Option<Strike> {
    let damage = *world.get::<&SpellDamage>(spell).ok()?;
    let bonus = world
        .get::<&Stats>(caster)
        .ok()
        .zip(stat)
        .map_or(0, |(stats, stat)| stats.modifier(stat));
    Some(Strike {
        min: damage.min,
        max: damage.max,
        bonus: bonus + extra,
        types: damage.types,
        level: 0,
        accuracy: 0,
        weapon: None,
    })
}

/// Whether `actor` can try to cast `spell`: they know it (it is in their
/// `Attachments`) or carry it as a scroll.
fn has(world: &World, actor: Entity, spell: Entity) -> bool {
    holding::attached(world, actor).contains(&spell)
        || (world.satisfies::<&Scroll>(spell) && holding::carried(world, actor).contains(&spell))
}

/// `actor` casting `spell`: they must know it and a discipline must claim
/// its school and allow it.
pub(crate) fn ready_cast(state: &GameState, actor: Entity, spell: Entity) -> Result<Ready, String> {
    match inspect_cast(state, actor, spell)? {
        (_, Some(reason)) => Err(reason),
        (ready, None) => Ok(ready),
    }
}

/// `ready_cast`, except that a cast the disciplines refuse still comes back,
/// with the reason, so a spell list can show it; `Err` only if `actor`
/// doesn't know `spell`. Handlers run on a cloned stream and a scratch
/// buffer, so asking changes nothing.
pub(crate) fn inspect_cast(
    state: &GameState,
    actor: Entity,
    spell: Entity,
) -> Result<(Ready, Option<String>), String> {
    let world = &state.world;
    let known = has(world, actor, spell);
    let does_something =
        world.satisfies::<&SpellDamage>(spell) || world.satisfies::<&Summon>(spell);
    let (true, Ok(data), true) = (
        known,
        world.get::<&Spell>(spell).map(|s| *s),
        does_something,
    ) else {
        return Err("You don't know that spell.".to_string());
    };
    let name = world
        .get::<&Name>(spell)
        .map_or_else(|_| "spell".to_string(), |name| name.0.clone());

    let mut rng = state.rngs.combat.clone();
    let mut out = Vec::new();
    let mut ctx = Ctx {
        world,
        map: &state.map,
        rng: &mut rng,
        out: &mut out,
    };
    let modified = settle(state.events.fire(
        &mut ctx,
        actor,
        ModifyCast {
            school: data.school,
            range: data.range,
            power: data.power,
            damage: 0,
        },
    ));
    let asked = CheckCast {
        caster: actor,
        spell,
        school: data.school,
        power: modified.power,
        now: state.now,
        claimed: false,
        self_paid: false,
        stat: None,
        blocked: None,
        costs: Vec::new(),
        way: None,
        mastery: mastery(state, actor),
    };
    // The spell's own parts first; the caster's disciplines skip a claimed
    // cast, but a caster's veto (a summon limit) still applies.
    let asked = settle(state.events.fire(&mut ctx, spell, asked));
    let check = if asked.blocked.is_some() {
        asked
    } else {
        settle(state.events.fire(&mut ctx, actor, asked))
    };
    let blocked = match check.blocked {
        Some(reason) => Some(reason),
        None if !check.claimed => Some(format!("You have no way to cast {name}.")),
        None => None,
    };
    let risk = settle(state.events.fire(
        &mut ctx,
        actor,
        CastRisk {
            caster: actor,
            school: data.school,
            fizzle_bp: 0,
        },
    ));
    // Skill steadies the cast and strengthens it, as it does a blow.
    let skills = skill::cast(world, state.blueprints.skills(), spell);
    let level = skill::effective(world, state.blueprints.skills(), actor, &skills);
    let ready = Ready {
        range: modified.range,
        strike: strike(world, actor, spell, check.stat, modified.damage).map(|mut strike| {
            strike.level = level;
            strike.bonus += skill::damage_bonus(level);
            strike
        }),
        summon: world.get::<&Summon>(spell).ok().map(|summon| *summon),
        chain: world.get::<&Chain>(spell).ok().map(|chain| *chain),
        delay: world.get::<&Delay>(spell).ok().map(|delay| *delay),
        costs: check.costs,
        area: world.get::<&Area>(spell).ok().map(|area| *area),
        // Reading a scroll well is a skill of its own.
        fizzle_bp: skill::steadied(
            risk.fizzle_bp,
            level
                + match check.way {
                    Some(Way::Scroll) => discipline_level(state, actor, Way::Scroll),
                    _ => 0,
                },
        ),
        school: data.school,
        power: modified.power,
        payer: if check.self_paid { spell } else { actor },
        skills,
        way: check.way,
        mastery: check.mastery,
    };
    Ok((ready, blocked))
}

/// Why `Cast` is refused, if it is.
pub(crate) fn check_cast(
    state: &GameState,
    actor: Entity,
    spell: Entity,
    at: (i32, i32),
) -> Result<(), String> {
    check_target(state, actor, &ready_cast(state, actor, spell)?, at)
}

/// Whether `at` is a tile the spell can be aimed at. Bolts and bursts are
/// aimed at a point, which must be in range and in view; cones and beams at
/// a direction, any tile but the caster's; a nova is not aimed at all.
pub(crate) fn check_target(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    at: (i32, i32),
) -> Result<(), String> {
    match ready.area.map(|area| area.shape) {
        None | Some(Shape::Burst { .. }) => check_range(state, actor, ready.range, at),
        Some(Shape::Cone | Shape::Beam) => match ranged::position(state, actor) {
            Some(from) if from == at => Err("You can't aim at yourself.".to_string()),
            Some(_) => Ok(()),
            None => Err("You can't aim from nowhere.".to_string()),
        },
        Some(Shape::Nova { .. }) => Ok(()),
    }
}

/// Where a cast at a tile goes and whom it catches: one answer for the cast
/// and for the aim preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Footprint {
    /// The bolt's flight, or a burst's before it bursts; empty for shapes
    /// that don't fly.
    pub path: Vec<(i32, i32)>,
    /// Where a missile comes down, or the aimed tile for the rest.
    pub landing: (i32, i32),
    /// The tiles an area covers, nearest its centre first; empty for a bolt.
    pub area: Vec<(i32, i32)>,
    /// Where falloff is measured from.
    pub centre: (i32, i32),
    /// The creatures it affects, in `SpawnSeq` order (a chain's in the order
    /// it leaps).
    pub caught: Vec<Entity>,
    /// Where its summons appear, in the order they are called.
    pub placed: Vec<(i32, i32)>,
}

/// `rng` is the spell stream, which placing summons draws from: the cast
/// passes the real one, a preview a clone, so both place them alike.
pub(crate) fn footprint(
    state: &GameState,
    actor: Entity,
    ready: &Ready,
    at: (i32, i32),
    rng: &mut Pcg64,
) -> Option<Footprint> {
    let from = ranged::position(state, actor)?;
    let shape = ready.area.map(|area| area.shape);
    // Bolts and bursts fly; a spell that only summons is placed, not thrown.
    let flies = match shape {
        None => ready.strike.is_some(),
        Some(Shape::Burst { .. }) => true,
        Some(Shape::Cone | Shape::Beam | Shape::Nova { .. }) => false,
    };
    let flight = flies.then(|| flight(state, actor, from, at, ready.range));
    let landing = flight.as_ref().map_or(at, |flight| flight.landing);
    let mut path = flight
        .as_ref()
        .map_or_else(Vec::new, |flight| flight.path.clone());
    let (tiles, centre, mut caught) = match ready.area {
        Some(area) => {
            let cover = cover(state, actor, area, from, landing, ready.range);
            (cover.tiles, cover.centre, cover.caught)
        }
        None => {
            let hit = flight.as_ref().and_then(|flight| flight.hit);
            (Vec::new(), landing, hit.into_iter().collect())
        }
    };
    if let (None, Some(chain)) = (ready.area, ready.chain) {
        jump(state, actor, chain, &mut caught, &mut path);
    }
    let placed = ready.summon.map_or_else(Vec::new, |summon| {
        placement(state, landing, summon.count, rng)
    });
    Some(Footprint {
        path,
        landing,
        area: tiles,
        centre,
        caught,
        placed,
    })
}

/// Carries a bolt on from its last victim to the nearest enemy of `actor`
/// not yet hit, within `chain.reach` and with open ground between (ties by
/// `SpawnSeq`), up to `chain.jumps` times. Each jump's line joins `path`, so
/// the animation follows it.
fn jump(
    state: &GameState,
    actor: Entity,
    chain: Chain,
    caught: &mut Vec<Entity>,
    path: &mut Vec<(i32, i32)>,
) {
    let world = &state.world;
    let creatures: Vec<(Entity, (i32, i32), SpawnSeq)> = world
        .query::<(Entity, &Position, &SpawnSeq)>()
        .with::<&Health>()
        .without::<&Dead>()
        .iter()
        .filter(|&(other, _, _)| other != actor && side::enemies(world, actor, other))
        .map(|(other, pos, seq)| (other, (pos.x, pos.y), *seq))
        .collect();
    for _ in 0..chain.jumps {
        let Some(&last) = caught.last() else {
            return;
        };
        let Some(from) = ranged::position(state, last) else {
            return;
        };
        let next = creatures
            .iter()
            .filter(|(other, at, _)| {
                !caught.contains(other)
                    && ranged::distance(from, *at) <= chain.reach
                    && ranged::line(from, *at)
                        .into_iter()
                        .all(|(x, y)| Tile::is_transparent(state.map.get(x, y)))
            })
            .min_by_key(|(_, at, seq)| (ranged::distance(from, *at), *seq));
        let Some(&(next, at, _)) = next else {
            return;
        };
        path.extend(ranged::line(from, at));
        caught.push(next);
    }
}

/// How far from the aimed spot a summon may appear.
const SUMMON_REACH: i32 = 2;

/// Up to `count` free tiles nearest `at`: open ground in view of it, within
/// `SUMMON_REACH`, with no creature or blocker. Tiles equally near are
/// shuffled from the spell stream, ring by ring and only as far as needed,
/// so where summons appear is fair but seeded.
fn placement(state: &GameState, at: (i32, i32), count: u32, rng: &mut Pcg64) -> Vec<(i32, i32)> {
    let count = count as usize;
    let ring = |(x, y): (i32, i32)| (x - at.0).pow(2) + (y - at.1).pow(2);
    let mut free: Vec<(i32, i32)> = area::tiles(
        &state.map,
        Shape::Nova {
            radius: SUMMON_REACH,
        },
        at,
        at,
        0,
    )
    .into_iter()
    .filter(|&tile| !occupied(&state.world, tile))
    .collect();
    let mut start = 0;
    while start < free.len() && start < count {
        let key = ring(free[start]);
        let end = free[start..]
            .iter()
            .position(|&tile| ring(tile) != key)
            .map_or(free.len(), |len| start + len);
        let group = &mut free[start..end];
        for i in (1..group.len()).rev() {
            group.swap(i, rng.random_range(0..=i));
        }
        start = end;
    }
    free.truncate(count);
    free
}

pub(crate) fn occupied(world: &World, tile: (i32, i32)) -> bool {
    world
        .query::<(&Position, Option<&Health>, Option<&BlocksMovement>)>()
        .iter()
        .any(|(pos, health, blocks)| {
            (pos.x, pos.y) == tile && (health.is_some() || blocks.is_some())
        })
}

/// The ground an area covers and who it affects there.
pub(crate) struct Cover {
    /// Nearest the centre first.
    pub tiles: Vec<(i32, i32)>,
    /// Where falloff is measured from.
    pub centre: (i32, i32),
    /// In `SpawnSeq` order.
    pub caught: Vec<Entity>,
}

/// What `area` covers from `from` toward `aim` (out to `range`): one answer
/// for spells and techniques (ADR 0024).
pub(crate) fn cover(
    state: &GameState,
    actor: Entity,
    area: Area,
    from: (i32, i32),
    aim: (i32, i32),
    range: i32,
) -> Cover {
    let tiles = area::tiles(&state.map, area.shape, from, aim, range);
    Cover {
        caught: caught(&state.world, actor, area.hits, &tiles),
        centre: area::centre(area.shape, from, aim),
        tiles,
    }
}

/// Living creatures standing on `tiles` whom `hits` lets the spell affect,
/// in `SpawnSeq` order.
fn caught(world: &World, caster: Entity, hits: Hits, tiles: &[(i32, i32)]) -> Vec<Entity> {
    let mut standing: Vec<(SpawnSeq, Entity)> = world
        .query::<(Entity, &Position, &SpawnSeq)>()
        .with::<&Health>()
        .without::<&Dead>()
        .iter()
        .filter(|&(_, pos, _)| tiles.contains(&(pos.x, pos.y)))
        .map(|(entity, _, seq)| (*seq, entity))
        .collect();
    standing.sort_by_key(|&(seq, _)| seq);
    let ours = side::side(world, caster);
    standing
        .into_iter()
        .map(|(_, entity)| entity)
        .filter(|&entity| match hits {
            Hits::All => true,
            Hits::Enemies => side::side(world, entity) != ours,
            Hits::Allies => side::side(world, entity) == ours,
        })
        .collect()
}

/// Applies `Command::Cast`: pays the cost, then works out the footprint and
/// gives every creature caught `DealDamage`. Each takes one combat-stream
/// roll, in `SpawnSeq` order, less the area's falloff for its distance from
/// the centre; a bolt that hits nothing draws nothing.
pub(crate) fn cast(state: &mut GameState, actor: Entity, spell: Entity, at: (i32, i32)) {
    let Ok(ready) = ready_cast(state, actor, spell) else {
        return;
    };
    let mut rng = state.rngs.spell.clone();
    let footprint = footprint(state, actor, &ready, at, &mut rng);
    state.rngs.spell = rng;
    let Some(footprint) = footprint else {
        return;
    };
    let _ = dispatch(
        state,
        ready.payer,
        PayCost {
            caster: actor,
            spell,
            school: ready.school,
            power: ready.power,
            now: state.now,
            mastery: ready.mastery,
        },
    );

    let name = state
        .world
        .get::<&Name>(spell)
        .map_or_else(|_| "spell".to_string(), |name| name.0.clone());
    // One spell-stream draw, and only when there is a chance at all, so
    // casters who never fizzle leave the stream untouched.
    if ready.fizzle_bp > 0 && state.rngs.spell.random_range(0..FIZZLE_CERTAIN) < ready.fizzle_bp {
        let caster = Subject::of(&state.world, actor);
        state.log.push(format!(
            "{} {name} fizzles.",
            capitalize(&caster.possessive())
        ));
        return;
    }

    // The flight, then the area spreading from its centre; a delayed spell
    // only flies now.
    let (glyph, fg) = state
        .world
        .get::<&Appearance>(spell)
        .map_or(('*', Rgb(255, 140, 40)), |look| (look.glyph, look.fg));
    let spreads = if ready.delay.is_some() {
        Vec::new()
    } else {
        footprint.area.clone()
    };
    for path in [&footprint.path, &spreads, &footprint.placed] {
        if !path.is_empty() {
            state.fx.push(Fx::Flight(Trail {
                glyph,
                fg,
                path: path.clone(),
            }));
        }
    }

    let aimed = match (ready.area, footprint.caught.first()) {
        (None, Some(&target)) => format!(" at {}", Subject::of(&state.world, target).noun()),
        _ => String::new(),
    };
    let message = Subject::of(&state.world, actor).sentence("cast", &format!("{name}{aimed}"));
    state.log.push(message);
    crate::lore::first_cast(state, actor, spell);

    if let (Some(delay), Some(strike), Some(area)) = (ready.delay, ready.strike, ready.area) {
        lay_rune(state, actor, footprint.landing, delay, strike, area);
        return;
    }
    if let Some(strike) = ready.strike {
        let falloff = ready.area.map_or(0, |area| area.falloff);
        for &target in &footprint.caught {
            let hit = roll_strike(&mut state.rngs.combat, strike);
            let away = ranged::position(state, target)
                .map_or(0, |at| ranged::distance(footprint.centre, at));
            state.commands.push_back(Command::DealDamage {
                target,
                amount: (hit.amount - falloff * away).max(0),
                cause: DamageCause::Spell {
                    caster: actor,
                    types: strike.types,
                },
            });
        }
    }
    if let Some(summon) = ready.summon {
        call(state, actor, summon, &footprint.placed);
    }
    // A cast teaches when it catches an enemy, named by the first one.
    let enemy = footprint
        .caught
        .iter()
        .copied()
        .find(|&target| side::enemies(&state.world, actor, target));
    if let Some(enemy) = enemy {
        // Paying for a cast that counted trains the discipline too.
        let mut trained = ready.skills.clone();
        trained.extend(ready.way.and_then(|way| discipline_skill(state, way)));
        trained.sort();
        skill::train(state, actor, &trained, enemy, skill::BASE_XP);
    }
}

/// Leaves a delayed spell's rune at `at`: a timed effect on `caster`
/// carrying the damage as rolled-for now, which bursts on expiry (`detonate`).
fn lay_rune(
    state: &mut GameState,
    caster: Entity,
    at: (i32, i32),
    delay: Delay,
    strike: Strike,
    area: Area,
) {
    let blast = Blast {
        min: strike.min,
        max: strike.max,
        bonus: strike.bonus,
        types: strike.types,
        area,
    };
    let timed = Timed {
        start: state.now,
        duration: delay.ticks,
    };
    let spawn = Spawn::blueprint("Rune")
        .at(at.0, at.1)
        .with(timed)
        .with(EffectTarget(caster))
        .with(blast)
        .attached_to(caster);
    match state.spawn(spawn) {
        Ok(rune) => {
            state.log.push("A rune glows on the ground.");
            effect::schedule_expiry(state, rune);
        }
        Err(err) => state.log.push(format!("Error: cannot lay a rune: {err}")),
    }
}

/// A rune's burst, from `effect::expire_effect`: the same as an area cast
/// centred on the rune, for `caster`. A cone or beam has no direction left,
/// so it bursts on the rune's tile alone.
pub(crate) fn detonate(state: &mut GameState, caster: Entity, at: (i32, i32), blast: Blast) {
    let radius = match blast.area.shape {
        Shape::Burst { radius } | Shape::Nova { radius } => radius,
        Shape::Cone | Shape::Beam => 0,
    };
    let tiles = area::tiles(&state.map, Shape::Burst { radius }, at, at, 0);
    let caught = caught(&state.world, caster, blast.area.hits, &tiles);
    state.log.push("The rune bursts!");
    state.fx.push(Fx::Flight(Trail {
        glyph: '*',
        fg: Rgb(255, 90, 20),
        path: tiles,
    }));
    let strike = Strike {
        min: blast.min,
        max: blast.max,
        bonus: blast.bonus,
        types: blast.types,
        weapon: None,
        level: 0,
        accuracy: 0,
    };
    for target in caught {
        let hit = roll_strike(&mut state.rngs.combat, strike);
        let away = ranged::position(state, target).map_or(0, |pos| ranged::distance(at, pos));
        state.commands.push_back(Command::DealDamage {
            target,
            amount: (hit.amount - blast.area.falloff * away).max(0),
            cause: DamageCause::Spell {
                caster,
                types: blast.types,
            },
        });
    }
}

/// Spawns `summon`'s creatures on `tiles`, on `caster`'s side, each held by
/// a `Binding` for the summon's lifetime.
fn call(state: &mut GameState, caster: Entity, summon: Summon, tiles: &[(i32, i32)]) {
    if tiles.is_empty() {
        state.log.push("Nothing answers: there is no room.");
    }
    for &(x, y) in tiles {
        let spawn = Spawn::id(summon.blueprint)
            .at(x, y)
            .with(Summoned { owner: caster });
        match state.spawn(spawn) {
            Ok(creature) => {
                let message = Subject::of(&state.world, creature).sentence("appear", "");
                state.log.push(message);
                effect::bind(state, creature, summon.lifetime);
            }
            Err(err) => state.log.push(format!("Error: cannot summon: {err}")),
        }
    }
}

/// Applies `Command::Recharge`: the spell cannot be cast for `duration`
/// ticks from now.
pub(crate) fn recharge(state: &mut GameState, spell: Entity, duration: u64) {
    let timed = Timed {
        start: state.now,
        duration,
    };
    let _ = state.world.insert_one(spell, Recharging(timed));
}

/// Applies `Command::SpendMana`: the regained mana is banked as of now and
/// `amount` taken from it.
pub(crate) fn spend_mana(state: &mut GameState, caster: Entity, amount: u32) {
    let Some((mana, rates)) = pool(&state.world, caster) else {
        return;
    };
    let _ = state
        .world
        .insert_one(caster, Mana(mana.spend(amount, rates, state.now)));
}

/// `caster`'s mana pool and its size and pace for their stats.
pub(crate) fn pool(world: &World, caster: Entity) -> Option<(Pool, PoolRates)> {
    let mana = world.get::<&Mana>(caster).ok()?.0.clone();
    let stats = world.get::<&Stats>(caster).ok().map(|stats| *stats);
    let rates = mana.rates(stats);
    Some((mana, rates))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Armor, Binding, Health};
    use crate::input::PlayerAction;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, see_everything, thing};
    use crate::view::ItemRef;

    const TICKS_PER_POWER: u64 = 300;

    /// A bare room with the player knowing a pyromancy spell of `power` 2
    /// (range 6, 3 damage) under the cooldown discipline.
    fn caster() -> (GameState, Entity) {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            CooldownCasting {
                school: School::Pyromancy,
                ticks_per_power: TICKS_PER_POWER,
                stat: Stat::Willpower,
            },
        );
        let spell = thing("firebolt")
            .spell(School::Pyromancy, 6, 2)
            .spell_damage(3, 3)
            .attached_to(player)
            .make(&mut state);
        (state, spell)
    }

    fn goblin(state: &mut GameState, x: i32) -> Entity {
        thing("goblin").at(x, 1).health(10).hostile().make(state)
    }

    fn health(state: &GameState, who: Entity) -> i32 {
        state.world.get::<&Health>(who).unwrap().current
    }

    fn cast_at(state: &mut GameState, spell: Entity, at: (i32, i32)) {
        let player = state.player;
        state.run(Command::Cast {
            actor: player,
            spell,
            at,
        });
    }

    fn skill_names(state: &GameState, spell: Entity) -> Vec<String> {
        let book = state.blueprints.skills();
        skill::cast(&state.world, book, spell)
            .into_iter()
            .map(|id| book.get(id).unwrap().name.clone())
            .collect()
    }

    #[test]
    fn a_cast_uses_magic_its_school_and_its_form() {
        let (mut state, bolt) = caster();
        assert_eq!(skill_names(&state, bolt), ["Magic", "Pyromancy", "Bolt"]);
        let ball = thing("fireball")
            .spell(School::Pyromancy, 6, 2)
            .spell_damage(1, 1)
            .area(Shape::Burst { radius: 1 }, Hits::All, 0)
            .make(&mut state);
        assert_eq!(skill_names(&state, ball), ["Magic", "Pyromancy", "Burst"]);
        let lance = thing("lance")
            .spell(School::Arcane, 6, 2)
            .spell_damage(1, 1)
            .with(Chain { jumps: 1, reach: 2 })
            .make(&mut state);
        assert_eq!(
            skill_names(&state, lance),
            ["Magic", "Arcane", "Bolt", "Chain"]
        );
    }

    #[test]
    fn skill_strengthens_and_steadies_a_cast() {
        let (mut state, bolt) = caster();
        let player = state.player;
        let fizzle = Fizzle {
            base_bp: 3_000,
            per_armor_bp: 0,
            per_modifier_bp: 0,
            stat: Stat::Willpower,
        };
        state.world.insert_one(player, fizzle).unwrap();
        let before = ready_cast(&state, player, bolt).unwrap();
        let book = state.blueprints.skills();
        let mut learned = crate::entity::Skills::default();
        for name in ["Magic", "Pyromancy", "Bolt"] {
            learned.set(book.id(name).unwrap(), skill::xp_for(8));
        }
        state.world.insert_one(player, learned).unwrap();
        let after = ready_cast(&state, player, bolt).unwrap();
        let (low, high) = before.strike.unwrap().range();
        assert_eq!(after.strike.unwrap().range(), (low + 2, high + 2));
        assert_eq!(
            (before.fizzle_bp, after.fizzle_bp),
            (3_000, 3_000 - 8 * 200)
        );
    }

    #[test]
    fn a_cast_that_catches_an_enemy_trains_and_one_that_catches_none_does_not() {
        let (mut state, bolt) = caster();
        let player = state.player;
        state
            .world
            .insert_one(player, crate::entity::Learner)
            .unwrap();
        let magic = state.blueprints.skills().id("Magic").unwrap();
        cast_at(&mut state, bolt, (5, 1));
        assert_eq!(skill::xp(&state.world, player, magic), 0);

        state.now = Tick(10_000);
        goblin(&mut state, 4);
        cast_at(&mut state, bolt, (4, 1));
        assert_eq!(skill::xp(&state.world, player, magic), skill::BASE_XP);
    }

    #[test]
    fn a_bolt_hits_the_first_creature_on_the_line() {
        let (mut state, spell) = caster();
        let near = goblin(&mut state, 3);
        let far = goblin(&mut state, 5);
        cast_at(&mut state, spell, (5, 1));
        assert_eq!(health(&state, near), 7);
        assert_eq!(health(&state, far), 10);
        assert!(last_log(&state).contains("takes 3 damage"));
    }

    #[test]
    fn spell_damage_is_untyped_so_armor_does_not_resist_it() {
        let (mut state, spell) = caster();
        let target = thing("goblin")
            .at(3, 1)
            .health(10)
            .armor(5)
            .hostile()
            .make(&mut state);
        assert_eq!(Armor::uniform(5).against(DamageTypes::default()), 0);
        cast_at(&mut state, spell, (3, 1));
        assert_eq!(health(&state, target), 7);
    }

    #[test]
    fn an_elemental_spell_is_resisted_by_its_element_alone() {
        use crate::entity::DamageType;
        let (mut state, spell) = caster();
        let _ = state.world.insert_one(
            spell,
            SpellDamage {
                min: 3,
                max: 3,
                types: DamageTypes::single(DamageType::Fire),
            },
        );
        let plated = thing("goblin")
            .at(3, 1)
            .health(10)
            .armor(5)
            .hostile()
            .make(&mut state);
        cast_at(&mut state, spell, (3, 1));
        assert_eq!(health(&state, plated), 7, "plate does not stop fire");

        let _ = state.world.remove_one::<Recharging>(spell);
        let _ = state.world.insert_one(
            plated,
            Armor {
                fire: 2,
                ..Armor::default()
            },
        );
        let aimed = crate::aim::aim(
            &state,
            crate::aim::Shot::Cast(crate::view::ItemRef::of(&state.world, spell).unwrap()),
            (3, 1),
        )
        .unwrap();
        assert_eq!(aimed.damage, (1, 1), "the preview takes the resistance off");
        cast_at(&mut state, spell, (3, 1));
        assert_eq!(health(&state, plated), 6);
    }

    #[test]
    fn casting_recharges_the_spell_for_power_times_the_rate() {
        let (mut state, spell) = caster();
        goblin(&mut state, 3);
        let before = state.now;
        cast_at(&mut state, spell, (3, 1));
        let recharging = *state.world.get::<&Recharging>(spell).unwrap();
        assert_eq!(recharging.0.start, before);
        assert_eq!(recharging.0.duration, 2 * TICKS_PER_POWER);
    }

    #[test]
    fn a_recharging_spell_is_refused_until_the_timestamps_say_otherwise() {
        let (mut state, spell) = caster();
        let target = goblin(&mut state, 3);
        let player = state.player;
        cast_at(&mut state, spell, (3, 1));
        assert_eq!(health(&state, target), 7);

        assert_eq!(
            check_cast(&state, player, spell, (3, 1)),
            Err("Your firebolt is not ready.".to_string())
        );
        cast_at(&mut state, spell, (3, 1));
        assert_eq!(health(&state, target), 7, "a refused cast changes nothing");

        state.now = state.now.after(2 * TICKS_PER_POWER);
        assert_eq!(check_cast(&state, player, spell, (3, 1)), Ok(()));
        cast_at(&mut state, spell, (3, 1));
        assert_eq!(health(&state, target), 4);
    }

    #[test]
    fn a_spell_without_a_discipline_for_its_school_cannot_be_cast() {
        let (mut state, spell) = caster();
        let player = state.player;
        let _ = state.world.remove_one::<CooldownCasting>(player);
        assert_eq!(
            check_cast(&state, player, spell, (3, 1)),
            Err("You have no way to cast firebolt.".to_string())
        );
    }

    #[test]
    fn only_known_spells_can_be_cast() {
        let (mut state, _) = caster();
        let player = state.player;
        let stray = thing("firebolt")
            .spell(School::Pyromancy, 6, 2)
            .spell_damage(3, 3)
            .make(&mut state);
        assert_eq!(
            check_cast(&state, player, stray, (3, 1)),
            Err("You don't know that spell.".to_string())
        );
    }

    #[test]
    fn out_of_range_and_unseen_targets_are_refused() {
        let (mut state, spell) = caster();
        let player = state.player;
        assert_eq!(
            check_cast(&state, player, spell, (9, 1)),
            Err("That is out of range.".to_string())
        );
        state.visible.set(4, 1, false);
        assert_eq!(
            check_cast(&state, player, spell, (4, 1)),
            Err("You can't see that spot.".to_string())
        );
    }

    #[test]
    fn a_bolt_that_hits_nothing_still_costs_the_recharge() {
        let (mut state, spell) = caster();
        cast_at(&mut state, spell, (4, 1));
        assert!(state.world.get::<&Recharging>(spell).is_ok());
        assert!(last_log(&state).starts_with("You cast firebolt"));
    }

    #[test]
    fn the_player_casts_through_an_action() {
        let (mut state, spell) = caster();
        let target = goblin(&mut state, 3);
        let item = ItemRef::of(&state.world, spell).unwrap();
        crate::time::take_turn(
            &mut state,
            PlayerAction::Cast {
                spell: item,
                at: (3, 1),
            },
        );
        assert_eq!(health(&state, target), 7);
    }

    #[test]
    fn the_spell_list_shows_the_price_then_why_it_is_blocked() {
        let (mut state, spell) = caster();
        let listed = crate::view::spells(&state);
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].name.as_str(), listed[0].range, listed[0].damage),
            ("firebolt", 6, Some((3, 3)))
        );
        assert_eq!(
            listed[0].preview,
            crate::view::CastPreview {
                costs: vec![CostLine {
                    resource: "turn recharge".to_string(),
                    amount: 6,
                }],
                blocked: None,
                fizzle_bp: 0,
            }
        );

        recharge(&mut state, spell, 600);
        let listed = crate::view::spells(&state);
        assert_eq!(
            listed[0].preview.blocked.as_deref(),
            Some("Your firebolt is not ready.")
        );
    }

    #[test]
    fn the_same_seed_and_casts_give_the_same_damage() {
        let run = || {
            let (mut state, spell) = caster();
            let _ = state.world.insert_one(
                spell,
                SpellDamage {
                    min: 1,
                    max: 6,
                    types: DamageTypes::default(),
                },
            );
            let target = goblin(&mut state, 3);
            cast_at(&mut state, spell, (3, 1));
            health(&state, target)
        };
        assert_eq!(run(), run());
    }

    const MANA_PER_POWER: u32 = 3;
    const REGEN_TICKS: u64 = 200;

    /// `caster()` plus the mana discipline for Arcane, a full pool of 10
    /// regaining a point per `REGEN_TICKS`, and an Arcane bolt of `power` 1
    /// (range 8, 2 damage).
    pub(super) fn mage() -> (GameState, Entity, Entity) {
        let (mut state, firebolt) = caster();
        let player = state.player;
        let _ = state.world.insert(
            player,
            (
                ManaCasting {
                    school: School::Arcane,
                    mana_per_power: MANA_PER_POWER,
                    stat: Stat::Intelligence,
                },
                Mana(Pool {
                    value: 10,
                    max: 10,
                    as_of: state.now,
                    regen_ticks: REGEN_TICKS,
                    growth: Vec::new(),
                }),
            ),
        );
        let missile = thing("magic missile")
            .spell(School::Arcane, 8, 1)
            .spell_damage(2, 2)
            .attached_to(player)
            .make(&mut state);
        (state, firebolt, missile)
    }

    fn mana(state: &GameState) -> u32 {
        let (pool, rates) = pool(&state.world, state.player).unwrap();
        pool.current(rates, state.now)
    }

    #[test]
    fn a_mana_spell_costs_power_times_the_rate_and_no_recharge() {
        let (mut state, _, missile) = mage();
        let target = goblin(&mut state, 3);
        cast_at(&mut state, missile, (3, 1));
        assert_eq!(health(&state, target), 8);
        assert_eq!(mana(&state), 10 - MANA_PER_POWER);
        assert!(state.world.get::<&Recharging>(missile).is_err());
    }

    #[test]
    fn without_enough_mana_the_cast_is_refused_and_nothing_is_spent() {
        let (mut state, _, missile) = mage();
        let target = goblin(&mut state, 3);
        let player = state.player;
        spend_mana(&mut state, player, 8);
        assert_eq!(
            check_cast(&state, player, missile, (3, 1)),
            Err("Not enough mana.".to_string())
        );
        cast_at(&mut state, missile, (3, 1));
        assert_eq!((health(&state, target), mana(&state)), (10, 2));
    }

    /// A base pool of 10 regaining a point per 200 ticks, which Intelligence
    /// deepens (+3 a point, +5% pace) and Willpower quickens (+1, +20%).
    fn grown_pool() -> Pool {
        use crate::entity::PoolGrowth;
        Pool {
            value: 99,
            max: 10,
            as_of: Tick::default(),
            regen_ticks: 200,
            growth: vec![
                PoolGrowth {
                    stat: Stat::Intelligence,
                    max: 3,
                    regen_bp: 500,
                },
                PoolGrowth {
                    stat: Stat::Willpower,
                    max: 1,
                    regen_bp: 2_000,
                },
            ],
        }
    }

    fn rates_with(intelligence: i32, willpower: i32) -> PoolRates {
        grown_pool().rates(Some(Stats {
            intelligence,
            willpower,
            ..Stats::AVERAGE
        }))
    }

    #[test]
    fn intelligence_deepens_the_pool_and_willpower_quickens_it() {
        let base = PoolRates {
            max: 10,
            regen_ticks: 200,
        };
        assert_eq!(rates_with(16, 16), base);
        assert_eq!(grown_pool().rates(None), base);
        // Modifier +2 each: 10 + 6 capacity at 11 000 pace; 10 + 2 at 14 000.
        assert_eq!(
            rates_with(20, 16),
            PoolRates {
                max: 16,
                regen_ticks: 181,
            }
        );
        assert_eq!(
            rates_with(16, 20),
            PoolRates {
                max: 12,
                regen_ticks: 142,
            }
        );
        // A dull mind (-3) shrinks the pool and slows it.
        assert_eq!(
            rates_with(10, 16),
            PoolRates {
                max: 1,
                regen_ticks: 235,
            }
        );
    }

    #[test]
    fn a_pool_never_shrinks_below_empty_or_slows_below_a_quarter_pace() {
        assert_eq!(
            rates_with(0, 0),
            PoolRates {
                max: 0,
                regen_ticks: 800,
            }
        );
        let still = Pool {
            regen_ticks: 0,
            ..grown_pool()
        };
        assert_eq!(still.rates(None).regen_ticks, 0, "still never regains");
    }

    #[test]
    fn the_casters_stats_set_the_pool_they_cast_from() {
        let (mut state, _, missile) = mage();
        let player = state.player;
        let _ = state.world.insert(
            player,
            (
                Mana(grown_pool()),
                Stats {
                    willpower: 20,
                    ..Stats::AVERAGE
                },
            ),
        );
        assert_eq!(mana(&state), 12, "a value above the maximum reads full");
        spend_mana(&mut state, player, 12);
        assert_eq!(
            check_cast(&state, player, missile, (3, 1)),
            Err("Not enough mana.".to_string())
        );
        state.now = state.now.after(3 * 142);
        assert_eq!(mana(&state), 3, "a point per 142 ticks, not 200");
        assert_eq!(check_cast(&state, player, missile, (3, 1)), Ok(()));
    }

    #[test]
    fn mana_comes_back_from_the_timestamps_and_keeps_partial_progress() {
        let (mut state, _, _) = mage();
        let player = state.player;
        spend_mana(&mut state, player, 6);
        state.now = state.now.after(REGEN_TICKS + REGEN_TICKS / 2);
        assert_eq!(mana(&state), 5);
        // Spending banks the point regained; the half-way progress is kept.
        spend_mana(&mut state, player, 1);
        state.now = state.now.after(REGEN_TICKS / 2);
        assert_eq!(mana(&state), 5);
        state.now = state.now.after(100 * REGEN_TICKS);
        assert_eq!(mana(&state), 10, "never past the maximum");
    }

    #[test]
    fn each_discipline_pays_only_for_its_own_school() {
        let (mut state, firebolt, missile) = mage();
        goblin(&mut state, 3);
        let player = state.player;
        cast_at(&mut state, firebolt, (3, 1));
        assert_eq!(mana(&state), 10, "firebolt costs no mana");
        assert_eq!(check_cast(&state, player, missile, (3, 1)), Ok(()));
    }

    #[test]
    fn a_spell_scales_with_the_stat_of_the_discipline_casting_it() {
        let (mut state, firebolt, missile) = mage();
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            Stats {
                intelligence: 20,
                ..Stats::AVERAGE
            },
        );
        let target = goblin(&mut state, 3);
        cast_at(&mut state, missile, (3, 1));
        assert_eq!(health(&state, target), 10 - (2 + 2), "Intelligence +2");
        cast_at(&mut state, firebolt, (3, 1));
        assert_eq!(health(&state, target), 6 - 3, "average Willpower adds 0");
    }

    #[test]
    fn the_spell_list_prices_mana_and_still_shows_it_when_short() {
        let (mut state, _, _) = mage();
        let player = state.player;
        let price = CostLine {
            resource: "mana".to_string(),
            amount: u64::from(MANA_PER_POWER),
        };
        let listed = crate::view::spells(&state);
        assert_eq!(listed[1].preview.costs, std::slice::from_ref(&price));
        spend_mana(&mut state, player, 9);
        let listed = crate::view::spells(&state);
        assert_eq!(
            listed[1].preview,
            crate::view::CastPreview {
                costs: vec![price],
                blocked: Some("Not enough mana.".to_string()),
                fizzle_bp: 0,
            }
        );
    }

    /// A pyromancy spell of `shape` for the player in `caster()`: range 6,
    /// 3 damage, no falloff.
    fn shaped(state: &mut GameState, shape: Shape, hits: Hits) -> Entity {
        thing("blast")
            .spell(School::Pyromancy, 6, 1)
            .spell_damage(3, 3)
            .area(shape, hits, 0)
            .attached_to(state.player)
            .make(state)
    }

    fn creature(state: &mut GameState, at: (i32, i32), hostile: bool) -> Entity {
        let spawn = thing("goblin").at(at.0, at.1).health(10);
        if hostile { spawn.hostile() } else { spawn }.make(state)
    }

    #[test]
    fn a_burst_hits_everyone_in_its_radius_and_no_one_outside() {
        let (mut state, _) = caster();
        let blast = shaped(&mut state, Shape::Burst { radius: 1 }, Hits::All);
        let below = goblin(&mut state, 5);
        let _ = state.world.insert_one(below, Position { x: 5, y: 2 });
        let beside = goblin(&mut state, 6);
        let outside = goblin(&mut state, 8);
        cast_at(&mut state, blast, (5, 1));
        assert_eq!(
            (
                health(&state, below),
                health(&state, beside),
                health(&state, outside)
            ),
            (7, 7, 10)
        );
        assert_eq!(last_log(&state), "The goblin takes 3 damage.");
    }

    #[test]
    fn a_burst_goes_off_on_the_first_creature_in_its_way() {
        let (mut state, _) = caster();
        let blast = shaped(&mut state, Shape::Burst { radius: 1 }, Hits::All);
        let near = goblin(&mut state, 4);
        let aimed = goblin(&mut state, 6);
        cast_at(&mut state, blast, (6, 1));
        assert_eq!((health(&state, near), health(&state, aimed)), (7, 10));
    }

    #[test]
    fn hits_decides_whether_the_caster_and_their_side_are_caught() {
        let run = |hits| {
            let (mut state, _) = caster();
            let blast = shaped(&mut state, Shape::Burst { radius: 1 }, hits);
            let foe = creature(&mut state, (2, 2), true);
            let friend = creature(&mut state, (3, 2), false);
            let _ = state.world.insert_one(
                state.player,
                Health {
                    current: 20,
                    max: 20,
                },
            );
            cast_at(&mut state, blast, (2, 1));
            let player = state.player;
            (
                health(&state, player),
                health(&state, foe),
                health(&state, friend),
            )
        };
        assert_eq!(run(Hits::All), (17, 7, 7));
        assert_eq!(run(Hits::Enemies), (20, 7, 10));
        assert_eq!(run(Hits::Allies), (17, 10, 7));
    }

    #[test]
    fn a_beam_passes_through_every_creature_on_its_line() {
        let (mut state, _) = caster();
        let beam = shaped(&mut state, Shape::Beam, Hits::Enemies);
        let near = goblin(&mut state, 3);
        let far = goblin(&mut state, 7);
        let beyond = goblin(&mut state, 8);
        cast_at(&mut state, beam, (2, 1));
        assert_eq!(
            (
                health(&state, near),
                health(&state, far),
                health(&state, beyond)
            ),
            (7, 7, 10),
            "range 6 from x = 1 ends at x = 7"
        );
    }

    #[test]
    fn a_cone_catches_what_is_ahead_and_spares_what_is_beside() {
        let (mut state, _) = caster();
        let cone = shaped(&mut state, Shape::Cone, Hits::Enemies);
        let ahead = creature(&mut state, (4, 2), true);
        let beside = creature(&mut state, (1, 4), true);
        cast_at(&mut state, cone, (2, 1));
        assert_eq!((health(&state, ahead), health(&state, beside)), (7, 10));
    }

    #[test]
    fn cones_and_beams_take_a_direction_and_novas_take_nothing() {
        let (mut state, _) = caster();
        let player = state.player;
        let cone = shaped(&mut state, Shape::Cone, Hits::Enemies);
        let nova = shaped(&mut state, Shape::Nova { radius: 1 }, Hits::Enemies);
        assert_eq!(
            check_cast(&state, player, cone, (1, 1)),
            Err("You can't aim at yourself.".to_string())
        );
        state.visible.set(30, 1, false);
        assert_eq!(
            check_cast(&state, player, cone, (30, 1)),
            Ok(()),
            "any direction"
        );
        assert_eq!(check_cast(&state, player, nova, (1, 1)), Ok(()));
    }

    #[test]
    fn a_nova_surrounds_the_caster() {
        let (mut state, _) = caster();
        let nova = shaped(&mut state, Shape::Nova { radius: 1 }, Hits::Enemies);
        let next = creature(&mut state, (2, 2), true);
        let away = goblin(&mut state, 4);
        cast_at(&mut state, nova, (1, 1));
        assert_eq!((health(&state, next), health(&state, away)), (7, 10));
    }

    #[test]
    fn falloff_takes_damage_off_per_tile_from_the_centre() {
        let (mut state, _) = caster();
        let blast = thing("blast")
            .spell(School::Pyromancy, 6, 1)
            .spell_damage(3, 3)
            .area(Shape::Burst { radius: 2 }, Hits::All, 1)
            .attached_to(state.player)
            .make(&mut state);
        let centre = goblin(&mut state, 5);
        let edge = goblin(&mut state, 7);
        // The burst goes off on the first goblin, at x = 5.
        cast_at(&mut state, blast, (7, 1));
        assert_eq!((health(&state, centre), health(&state, edge)), (7, 9));
    }

    #[test]
    fn everyone_caught_rolls_in_spawn_order_from_the_combat_stream() {
        let run = || {
            let (mut state, _) = caster();
            let blast = thing("blast")
                .spell(School::Pyromancy, 6, 1)
                .spell_damage(1, 6)
                .area(Shape::Burst { radius: 1 }, Hits::All, 0)
                .attached_to(state.player)
                .make(&mut state);
            let goblins: Vec<Entity> = (4..=6).map(|x| goblin(&mut state, x)).collect();
            let _ = state.world.insert_one(goblins[0], Position { x: 5, y: 2 });
            cast_at(&mut state, blast, (5, 1));
            goblins
                .iter()
                .map(|&goblin| health(&state, goblin))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    const LIFETIME: u64 = 3000;

    /// `caster()` plus a pyromancy spell calling `count` spirit wolves.
    fn summoner(count: u32) -> (GameState, Entity) {
        let (mut state, _) = caster();
        let wolf = state.blueprints.id("SpiritWolf").unwrap();
        let call = thing("summon wolf")
            .spell(School::Pyromancy, 5, 1)
            .with(Summon {
                blueprint: wolf,
                count,
                lifetime: LIFETIME,
            })
            .attached_to(state.player)
            .make(&mut state);
        (state, call)
    }

    fn summons_of(state: &GameState, owner: Entity) -> Vec<Entity> {
        state
            .world
            .query::<(Entity, &Summoned)>()
            .iter()
            .filter(|&(_, summoned)| summoned.owner == owner)
            .map(|(entity, _)| entity)
            .collect()
    }

    fn binding(state: &GameState, summon: Entity) -> Entity {
        holding::attached(&state.world, summon)
            .into_iter()
            .find(|&effect| state.world.satisfies::<&Binding>(effect))
            .unwrap()
    }

    fn position_of(state: &GameState, entity: Entity) -> (i32, i32) {
        ranged::position(state, entity).unwrap()
    }

    #[test]
    fn a_summoning_calls_creatures_beside_the_spot_on_the_casters_side() {
        let (mut state, call) = summoner(2);
        let player = state.player;
        cast_at(&mut state, call, (4, 4));
        let wolves = summons_of(&state, player);
        assert_eq!(wolves.len(), 2);
        assert_eq!(
            position_of(&state, wolves[0]),
            (4, 4),
            "the spot itself first"
        );
        assert_eq!(ranged::distance((4, 4), position_of(&state, wolves[1])), 1);
        assert!(!side::enemies(&state.world, player, wolves[0]));
        assert_eq!(last_log(&state), "The spirit wolf appears.");
        let held = *state
            .world
            .get::<&Timed>(binding(&state, wolves[0]))
            .unwrap();
        assert_eq!(held.duration, LIFETIME);
    }

    #[test]
    fn summons_skip_taken_ground() {
        let (mut state, call) = summoner(1);
        let player = state.player;
        goblin(&mut state, 4);
        cast_at(&mut state, call, (4, 1));
        let wolf = summons_of(&state, player)[0];
        assert_ne!(position_of(&state, wolf), (4, 1));
        assert_eq!(ranged::distance((4, 1), position_of(&state, wolf)), 1);
    }

    #[test]
    fn where_summons_appear_is_seeded() {
        let run = || {
            let (mut state, call) = summoner(3);
            cast_at(&mut state, call, (6, 6));
            let player = state.player;
            summons_of(&state, player)
                .into_iter()
                .map(|wolf| position_of(&state, wolf))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_summon_is_dismissed_when_its_binding_runs_out_unless_extended() {
        let (mut state, call) = summoner(2);
        let player = state.player;
        cast_at(&mut state, call, (4, 4));
        let wolves = summons_of(&state, player);
        let lasting = binding(&state, wolves[1]);
        state.run(Command::ModifyDuration {
            effect: lasting,
            change: crate::effect::DurationChange::AddTicks(1000),
        });

        state.now = state.now.after(LIFETIME);
        let fading = binding(&state, wolves[0]);
        state.run(Command::ExpireEffect(fading));
        state.run(Command::ExpireEffect(lasting));
        assert!(!state.world.contains(wolves[0]));
        assert!(state.world.contains(wolves[1]), "extended past now");
        assert_eq!(last_log(&state), "The spirit wolf fades away.");
    }

    #[test]
    fn summons_leave_with_their_owner() {
        let (mut state, _) = caster();
        let shaman = goblin(&mut state, 6);
        let imp = thing("imp")
            .at(7, 1)
            .health(3)
            .summoned_by(shaman)
            .make(&mut state);
        state.run(Command::Die { entity: shaman });
        assert!(!state.world.contains(imp));
        assert_eq!(last_log(&state), "The imp fades away.");
    }

    #[test]
    fn a_summon_limit_refuses_a_cast_that_would_go_over() {
        let (mut state, call) = summoner(2);
        let player = state.player;
        let _ = state.world.insert_one(player, SummonLimit { max: 3 });
        assert_eq!(check_cast(&state, player, call, (4, 4)), Ok(()));
        cast_at(&mut state, call, (4, 4));
        let _ = state.world.remove_one::<Recharging>(call);
        assert_eq!(
            check_cast(&state, player, call, (6, 6)),
            Err("You can't hold any more summons.".to_string())
        );
    }

    #[test]
    fn a_summoning_is_placed_not_thrown_and_needs_a_visible_spot() {
        let (mut state, call) = summoner(1);
        let player = state.player;
        goblin(&mut state, 2);
        state.visible.set(4, 4, false);
        assert_eq!(
            check_cast(&state, player, call, (4, 4)),
            Err("You can't see that spot.".to_string())
        );
        cast_at(&mut state, call, (4, 1));
        let wolf = summons_of(&state, player)[0];
        assert_eq!(
            position_of(&state, wolf),
            (4, 1),
            "not stopped by the goblin"
        );
    }
}

#[cfg(test)]
mod paying_tests {
    use super::*;
    use crate::entity::{Armor, Health};
    use crate::state::tests::bare_state;
    use crate::testkit::{see_everything, thing};

    fn health(state: &GameState, who: Entity) -> i32 {
        state.world.get::<&Health>(who).unwrap().current
    }

    fn goblin(state: &mut GameState, x: i32) -> Entity {
        thing("goblin").at(x, 1).health(10).hostile().make(state)
    }

    fn cast_at(state: &mut GameState, spell: Entity, at: (i32, i32)) {
        let player = state.player;
        state.run(Command::Cast {
            actor: player,
            spell,
            at,
        });
    }

    /// The player with blood magic for Hemomancy and a power-2 bolt of it.
    fn blood_mage() -> (GameState, Entity) {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            BloodCasting {
                school: School::Hemomancy,
                hp_per_power: 2,
                stat: Stat::Willpower,
            },
        );
        let bolt = thing("blood bolt")
            .spell(School::Hemomancy, 6, 2)
            .spell_damage(5, 5)
            .attached_to(player)
            .make(&mut state);
        (state, bolt)
    }

    #[test]
    fn blood_magic_costs_health_and_refuses_what_would_kill() {
        let (mut state, bolt) = blood_mage();
        let player = state.player;
        let target = goblin(&mut state, 3);
        let before = health(&state, player);
        cast_at(&mut state, bolt, (3, 1));
        assert_eq!(health(&state, player), before - 4);
        assert_eq!(health(&state, target), 5);

        let _ = state.world.insert_one(
            player,
            Health {
                current: 4,
                max: 20,
            },
        );
        assert_eq!(
            check_cast(&state, player, bolt, (3, 1)),
            Err("That would kill you.".to_string())
        );
    }

    /// A carried scroll of a power-4 pyromancy burst.
    fn scroll(state: &mut GameState) -> Entity {
        let player = state.player;
        thing("scroll of fireball")
            .item()
            .spell(School::Pyromancy, 6, 4)
            .spell_damage(3, 3)
            .with(Scroll)
            .carried_by(player)
            .make(state)
    }

    fn learn(state: &mut GameState, levels: &[(&str, u32)]) {
        let book = state.blueprints.skills();
        let mut learned = crate::entity::Skills::default();
        for &(name, level) in levels {
            learned.set(book.id(name).unwrap(), skill::xp_for(level));
        }
        let player = state.player;
        state.world.insert_one(player, learned).unwrap();
    }

    fn price(state: &GameState, spell: Entity) -> u64 {
        ready_cast(state, state.player, spell).unwrap().costs[0].amount
    }

    #[test]
    fn discipline_skills_ease_their_price_but_never_below_one() {
        let (mut state, _, missile) = super::tests::mage();
        assert_eq!(price(&state, missile), 3);
        learn(&mut state, &[("Channeling", 8)]);
        assert_eq!(price(&state, missile), 1);
        learn(&mut state, &[("Channeling", 20)]);
        assert_eq!(price(&state, missile), 1);

        let (mut state, bolt) = blood_mage();
        assert_eq!(price(&state, bolt), 4);
        learn(&mut state, &[("Bloodletting", 4)]);
        assert_eq!(price(&state, bolt), 3);
        let player = state.player;
        let before = health(&state, player);
        goblin(&mut state, 3);
        cast_at(&mut state, bolt, (3, 1));
        assert_eq!(
            health(&state, player),
            before - 3,
            "the eased price is paid"
        );
    }

    #[test]
    fn scroll_lore_steadies_a_scroll() {
        let mut state = bare_state();
        let player = state.player;
        let fizzle = Fizzle {
            base_bp: 3_000,
            per_armor_bp: 0,
            per_modifier_bp: 0,
            stat: Stat::Willpower,
        };
        state.world.insert_one(player, fizzle).unwrap();
        let scroll = scroll(&mut state);
        assert_eq!(ready_cast(&state, player, scroll).unwrap().fizzle_bp, 3_000);
        learn(&mut state, &[("Scroll lore", 5)]);
        assert_eq!(ready_cast(&state, player, scroll).unwrap().fizzle_bp, 2_000);
    }

    #[test]
    fn paying_for_a_cast_that_counts_trains_the_discipline() {
        let (mut state, _, missile) = super::tests::mage();
        let player = state.player;
        state
            .world
            .insert_one(player, crate::entity::Learner)
            .unwrap();
        let channeling = state.blueprints.skills().id("Channeling").unwrap();
        cast_at(&mut state, missile, (6, 1));
        assert_eq!(
            skill::xp(&state.world, player, channeling),
            0,
            "nobody caught"
        );
        goblin(&mut state, 3);
        cast_at(&mut state, missile, (3, 1));
        assert_eq!(skill::xp(&state.world, player, channeling), skill::BASE_XP);
    }

    #[test]
    fn a_scroll_needs_no_discipline_and_is_used_up() {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        let scroll = scroll(&mut state);
        let target = goblin(&mut state, 3);
        assert_eq!(check_cast(&state, player, scroll, (3, 1)), Ok(()));
        cast_at(&mut state, scroll, (3, 1));
        assert_eq!(health(&state, target), 7);
        assert!(!state.world.contains(scroll));
        assert!(
            state
                .log
                .recent(5)
                .any(|line| line == "The scroll of fireball crumbles to dust.")
        );
    }

    #[test]
    fn a_casters_discipline_never_pays_for_a_scroll() {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            CooldownCasting {
                school: School::Pyromancy,
                ticks_per_power: 300,
                stat: Stat::Willpower,
            },
        );
        let scroll = scroll(&mut state);
        let listed = crate::view::spells(&state);
        assert_eq!(listed[0].preview.costs.len(), 1);
        assert_eq!(listed[0].preview.costs[0].resource, "scroll");
        cast_at(&mut state, scroll, (3, 1));
        assert!(state.commands.is_empty());
        assert!(
            !state.world.query::<&Recharging>().iter().any(|_| true),
            "nothing recharges"
        );
    }

    #[test]
    fn a_scroll_on_the_floor_or_in_someone_elses_pack_cannot_be_read() {
        let mut state = bare_state();
        let player = state.player;
        let lying = thing("scroll")
            .item()
            .spell(School::Pyromancy, 6, 1)
            .spell_damage(1, 1)
            .with(Scroll)
            .at(2, 1)
            .make(&mut state);
        assert_eq!(
            check_cast(&state, player, lying, (3, 1)),
            Err("You don't know that spell.".to_string())
        );
    }

    #[test]
    fn a_worn_focus_changes_the_cast_and_a_carried_one_does_nothing() {
        let (mut state, bolt) = blood_mage();
        let player = state.player;
        let focus = Focus {
            school: School::Hemomancy,
            range: 2,
            power: -1,
            damage: 1,
        };
        let worn = thing("bone circlet")
            .item()
            .with(focus)
            .equipped_on(player, 0)
            .make(&mut state);
        let ready = ready_cast(&state, player, bolt).unwrap();
        assert_eq!((ready.range, ready.power), (8, 1));
        assert_eq!(ready.strike.map(Strike::range), Some((6, 6)));
        assert_eq!(ready.costs[0].amount, 2, "one power at 2 HP each");

        crate::equipment::unequip(&mut state, player, worn);
        let ready = ready_cast(&state, player, bolt).unwrap();
        assert_eq!((ready.range, ready.power), (6, 2));
    }

    fn fizzling(state: &mut GameState) {
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            Fizzle {
                base_bp: 0,
                per_armor_bp: 2_500,
                per_modifier_bp: 0,
                stat: Stat::Willpower,
            },
        );
    }

    #[test]
    fn armor_makes_casting_fizzle_and_a_fizzle_still_costs() {
        let (mut state, bolt) = blood_mage();
        let player = state.player;
        fizzling(&mut state);
        assert_eq!(ready_cast(&state, player, bolt).unwrap().fizzle_bp, 0);
        let _ = state.world.insert_one(player, Armor::uniform(2));
        // 3 kinds × 2 points × 25%: certain.
        assert_eq!(ready_cast(&state, player, bolt).unwrap().fizzle_bp, 10_000);
        assert_eq!(crate::view::spells(&state)[0].preview.fizzle_bp, 10_000);

        let target = goblin(&mut state, 3);
        let before = health(&state, player);
        cast_at(&mut state, bolt, (3, 1));
        assert!(
            state
                .log
                .recent(3)
                .any(|line| line == "Your blood bolt fizzles.")
        );
        assert_eq!(health(&state, target), 10);
        assert_eq!(health(&state, player), before - 4, "paid all the same");
    }

    #[test]
    fn without_a_chance_to_fizzle_the_spell_stream_is_untouched() {
        let (mut state, bolt) = blood_mage();
        fizzling(&mut state);
        goblin(&mut state, 3);
        let before = state.rngs.spell.clone();
        cast_at(&mut state, bolt, (3, 1));
        assert_eq!(state.rngs.spell, before);
    }
}

#[cfg(test)]
mod complex_tests {
    use super::*;
    use crate::entity::SpellAi;
    use crate::state::tests::bare_state;
    use crate::testkit::{see_everything, thing};

    fn health(state: &GameState, who: Entity) -> i32 {
        state.world.get::<&Health>(who).unwrap().current
    }

    fn creature(state: &mut GameState, at: (i32, i32), hostile: bool) -> Entity {
        let spawn = thing("goblin").at(at.0, at.1).health(10);
        if hostile { spawn.hostile() } else { spawn }.make(state)
    }

    fn cast_at(state: &mut GameState, spell: Entity, at: (i32, i32)) {
        let player = state.player;
        state.run(Command::Cast {
            actor: player,
            spell,
            at,
        });
    }

    /// The player with a cooldown discipline for pyromancy and `spell`.
    fn with_spell(spell: crate::spawn::Spawn) -> (GameState, Entity) {
        let mut state = bare_state();
        see_everything(&mut state);
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            CooldownCasting {
                school: School::Pyromancy,
                ticks_per_power: 100,
                stat: Stat::Willpower,
            },
        );
        let spell = spell.attached_to(player).make(&mut state);
        (state, spell)
    }

    fn chain_bolt() -> crate::spawn::Spawn {
        thing("chain lightning")
            .spell(School::Pyromancy, 6, 1)
            .spell_damage(3, 3)
            .with(Chain { jumps: 2, reach: 3 })
    }

    #[test]
    fn a_chain_leaps_to_the_nearest_enemies_and_spares_friends() {
        let (mut state, chain) = with_spell(chain_bolt());
        let first = creature(&mut state, (3, 1), true);
        let friend = creature(&mut state, (4, 1), false);
        let second = creature(&mut state, (5, 2), true);
        let far = creature(&mut state, (12, 1), true);
        cast_at(&mut state, chain, (3, 1));
        assert_eq!(
            (
                health(&state, first),
                health(&state, friend),
                health(&state, second),
                health(&state, far),
            ),
            (7, 10, 7, 10),
            "two leaps were allowed but only one enemy is within reach"
        );
    }

    #[test]
    fn a_chain_never_hits_the_same_creature_twice() {
        let (mut state, chain) = with_spell(chain_bolt());
        let only = creature(&mut state, (3, 1), true);
        cast_at(&mut state, chain, (3, 1));
        assert_eq!(health(&state, only), 7);
    }

    fn rune_spell() -> crate::spawn::Spawn {
        thing("fire rune")
            .spell(School::Pyromancy, 6, 1)
            .spell_damage(4, 4)
            .area(Shape::Burst { radius: 1 }, Hits::All, 0)
            .with(Delay { ticks: 300 })
    }

    fn rune(state: &GameState) -> Entity {
        state
            .world
            .query::<(Entity, &Blast)>()
            .iter()
            .map(|(entity, _)| entity)
            .next()
            .unwrap()
    }

    #[test]
    fn a_delayed_spell_leaves_a_rune_that_bursts_on_who_is_there_then() {
        let (mut state, spell) = with_spell(rune_spell());
        let early = creature(&mut state, (5, 2), true);
        cast_at(&mut state, spell, (5, 1));
        assert_eq!(health(&state, early), 10, "nothing happens yet");
        let rune = rune(&state);
        assert_eq!(ranged::position(&state, rune), Some((5, 1)));

        let _ = state.world.insert_one(early, Position { x: 9, y: 9 });
        let late = creature(&mut state, (6, 1), true);
        state.now = state.now.after(300);
        state.run(Command::ExpireEffect(rune));
        assert!(!state.world.contains(rune));
        assert_eq!((health(&state, early), health(&state, late)), (10, 6));
    }

    #[test]
    fn a_rune_waits_for_its_timestamps_and_can_be_delayed() {
        let (mut state, spell) = with_spell(rune_spell());
        let victim = creature(&mut state, (5, 2), true);
        cast_at(&mut state, spell, (5, 1));
        let rune = rune(&state);
        state.run(Command::ModifyDuration {
            effect: rune,
            change: crate::effect::DurationChange::AddTicks(200),
        });
        state.now = state.now.after(300);
        state.run(Command::ExpireEffect(rune));
        assert!(state.world.contains(rune), "not due yet");
        assert_eq!(health(&state, victim), 10);
        state.now = state.now.after(200);
        state.run(Command::ExpireEffect(rune));
        assert_eq!(health(&state, victim), 6);
    }

    /// A goblin shaman at `at` with blood magic and a blood bolt.
    fn shaman(state: &mut GameState, at: (i32, i32), health: i32) -> Entity {
        let shaman = thing("goblin shaman")
            .at(at.0, at.1)
            .health(health)
            .hostile()
            .with(BloodCasting {
                school: School::Hemomancy,
                hp_per_power: 1,
                stat: Stat::Willpower,
            })
            .with(SpellAi)
            .make(state);
        thing("blood bolt")
            .spell(School::Hemomancy, 5, 2)
            .spell_damage(2, 4)
            .attached_to(shaman)
            .make(state);
        shaman
    }

    #[test]
    fn a_shaman_casts_at_the_player_in_view_and_range() {
        let mut state = bare_state();
        see_everything(&mut state);
        let shaman = shaman(&mut state, (4, 1), 10);
        let command = crate::ai::decide(&state, shaman);
        assert!(
            matches!(command, Command::Cast { actor, at: (1, 1), .. } if actor == shaman),
            "{command:?}"
        );
        state.run(command);
        assert_eq!(health(&state, shaman), 8, "two HP of blood");
        assert!(health(&state, state.player) < 20);
    }

    #[test]
    fn a_shaman_too_weak_to_bleed_fights_by_hand() {
        let mut state = bare_state();
        see_everything(&mut state);
        let shaman = shaman(&mut state, (4, 1), 2);
        assert_eq!(
            crate::ai::decide(&state, shaman),
            Command::TryMove(shaman, crate::input::Dir::W)
        );
    }

    #[test]
    fn a_shaman_will_not_bolt_through_its_own_side() {
        let mut state = bare_state();
        see_everything(&mut state);
        let shaman = shaman(&mut state, (4, 1), 10);
        creature(&mut state, (3, 1), true);
        assert!(!matches!(
            crate::ai::decide(&state, shaman),
            Command::Cast { .. }
        ));
    }
}
