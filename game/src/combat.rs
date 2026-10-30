//! Combat events and handlers, plus the shared rolls used by both the real
//! command applier and the side-effect-free `preview_damage`.

use std::ops::ControlFlow;

use hecs::{Entity, World};
use rand::RngExt;
use rand_pcg::Pcg64;

use crate::command::{Command, DamageCause, dispatch};
use crate::effect::BASIS_POINTS;
use crate::entity::{
    Armor, DamageType, DamageTypes, Deflect, Footwork, Graze, Health, Launcher, Melee, Natural,
    OnHit, Projectile, Stat, Stats, Throwable, Weapon,
};
use crate::equipment::equipped;
use crate::event::{Ctx, EventRegistry, settle};
use crate::fx::{self, Fx};
use crate::grammar::{Subject, capitalize};
use crate::prio;
use crate::side;
use crate::skill::{self, Launch, SkillBook, SkillId};
use crate::state::GameState;

/// Fired at a target before damage is applied. Handlers may rewrite `amount`
/// and add the resistance they provide to `armor`; `dealt` combines them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BeforeTakeDamage {
    pub target: Entity,
    pub amount: i32,
    pub armor: Armor,
    pub cause: DamageCause,
}

impl BeforeTakeDamage {
    /// The event for `amount` of `cause` at `target`, leaned by the current
    /// phase first (docs/plans/phases.md). The one place damage enters, so
    /// real hits and `preview_damage` lean alike.
    pub fn new(state: &GameState, target: Entity, amount: i32, cause: DamageCause) -> Self {
        Self {
            target,
            amount: crate::phase::leaned(state, amount, cause.types()),
            armor: Armor::default(),
            cause,
        }
    }

    /// What gets through: `amount` less the gathered armor against this
    /// blow's kinds, never negative. The one place that sum is made, so real
    /// hits and `preview_damage` agree.
    pub fn dealt(&self) -> i32 {
        (self.amount - self.armor.against(self.cause.types())).max(0)
    }
}

/// Notification fired after damage has been applied to `Health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Damaged {
    pub target: Entity,
    pub amount: i32,
}

/// Fired at the attacker (and so its equipment) after one of its melee hits
/// dealt more than 0 damage. Effect damage never fires it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AfterHit {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: i32,
    /// Only a graze: on-hit effects apply only if meant for grazes.
    pub graze: bool,
}

pub(crate) fn register(registry: &mut EventRegistry) {
    registry.on(prio::ARMOR, armor_reduces_damage);
    registry.on(prio::ON_HIT, on_hit_applies_effects);
    registry.on(prio::FOOTWORK, footwork_steadies_dodges);
}

/// Rolls every entry on the combat stream, even at 0 or 10 000 bp, so the
/// number of draws per hit doesn't depend on the chances.
/// A graze still rolls every entry but applies only those `on_graze`.
fn on_hit_applies_effects(
    ctx: &mut Ctx<'_>,
    on_hit: &OnHit,
    ev: AfterHit,
) -> ControlFlow<AfterHit, AfterHit> {
    for entry in &on_hit.0 {
        let rolled = ctx.rng.random_range(0..BASIS_POINTS) < u64::from(entry.chance_bp);
        if rolled && (entry.on_graze || !ev.graze) {
            ctx.out.push(Command::ApplyEffect {
                target: ev.target,
                blueprint: entry.effect,
            });
        }
    }
    ControlFlow::Continue(ev)
}

/// A defender with `Footwork` dodges perfectly more often.
fn footwork_steadies_dodges(
    _ctx: &mut Ctx<'_>,
    footwork: &Footwork,
    mut ev: AttackOdds,
) -> ControlFlow<AttackOdds, AttackOdds> {
    if ev.asking == ev.defender {
        ev.perfect_bp = ev.perfect_bp.saturating_add(footwork.perfect_bp);
    }
    ControlFlow::Continue(ev)
}

/// Each worn piece adds its resistance; the total is applied once, per kind,
/// in `BeforeTakeDamage::dealt`. Effect damage such as bleeding is of no kind,
/// so it passes: armor stops blows, not what is already inside.
fn armor_reduces_damage(
    _ctx: &mut Ctx<'_>,
    armor: &Armor,
    mut ev: BeforeTakeDamage,
) -> ControlFlow<BeforeTakeDamage, BeforeTakeDamage> {
    ev.armor = ev.armor.plus(*armor);
    ControlFlow::Continue(ev)
}

/// How `attacker` hits in melee: the dice and the stat bonus added to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strike {
    pub min: i32,
    pub max: i32,
    pub bonus: i32,
    pub types: DamageTypes,
    /// The equipped weapon used (not for natural `Melee`), or the missile.
    pub weapon: Option<Entity>,
    /// The attacker's effective skill level at it (ADR 0017).
    pub level: u32,
    /// What its hit chance is reckoned from: `level` plus the stat modifier.
    pub accuracy: i32,
    /// A shot's launcher holds steady this many tiles; each tile beyond takes
    /// `FALLOFF_BP` off the hit chance. `None` for blows, throws and spells.
    pub steady: Option<i32>,
    /// Shot with an enemy beside the shooter: a bow wants room to draw.
    pub point_blank: bool,
    /// Drawn on the move: shot soon after another, with a step between.
    pub snap: bool,
}

impl Strike {
    /// Lowest and highest damage before armor; never negative.
    pub fn range(self) -> (i32, i32) {
        let max = self.max.max(self.min);
        ((self.min + self.bonus).max(0), (max + self.bonus).max(0))
    }
}

/// The equipped `Weapon` on the lowest-numbered body part, else natural
/// `Melee` (a Strength, blunt weapon); `None` if `attacker` has neither. The
/// wielder's `Stats` modifier for the weapon's stat, plus the damage bonus of
/// their skill level at this strike (ADR 0017), is the bonus.
pub(crate) fn strike(world: &World, skills: &SkillBook, attacker: Entity) -> Option<Strike> {
    blow_strike(world, skills, attacker, &BlowMods::default()).map(|(strike, _)| strike)
}

/// `strike` shifted by `mods`, with the skills it uses: the weapon's
/// (`skill::melee`), or `mods.uses` instead. An unarmed blow skips what is
/// wielded for the natural `Melee`.
pub(crate) fn blow_strike(
    world: &World,
    skills: &SkillBook,
    attacker: Entity,
    mods: &BlowMods,
) -> Option<(Strike, Vec<SkillId>)> {
    let mut worn = equipped(world, attacker);
    worn.sort_by_key(|&(_, part)| part);
    let wielded = worn
        .into_iter()
        .filter(|_| !mods.unarmed)
        .find_map(|(item, _)| {
            let weapon = *world.get::<&Weapon>(item).ok()?;
            Some((
                weapon.min,
                weapon.max,
                weapon.stat,
                weapon.types,
                Some(item),
            ))
        });
    let natural = || {
        let melee = *world.get::<&Melee>(attacker).ok()?;
        let types = world
            .get::<&Natural>(attacker)
            .map_or(DamageTypes::single(DamageType::Blunt), |natural| natural.0);
        Some((melee.min, melee.max, Stat::Strength, types, None))
    };
    let (min, max, stat, types, weapon) = wielded.or_else(natural)?;
    let bonus = world
        .get::<&Stats>(attacker)
        .map_or(0, |stats| stats.modifier(stat));
    let strike = Strike {
        min,
        max,
        bonus,
        types,
        weapon,
        level: 0,
        accuracy: 0,
        steady: None,
        point_blank: false,
        snap: false,
    };
    let used = match mods.uses.as_deref() {
        Some(uses) => uses.to_vec(),
        None => skill::melee(world, skills, attacker, &strike),
    };
    let mut strike = skilled(world, skills, attacker, strike, &used);
    strike.accuracy += mods.accuracy;
    Some((strike, used))
}

