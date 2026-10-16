//! Combat events and handlers, plus the shared rolls used by both the real
//! command applier and the side-effect-free `preview_damage`.

use std::ops::ControlFlow;

use hecs::{Entity, World};
use rand::RngExt;
use rand_pcg::Pcg64;

use crate::command::{Command, DamageCause};
use crate::effect::BASIS_POINTS;
use crate::entity::{Armor, Health, Melee, OnHit, Stat, Stats, Weapon};
use crate::equipment::equipped;
use crate::event::{Ctx, EventRegistry, settle};
use crate::prio;
use crate::state::GameState;

/// Fired at a target before damage is applied; handlers may reduce `amount`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BeforeTakeDamage {
    pub target: Entity,
    pub amount: i32,
    pub cause: DamageCause,
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
}

pub(crate) fn register(registry: &mut EventRegistry) {
    registry.on(prio::ARMOR, armor_reduces_damage);
    registry.on(prio::ON_HIT, on_hit_applies_effects);
}

/// Rolls every entry on the combat stream, even at 0 or 10 000 bp, so the
/// number of draws per hit doesn't depend on the chances.
fn on_hit_applies_effects(
    ctx: &mut Ctx<'_>,
    on_hit: &OnHit,
    ev: AfterHit,
) -> ControlFlow<AfterHit, AfterHit> {
    for entry in &on_hit.0 {
        if ctx.rng.random_range(0..BASIS_POINTS) < u64::from(entry.chance_bp) {
            ctx.out.push(Command::ApplyEffect {
                target: ev.target,
                blueprint: entry.effect,
            });
        }
    }
    ControlFlow::Continue(ev)
}

/// Armor stops blows, not what is already inside: effect damage such as
/// bleeding passes through.
fn armor_reduces_damage(
    _ctx: &mut Ctx<'_>,
    armor: &Armor,
    mut ev: BeforeTakeDamage,
) -> ControlFlow<BeforeTakeDamage, BeforeTakeDamage> {
    if !matches!(ev.cause, DamageCause::Melee { .. }) {
        return ControlFlow::Continue(ev);
    }
    ev.amount = (ev.amount - armor.av).max(0);
    if ev.amount == 0 {
        ControlFlow::Break(ev)
    } else {
        ControlFlow::Continue(ev)
    }
}

/// How `attacker` hits in melee: the dice and the stat bonus added to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strike {
    pub min: i32,
    pub max: i32,
    pub bonus: i32,
    /// The equipped weapon used, if not natural `Melee`.
    pub weapon: Option<Entity>,
}

impl Strike {
    /// Lowest and highest damage before armor; never negative.
    pub fn range(self) -> (i32, i32) {
        let max = self.max.max(self.min);
        ((self.min + self.bonus).max(0), (max + self.bonus).max(0))
    }
}

/// The equipped `Weapon` on the lowest-numbered body part, else natural
/// `Melee` (a Strength weapon); `None` if `attacker` has neither. The
/// wielder's `Stats` modifier for the weapon's stat is the bonus.
pub(crate) fn strike(world: &World, attacker: Entity) -> Option<Strike> {
    let mut worn = equipped(world, attacker);
    worn.sort_by_key(|&(_, part)| part);
    let wielded = worn.into_iter().find_map(|(item, _)| {
        let weapon = *world.get::<&Weapon>(item).ok()?;
        Some((weapon.min, weapon.max, weapon.stat, Some(item)))
    });
    let natural = || {
        let melee = *world.get::<&Melee>(attacker).ok()?;
        Some((melee.min, melee.max, Stat::Strength, None))
    };
    let (min, max, stat, weapon) = wielded.or_else(natural)?;
    let bonus = world
        .get::<&Stats>(attacker)
        .map_or(0, |stats| stats.modifier(stat));
    Some(Strike {
        min,
        max,
        bonus,
        weapon,
    })
}

