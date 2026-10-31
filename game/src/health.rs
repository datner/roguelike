//! Health that grows (docs/plans/sturdiness.md): max health is the blueprint's base plus what
//! Toughness adds, and Toughness grows by being hurt in a fight. One place pays the skill off, as
//! ADR 0017 asks. Sturdiness stays a composite: armor, resistances and avoidance are their own
//! layers, grown elsewhere.

use hecs::Entity;

use crate::command::DamageCause;
use crate::entity::{BaseHealth, Hardiness, Health, Wounds};
use crate::skill;
use crate::state::GameState;

/// A new holder's base, and its max health for the Toughness it starts with.
pub(crate) fn start(state: &mut GameState, entity: Entity) {
    if !state.world.satisfies::<&Hardiness>(entity) {
        return;
    }
    if let Ok(max) = state.world.get::<&Health>(entity).map(|health| health.max) {
        let _ = state.world.insert_one(entity, BaseHealth(max));
    }
    refresh(state, entity);
}

/// Max health from Toughness right now: `per_level` per level.
fn from_toughness(state: &GameState, entity: Entity) -> i32 {
    let Ok(per_level) = state.world.get::<&Hardiness>(entity).map(|h| h.per_level) else {
        return 0;
    };
    let level = state.blueprints.skills().core().map_or(0, |core| {
        skill::level(skill::xp(&state.world, entity, core.toughness))
    });
    per_level * i32::try_from(level).unwrap_or(0)
}

/// Max health brought up to date with Toughness. What it gains is healed:
/// growing tougher never leaves you hurt.
pub(crate) fn refresh(state: &mut GameState, entity: Entity) {
    let Ok(base) = state.world.get::<&BaseHealth>(entity).map(|base| base.0) else {
        return;
    };
    let max = base + from_toughness(state, entity);
    if let Ok(mut health) = state.world.get::<&mut Health>(entity) {
        let grown = max - health.max;
        health.max = max;
        health.current = (health.current + grown.max(0)).min(max);
    }
}

/// `target` took `dealt` from `cause`: hurt by an enemy, it trains Toughness
/// (stakes, so familiarity and novelty apply) and owes the wound for
/// Recovery to heal back. A fall is not a fight.
pub(crate) fn hurt(state: &mut GameState, target: Entity, cause: DamageCause, dealt: i32) {
    let source = match cause {
        DamageCause::Melee { attacker, .. } | DamageCause::Ranged { attacker, .. } => attacker,
        DamageCause::Spell { caster, .. } => caster,
        DamageCause::Effect { effect } => effect,
        DamageCause::Fall => return,
    };
    if dealt <= 0 || !state.world.satisfies::<&crate::entity::Learner>(target) {
        return;
    }
    if let Ok(mut wounds) = state.world.get::<&mut Wounds>(target) {
        wounds.owed += u32::try_from(dealt).unwrap_or(0);
    } else {
        let owed = u32::try_from(dealt).unwrap_or(0);
        let _ = state.world.insert_one(target, Wounds { owed });
    }
    let Some(core) = state.blueprints.skills().core() else {
        return;
    };
    skill::train(state, target, &[core.toughness], source, skill::BASE_XP);
    refresh(state, target);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::blueprint::Blueprints;
    use crate::command::Command;
    use crate::entity::Skills;
    use crate::state::tests::bare_state_with;

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    fn health(state: &GameState) -> (i32, i32) {
        let health = *state.world.get::<&Health>(state.player).unwrap();
        (health.current, health.max)
    }

    fn toughness_xp(state: &GameState) -> u32 {
        let core = state.blueprints.skills().core().unwrap();
        skill::xp(&state.world, state.player, core.toughness)
    }

    #[test]
    fn origins_start_at_their_own_health() {
        for (origin, max) in [("PitFighter", 26), ("HedgeMage", 16), ("Wanderer", 22)] {
            let state = GameState::new_as(1, data(), Some(origin)).unwrap();
            assert_eq!(health(&state), (max, max), "{origin}");
        }
    }

    #[test]
    fn a_blow_from_an_enemy_trains_toughness_and_a_fall_does_not() {
        let mut state = bare_state_with(data());
        let goblin = state
            .spawn(crate::spawn::Spawn::blueprint("Goblin").at(2, 1))
            .unwrap();
        let player = state.player;
        state.run(Command::DealDamage {
            target: player,
            amount: 2,
            cause: DamageCause::Fall,
        });
        assert_eq!(toughness_xp(&state), 0);
        state.run(Command::DealDamage {
            target: player,
            amount: 2,
            cause: DamageCause::Melee {
                attacker: goblin,
                types: crate::entity::DamageTypes::default(),
                graze: false,
            },
        });
        assert!(toughness_xp(&state) > 0);
        assert_eq!(
            state.world.get::<&Wounds>(player).unwrap().owed,
            2,
            "the fall is no wound"
        );
    }

    #[test]
    fn a_level_of_toughness_raises_max_health_and_heals_the_gain() {
        let mut state = bare_state_with(data());
        let player = state.player;
        state.world.get::<&mut Health>(player).unwrap().current = 10;
        let core = state.blueprints.skills().core().unwrap();
        let mut skills = Skills::default();
        skills.set(core.toughness, skill::xp_for(3));
        state.world.insert_one(player, skills).unwrap();
        refresh(&mut state, player);
        assert_eq!(health(&state), (16, 28));
        let mut skills = Skills::default();
        skills.set(core.toughness, skill::xp_for(10));
        state.world.insert_one(player, skills).unwrap();
        refresh(&mut state, player);
        assert_eq!(health(&state).1, 42, "a veteran at Toughness 10");
    }
}