/// How a technique shifts a blow (ADR 0024). The default is a plain bump.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BlowMods {
    pub accuracy: i32,
    /// Percent added to the damage rolled; negative takes it away.
    pub damage_pct: i32,
    /// The skills the blow uses, for its level and its training, instead of
    /// the weapon's: a blow that is not plain weapon work.
    pub uses: Option<Vec<SkillId>>,
    /// Percent more the attacker learns from it.
    pub xp_pct: u32,
    /// Struck with the body, whatever is wielded (`Blow::unarmed`).
    pub unarmed: bool,
}

impl BlowMods {
    /// `amount` of damage after `damage_pct`; never negative.
    pub(crate) fn scale(&self, amount: i32) -> i32 {
        (amount * (100 + self.damage_pct) / 100).max(0)
    }
}

/// One blow from `attacker` at `target`: the odds, how it lands, the damage,
/// and what both sides learn from it. A bump is a blow with no mods. `None`
/// if `attacker` has nothing to strike with.
pub(crate) fn blow(
    state: &mut GameState,
    attacker: Entity,
    target: Entity,
    mods: &BlowMods,
) -> Option<Outcome> {
    let (strike, used) = blow_strike(&state.world, state.blueprints.skills(), attacker, mods)?;
    let (odds, reasons) = odds_explained(state, &strike, attacker, target);
    let outcome = resolve(&mut state.rngs.combat, &odds);
    announce(state, attacker, target, outcome, &explain(&odds, &reasons));
    if let Outcome::Hit | Outcome::Graze = outcome {
        let hit = roll_strike(&mut state.rngs.combat, strike);
        let amount = mods.scale(hit.amount);
        let graze = outcome == Outcome::Graze;
        state.commands.push_back(Command::DealDamage {
            target,
            amount: if graze { amount / 2 } else { amount },
            cause: DamageCause::Melee {
                attacker,
                types: hit.types,
                graze,
            },
        });
    }
    // Only a fight teaches: hitting a friend or a summon of your own does not.
    if side::enemies(&state.world, attacker, target) {
        skill::after_blow(state, &used, &odds, outcome, mods.xp_pct);
    }
    Some(outcome)
}

/// One melee hit before armor: how much, and of which kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hit {
    pub amount: i32,
    pub types: DamageTypes,
}

/// Raw damage of one of `attacker`'s melee hits (see `strike`), or `None`
/// if it can't attack. Always exactly one draw from `rng`.
pub(crate) fn roll_melee(
    world: &World,
    skills: &SkillBook,
    rng: &mut Pcg64,
    attacker: Entity,
) -> Option<Hit> {
    Some(roll_strike(rng, strike(world, skills, attacker)?))
}

/// One draw from `rng` for `strike`.
pub(crate) fn roll_strike(rng: &mut Pcg64, strike: Strike) -> Hit {
    let roll = rng.random_range(strike.min..=strike.max.max(strike.min));
    Hit {
        amount: (roll + strike.bonus).max(0),
        types: strike.types,
    }
}

/// A certain chance, in basis points.
pub(crate) const CERTAIN: u32 = 10_000;
/// Hit chance between equals, and per point of accuracy over defence.
const EVEN_HIT_BP: i32 = 7_500;
const HIT_BP_PER_POINT: i32 = 400;
/// Hit chance never goes past these, so no blow is hopeless or sure.
const MIN_HIT_BP: i32 = 500;
/// Added to the hit chance against a defender for each enemy beside it
/// beyond the first (docs/plans/fights.md): corridors and corners matter.
const OUTNUMBERED_BP: i32 = 1_000;
/// Taken off a shot's hit chance for each tile beyond its launcher's
/// `steady` range (docs/plans/fights.md).
const FALLOFF_BP: i32 = 500;
/// Taken off a snap shot: an arrow drawn and set on the move.
const SNAP_BP: i32 = 2_000;
const MAX_HIT_BP: i32 = 9_500;
/// How often a miss grazes, for a weapon without its own `Graze`.
pub(crate) const DEFAULT_GRAZE_BP: u32 = 3_000;
/// Perfect-dodge and deflect chance per level of Poise and Deflection.
const BP_PER_DEFENCE_LEVEL: u32 = 100;

/// The chances of each way a blow can land, in basis points (ADR 0017).
/// Gathered by firing it at the attacker, then the defender, so parts can
/// adjust them (an ability that makes every dodge perfect sets
/// `perfect_bp` to `CERTAIN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttackOdds {
    pub attacker: Entity,
    pub defender: Entity,
    /// Which of the two is being asked now, so a part on either knows
    /// whether its holder gives the blow or takes it.
    pub asking: Entity,
    pub hit_bp: u32,
    /// Asked only after a miss: dodged so cleanly that no graze is rolled.
    pub perfect_bp: u32,
    /// Asked only after a miss that wasn't a perfect dodge.
    pub graze_bp: u32,
    /// Asked only of a graze, and only with a `deflector`.
    pub deflect_bp: u32,
    /// The defender's worn item that would turn a graze aside.
    pub deflector: Option<Entity>,
}

impl AttackOdds {
    /// The chance, out of `CERTAIN`, that the blow ends as a graze: it
    /// misses, isn't dodged perfectly, grazes, and isn't deflected.
    pub(crate) fn graze_overall_bp(&self) -> u32 {
        let not = |bp: u32| u64::from(CERTAIN - bp.min(CERTAIN));
        let deflect = if self.deflector.is_some() {
            self.deflect_bp
        } else {
            0
        };
        let chance = not(self.hit_bp)
            * not(self.perfect_bp)
            * u64::from(self.graze_bp.min(CERTAIN))
            * not(deflect)
            / u64::from(CERTAIN).pow(3);
        u32::try_from(chance).unwrap_or(CERTAIN)
    }
}

/// How a blow resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Hit,
    /// Half damage, and only on-hit effects meant for grazes.
    Graze,
    /// A graze turned aside by this worn item: nothing gets through.
    Deflected(Entity),
    Miss,
    /// No graze is even rolled.
    PerfectDodge,
}

/// Fired at the defender after a perfect dodge, for parts that react to it
/// (a riposte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PerfectDodge {
    pub attacker: Entity,
    pub defender: Entity,
}

/// Fired at the defender after `by` turned a graze aside, for parts that
/// react to it (a shield bash, wear on the item).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deflected {
    pub attacker: Entity,
    pub defender: Entity,
    pub by: Entity,
}

/// The odds of `strike` against `defender`. The attacker's accuracy against
/// the defender's Dodge and Agility gives the hit chance; the weapon, the
/// graze chance; Poise, the perfect-dodge chance; the best `Deflect` worn
/// plus Deflection, the deflect chance. Then parts adjust them, on a cloned
/// RNG with their commands discarded, so asking changes nothing.
pub(crate) fn odds(
    state: &GameState,
    strike: &Strike,
    attacker: Entity,
    defender: Entity,
) -> AttackOdds {
    odds_explained(state, strike, attacker, defender).0
}

