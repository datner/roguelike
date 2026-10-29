//! Aiming, for frontends (ADR 0013): which tiles are worth targeting and
//! what a shot at one would do, in plain data. Computed with the same
//! functions the commands use, so the preview cannot disagree with the shot.

use hecs::Entity;

use crate::ammo;
use crate::combat::{self, Strike};
use crate::entity::{DamageTypes, Dead, Health, Name, Position};
use crate::ranged::{check_range, distance, flight, point_blank, ready_fire, ready_throw};
use crate::side;
use crate::spell::{check_target, footprint, ready_cast};
use crate::state::GameState;
use crate::technique;
use crate::view::{ItemRef, castable, held, total_armor};

/// What the player is aiming with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    /// A carried or equipped throwable.
    Throw(ItemRef),
    /// The wielded launcher, with the arrow it would use.
    Fire,
    /// A known spell.
    Cast(ItemRef),
    /// A known technique (ADR 0024).
    Technique(ItemRef),
}

/// How far a shot reaches and what it hits for: the part of a missile's or
/// a spell's readiness that aiming needs.
struct Ready {
    range: i32,
    /// `None` for a spell that does no damage.
    strike: Option<Strike>,
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
    /// The tiles an area spell would cover, nearest its centre first; empty
    /// for anything that hits one creature.
    pub area: Vec<(i32, i32)>,
    /// How many creatures it would affect.
    pub caught: usize,
    /// Where a summoning would put its creatures; the preview draws from a
    /// clone of the stream the cast uses, so they agree.
    pub summons: Vec<(i32, i32)>,
    /// How a missile would meet its target, from the odds the shot rolls
    /// (ADR 0017); `None` without a target, and for spells.
    pub chances: Option<Chances>,
    /// A launcher fired with an enemy alongside: its aim suffers
    /// (`ranged::POINT_BLANK`), already counted in `chances`.
    pub point_blank: bool,
}

/// Chances, out of 10 000, of each way a shot can land that does damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chances {
    pub hit_bp: u32,
    /// Missed, not dodged perfectly, grazed and not deflected.
    pub graze_bp: u32,
}

/// The shot as it would go now, or why it can't; for the player.
fn ready(state: &GameState, shot: Shot) -> Result<Ready, String> {
    let (range, strike) = match shot {
        Shot::Throw(item) => {
            let item = item
                .find(&state.world, &held(state))
                .ok_or_else(|| "You do not have that.".to_string())?;
            let ready = ready_throw(state, state.player, item)?;
            (ready.range, Some(ready.strike))
        }
        Shot::Fire => {
            let ready = ready_fire(state, state.player)?;
            (ready.range, Some(ready.strike))
        }
        Shot::Cast(spell) => {
            let spell = spell
                .find(&state.world, &castable(state))
                .ok_or_else(|| "You don't know that spell.".to_string())?;
            let ready = ready_cast(state, state.player, spell)?;
            (ready.range, ready.strike)
        }
        Shot::Technique(technique) => {
            let ready = technique::ready(state, state.player, find_technique(state, technique)?)?;
            (ready.reach, Some(ready.strike))
        }
    };
    Ok(Ready { range, strike })
}