/// Raw damage of one of `attacker`'s melee hits (see `strike`), or `None`
/// if it can't attack. Always exactly one draw from `rng`.
pub(crate) fn roll_melee(world: &World, rng: &mut Pcg64, attacker: Entity) -> Option<i32> {
    let strike = strike(world, attacker)?;
    let roll = rng.random_range(strike.min..=strike.max.max(strike.min));
    Some((roll + strike.bonus).max(0))
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
    let raw = roll_melee(&state.world, &mut rng, attacker)?;
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
            amount: raw,
            cause: DamageCause::Melee { attacker },
        },
    ));
    Some(ev.amount.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::tests::{BLOW, UNSOURCED};
    use crate::entity::{Bleeding, OnHitEntry};
    use crate::spawn::Spawn;
    use crate::state::MessageLog;
    use crate::state::tests::{bare_state, bare_state_seeded};
    use crate::testkit::{blank, thing};

    #[test]
    fn stat_modifiers_follow_qud() {
        let modifier = |strength| {
            Stats {
                strength,
                agility: 16,
            }
            .modifier(Stat::Strength)
        };
        let table: Vec<i32> = [12, 13, 14, 15, 16, 17, 18, 19, 20].map(modifier).to_vec();
        assert_eq!(table, [-2, -2, -1, -1, 0, 0, 1, 1, 2]);
    }

    /// A fighter with natural 2-4 `Melee` and the given scores.
    fn fighter(state: &mut GameState, strength: i32, agility: i32) -> Entity {
        blank().melee(2, 4).stats(strength, agility).spawn(state)
    }

    fn knife() -> Spawn {
        thing("knife").weapon(1, 3, Stat::Agility)
    }

    #[test]
    fn bare_hands_add_strength_to_natural_melee() {
        let mut state = bare_state();
        let strong = fighter(&mut state, 20, 10);
        let strike = strike(&state.world, strong).unwrap();
        assert_eq!((strike.range(), strike.weapon), ((4, 6), None));
    }

    #[test]
    fn an_equipped_weapon_replaces_natural_melee_and_uses_its_stat() {
        let mut state = bare_state();
        let nimble = fighter(&mut state, 10, 20);
        let blade = knife().equipped_on(nimble, 2).spawn(&mut state);

        let strike = strike(&state.world, nimble).unwrap();
        assert_eq!((strike.range(), strike.weapon), ((3, 5), Some(blade)));
        let mut rng = state.rngs.combat.clone();
        for _ in 0..50 {
            let damage = roll_melee(&state.world, &mut rng, nimble).unwrap();
            assert!((3..=5).contains(&damage), "{damage}");
        }
    }

    #[test]
    fn a_carried_weapon_is_not_used() {
        let mut state = bare_state();
        let fighter = fighter(&mut state, 16, 20);
        knife().carried_by(fighter).spawn(&mut state);
        assert_eq!(strike(&state.world, fighter).unwrap().weapon, None);
    }

    #[test]
    fn a_weak_hit_never_heals() {
        let mut state = bare_state();
        let feeble = fighter(&mut state, 4, 16);
        assert_eq!(strike(&state.world, feeble).unwrap().range(), (0, 0));
        let mut rng = state.rngs.combat.clone();
        assert_eq!(roll_melee(&state.world, &mut rng, feeble), Some(0));
    }

    fn health(state: &GameState, entity: Entity) -> i32 {
        state.world.get::<&Health>(entity).unwrap().current
    }

    fn last_log(state: &GameState) -> &str {
        state
            .log
            .recent(1)
            .next()
            .map(String::as_str)
            .unwrap_or_default()
    }

    fn armored_dummy(state: &mut GameState, av: i32) -> (Entity, Entity) {
        let dummy = thing("dummy").at(5, 5).health(10).spawn(state);
        let armor = thing("plate").armor(av).attached_to(dummy).spawn(state);
        (dummy, armor)
    }

    fn bleed_on_hit(state: &GameState, chance_bp: u32) -> OnHit {
        OnHit(vec![OnHitEntry {
            effect: state.blueprints.id("Bleeding").unwrap(),
            chance_bp,
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
        let (dummy, _) = armored_dummy(&mut state, 0);
        armed_player(&mut state, 10_000);
        player_hits(&mut state, dummy);
        assert_eq!(bleeds(&state, dummy), 1);
        assert_eq!(last_log(&state), "The dummy starts bleeding.");
    }

    #[test]
    fn a_kill_shot_logs_the_death_and_applies_nothing() {
        let mut state = bare_state();
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
        let (dummy, _) = armored_dummy(&mut state, 0);
        let on_hit = bleed_on_hit(&state, 10_000);
        let player = state.player;
        blank().with(on_hit).attached_to(player).spawn(&mut state);
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

    #[test]
    fn preview_is_pure_and_matches_the_next_real_attack() {
        let mut state = bare_state();
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
        let rock = thing("rock").spawn(&mut state);
        assert_eq!(preview_damage(&state, state.player, rock), None);
        assert_eq!(preview_damage(&state, rock, state.player), None);
    }
}