/// One named part of a hit chance (docs/plans/legibility.md): what added it
/// and how much, in basis points. A chance's reasons add up to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub what: String,
    pub bp: i32,
}

/// `odds`, with the hit chance's reasons: the base (accuracy against
/// dodge), being outnumbered, each part that moved it (by its name), and the
/// limit when it clamps.
pub(crate) fn odds_explained(
    state: &GameState,
    strike: &Strike,
    attacker: Entity,
    defender: Entity,
) -> (AttackOdds, Vec<Reason>) {
    let world = &state.world;
    let core = state.blueprints.skills().core();
    let level =
        |skill: Option<SkillId>| skill.map_or(0, |id| skill::level(skill::xp(world, defender, id)));
    let dodge = i32::try_from(level(core.map(|c| c.dodge))).unwrap_or(0)
        + world
            .get::<&Stats>(defender)
            .map_or(0, |stats| stats.modifier(Stat::Agility));
    let crowd = i32::try_from(outnumbered(state, defender)).unwrap_or(0);
    let base = EVEN_HIT_BP + HIT_BP_PER_POINT * (strike.accuracy - dodge);
    let beyond = strike.steady.map_or(0, |steady| {
        let at = |who: Entity| {
            world
                .get::<&crate::entity::Position>(who)
                .ok()
                .map(|p| (p.x, p.y))
        };
        at(attacker)
            .zip(at(defender))
            .map_or(0, |(a, d)| (crate::ranged::distance(a, d) - steady).max(0))
    });
    let point_blank = if strike.point_blank {
        -HIT_BP_PER_POINT * crate::ranged::POINT_BLANK
    } else {
        0
    };
    let snap = if strike.snap { -SNAP_BP } else { 0 };
    // The phase's RangedFalloff, the one place it is read: a missile (one
    // with a steady range) loses it for every tile it flies.
    let phase_falloff = match strike.steady {
        None => 0,
        Some(_) => {
            let pct = crate::phase::rule(state, |rule| match rule {
                crate::phase::Rule::RangedFalloff(pct) => Some(pct),
                _ => None,
            });
            let at = |who: Entity| {
                world
                    .get::<&crate::entity::Position>(who)
                    .ok()
                    .map(|p| (p.x, p.y))
            };
            let tiles = at(attacker)
                .zip(at(defender))
                .map_or(0, |(a, d)| crate::ranged::distance(a, d));
            -i32::try_from(pct).unwrap_or(0) * 100 * tiles
        }
    };
    let raw =
        base + OUTNUMBERED_BP * crowd - FALLOFF_BP * beyond + point_blank + snap + phase_falloff;
    let hit_bp = raw.clamp(MIN_HIT_BP, MAX_HIT_BP);
    let mut reasons = vec![Reason {
        what: format!("accuracy {} against dodge {dodge}", strike.accuracy),
        bp: base,
    }];
    if crowd > 0 {
        reasons.push(Reason {
            what: format!("outnumbered by {crowd}"),
            bp: OUTNUMBERED_BP * crowd,
        });
    }
    if point_blank != 0 {
        reasons.push(Reason {
            what: "point blank".to_string(),
            bp: point_blank,
        });
    }
    if snap != 0 {
        reasons.push(Reason {
            what: "drawn on the move".to_string(),
            bp: snap,
        });
    }
    if beyond > 0 {
        reasons.push(Reason {
            what: format!("range, {beyond} past steady"),
            bp: -FALLOFF_BP * beyond,
        });
    }
    if phase_falloff != 0 {
        let phase = crate::phase::current(state).map_or("", |phase| phase.name.as_str());
        reasons.push(Reason {
            what: format!("{phase}, over the distance"),
            bp: phase_falloff,
        });
    }
    if hit_bp != raw {
        reasons.push(Reason {
            what: "the limit".to_string(),
            bp: hit_bp - raw,
        });
    }
    let graze_bp = strike
        .weapon
        .and_then(|weapon| world.get::<&Graze>(weapon).ok().map(|graze| graze.0))
        .unwrap_or(DEFAULT_GRAZE_BP);
    let mut worn = equipped(world, defender);
    worn.sort_by_key(|&(_, part)| part);
    let deflector = worn
        .into_iter()
        .filter_map(|(item, _)| Some((world.get::<&Deflect>(item).ok()?.0, item)))
        .fold(None, |best: Option<(u32, Entity)>, (bp, item)| match best {
            Some((top, _)) if top >= bp => best,
            _ => Some((bp, item)),
        });
    let deflect_bp = deflector.map_or(0, |(bp, _)| {
        bp + BP_PER_DEFENCE_LEVEL * level(core.map(|c| c.deflection))
    });

    let mut ev = AttackOdds {
        attacker,
        defender,
        asking: attacker,
        hit_bp: u32::try_from(hit_bp).unwrap_or(0),
        perfect_bp: BP_PER_DEFENCE_LEVEL * level(core.map(|c| c.poise)),
        graze_bp,
        deflect_bp,
        deflector: deflector.map(|(_, item)| item),
    };
    let mut rng = state.rngs.combat.clone();
    let mut scratch = Vec::new();
    let mut ctx = Ctx {
        world,
        map: &state.map,
        rng: &mut rng,
        out: &mut scratch,
    };
    let mut moved: Vec<(Entity, i32)> = Vec::new();
    for asking in [attacker, defender] {
        ev.asking = asking;
        let mut trace = |holder: Entity, before: &AttackOdds, after: &AttackOdds| {
            let change = i64::from(after.hit_bp) - i64::from(before.hit_bp);
            if change != 0 {
                moved.push((holder, i32::try_from(change).unwrap_or(0)));
            }
        };
        ev = settle(state.events.fire_traced(&mut ctx, asking, ev, &mut trace));
    }
    for (holder, bp) in moved {
        let what = world
            .get::<&crate::entity::Name>(holder)
            .map_or_else(|_| "something".to_string(), |name| name.0.clone());
        reasons.push(Reason { what, bp });
    }
    (ev, reasons)
}

/// How many enemies stand beside `defender` beyond the first: each makes it
/// easier to hit. No facing, so this is the one positional rule.
pub(crate) fn outnumbered(state: &GameState, defender: Entity) -> u32 {
    use crate::entity::{Dead, Position};
    let world = &state.world;
    let Ok(at) = world.get::<&Position>(defender).map(|pos| *pos) else {
        return 0;
    };
    let beside = world
        .query::<(Entity, &Position)>()
        .with::<&Health>()
        .without::<&Dead>()
        .iter()
        .filter(|&(other, pos)| {
            other != defender
                && (pos.x - at.x).abs().max((pos.y - at.y).abs()) == 1
                && side::enemies(world, defender, other)
        })
        .count();
    u32::try_from(beside.saturating_sub(1)).unwrap_or(0)
}

/// Rolls `odds` on `rng`: hit, else perfect dodge, else graze or miss, and a
/// graze may be deflected. A chance of 0 or `CERTAIN` draws nothing, so
/// creatures without the parts involved leave the stream untouched.
pub(crate) fn resolve(rng: &mut Pcg64, odds: &AttackOdds) -> Outcome {
    let mut roll = |bp: u32| match bp {
        0 => false,
        bp if bp >= CERTAIN => true,
        bp => rng.random_range(0..BASIS_POINTS) < u64::from(bp),
    };
    if roll(odds.hit_bp) {
        return Outcome::Hit;
    }
    if roll(odds.perfect_bp) {
        return Outcome::PerfectDodge;
    }
    if !roll(odds.graze_bp) {
        return Outcome::Miss;
    }
    match odds.deflector {
        Some(item) if roll(odds.deflect_bp) => Outcome::Deflected(item),
        _ => Outcome::Graze,
    }
}

