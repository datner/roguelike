//! Aiming, for frontends (ADR 0013): which tiles are worth targeting and
//! what a shot at one would do, in plain data. Computed with the same
//! functions the commands use, so the preview cannot disagree with the shot.

use crate::ammo;
use crate::entity::{DamageTypes, Dead, Hostile, Name, Position};
use crate::ranged::{Ready, check, distance, flight, ready_fire, ready_throw};
use crate::state::GameState;
use crate::view::{ItemRef, held, total_armor};

/// What the player is aiming with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    /// A carried or equipped throwable.
    Throw(ItemRef),
    /// The wielded launcher, with the arrow it would use.
    Fire,
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

/// The shot as it would go now, or why it can't; for the player.
fn ready(state: &GameState, shot: Shot) -> Result<Ready, String> {
    match shot {
        Shot::Throw(item) => {
            let item = item
                .find(&state.world, &held(state))
                .ok_or_else(|| "You do not have that.".to_string())?;
            ready_throw(state, state.player, item)
        }
        Shot::Fire => ready_fire(state, state.player),
    }
}

fn origin(state: &GameState) -> Option<(i32, i32)> {
    let at = state.world.get::<&Position>(state.player).ok()?;
    Some((at.x, at.y))
}

/// What pressing "fire" should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireStep {
    /// Aim with `Shot::Fire`.
    Aim,
    /// The bow has no arrow ready but the pack has one: nock it first (a
    /// turn), then fire.
    Nock(ItemRef),
    /// Can't shoot, and nothing to do about it.
    Cannot(&'static str),
}

pub fn fire_step(state: &GameState) -> FireStep {
    let world = &state.world;
    let Some((_, launcher)) = ammo::wielded_launcher(world, state.player) else {
        return FireStep::Cannot("You have nothing to fire.");
    };
    if ready_fire(state, state.player).is_ok() {
        return FireStep::Aim;
    }
    match ammo::loose_ammo(world, state.player, launcher.ammo).and_then(|a| ItemRef::of(world, a)) {
        Some(arrow) => FireStep::Nock(arrow),
        None => FireStep::Cannot("You have no arrows."),
    }
}

/// Living hostiles the player can see and the shot could reach, nearest
/// first (ties by row, then column): where a cursor should start and cycle.
pub fn aim_targets(state: &GameState, shot: Shot) -> Vec<(i32, i32)> {
    let (Ok(ready), Some(origin)) = (ready(state, shot), origin(state)) else {
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
            distance(origin, at) <= ready.range
                && state.visible.get(at.0, at.1).copied().unwrap_or(false)
        })
        .collect();
    targets.sort_by_key(|&(x, y)| (distance(origin, (x, y)), y, x));
    targets
}

/// Where the player stands and how far `shot` reaches; `None` if the shot
/// is not available (no such item, no arrow).
pub fn aim_range(state: &GameState, shot: Shot) -> Option<((i32, i32), i32)> {
    Some((origin(state)?, ready(state, shot).ok()?.range))
}

/// What a shot at `at` would do; `None` if the shot is not available.
pub fn aim(state: &GameState, shot: Shot, at: (i32, i32)) -> Option<Aim> {
    let ready = ready(state, shot).ok()?;
    let origin = origin(state)?;
    let flight = flight(state, state.player, origin, at, ready.range);
    let (low, high) = ready.strike.range();
    let resisted = flight.hit.map_or(0, |target| {
        total_armor(&state.world, target).against(ready.strike.types)
    });
    let target = flight.hit.map(|target| {
        state
            .world
            .get::<&Name>(target)
            .map_or_else(|_| "creature".to_string(), |name| name.0.clone())
    });
    Some(Aim {
        origin,
        range: ready.range,
        path: flight.path,
        landing: flight.landing,
        target,
        damage: ((low - resisted).max(0), (high - resisted).max(0)),
        types: ready.strike.types,
        problem: check(state, state.player, &ready, at).err(),
    })
}
#[cfg(test)]
mod tests {
    use hecs::Entity;

    use super::*;
    use crate::entity::{AmmoKind, DamageType, Stat};
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
    fn bow(state: &mut GameState) {
        thing("bow")
            .item()
            .launcher(8, AmmoKind::Arrow, Stat::Agility)
            .equipped_on(state.player, 3)
            .make(state);
    }

    fn arrow() -> crate::spawn::Spawn {
        thing("arrow")
            .item()
            .ammo(AmmoKind::Arrow)
            .projectile(2, 2, &[DamageType::Piercing], 0)
    }

    #[test]
    fn pressing_fire_aims_nocks_or_explains() {
        let mut state = bare_state();
        assert_eq!(
            fire_step(&state),
            FireStep::Cannot("You have nothing to fire.")
        );

        bow(&mut state);
        assert_eq!(fire_step(&state), FireStep::Cannot("You have no arrows."));

        let loose = arrow().carried_by(state.player).make(&mut state);
        let loose = ItemRef::of(&state.world, loose).unwrap();
        assert_eq!(fire_step(&state), FireStep::Nock(loose));

        thing("quiver")
            .item()
            .quiver(AmmoKind::Arrow, 5)
            .equipped_on(state.player, 2)
            .make(&mut state);
        assert_eq!(
            fire_step(&state),
            FireStep::Nock(loose),
            "an empty quiver supplies nothing"
        );
    }

    #[test]
    fn a_fire_aim_previews_the_next_arrow() {
        let mut state = bare_state();
        bow(&mut state);
        let quiver = thing("quiver")
            .item()
            .quiver(AmmoKind::Arrow, 5)
            .equipped_on(state.player, 2)
            .make(&mut state);
        let arrow = arrow().make(&mut state);
        crate::holding::stow(&mut state.world, quiver, arrow);
        goblin(&mut state, 4, 1);
        see_everything(&mut state);

        assert_eq!(fire_step(&state), FireStep::Aim);
        assert_eq!(aim_targets(&state, Shot::Fire), [(4, 1)]);
        let aim = aim(&state, Shot::Fire, (4, 1)).unwrap();
        assert_eq!((aim.range, aim.damage), (8, (2, 2)));
        assert_eq!(aim.target.as_deref(), Some("goblin"));
    }
}
