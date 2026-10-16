//! Spells (ADR 0014): what makes a cast possible and what it costs lives in
//! the caster's disciplines, what it does lives in the spell. A cast fires
//! `CheckCast` and `PayCost` at the caster, so a discipline is a part with
//! handlers and `cast` never learns which resources exist.

use std::ops::ControlFlow;

use hecs::Entity;

use crate::cell::Rgb;
use crate::combat::{Strike, roll_strike};
use crate::command::{Command, DamageCause, dispatch};
use crate::entity::{
    Appearance, CooldownCasting, DamageTypes, Name, Recharging, School, Spell, SpellDamage, Stats,
    Timed,
};
use crate::event::{Ctx, EventRegistry, settle};
use crate::grammar::Subject;
use crate::holding;
use crate::prio;
use crate::ranged::{self, Trail, check_range, flight};
use crate::state::GameState;
use crate::time::Tick;

/// Asked of the caster before a cast. A discipline for the spell's school
/// sets `claimed`; one that cannot pay sets `blocked` to the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckCast {
    pub spell: Entity,
    pub school: School,
    pub now: Tick,
    pub claimed: bool,
    pub blocked: Option<String>,
}

/// Fired at the caster as the cast happens: every discipline that claimed
/// the school pays its price by emitting commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayCost {
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
            let ticks = u64::try_from(ev.power).unwrap_or(0) * rule.ticks_per_power;
            ctx.out.push(Command::Recharge {
                spell: ev.spell,
                duration: ticks,
            });
        }
        ControlFlow::Continue(ev)
    });
}

/// A spell the caster can cast right now, checked the same way for the
/// command and for a preview.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ready {
    pub range: i32,
    school: School,
    power: i32,
    damage: SpellDamage,
}

/// `actor` casting `spell`: they must know it and a discipline must claim
/// its school and allow it. Handlers run on a cloned stream and a scratch
/// buffer, so asking changes nothing.
pub(crate) fn ready_cast(state: &GameState, actor: Entity, spell: Entity) -> Result<Ready, String> {
    let world = &state.world;
    let known = holding::attached(world, actor).contains(&spell);
    let (true, Ok(data), Ok(damage)) = (
        known,
        world.get::<&Spell>(spell).map(|s| *s),
        world.get::<&SpellDamage>(spell).map(|d| *d),
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
            spell,
            school: data.school,
            now: state.now,
            claimed: false,
            blocked: None,
        },
    ));
    if let Some(reason) = check.blocked {
        return Err(reason);
    }
    if !check.claimed {
        return Err(format!("You have no way to cast {name}."));
    }
    Ok(Ready {
        range: data.range,
        school: data.school,
        power: data.power,
        damage,
    })
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
    let bonus = state
        .world
        .get::<&Stats>(actor)
        .ok()
        .zip(ready.damage.stat)
        .map_or(0, |(stats, stat)| stats.modifier(stat));
    let hit = roll_strike(
        &mut state.rngs.combat,
        Strike {
            min: ready.damage.min,
            max: ready.damage.max,
            bonus,
            types: DamageTypes::default(),
            weapon: None,
        },
    );
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
            },
        );
        let spell = thing("firebolt")
            .spell(School::Pyromancy, 6, 2)
            .spell_damage(3, 3, None)
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
            .spell_damage(3, 3, None)
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
    fn the_same_seed_and_casts_give_the_same_damage() {
        let run = || {
            let (mut state, spell) = caster();
            let _ = state.world.insert_one(
                spell,
                SpellDamage {
                    min: 1,
                    max: 6,
                    stat: None,
                },
            );
            let target = goblin(&mut state, 3);
            cast_at(&mut state, spell, (3, 1));
            health(&state, target)
        };
        assert_eq!(run(), run());
    }
}