/// Logs how a blow from `attacker` at `target` resolved, and fires
/// `PerfectDodge` or `Deflected` at the target. A hit says nothing here:
/// the damage it deals is logged when dealt.
/// The why of a blow's outcome line: "75% to hit: accuracy 2 against dodge
/// 2 75, outnumbered by 1 +10".
pub(crate) fn explain(odds: &AttackOdds, reasons: &[Reason]) -> String {
    let parts: Vec<String> = reasons
        .iter()
        .enumerate()
        .map(|(i, reason)| {
            let points = reason.bp / 100;
            if i == 0 {
                format!("{} {points}", reason.what)
            } else {
                format!("{} {points:+}", reason.what)
            }
        })
        .collect();
    format!("{}% to hit: {}", odds.hit_bp / 100, parts.join(", "))
}

/// Logs a blow's outcome (a hit says nothing here; its damage line does),
/// with `why` beside the line.
pub(crate) fn announce(
    state: &mut GameState,
    attacker: Entity,
    target: Entity,
    outcome: Outcome,
    why: &str,
) {
    let shown = match outcome {
        Outcome::Hit => None,
        Outcome::Graze => Some(fx::Outcome::Graze),
        Outcome::Miss => Some(fx::Outcome::Miss),
        Outcome::PerfectDodge => Some(fx::Outcome::Dodge),
        Outcome::Deflected(_) => Some(fx::Outcome::Deflect),
    };
    if let Some(kind) = shown {
        fx::at(state, target, |at| Fx::Outcome { at, kind });
    }
    let striker = Subject::of(&state.world, attacker);
    let struck = Subject::of(&state.world, target);
    match outcome {
        Outcome::Hit => {}
        Outcome::Graze => state
            .log
            .push_with(striker.sentence("graze", struck.noun()), why),
        Outcome::Miss => state
            .log
            .push_with(striker.sentence("miss", struck.noun()), why),
        Outcome::PerfectDodge => {
            state
                .log
                .push_with(struck.sentence("dodge", "perfectly"), why);
            let _ = dispatch(
                state,
                target,
                PerfectDodge {
                    attacker,
                    defender: target,
                },
            );
        }
        Outcome::Deflected(by) => {
            let item = Subject::of(&state.world, by);
            state.log.push_with(
                capitalize(&format!(
                    "{} {} turns {} blow.",
                    struck.possessive(),
                    item.noun().trim_start_matches("the "),
                    striker.possessive()
                )),
                why,
            );
            let _ = dispatch(
                state,
                target,
                Deflected {
                    attacker,
                    defender: target,
                    by,
                },
            );
        }
    }
}

/// How `shooter` fires `arrow` from `launcher` (an entity with `spec`): the
/// ammunition's dice and kinds, plus the shooter's modifier for the
/// launcher's stat and the damage bonus of their skill at it; and the
/// arrow's chance (out of 10 000) to break on a hit. `None` if `arrow` is
/// not a projectile.
pub(crate) fn fire_strike(
    world: &World,
    skills: &SkillBook,
    shooter: Entity,
    (launcher, spec): (Entity, Launcher),
    arrow: Entity,
) -> Option<(Strike, u32)> {
    let projectile = *world.get::<&Projectile>(arrow).ok()?;
    let bonus = world
        .get::<&Stats>(shooter)
        .map_or(0, |stats| stats.modifier(spec.stat));
    let strike = Strike {
        min: projectile.min,
        max: projectile.max,
        bonus,
        types: projectile.types,
        weapon: Some(arrow),
        level: 0,
        accuracy: 0,
        steady: Some(spec.steady),
        point_blank: false,
        snap: false,
    };
    let used = skill::missile(world, skills, Launch::Fired(launcher), &strike);
    Some((
        skilled(world, skills, shooter, strike, &used),
        projectile.break_bp,
    ))
}

/// How `thrower` hurls `missile`: its `Throwable` dice plus the thrower's
/// stat modifier and the damage bonus of their skill at it. `None` if it
/// can't be thrown.
pub(crate) fn throw_strike(
    world: &World,
    skills: &SkillBook,
    thrower: Entity,
    missile: Entity,
) -> Option<Strike> {
    let throwable = *world.get::<&Throwable>(missile).ok()?;
    let bonus = world
        .get::<&Stats>(thrower)
        .map_or(0, |stats| stats.modifier(throwable.stat));
    let strike = Strike {
        min: throwable.min,
        max: throwable.max,
        bonus,
        types: throwable.types,
        weapon: Some(missile),
        level: 0,
        accuracy: 0,
        steady: None,
        point_blank: false,
        snap: false,
    };
    let used = skill::missile(world, skills, Launch::Thrown, &strike);
    Some(skilled(world, skills, thrower, strike, &used))
}

/// `strike` (its `bonus` still the bare stat modifier) as `attacker` makes
/// it with `used` skills: their effective level, the accuracy reckoned from
/// it, and its damage bonus.
fn skilled(
    world: &World,
    skills: &SkillBook,
    attacker: Entity,
    mut strike: Strike,
    used: &[SkillId],
) -> Strike {
    strike.level = skill::effective(world, skills, attacker, used);
    strike.accuracy = strike.bonus + i32::try_from(strike.level).unwrap_or(0);
    strike.bonus += skill::damage_bonus(strike.level);
    strike
}

