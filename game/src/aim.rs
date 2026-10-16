//! Aiming, for frontends (ADR 0013): which tiles are worth targeting and
//! what a shot at one would do, in plain data. Computed with the same
//! functions the commands use, so the preview cannot disagree with the shot.

use hecs::Entity;

use crate::combat::throw_strike;
use crate::entity::{DamageTypes, Dead, Hostile, Name, Position, Throwable};
use crate::ranged::{check_throw, distance, flight};
use crate::state::GameState;
use crate::view::{ItemRef, held, total_armor};

/// What the player is aiming with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    Throw(ItemRef),
}

/// What a shot at one tile would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aim {
    pub origin: (i32, i32),
    pub range: i32,
    /// Tiles the missile passes, the landing tile last.
    pub path: Vec<(i32, i32)>,
    pub landing: (i32, i32),
    /// The creature it would hit, by bare noun.
    pub target: Option<String>,
    /// Damage range after the target's armor (before, with no target).
    pub damage: (i32, i32),
    pub types: DamageTypes,
    /// Why the shot can't be taken, if it can't.
    pub problem: Option<String>,
}

struct Resolved {
    item: Entity,
    throwable: Throwable,
    origin: (i32, i32),
}

fn resolve(state: &GameState, shot: Shot) -> Option<Resolved> {
    let Shot::Throw(item) = shot;
    let item = item.find(&state.world, &held(state))?;
    let throwable = *state.world.get::<&Throwable>(item).ok()?;
    let origin = state
        .world
        .get::<&Position>(state.player)
        .ok()
        .map(|pos| (pos.x, pos.y))?;
    Some(Resolved {
        item,
        throwable,
        origin,
    })
}

/// Living hostiles the player can see and the shot could reach, nearest
/// first (ties by row, then column): where a cursor should start and cycle.
pub fn aim_targets(state: &GameState, shot: Shot) -> Vec<(i32, i32)> {
    let Some(resolved) = resolve(state, shot) else {
        return Vec::new();
    };
    let mut targets: Vec<(i32, i32)> = state
        .world
        .query::<&Position>()
        .with::<&Hostile>()
        .without::<&Dead>()
        .iter()
        .map(|pos| (pos.x, pos.y))
        .filter(|&at| {
            distance(resolved.origin, at) <= resolved.throwable.range
                && state.visible.get(at.0, at.1).copied().unwrap_or(false)
        })
        .collect();
    targets.sort_by_key(|&(x, y)| (distance(resolved.origin, (x, y)), y, x));
    targets
}

/// Where the player stands and how far `shot` reaches; `None` if the shot is
/// no longer available (the item is gone).
pub fn aim_range(state: &GameState, shot: Shot) -> Option<((i32, i32), i32)> {
    let resolved = resolve(state, shot)?;
    Some((resolved.origin, resolved.throwable.range))
}

/// What a shot at `at` would do; `None` if the shot is no longer available.
pub fn aim(state: &GameState, shot: Shot, at: (i32, i32)) -> Option<Aim> {
    let resolved = resolve(state, shot)?;
    let strike = throw_strike(&state.world, state.player, resolved.item)?;
    let flight = flight(
        state,
        state.player,
        resolved.origin,
        at,
        resolved.throwable.range,
    );
    let (low, high) = strike.range();
    let resisted = flight.hit.map_or(0, |target| {
        total_armor(&state.world, target).against(strike.types)
    });
    let target = flight.hit.map(|target| {
        state
            .world
            .get::<&Name>(target)
            .map_or_else(|_| "creature".to_string(), |name| name.0.clone())
    });
    Some(Aim {
        origin: resolved.origin,
        range: resolved.throwable.range,
        path: flight.path,
        landing: flight.landing,
        target,
        damage: ((low - resisted).max(0), (high - resisted).max(0)),
        types: strike.types,
        problem: check_throw(state, state.player, resolved.item, at).err(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{DamageType, Stat};
    use crate::state::tests::bare_state;
    use crate::testkit::{see_everything, thing};

    fn knife(state: &mut GameState) -> Shot {
        let item = thing("throwing knife")
            .throwable(
                6,
                (1, 2),
                Stat::Agility,
                &[DamageType::Slashing, DamageType::Piercing],
            )
            .item()
            .carried_by(state.player)
            .make(state);
        Shot::Throw(ItemRef::of(&state.world, item).unwrap())
    }

    fn goblin(state: &mut GameState, x: i32, y: i32) -> Entity {
        thing("goblin").at(x, y).health(5).hostile().make(state)
    }

    #[test]
    fn targets_are_visible_hostiles_in_range_nearest_first() {
        let mut state = bare_state();
        let shot = knife(&mut state);
        goblin(&mut state, 5, 1);
        goblin(&mut state, 3, 2);
        goblin(&mut state, 12, 1);
        see_everything(&mut state);
        assert_eq!(aim_targets(&state, shot), [(3, 2), (5, 1)]);
        state.visible.set(3, 2, false);
        assert_eq!(aim_targets(&state, shot), [(5, 1)]);
    }

    #[test]
    fn an_aim_reports_the_target_and_damage_after_armor() {
        let mut state = bare_state();
        let shot = knife(&mut state);
        let target = goblin(&mut state, 4, 1);
        thing("mail")
            .with(crate::entity::Armor {
                slashing: 1,
                ..Default::default()
            })
            .attached_to(target)
            .make(&mut state);
        see_everything(&mut state);
        let aim = aim(&state, shot, (4, 1)).unwrap();
        assert_eq!(aim.target.as_deref(), Some("goblin"));
        assert_eq!(
            (aim.landing, aim.path.len(), aim.problem),
            ((4, 1), 3, None)
        );
        // 1-2 dice; armor takes the weaker of slashing 1 / piercing 0.
        assert_eq!(aim.damage, (1, 2));
    }

    #[test]
    fn an_aim_says_why_it_cannot_be_taken() {
        let mut state = bare_state();
        let shot = knife(&mut state);
        see_everything(&mut state);
        let far = aim(&state, shot, (15, 1)).unwrap();
        assert_eq!(far.problem.as_deref(), Some("That is out of range."));
        state.visible.set(3, 1, false);
        let hidden = aim(&state, shot, (3, 1)).unwrap();
        assert_eq!(hidden.problem.as_deref(), Some("You can't see that spot."));
    }
}
