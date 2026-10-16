//! Spells (ADR 0014): what makes a cast possible and what it costs lives in
//! the caster's disciplines, what it does lives in the spell. A cast fires
//! `CheckCast` and `PayCost` at the caster, so a discipline is a part with
//! handlers and `cast` never learns which resources exist.

use std::ops::ControlFlow;

use hecs::{Entity, World};

use crate::cell::Rgb;
use crate::combat::{Strike, roll_strike};
use crate::command::{Command, DamageCause, dispatch};
use crate::cost::TICKS_PER_TURN;
use crate::entity::{
    Appearance, CooldownCasting, DamageTypes, Mana, ManaCasting, Name, Recharging, School, Spell,
    SpellDamage, Stat, Stats, Timed,
};
use crate::event::{Ctx, EventRegistry, settle};
use crate::grammar::Subject;
use crate::holding;
use crate::prio;
use crate::ranged::{self, Trail, check_range, flight};
use crate::state::GameState;
use crate::time::Tick;
use crate::view::CostLine;

/// Asked of the caster before a cast. A discipline for the spell's school
/// sets `claimed`, the `stat` the spell scales with, and describes its price
/// in `costs` (even when it then refuses, so a list can show the price); one
/// that cannot pay sets `blocked` to the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckCast {
    pub caster: Entity,
    pub spell: Entity,
    pub school: School,
    pub power: i32,
    pub now: Tick,
    pub claimed: bool,
    pub stat: Option<Stat>,
    pub blocked: Option<String>,
    pub costs: Vec<CostLine>,
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
}

pub(crate) fn register(registry: &mut EventRegistry) {
    registry.on::<CheckCast, CooldownCasting>(prio::COOLDOWN, |ctx, rule, mut ev| {
        if rule.school != ev.school {
            return ControlFlow::Continue(ev);
        }
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
        if rule.school != ev.school {
            return ControlFlow::Continue(ev);
        }
        ev.claimed = true;
        ev.stat = Some(rule.stat);
        let price = mana_price(rule, ev.power);
        let pool = ctx
            .world
            .get::<&Mana>(ev.caster)
            .map_or(0, |mana| mana.current(ev.now));
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
                amount: mana_price(rule, ev.power),
            });
        }
        ControlFlow::Continue(ev)
    });
}

fn recharge_ticks(rule: &CooldownCasting, power: i32) -> u64 {
    u64::try_from(power).unwrap_or(0) * rule.ticks_per_power
}

fn mana_price(rule: &ManaCasting, power: i32) -> u32 {
    u32::try_from(power).unwrap_or(0) * rule.mana_per_power
}

/// A cast as it would go now, checked the same way for the command, the aim
/// preview and the spell list.
#[derive(Debug, Clone)]
pub(crate) struct Ready {
    pub range: i32,
    pub strike: Strike,
    /// What the claiming disciplines will take, as they describe it.
    pub costs: Vec<CostLine>,
    school: School,
    power: i32,
}

/// What `spell` hits for when `caster` casts it: its dice plus the caster's
/// modifier for `stat` (the claiming discipline's), untyped. `None` if it
/// does no damage.
fn strike(world: &World, caster: Entity, spell: Entity, stat: Option<Stat>) -> Option<Strike> {
    let damage = *world.get::<&SpellDamage>(spell).ok()?;
    let bonus = world
        .get::<&Stats>(caster)
        .ok()
        .zip(stat)
        .map_or(0, |(stats, stat)| stats.modifier(stat));
    Some(Strike {
        min: damage.min,
        max: damage.max,
        bonus,
        types: DamageTypes::default(),
        weapon: None,
    })
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
    let known = holding::attached(world, actor).contains(&spell);
    let (true, Ok(data), true) = (
        known,
        world.get::<&Spell>(spell).map(|s| *s),
        world.satisfies::<&SpellDamage>(spell),
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
    let check = settle(state.events.fire(
        &mut ctx,
        actor,
        CheckCast {
            caster: actor,
            spell,
            school: data.school,
            power: data.power,
            now: state.now,
            claimed: false,
            stat: None,
            blocked: None,
            costs: Vec::new(),
        },
    ));
    let blocked = match check.blocked {
        Some(reason) => Some(reason),
        None if !check.claimed => Some(format!("You have no way to cast {name}.")),
        None => None,
    };
    let strike = strike(world, actor, spell, check.stat)
        .ok_or_else(|| "You don't know that spell.".to_string())?;
    let ready = Ready {
        range: data.range,
        strike,
        costs: check.costs,
        school: data.school,
        power: data.power,
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
    check_range(state, actor, ready_cast(state, actor, spell)?.range, at)
}

/// Applies `Command::Cast`: pays the cost, then the bolt flies to the first
/// creature or wall on the line and the first creature takes `DealDamage`.
/// The damage roll is one combat-stream draw, made only when something is hit.
pub(crate) fn cast(state: &mut GameState, actor: Entity, spell: Entity, at: (i32, i32)) {
    let Ok(ready) = ready_cast(state, actor, spell) else {
        return;
    };
    let Some(from) = ranged::position(state, actor) else {
        return;
    };
    let _ = dispatch(
        state,
        actor,
        PayCost {
            caster: actor,
            spell,
            school: ready.school,
            power: ready.power,
            now: state.now,
        },
    );

    let flight = flight(state, actor, from, at, ready.range);
    if !flight.path.is_empty() {
        let (glyph, fg) = state
            .world
            .get::<&Appearance>(spell)
            .map_or(('*', Rgb(255, 140, 40)), |look| (look.glyph, look.fg));
        state.trails.push(Trail {
            glyph,
            fg,
            path: flight.path.clone(),
        });
    }

    let name = state
        .world
        .get::<&Name>(spell)
        .map_or_else(|_| "spell".to_string(), |name| name.0.clone());
    let aimed = flight.hit.map_or_else(String::new, |target| {
        format!(" at {}", Subject::of(&state.world, target).noun())
    });
    let message = Subject::of(&state.world, actor).sentence("cast", &format!("{name}{aimed}"));
    state.log.push(message);

    let Some(target) = flight.hit else {
        return;
    };
    let hit = roll_strike(&mut state.rngs.combat, ready.strike);
    state.commands.push_back(Command::DealDamage {
        target,
        amount: hit.amount,
        cause: DamageCause::Spell { caster: actor },
    });
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
    let Ok(mana) = state.world.get::<&Mana>(caster).map(|mana| *mana) else {
        return;
    };
    let _ = state
        .world
        .insert_one(caster, mana.spend(amount, state.now));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Armor, Health};
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
            ("firebolt", 6, (3, 3))
        );
        assert_eq!(
            listed[0].preview,
            crate::view::CastPreview {
                costs: vec![CostLine {
                    resource: "turn recharge".to_string(),
                    amount: 6,
                }],
                blocked: None,
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
            let _ = state
                .world
                .insert_one(spell, SpellDamage { min: 1, max: 6 });
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
    fn mage() -> (GameState, Entity, Entity) {
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
                Mana {
                    value: 10,
                    max: 10,
                    as_of: state.now,
                    regen_ticks: REGEN_TICKS,
                },
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
        let pool = *state.world.get::<&Mana>(state.player).unwrap();
        pool.current(state.now)
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
            }
        );
    }
}