/// Damage the next real `Attack { attacker, target }` would deal, computed on
/// a cloned RNG with commands emitted into a scratch buffer. Leaves the world,
/// RNG, log and queue untouched. `None` if the attacker has no `Melee` or the
/// target has no `Health`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "for a future look/target UI; needs an intent-level API before a frontend can use it"
    )
)]
pub(crate) fn preview_damage(state: &GameState, attacker: Entity, target: Entity) -> Option<i32> {
    if !state.world.satisfies::<&Health>(target) {
        return None;
    }
    let mut rng = state.rngs.combat.clone();
    let hit = roll_melee(&state.world, state.blueprints.skills(), &mut rng, attacker)?;
    let mut scratch = Vec::new();
    let mut ctx = Ctx {
        world: &state.world,
        map: &state.map,
        rng: &mut rng,
        out: &mut scratch,
    };
    let cause = DamageCause::Melee {
        attacker,
        types: hit.types,
        graze: false,
    };
    let before = BeforeTakeDamage::new(state, target, hit.amount, cause);
    let ev = settle(state.events.fire(&mut ctx, target, before));
    Some(ev.dealt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::tests::{BLOW, UNSOURCED};
    use crate::entity::{Bleeding, OnHitEntry};
    use crate::spawn::Spawn;
    use crate::state::MessageLog;
    use crate::state::tests::{bare_state, bare_state_seeded};
    use crate::testkit::{blank, last_log, sure_hits, thing};

    #[test]
    fn stat_modifiers_follow_qud() {
        let modifier = |strength| {
            Stats {
                strength,
                agility: 16,
                ..Stats::AVERAGE
            }
            .modifier(Stat::Strength)
        };
        let table: Vec<i32> = [12, 13, 14, 15, 16, 17, 18, 19, 20].map(modifier).to_vec();
        assert_eq!(table, [-2, -2, -1, -1, 0, 0, 1, 1, 2]);
    }

    /// A fighter with natural 2-4 `Melee` and the given scores.
    fn fighter(state: &mut GameState, strength: i32, agility: i32) -> Entity {
        blank().melee(2, 4).stats(strength, agility).make(state)
    }

    fn knife() -> Spawn {
        thing("knife").weapon(1, 3, Stat::Agility)
    }

    #[test]
    fn bare_hands_add_strength_to_natural_melee() {
        let mut state = bare_state();
        let strong = fighter(&mut state, 20, 10);
        let strike = strike(&state.world, state.blueprints.skills(), strong).unwrap();
        assert_eq!((strike.range(), strike.weapon), ((4, 6), None));
    }

    #[test]
    fn an_equipped_weapon_replaces_natural_melee_and_uses_its_stat() {
        let mut state = bare_state();
        let nimble = fighter(&mut state, 10, 20);
        let blade = knife().equipped_on(nimble, 2).make(&mut state);

        let strike = strike(&state.world, state.blueprints.skills(), nimble).unwrap();
        assert_eq!((strike.range(), strike.weapon), ((3, 5), Some(blade)));
        let mut rng = state.rngs.combat.clone();
        for _ in 0..50 {
            let damage = roll_melee(&state.world, state.blueprints.skills(), &mut rng, nimble)
                .unwrap()
                .amount;
            assert!((3..=5).contains(&damage), "{damage}");
        }
    }

    #[test]
    fn a_carried_weapon_is_not_used() {
        let mut state = bare_state();
        let fighter = fighter(&mut state, 16, 20);
        knife().carried_by(fighter).make(&mut state);
        assert_eq!(
            strike(&state.world, state.blueprints.skills(), fighter)
                .unwrap()
                .weapon,
            None
        );
    }

    #[test]
    fn a_weak_hit_never_heals() {
        let mut state = bare_state();
        let feeble = fighter(&mut state, 4, 16);
        assert_eq!(
            strike(&state.world, state.blueprints.skills(), feeble)
                .unwrap()
                .range(),
            (0, 0)
        );
        let mut rng = state.rngs.combat.clone();
        assert_eq!(
            roll_melee(&state.world, state.blueprints.skills(), &mut rng, feeble)
                .map(|hit| hit.amount),
            Some(0)
        );
    }

    fn health(state: &GameState, entity: Entity) -> i32 {
        state.world.get::<&Health>(entity).unwrap().current
    }

    fn armored_dummy(state: &mut GameState, av: i32) -> (Entity, Entity) {
        let dummy = thing("dummy").at(5, 5).health(10).make(state);
        let armor = thing("plate").armor(av).attached_to(dummy).make(state);
        (dummy, armor)
    }

    fn bleed_on_hit(state: &GameState, chance_bp: u32) -> OnHit {
        OnHit(vec![OnHitEntry {
            effect: state.blueprints.id("Bleeding").unwrap(),
            chance_bp,
            on_graze: false,
        }])
    }

    /// The player hits for exactly 3 and carries a bleed-on-hit themselves.
    fn armed_player(state: &mut GameState, chance_bp: u32) {
        let on_hit = bleed_on_hit(state, chance_bp);
        let player = state.player;
        state
            .world
            .insert(player, (Melee { min: 3, max: 3 }, on_hit))
            .unwrap();
    }

    fn player_hits(state: &mut GameState, target: Entity) {
        state.run(Command::Attack {
            attacker: state.player,
            target,
        });
    }

    fn bleeds(state: &GameState, entity: Entity) -> usize {
        crate::holding::attached(&state.world, entity)
            .into_iter()
            .filter(|&attached| state.world.satisfies::<&Bleeding>(attached))
            .count()
    }

    #[test]
    fn a_certain_on_hit_applies_bleeding() {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
        let (dummy, _) = armored_dummy(&mut state, 0);
        armed_player(&mut state, 10_000);
        player_hits(&mut state, dummy);
        assert_eq!(bleeds(&state, dummy), 1);
        assert_eq!(last_log(&state), "The dummy starts bleeding.");
    }

    #[test]
    fn a_kill_shot_logs_the_death_and_applies_nothing() {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
        let (dummy, _) = armored_dummy(&mut state, 0);
        state.world.get::<&mut Health>(dummy).unwrap().current = 3;
        armed_player(&mut state, 10_000);
        player_hits(&mut state, dummy);
        assert!(!state.world.contains(dummy));
        assert_eq!(last_log(&state), "The dummy dies.");
        assert!(!state.log.recent(10).any(|m| m.contains("bleeding")));
        assert_eq!(state.world.query::<&Bleeding>().iter().count(), 0);
    }

    #[test]
    fn a_zero_chance_on_hit_never_applies() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 0);
        state.world.get::<&mut Health>(dummy).unwrap().current = 1000;
        armed_player(&mut state, 0);
        for _ in 0..50 {
            player_hits(&mut state, dummy);
        }
        assert_eq!(bleeds(&state, dummy), 0);
    }

    #[test]
    fn a_hit_fully_absorbed_by_armor_applies_nothing() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 3);
        armed_player(&mut state, 10_000);
        player_hits(&mut state, dummy);
        assert_eq!(health(&state, dummy), 10);
        assert_eq!(bleeds(&state, dummy), 0);
    }

    #[test]
    fn on_hit_on_equipment_triggers_for_its_wearers_hits() {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
        let (dummy, _) = armored_dummy(&mut state, 0);
        let on_hit = bleed_on_hit(&state, 10_000);
        let player = state.player;
        blank().with(on_hit).attached_to(player).make(&mut state);
        state
            .world
            .insert_one(player, Melee { min: 3, max: 3 })
            .unwrap();
        player_hits(&mut state, dummy);
        assert_eq!(bleeds(&state, dummy), 1);
    }

    /// Returns the log and how many hits made the dummy bleed.
    fn thirty_coin_flip_hits(seed: u64) -> (MessageLog, usize) {
        let mut state = bare_state_seeded(seed);
        let (dummy, _) = armored_dummy(&mut state, 0);
        state.world.get::<&mut Health>(dummy).unwrap().current = 1000;
        armed_player(&mut state, 5_000);
        for _ in 0..30 {
            player_hits(&mut state, dummy);
        }
        (state.log.clone(), bleeds(&state, dummy))
    }

    #[test]
    fn on_hit_outcomes_are_deterministic_per_seed() {
        let (log, applied) = thirty_coin_flip_hits(4);
        assert_eq!(thirty_coin_flip_hits(4), (log, applied));
        assert!(0 < applied && applied < 30, "{applied} of 30 at 50%");
    }

    #[test]
    fn armor_on_an_attachment_reduces_damage() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 2);
        state.run(Command::DealDamage {
            target: dummy,
            amount: 5,
            cause: BLOW,
        });
        assert_eq!(health(&state, dummy), 7);
        assert_eq!(last_log(&state), "The dummy takes 3 damage (2 blocked).");
    }

    #[test]
    fn fully_absorbed_damage_leaves_health_unchanged() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 2);
        state.run(Command::DealDamage {
            target: dummy,
            amount: 2,
            cause: BLOW,
        });
        assert_eq!(health(&state, dummy), 10);
        assert_eq!(last_log(&state), "The dummy's armor absorbs the blow.");
    }

    #[test]
    fn what_a_blow_did_is_an_effect_where_the_player_sees() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 2);
        state.visible.set(5, 5, true);
        state.run(Command::DealDamage {
            target: dummy,
            amount: 2,
            cause: BLOW,
        });
        let player = state.player;
        announce(&mut state, player, dummy, Outcome::Miss, "");
        let outcome = |kind| Fx::Outcome { at: (5, 5), kind };
        assert_eq!(
            fx::take_fx(&mut state),
            [outcome(fx::Outcome::Absorbed), outcome(fx::Outcome::Miss)]
        );
        state.visible.set(5, 5, false);
        announce(&mut state, player, dummy, Outcome::Graze, "");
        assert!(fx::take_fx(&mut state).is_empty(), "unseen");
    }

    #[test]
    fn armor_does_not_reduce_effect_damage() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 2);
        state.run(Command::DealDamage {
            target: dummy,
            amount: 1,
            cause: UNSOURCED,
        });
        assert_eq!(health(&state, dummy), 9);
        assert_eq!(last_log(&state), "The dummy takes 1 damage.");
    }

    use DamageType::{Blunt, Piercing, Slashing};

    fn blow_of(types: &[DamageType]) -> DamageCause {
        DamageCause::Melee {
            attacker: Entity::DANGLING,
            types: DamageTypes::of(types),
            graze: false,
        }
    }

    /// Health lost by a 10 HP dummy wearing `pieces` when hit for 5.
    fn lost_to(pieces: &[Armor], types: &[DamageType]) -> i32 {
        let mut state = bare_state();
        let dummy = thing("dummy").at(5, 5).health(10).make(&mut state);
        for &piece in pieces {
            thing("piece")
                .with(piece)
                .attached_to(dummy)
                .make(&mut state);
        }
        state.run(Command::DealDamage {
            target: dummy,
            amount: 5,
            cause: blow_of(types),
        });
        10 - health(&state, dummy)
    }

    const SCALY: Armor = Armor {
        slashing: 3,
        blunt: 0,
        piercing: 1,
        fire: 0,
        cold: 0,
        shock: 0,
        acid: 0,
    };

    #[test]
    fn armor_resists_each_kind_separately() {
        let lost = |kind| lost_to(&[SCALY], &[kind]);
        assert_eq!((lost(Slashing), lost(Blunt), lost(Piercing)), (2, 5, 4));
    }

    #[test]
    fn a_blow_of_several_kinds_goes_through_the_weakest_spot() {
        assert_eq!(lost_to(&[SCALY], &[Slashing, Piercing]), 4);
        assert_eq!(lost_to(&[SCALY], &[Slashing, Blunt]), 5);
    }

    #[test]
    fn pieces_add_up_per_kind_before_the_weakest_is_chosen() {
        let a = Armor {
            slashing: 2,
            ..Armor::default()
        };
        let b = Armor {
            piercing: 2,
            ..Armor::default()
        };
        assert_eq!(lost_to(&[a, b], &[Slashing, Piercing]), 3);
        assert_eq!(lost_to(&[a, b], &[Slashing]), 3);
    }

    #[test]
    fn more_kinds_are_never_worse_than_either_alone() {
        let kinds = |armor: Armor, types: &[DamageType]| armor.against(DamageTypes::of(types));
        for slashing in 0..4 {
            for piercing in 0..4 {
                let armor = Armor {
                    slashing,
                    piercing,
                    blunt: 0,
                    ..Armor::default()
                };
                let both = kinds(armor, &[Slashing, Piercing]);
                assert!(both <= kinds(armor, &[Slashing]) && both <= kinds(armor, &[Piercing]));
            }
        }
    }

    #[test]
    fn damage_types_read_as_a_list_in_data() {
        let types: DamageTypes = ron::from_str("[Slashing, Piercing]").unwrap();
        assert_eq!(types, DamageTypes::of(&[Piercing, Slashing]));
        let again: DamageTypes = ron::from_str(&ron::to_string(&types).unwrap()).unwrap();
        assert_eq!(again, types);
    }

    #[test]
    fn bare_hands_are_blunt_and_a_weapon_brings_its_own_kinds() {
        let mut state = bare_state();
        let brawler = fighter(&mut state, 16, 16);
        assert_eq!(
            strike(&state.world, state.blueprints.skills(), brawler)
                .unwrap()
                .types,
            DamageTypes::of(&[Blunt])
        );
        thing("spear")
            .with(Weapon {
                min: 1,
                max: 2,
                stat: Stat::Strength,
                types: DamageTypes::of(&[Slashing, Piercing]),
            })
            .equipped_on(brawler, 2)
            .make(&mut state);
        assert_eq!(
            strike(&state.world, state.blueprints.skills(), brawler)
                .unwrap()
                .types,
            DamageTypes::of(&[Slashing, Piercing])
        );
    }

    #[test]
    fn a_real_attack_is_resisted_by_the_weapons_kind() {
        let mut state = bare_state();
        let attacker = fighter(&mut state, 16, 16);
        sure_hits(&mut state, attacker);
        let target = thing("dummy").at(5, 5).health(100).make(&mut state);
        thing("plate")
            .with(Armor {
                blunt: 3,
                ..Armor::default()
            })
            .attached_to(target)
            .make(&mut state);
        state
            .world
            .insert_one(attacker, Melee { min: 4, max: 4 })
            .unwrap();
        state.run(Command::Attack { attacker, target });
        assert_eq!(health(&state, target), 99);
    }

    #[test]
    fn preview_is_pure_and_matches_the_next_real_attack() {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
        let (dummy, _) = armored_dummy(&mut state, 1);
        state.world.get::<&mut Health>(dummy).unwrap().current = 1000;
        state
            .world
            .insert_one(state.player, Melee { min: 1, max: 20 })
            .unwrap();

        for _ in 0..20 {
            let rng_before = state.rngs.clone();
            let log_before = state.log.clone();
            let entities_before = state.world.len();
            let health_before = health(&state, dummy);

            let preview = preview_damage(&state, state.player, dummy).expect("player can attack");

            assert_eq!(state.rngs, rng_before);
            assert_eq!(state.log, log_before);
            assert!(state.commands.is_empty());
            assert_eq!(state.world.len(), entities_before);
            assert_eq!(health(&state, dummy), health_before);

            state.run(Command::Attack {
                attacker: state.player,
                target: dummy,
            });
            assert_eq!(health_before - health(&state, dummy), preview);
        }
    }

    #[test]
    fn a_leaning_phase_strengthens_its_element_for_anyone_and_the_preview_agrees() {
        let mut state = bare_state();
        state.place = crate::dive::Place::Floor {
            dungeon: "Test".into(),
            phase: "Embers".into(),
            dive: 1,
            floor: 1,
        };
        let player = state.player;
        let goblin = thing("goblin").health(1000).at(2, 1).make(&mut state);
        state.world.get::<&mut Health>(player).unwrap().current = 1000;
        for (from, to) in [(goblin, player), (player, goblin)] {
            for (types, taken) in [(&[DamageType::Fire][..], 15), (&[DamageType::Cold], 10)] {
                let before = health(&state, to);
                state.run(Command::DealDamage {
                    target: to,
                    amount: 10,
                    cause: DamageCause::Melee {
                        attacker: from,
                        types: DamageTypes::of(types),
                        graze: false,
                    },
                });
                assert_eq!(before - health(&state, to), taken, "{types:?}");
            }
        }

        sure_hits(&mut state, player);
        thing("brand")
            .with(Weapon {
                min: 1,
                max: 20,
                stat: Stat::Strength,
                types: DamageTypes::single(DamageType::Fire),
            })
            .equipped_on(player, 0)
            .make(&mut state);
        for _ in 0..10 {
            let before = health(&state, goblin);
            let preview = preview_damage(&state, player, goblin).unwrap();
            state.run(Command::Attack {
                attacker: player,
                target: goblin,
            });
            assert_eq!(before - health(&state, goblin), preview);
        }
    }

    #[test]
    fn preview_needs_melee_and_health() {
        let mut state = bare_state();
        let rock = thing("rock").make(&mut state);
        assert_eq!(preview_damage(&state, state.player, rock), None);
        assert_eq!(preview_damage(&state, rock, state.player), None);
    }
}

