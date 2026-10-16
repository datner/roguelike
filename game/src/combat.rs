//! Combat events and handlers, plus the shared rolls used by both the real
//! command applier and the side-effect-free `preview_damage`.

use std::ops::ControlFlow;

use hecs::{Entity, World};
use rand::RngExt;
use rand_pcg::Pcg64;

use crate::command::{Command, DamageCause};
use crate::effect::BASIS_POINTS;
use crate::entity::{
    Armor, DamageType, DamageTypes, Deflect, Graze, Health, Launcher, Melee, OnHit, Projectile,
    Stat, Stats, Throwable, Weapon,
};
use crate::equipment::equipped;
use crate::event::{Ctx, EventRegistry, settle};
use crate::prio;
use crate::skill::{self, SkillBook, SkillId};
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
    /// The equipped weapon used, if not natural `Melee`.
    pub weapon: Option<Entity>,
    /// The attacker's effective skill level at it (ADR 0017).
    pub level: u32,
    /// What its hit chance is reckoned from: `level` plus the stat modifier.
    pub accuracy: i32,
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
    let mut worn = equipped(world, attacker);
    worn.sort_by_key(|&(_, part)| part);
    let wielded = worn.into_iter().find_map(|(item, _)| {
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
        Some((
            melee.min,
            melee.max,
            Stat::Strength,
            DamageTypes::single(DamageType::Blunt),
            None,
        ))
    };
    let (min, max, stat, types, weapon) = wielded.or_else(natural)?;
    let bonus = world
        .get::<&Stats>(attacker)
        .map_or(0, |stats| stats.modifier(stat));
    let mut strike = Strike {
        min,
        max,
        bonus,
        types,
        weapon,
        level: 0,
        accuracy: 0,
    };
    strike.level = skill::effective(
        world,
        skills,
        attacker,
        &skill::melee(world, skills, &strike),
    );
    strike.accuracy = bonus + i32::try_from(strike.level).unwrap_or(0);
    strike.bonus += skill::damage_bonus(strike.level);
    Some(strike)
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
    let world = &state.world;
    let core = state.blueprints.skills().core();
    let level =
        |skill: Option<SkillId>| skill.map_or(0, |id| skill::level(skill::xp(world, defender, id)));
    let dodge = i32::try_from(level(core.map(|c| c.dodge))).unwrap_or(0)
        + world
            .get::<&Stats>(defender)
            .map_or(0, |stats| stats.modifier(Stat::Agility));
    let hit_bp =
        (EVEN_HIT_BP + HIT_BP_PER_POINT * (strike.accuracy - dodge)).clamp(MIN_HIT_BP, MAX_HIT_BP);
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
    for target in [attacker, defender] {
        ev = settle(state.events.fire(&mut ctx, target, ev));
    }
    ev
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

/// How `shooter` fires `arrow` from `launcher`: the ammunition's dice and
/// kinds, plus the shooter's modifier for the launcher's stat; and the
/// arrow's chance (out of 10 000) to break on a hit. `None` if `arrow` is
/// not a projectile.
pub(crate) fn fire_strike(
    world: &World,
    shooter: Entity,
    launcher: Launcher,
    arrow: Entity,
) -> Option<(Strike, u32)> {
    let projectile = *world.get::<&Projectile>(arrow).ok()?;
    let bonus = world
        .get::<&Stats>(shooter)
        .map_or(0, |stats| stats.modifier(launcher.stat));
    let strike = Strike {
        min: projectile.min,
        max: projectile.max,
        bonus,
        types: projectile.types,
        weapon: None,
        level: 0,
        accuracy: 0,
    };
    Some((strike, projectile.break_bp))
}

/// How `thrower` hurls `missile`: its `Throwable` dice plus the thrower's
/// stat modifier. `None` if it can't be thrown.
pub(crate) fn throw_strike(world: &World, thrower: Entity, missile: Entity) -> Option<Strike> {
    let throwable = *world.get::<&Throwable>(missile).ok()?;
    let bonus = world
        .get::<&Stats>(thrower)
        .map_or(0, |stats| stats.modifier(throwable.stat));
    Some(Strike {
        min: throwable.min,
        max: throwable.max,
        bonus,
        types: throwable.types,
        weapon: None,
        level: 0,
        accuracy: 0,
    })
}

/// Damage the next real `Attack { attacker, target }` would deal, computed on
/// a cloned RNG with commands emitted into a scratch buffer. Leaves the world,
/// RNG, log and queue untouched. `None` if the attacker has no `Melee` or the
/// target has no `Health`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "for a future look/target UI; needs an intent-level API before term can use it"
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
    let ev = settle(state.events.fire(
        &mut ctx,
        target,
        BeforeTakeDamage {
            target,
            amount: hit.amount,
            armor: Armor::default(),
            cause: DamageCause::Melee {
                attacker,
                types: hit.types,
                graze: false,
            },
        },
    ));
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