fn find_technique(state: &GameState, technique: ItemRef) -> Result<Entity, String> {
    technique
        .find(&state.world, &technique::known(&state.world, state.player))
        .ok_or_else(|| "You don't know that technique.".to_string())
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

/// Living enemies the player can see and the shot could reach, nearest
/// first (ties by row, then column): where a cursor should start and cycle.
pub fn aim_targets(state: &GameState, shot: Shot) -> Vec<(i32, i32)> {
    let (Ok(ready), Some(origin)) = (ready(state, shot), origin(state)) else {
        return Vec::new();
    };
    let mut targets: Vec<(i32, i32)> = state
        .world
        .query::<(Entity, &Position)>()
        .with::<&Health>()
        .without::<&Dead>()
        .iter()
        .filter(|&(entity, _)| side::enemies(&state.world, state.player, entity))
        .map(|(_, pos)| (pos.x, pos.y))
        .filter(|&at| {
            distance(origin, at) <= ready.range
                && state.visible.get(at.0, at.1).copied().unwrap_or(false)
                && !crate::fov::hidden_in_grass(state, Position { x: at.0, y: at.1 })
        })
        .filter(|&at| match shot {
            // Only what the technique could strike from here.
            Shot::Technique(technique) => {
                aim_technique(state, technique, at).is_some_and(|aim| aim.problem.is_none())
            }
            _ => true,
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
    match shot {
        Shot::Cast(spell) => return aim_cast(state, spell, at),
        Shot::Technique(technique) => return aim_technique(state, technique, at),
        Shot::Throw(_) | Shot::Fire => {}
    }
    let ready = ready(state, shot).ok()?;
    let strike = ready.strike?;
    let origin = origin(state)?;
    let flight = flight(state, state.player, origin, at, ready.range);
    let (low, high) = strike.range();
    let resisted = flight.hit.map_or(0, |target| {
        total_armor(&state.world, target).against(strike.types)
    });
    Some(Aim {
        origin,
        range: ready.range,
        path: flight.path,
        landing: flight.landing,
        target: flight.hit.map(|target| noun(state, target)),
        damage: ((low - resisted).max(0), (high - resisted).max(0)),
        types: strike.types,
        problem: check_range(state, state.player, ready.range, at).err(),
        area: Vec::new(),
        caught: usize::from(flight.hit.is_some()),
        summons: Vec::new(),
        chances: flight.hit.map(|target| {
            let odds = combat::odds(state, &strike, state.player, target);
            Chances {
                hit_bp: odds.hit_bp,
                graze_bp: odds.graze_overall_bp(),
            }
        }),
        point_blank: matches!(shot, Shot::Fire) && point_blank(state, state.player),
    })
}

/// A spell's `aim`, from the footprint its cast would use. A bolt's damage
/// is shown after its target's resistance, as a missile's is after armor;
/// an area's before, since each creature caught resists on its own.
fn aim_cast(state: &GameState, spell: ItemRef, at: (i32, i32)) -> Option<Aim> {
    let spell = spell.find(&state.world, &castable(state))?;
    let ready = ready_cast(state, state.player, spell).ok()?;
    let footprint = footprint(
        state,
        state.player,
        &ready,
        at,
        &mut state.rngs.spell.clone(),
    )?;
    Some(Aim {
        origin: origin(state)?,
        range: ready.range,
        target: footprint.caught.first().map(|&target| noun(state, target)),
        damage: ready.strike.map_or((0, 0), |strike| {
            let (low, high) = strike.range();
            let resisted = match (ready.area, footprint.caught.first()) {
                (None, Some(&target)) => total_armor(&state.world, target).against(strike.types),
                _ => 0,
            };
            ((low - resisted).max(0), (high - resisted).max(0))
        }),
        types: ready
            .strike
            .map_or_else(DamageTypes::default, |strike| strike.types),
        problem: check_target(state, state.player, &ready, at).err(),
        caught: footprint.caught.len(),
        path: footprint.path,
        landing: footprint.landing,
        area: footprint.area,
        summons: footprint.placed,
        chances: None,
        point_blank: false,
    })
}

/// A technique's `aim`, from the footprint its performance would use: the
/// ground its performer crosses, whom it strikes, the damage after armor and
/// the odds of the blow.
fn aim_technique(state: &GameState, technique: ItemRef, at: (i32, i32)) -> Option<Aim> {
    let technique = find_technique(state, technique).ok()?;
    let ready = technique::ready(state, state.player, technique).ok()?;
    let origin = origin(state)?;
    let footprint = technique::footprint(state, state.player, &ready, at);
    let targets = footprint
        .as_ref()
        .map_or_else(|_| Vec::new(), |footprint| footprint.targets.clone());
    // One blow is shown after its target's armor and with its odds; an area's
    // before, since each creature caught meets the blow on its own.
    let single = targets.first().copied().filter(|_| ready.area.is_none());
    // A shot is previewed from where it would fly, as `perform` fires it.
    let stand = footprint.as_ref().ok().map(|footprint| footprint.stand);
    let shot = stand
        .filter(|_| ready.loose)
        .and_then(|stand| technique::fired(state, state.player, &ready, stand).ok());
    let strike = shot.as_ref().map_or(ready.strike, |shot| shot.strike);
    let scale = |amount| match shot {
        Some(_) => amount,
        None => ready.mods.scale(amount),
    };
    let (low, high) = strike.range();
    let resisted = single.map_or(0, |target| {
        total_armor(&state.world, target).against(strike.types)
    });
    let step = ((at.0 - origin.0).signum(), (at.1 - origin.1).signum());
    let path = match (ready.area, &shot, stand) {
        (Some(_), ..) => Vec::new(),
        (None, Some(shot), Some(stand)) => flight(state, state.player, stand, at, shot.range).path,
        (None, ..) => (1..=distance(origin, at).min(ready.reach))
            .map(|k| (origin.0 + step.0 * k, origin.1 + step.1 * k))
            .collect(),
    };
    Some(Aim {
        origin,
        range: ready.reach,
        path,
        landing: at,
        target: targets.first().map(|&target| noun(state, target)),
        damage: (
            (scale(low) - resisted).max(0),
            (scale(high) - resisted).max(0),
        ),
        types: strike.types,
        problem: footprint.as_ref().err().cloned(),
        area: footprint.map_or_else(|_| Vec::new(), |footprint| footprint.area),
        caught: targets.len(),
        summons: Vec::new(),
        chances: single.map(|target| {
            let odds = combat::odds(state, &strike, state.player, target);
            Chances {
                hit_bp: odds.hit_bp,
                graze_bp: odds.graze_overall_bp(),
            }
        }),
        point_blank: shot.is_some()
            && stand.is_some_and(|stand| crate::ranged::point_blank_at(state, state.player, stand)),
    })
}

fn noun(state: &GameState, entity: Entity) -> String {
    state
        .world
        .get::<&Name>(entity)
        .map_or_else(|_| "creature".to_string(), |name| name.0.clone())
}

#[cfg(test)]
mod tests {
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
        // Equals: 75% to hit; of the other 25%, 30% graze.
        assert_eq!(
            aim.chances,
            Some(Chances {
                hit_bp: 7_500,
                graze_bp: 750
            })
        );
        assert_eq!(super::aim(&state, shot, (1, 4)).unwrap().chances, None);
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

    /// A bow with a stocked quiver, a goblin to shoot at (4, 1), and the hit
    /// chance at it.
    fn shooting() -> (GameState, impl Fn(&GameState) -> (u32, bool)) {
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
        let chance = |state: &GameState| {
            let aim = aim(state, Shot::Fire, (4, 1)).unwrap();
            (aim.chances.unwrap().hit_bp, aim.point_blank)
        };
        (state, chance)
    }

    #[test]
    fn shooting_with_an_enemy_alongside_is_harder() {
        let (mut state, chance) = shooting();
        let (open, close) = chance(&state);
        assert!(!close);

        thing("ally").at(1, 2).health(5).make(&mut state);
        assert_eq!(
            chance(&state),
            (open, false),
            "an ally alongside is no bother"
        );
        goblin(&mut state, 3, 3);
        assert_eq!(
            chance(&state),
            (open, false),
            "two tiles off is room enough"
        );

        let close = goblin(&mut state, 2, 2);
        let (pressed, flagged) = chance(&state);
        assert!(flagged && pressed < open, "{pressed} vs {open}");
        let ready = crate::ranged::ready_fire(&state, state.player).unwrap();
        let _ = state.world.despawn(close);
        let calm = crate::ranged::ready_fire(&state, state.player).unwrap();
        assert_eq!(
            ready.strike.accuracy,
            calm.strike.accuracy - crate::ranged::POINT_BLANK,
            "the shot rolls the odds the preview shows"
        );
    }

    #[test]
    fn a_thrown_knife_is_no_harder_up_close() {
        let mut state = bare_state();
        let shot = knife(&mut state);
        goblin(&mut state, 4, 1);
        see_everything(&mut state);
        let chance = |state: &GameState| aim(state, shot, (4, 1)).unwrap().chances;
        let open = chance(&state);
        goblin(&mut state, 2, 2);
        assert_eq!(chance(&state), open);
        assert!(!aim(&state, shot, (4, 1)).unwrap().point_blank);
    }

    fn firebolt(state: &mut GameState) -> (Entity, Shot) {
        use crate::entity::{CooldownCasting, School, Stat};
        let _ = state.world.insert_one(
            state.player,
            CooldownCasting {
                school: School::Pyromancy,
                ticks_per_power: 300,
                stat: Stat::Willpower,
            },
        );
        let spell = thing("firebolt")
            .spell(School::Pyromancy, 6, 2)
            .spell_damage(4, 4)
            .attached_to(state.player)
            .make(state);
        (spell, Shot::Cast(ItemRef::of(&state.world, spell).unwrap()))
    }

    #[test]
    fn a_cast_aim_agrees_with_the_cast_and_armor_does_not_count() {
        let mut state = bare_state();
        let (spell, shot) = firebolt(&mut state);
        let spell = ItemRef::of(&state.world, spell).unwrap();
        let near = thing("goblin")
            .at(3, 1)
            .health(10)
            .armor(3)
            .hostile()
            .make(&mut state);
        goblin(&mut state, 5, 1);
        see_everything(&mut state);

        assert_eq!(aim_targets(&state, shot), [(3, 1), (5, 1)]);
        let preview = aim(&state, shot, (5, 1)).unwrap();
        assert_eq!(preview.target.as_deref(), Some("goblin"));
        assert_eq!(
            (preview.landing, preview.damage, preview.problem),
            ((3, 1), (4, 4), None)
        );

        crate::time::take_turn(
            &mut state,
            crate::input::PlayerAction::Cast { spell, at: (5, 1) },
        );
        let health = state.world.get::<&crate::entity::Health>(near).unwrap();
        assert_eq!(health.current, 10 - 4);
    }

    #[test]
    fn a_recharging_spell_cannot_be_aimed() {
        let mut state = bare_state();
        let (spell, shot) = firebolt(&mut state);
        see_everything(&mut state);
        assert!(aim_range(&state, shot).is_some());
        crate::spell::recharge(&mut state, spell, 600);
        assert_eq!(aim_range(&state, shot), None);
        assert!(aim_targets(&state, shot).is_empty());
    }

    #[test]
    fn an_area_aim_shows_the_ground_and_how_many_it_catches() {
        use crate::entity::{Hits, Shape};
        let mut state = bare_state();
        let (bolt, _) = firebolt(&mut state);
        let _ = state.world.insert_one(
            bolt,
            crate::entity::Area {
                shape: Shape::Burst { radius: 1 },
                hits: Hits::All,
                falloff: 0,
            },
        );
        let shot = Shot::Cast(ItemRef::of(&state.world, bolt).unwrap());
        goblin(&mut state, 5, 1);
        goblin(&mut state, 6, 2);
        goblin(&mut state, 8, 1);
        see_everything(&mut state);

        let preview = aim(&state, shot, (5, 1)).unwrap();
        // Against the top wall: the row above is not open ground.
        assert_eq!((preview.landing, preview.area.len()), ((5, 1), 6));
        assert_eq!(preview.area[0], (5, 1), "the centre first");
        assert_eq!((preview.caught, preview.problem), (2, None));
    }
}