#[cfg(test)]
mod odds_tests {
    use super::*;
    use crate::entity::{Learner, OnHitEntry, Skills};
    use crate::skill::xp_for;
    use crate::state::MessageLog;
    use crate::state::tests::bare_state;
    use crate::testkit::{blank, thing};

    /// A 1000 HP dummy beside the player, the player with natural 4-4.
    fn duel(state: &mut GameState) -> Entity {
        let player = state.player;
        state
            .world
            .insert_one(player, Melee { min: 4, max: 4 })
            .unwrap();
        thing("dummy").health(1000).hostile().at(2, 1).make(state)
    }

    fn odds_of(state: &GameState, attacker: Entity, defender: Entity) -> AttackOdds {
        let strike = strike(&state.world, state.blueprints.skills(), attacker).unwrap();
        odds(state, &strike, attacker, defender)
    }

    fn learn(state: &mut GameState, creature: Entity, levels: &[(&str, u32)]) {
        let mut skills = Skills::default();
        for &(name, level) in levels {
            let id = state.blueprints.skills().id(name).unwrap();
            skills.set(id, xp_for(level));
        }
        state.world.insert_one(creature, skills).unwrap();
    }

    fn odds_with(
        hit_bp: u32,
        perfect_bp: u32,
        graze_bp: u32,
        deflector: Option<Entity>,
    ) -> AttackOdds {
        AttackOdds {
            attacker: Entity::DANGLING,
            defender: Entity::DANGLING,
            asking: Entity::DANGLING,
            hit_bp,
            perfect_bp,
            graze_bp,
            deflect_bp: CERTAIN,
            deflector,
        }
    }

