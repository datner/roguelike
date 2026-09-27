//! Combat events and handlers, plus the shared rolls used by both the real
//! command applier and the side-effect-free `preview_damage`.

use std::ops::ControlFlow;

use hecs::{Entity, World};
use rand::RngExt;
use rand_pcg::Pcg64;

use crate::entity::{Armor, Health, Melee};
use crate::event::{Ctx, EventRegistry, settle};
use crate::prio;
use crate::state::GameState;

/// Fired at a target before damage is applied; handlers may reduce `amount`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeforeTakeDamage {
    pub target: Entity,
    pub amount: i32,
}

/// Notification fired after damage has been applied to `Health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Damaged {
    pub target: Entity,
    pub amount: i32,
}

pub fn register(registry: &mut EventRegistry) {
    registry.on(prio::ARMOR, armor_reduces_damage);
}

fn armor_reduces_damage(
    _ctx: &mut Ctx<'_>,
    armor: &Armor,
    mut ev: BeforeTakeDamage,
) -> ControlFlow<BeforeTakeDamage, BeforeTakeDamage> {
    ev.amount = (ev.amount - armor.av).max(0);
    if ev.amount == 0 {
        ControlFlow::Break(ev)
    } else {
        ControlFlow::Continue(ev)
    }
}

/// Raw damage from `attacker`'s `Melee`, or `None` if it can't attack.
pub(crate) fn roll_melee(world: &World, rng: &mut Pcg64, attacker: Entity) -> Option<i32> {
    let melee = *world.get::<&Melee>(attacker).ok()?;
    Some(rng.random_range(melee.min..=melee.max.max(melee.min)))
}

/// Damage the next real `Attack { attacker, target }` would deal, computed on
/// a cloned RNG with commands emitted into a scratch buffer. Leaves the world,
/// RNG, log and queue untouched. `None` if the attacker has no `Melee` or the
/// target has no `Health`.
pub fn preview_damage(state: &GameState, attacker: Entity, target: Entity) -> Option<i32> {
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
        },
    ));
    Some(ev.amount.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply_commands};
    use crate::entity::{Attachments, Name, Position};
    use crate::state::tests::bare_state;

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
        let armor = state.world.spawn((Name("plate".into()), Armor { av }));
        let dummy = state.world.spawn((
            Name("dummy".into()),
            Position { x: 5, y: 5 },
            Health {
                current: 10,
                max: 10,
            },
            Attachments(vec![armor]),
        ));
        (dummy, armor)
    }

    #[test]
    fn armor_on_an_attachment_reduces_damage() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 2);
        state.commands.push_back(Command::DealDamage {
            target: dummy,
            amount: 5,
        });
        apply_commands(&mut state);
        assert_eq!(health(&state, dummy), 7);
        assert_eq!(last_log(&state), "The dummy takes 3 damage (2 blocked).");
    }

    #[test]
    fn fully_absorbed_damage_leaves_health_unchanged() {
        let mut state = bare_state();
        let (dummy, _) = armored_dummy(&mut state, 2);
        state.commands.push_back(Command::DealDamage {
            target: dummy,
            amount: 2,
        });
        apply_commands(&mut state);
        assert_eq!(health(&state, dummy), 10);
        assert_eq!(last_log(&state), "The dummy's armor absorbs the blow.");
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

            state.commands.push_back(Command::Attack {
                attacker: state.player,
                target: dummy,
            });
            apply_commands(&mut state);
            assert_eq!(health_before - health(&state, dummy), preview);
        }
    }

    #[test]
    fn preview_needs_melee_and_health() {
        let mut state = bare_state();
        let rock = state.world.spawn((Name("rock".into()),));
        assert_eq!(preview_damage(&state, state.player, rock), None);
        assert_eq!(preview_damage(&state, rock, state.player), None);
    }
}