    /// Attacks `target` until an attack logs a line containing `marker`, and
    /// returns the health that attack cost the target. The log then holds
    /// only that attack's lines.
    fn attack_until(state: &mut GameState, target: Entity, marker: &str) -> i32 {
        for _ in 0..500 {
            let before = state.world.get::<&Health>(target).unwrap().current;
            state.log = MessageLog::default();
            state.run(Command::Attack {
                attacker: state.player,
                target,
            });
            if state
                .log
                .recent(usize::MAX)
                .any(|line| line.contains(marker))
            {
                return before - state.world.get::<&Health>(target).unwrap().current;
            }
        }
        panic!("never saw {marker:?}");
    }

    #[test]
    fn equals_hit_three_times_in_four_and_the_odds_are_clamped() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        let player = state.player;
        let even = odds_of(&state, player, dummy);
        assert_eq!(even.hit_bp, 7_500);
        assert_eq!((even.perfect_bp, even.graze_bp), (0, DEFAULT_GRAZE_BP));
        assert_eq!((even.deflect_bp, even.deflector), (0, None));

        learn(
            &mut state,
            player,
            &[("Melee", 20), ("Unarmed", 20), ("Blunt", 20)],
        );
        assert_eq!(odds_of(&state, player, dummy).hit_bp, 9_500);
        learn(&mut state, player, &[]);
        learn(&mut state, dummy, &[("Dodge", 20)]);
        assert_eq!(odds_of(&state, player, dummy).hit_bp, 500);
    }

    #[test]
    fn each_defence_skill_moves_its_own_chance() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        let player = state.player;
        learn(
            &mut state,
            dummy,
            &[("Dodge", 2), ("Poise", 3), ("Deflection", 4)],
        );
        let odds = odds_of(&state, player, dummy);
        assert_eq!(odds.hit_bp, 7_500 - 2 * 400);
        assert_eq!(odds.perfect_bp, 300);
        assert_eq!(odds.deflect_bp, 0, "nothing worn to deflect with");

        let buckler = thing("buckler")
            .with(Deflect(1_000))
            .equipped_on(dummy, 2)
            .make(&mut state);
        thing("pauldron")
            .with(Deflect(500))
            .equipped_on(dummy, 0)
            .make(&mut state);
        let odds = odds_of(&state, player, dummy);
        assert_eq!((odds.deflect_bp, odds.deflector), (1_400, Some(buckler)));
    }

    #[test]
    fn each_enemy_beside_the_defender_beyond_the_first_makes_it_easier_to_hit() {
        let mut state = bare_state();
        let player = state.player;
        let first = thing("goblin")
            .health(10)
            .hostile()
            .with(Melee { min: 1, max: 1 })
            .at(2, 1)
            .make(&mut state);
        let alone = odds_of(&state, first, player).hit_bp;
        assert_eq!(outnumbered(&state, player), 0);
        thing("goblin")
            .health(10)
            .hostile()
            .at(1, 2)
            .make(&mut state);
        thing("goblin")
            .health(10)
            .hostile()
            .at(2, 2)
            .make(&mut state);
        assert_eq!(outnumbered(&state, player), 2);
        assert_eq!(odds_of(&state, first, player).hit_bp, alone + 2 * 1_000);
        // A goblin two tiles off is not beside you.
        thing("goblin")
            .health(10)
            .hostile()
            .at(3, 3)
            .make(&mut state);
        assert_eq!(outnumbered(&state, player), 2);
    }

    #[test]
    fn the_odds_carry_their_reasons_and_they_add_up() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        let player = state.player;
        let strike = strike(&state.world, state.blueprints.skills(), player).unwrap();
        let (odds, reasons) = odds_explained(&state, &strike, player, dummy);
        assert_eq!(reasons.len(), 1, "an even blow: the base alone");
        assert!(reasons[0].what.starts_with("accuracy"));
        assert_eq!(reasons[0].bp, i32::try_from(odds.hit_bp).unwrap());

        thing("goblin")
            .health(10)
            .hostile()
            .at(1, 2)
            .make(&mut state);
        thing("goblin")
            .health(10)
            .hostile()
            .at(3, 1)
            .make(&mut state);
        let goblin = thing("goblin")
            .health(10)
            .hostile()
            .with(Melee { min: 1, max: 1 })
            .at(2, 2)
            .make(&mut state);
        let strike = strike_of(&state, goblin);
        let (odds, reasons) = odds_explained(&state, &strike, goblin, player);
        assert!(
            reasons
                .iter()
                .any(|r| r.what == "outnumbered by 2" && r.bp == 2_000)
        );
        let sum: i32 = reasons.iter().map(|r| r.bp).sum();
        assert_eq!(sum, i32::try_from(odds.hit_bp).unwrap());
    }

    fn strike_of(state: &GameState, attacker: Entity) -> Strike {
        strike(&state.world, state.blueprints.skills(), attacker).unwrap()
    }

    #[test]
    fn a_part_that_moves_the_odds_is_named() {
        let data = crate::blueprint::Blueprints::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap();
        let mut state = crate::state::tests::bare_state_with(data);
        let goblin = state
            .spawn(crate::spawn::Spawn::blueprint("Goblin").at(2, 1))
            .unwrap();
        let off_balance = state.blueprints.id("OffBalance").unwrap();
        crate::effect::apply_effect(&mut state, goblin, off_balance);
        let player = state.player;
        let strike = strike_of(&state, player);
        let (odds, reasons) = odds_explained(&state, &strike, player, goblin);
        assert!(
            reasons.iter().any(|r| r.what == "reeling" && r.bp > 0),
            "{reasons:?}"
        );
        let sum: i32 = reasons.iter().map(|r| r.bp).sum();
        assert_eq!(sum, i32::try_from(odds.hit_bp).unwrap());
    }

    #[test]
    fn a_shot_past_its_steady_range_loses_accuracy_per_tile() {
        let mut state = bare_state();
        let player = state.player;
        let dummy = thing("dummy")
            .health(1000)
            .hostile()
            .at(6, 1)
            .make(&mut state);
        let mut strike = strike_of(&state, player);
        let (even, _) = odds_explained(&state, &strike, player, dummy);
        strike.steady = Some(2);
        let (odds, reasons) = odds_explained(&state, &strike, player, dummy);
        assert_eq!(
            odds.hit_bp,
            even.hit_bp - 3 * 500,
            "five tiles off, steady for two"
        );
        assert!(
            reasons
                .iter()
                .any(|r| r.what == "range, 3 past steady" && r.bp == -1_500)
        );
        strike.steady = Some(5);
        assert_eq!(
            odds_explained(&state, &strike, player, dummy).0.hit_bp,
            even.hit_bp
        );
    }

    #[test]
    fn a_phases_ranged_falloff_costs_a_shot_every_tile_and_spares_a_blow() {
        let mut state = bare_state();
        let player = state.player;
        let dummy = thing("dummy")
            .health(1000)
            .hostile()
            .at(6, 1)
            .make(&mut state);
        let mut strike = strike_of(&state, player);
        strike.steady = Some(9);
        let (even, _) = odds_explained(&state, &strike, player, dummy);
        // The test phases' embers bend every rule; RangedFalloff is 10%.
        crate::phase::tests::set_floor_phase(&mut state, "Embers");
        let (odds, reasons) = odds_explained(&state, &strike, player, dummy);
        assert_eq!(odds.hit_bp, even.hit_bp - 5 * 1_000, "five tiles");
        assert!(
            reasons
                .iter()
                .any(|r| r.what == "the embers, over the distance")
        );
        strike.steady = None;
        let (blow, _) = odds_explained(&state, &strike, player, dummy);
        assert_eq!(blow.hit_bp, even.hit_bp, "a blow flies nowhere");
    }

    #[test]
    fn a_miss_says_why_beside_its_line() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        let player = state.player;
        let strike = strike_of(&state, player);
        let (odds, reasons) = odds_explained(&state, &strike, player, dummy);
        let why = explain(&odds, &reasons);
        assert_eq!(why, "75% to hit: accuracy 0 against dodge 0 75");
        announce(&mut state, player, dummy, Outcome::Miss, &why);
        let (line, because) = state.log.recent_with_why(1).next().unwrap();
        assert_eq!(line, "You miss the dummy.");
        assert_eq!(because, Some(why.as_str()));
        state.log.push("Something else.");
        assert_eq!(state.log.recent_with_why(1).next().unwrap().1, None);
    }

    #[test]
    fn asking_the_odds_draws_nothing() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        let before = state.rngs.combat.clone();
        odds_of(&state, state.player, dummy);
        assert_eq!(state.rngs.combat, before);
    }

    #[test]
    fn a_roll_is_drawn_only_when_it_could_go_either_way() {
        let mut rng = bare_state().rngs.combat;
        let untouched = rng.clone();
        assert_eq!(
            resolve(&mut rng, &odds_with(CERTAIN, 0, 0, None)),
            Outcome::Hit
        );
        assert_eq!(
            resolve(
                &mut rng,
                &odds_with(0, CERTAIN, CERTAIN, Some(Entity::DANGLING))
            ),
            Outcome::PerfectDodge,
            "a perfect dodge rolls no graze, so nothing can deflect"
        );
        assert_eq!(resolve(&mut rng, &odds_with(0, 0, 0, None)), Outcome::Miss);
        assert_eq!(
            resolve(&mut rng, &odds_with(0, 0, CERTAIN, None)),
            Outcome::Graze
        );
        assert_eq!(
            resolve(&mut rng, &odds_with(0, 0, CERTAIN, Some(Entity::DANGLING))),
            Outcome::Deflected(Entity::DANGLING)
        );
        assert_eq!(rng, untouched);

        let mut once = untouched.clone();
        once.random_range(0..BASIS_POINTS);
        resolve(&mut rng, &odds_with(5_000, 0, 0, None));
        assert_eq!(rng, once);
    }

    #[test]
    fn a_graze_halves_the_damage_and_applies_only_effects_meant_for_grazes() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        let bleeding = state.blueprints.id("Bleeding").unwrap();
        let on_hit = |on_graze| {
            OnHit(vec![OnHitEntry {
                effect: bleeding,
                chance_bp: CERTAIN,
                on_graze,
            }])
        };
        let player = state.player;
        let charm = blank()
            .with(on_hit(false))
            .attached_to(player)
            .make(&mut state);
        assert_eq!(attack_until(&mut state, dummy, "You graze the dummy."), 2);
        assert!(
            !state
                .log
                .recent(usize::MAX)
                .any(|line| line.contains("bleeding"))
        );

        state.world.insert_one(charm, on_hit(true)).unwrap();
        attack_until(&mut state, dummy, "You graze the dummy.");
        assert!(
            state
                .log
                .recent(usize::MAX)
                .any(|line| line == "The dummy starts bleeding.")
        );
    }

    #[test]
    fn a_deflected_graze_does_nothing_and_names_what_turned_it() {
        let mut state = bare_state();
        let dummy = duel(&mut state);
        thing("buckler")
            .with(Deflect(CERTAIN))
            .equipped_on(dummy, 2)
            .make(&mut state);
        let lost = attack_until(&mut state, dummy, "turns");
        assert_eq!(lost, 0);
        assert!(
            state
                .log
                .recent(1)
                .any(|line| line == "The dummy's buckler turns your blow.")
        );
        attack_until(&mut state, dummy, "You miss the dummy.");
    }

    #[test]
    fn a_defender_learns_from_being_attacked_and_most_from_a_perfect_dodge() {
        let mut state = bare_state();
        let player = state.player;
        state.world.insert_one(player, Learner).unwrap();
        learn(&mut state, player, &[("Poise", 1)]);
        let goblin = thing("goblin")
            .health(10)
            .melee(0, 0)
            .hostile()
            .at(2, 1)
            .make(&mut state);
        let dodge = state.blueprints.skills().id("Dodge").unwrap();
        let mut gains = Vec::new();
        for _ in 0..500 {
            let before = crate::skill::xp(&state.world, player, dodge);
            state.log = MessageLog::default();
            state.run(Command::Attack {
                attacker: goblin,
                target: player,
            });
            gains.push(crate::skill::xp(&state.world, player, dodge) - before);
            if state
                .log
                .recent(usize::MAX)
                .any(|line| line == "You dodge perfectly.")
            {
                break;
            }
        }
        let [.., ordinary, perfect] = gains[..] else {
            panic!("the first attack was dodged perfectly");
        };
        assert!(ordinary > 0);
        assert!(
            perfect > 2 * ordinary - 2,
            "{perfect} vs {ordinary}: nearly double"
        );
    }
}
