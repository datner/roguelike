//! Ammunition handling (ADR 0013): which launcher is wielded, nocking an
//! arrow, and the quiver. A nocked arrow is the only thing in its
//! launcher's `Inventory`; a quiver's arrows are its `Inventory`, so both
//! stay inert and are saved and despawned with their holder.

use hecs::{Entity, World};

use crate::entity::{Ammo, AmmoKind, Launcher, Quiver};
use crate::equipment::equipped;
use crate::grammar::Subject;
use crate::holding;
use crate::state::GameState;

/// The launcher `actor` wields: the one on their lowest-numbered body part.
pub(crate) fn wielded_launcher(world: &World, actor: Entity) -> Option<(Entity, Launcher)> {
    let mut worn = equipped(world, actor);
    worn.sort_by_key(|&(_, part)| part);
    worn.into_iter()
        .find_map(|(item, _)| Some((item, *world.get::<&Launcher>(item).ok()?)))
}

/// `actor`'s equipped quivers for `kind`, lowest body part first.
fn quivers(world: &World, actor: Entity, kind: AmmoKind) -> Vec<(Entity, Quiver)> {
    let mut worn = equipped(world, actor);
    worn.sort_by_key(|&(_, part)| part);
    worn.into_iter()
        .filter_map(|(item, _)| Some((item, *world.get::<&Quiver>(item).ok()?)))
        .filter(|&(_, quiver)| quiver.ammo == kind)
        .collect()
}

fn kind_of(world: &World, item: Entity) -> Option<AmmoKind> {
    world.get::<&Ammo>(item).ok().map(|ammo| ammo.0)
}

/// The arrow `launcher` would fire next, and what holds it: the nocked one
/// first, otherwise the first in an equipped quiver.
pub(crate) fn next_arrow(
    world: &World,
    actor: Entity,
    launcher: Entity,
    kind: AmmoKind,
) -> Option<(Entity, Entity)> {
    let nocked = holding::carried(world, launcher)
        .into_iter()
        .find(|&arrow| kind_of(world, arrow) == Some(kind));
    if let Some(arrow) = nocked {
        return Some((arrow, launcher));
    }
    quivers(world, actor, kind)
        .into_iter()
        .find_map(|(quiver, _)| Some((*holding::carried(world, quiver).first()?, quiver)))
}

/// A loose arrow in the pack that `kind` of launcher can nock, oldest first.
pub(crate) fn loose_ammo(world: &World, actor: Entity, kind: AmmoKind) -> Option<Entity> {
    holding::carried(world, actor)
        .into_iter()
        .find(|&item| kind_of(world, item) == Some(kind))
}

/// The nocked arrow of `launcher`, if any.
pub(crate) fn nocked(world: &World, launcher: Entity) -> Option<Entity> {
    holding::carried(world, launcher).first().copied()
}

/// How many arrows `quiver` holds.
pub(crate) fn stored(world: &World, quiver: Entity) -> usize {
    holding::carried(world, quiver).len()
}

/// Why `actor` can't nock `arrow`, or the launcher it would go on.
pub(crate) fn check_nock(
    state: &GameState,
    actor: Entity,
    arrow: Entity,
) -> Result<Entity, String> {
    let world = &state.world;
    if !holding::carried(world, actor).contains(&arrow) {
        return Err("You do not have that.".to_string());
    }
    let Some(kind) = kind_of(world, arrow) else {
        return Err("You can't nock that.".to_string());
    };
    let Some((launcher, spec)) = wielded_launcher(world, actor) else {
        return Err("You have nothing to nock it to.".to_string());
    };
    if spec.ammo != kind {
        return Err("That doesn't fit your weapon.".to_string());
    }
    Ok(launcher)
}

/// Applies `Command::Nock`: the arrow goes from the pack onto the launcher;
/// one already nocked goes back to the pack.
pub(crate) fn nock(state: &mut GameState, actor: Entity, arrow: Entity) {
    let Ok(launcher) = check_nock(state, actor, arrow) else {
        return;
    };
    if let Some(old) = nocked(&state.world, launcher) {
        holding::unstow(&mut state.world, launcher, old);
        holding::stow(&mut state.world, actor, old);
    }
    holding::unstow(&mut state.world, actor, arrow);
    holding::stow(&mut state.world, launcher, arrow);
    let what = Subject::of(&state.world, arrow);
    let message = Subject::of(&state.world, actor).sentence("nock", what.noun());
    state.log.push(message);
}

/// An equipped quiver with room for `arrow`.
pub(crate) fn quiver_with_room(world: &World, actor: Entity, arrow: Entity) -> Option<Entity> {
    let kind = kind_of(world, arrow)?;
    quivers(world, actor, kind)
        .into_iter()
        .find(|&(quiver, spec)| stored(world, quiver) < spec.capacity as usize)
        .map(|(quiver, _)| quiver)
}

/// Why `actor` can't put `arrow` in a quiver, or the quiver it would go in.
pub(crate) fn check_quiver(
    state: &GameState,
    actor: Entity,
    arrow: Entity,
) -> Result<Entity, String> {
    let world = &state.world;
    if !holding::carried(world, actor).contains(&arrow) {
        return Err("You do not have that.".to_string());
    }
    if kind_of(world, arrow).is_none() {
        return Err("That doesn't go in a quiver.".to_string());
    }
    quiver_with_room(world, actor, arrow)
        .ok_or_else(|| "You have no quiver with room for that.".to_string())
}

/// Applies `Command::Quiver`: loose ammunition from the pack into the quiver.
pub(crate) fn quiver(state: &mut GameState, actor: Entity, arrow: Entity) {
    let Ok(quiver) = check_quiver(state, actor, arrow) else {
        return;
    };
    holding::unstow(&mut state.world, actor, arrow);
    holding::stow(&mut state.world, quiver, arrow);
    log_quivered(state, actor, arrow);
}

/// Picking up ammunition fills an equipped quiver before the pack (ADR
/// 0013). Returns whether `arrow` went in one.
pub(crate) fn stow_found(state: &mut GameState, actor: Entity, arrow: Entity) -> bool {
    let Some(quiver) = quiver_with_room(&state.world, actor, arrow) else {
        return false;
    };
    holding::stow(&mut state.world, quiver, arrow);
    log_quivered(state, actor, arrow);
    true
}

fn log_quivered(state: &mut GameState, actor: Entity, arrow: Entity) {
    let you = Subject::of(&state.world, actor);
    let what = Subject::of(&state.world, arrow);
    let rest = format!("{} in {} quiver", what.noun(), you.possessive());
    state.log.push(you.sentence("put", &rest));
}

/// A launcher leaving `actor`'s hands gives its nocked arrow back to the pack.
pub(crate) fn return_nocked(world: &mut World, actor: Entity, launcher: Entity) {
    for arrow in holding::carried(world, launcher) {
        holding::unstow(world, launcher, arrow);
        holding::stow(world, actor, arrow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::entity::{DamageType, Stat};
    use crate::spawn::Spawn;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};

    fn bow(state: &mut GameState) -> Entity {
        thing("bow")
            .item()
            .launcher(8, AmmoKind::Arrow, Stat::Agility)
            .equipped_on(state.player, 3)
            .make(state)
    }

    fn quiver_of(state: &mut GameState, capacity: u32) -> Entity {
        thing("quiver")
            .item()
            .quiver(AmmoKind::Arrow, capacity)
            .equipped_on(state.player, 2)
            .make(state)
    }

    fn arrow() -> Spawn {
        thing("arrow")
            .item()
            .ammo(AmmoKind::Arrow)
            .projectile(1, 1, &[DamageType::Piercing], 0)
    }

    #[test]
    fn nocking_moves_an_arrow_from_the_pack_to_the_bow() {
        let mut state = bare_state();
        let bow = bow(&mut state);
        let spec = arrow().carried_by(state.player).make(&mut state);
        state.run(Command::Nock {
            actor: state.player,
            arrow: spec,
        });
        assert_eq!(nocked(&state.world, bow), Some(spec));
        assert!(holding::carried(&state.world, state.player).is_empty());
        assert_eq!(last_log(&state), "You nock the arrow.");
    }

    #[test]
    fn nocking_over_a_nocked_arrow_swaps_it_back_to_the_pack() {
        let mut state = bare_state();
        let bow = bow(&mut state);
        let first = arrow().carried_by(state.player).make(&mut state);
        let second = arrow().carried_by(state.player).make(&mut state);
        for arrow in [first, second] {
            state.run(Command::Nock {
                actor: state.player,
                arrow,
            });
        }
        assert_eq!(nocked(&state.world, bow), Some(second));
        assert_eq!(holding::carried(&state.world, state.player), [first]);
    }

    #[test]
    fn only_matching_carried_ammunition_can_be_nocked() {
        let mut state = bare_state();
        bow(&mut state);
        let lump = thing("lump")
            .item()
            .carried_by(state.player)
            .make(&mut state);
        let loose = arrow().at(3, 3).make(&mut state);
        let player = state.player;
        assert_eq!(
            check_nock(&state, player, lump),
            Err("You can't nock that.".to_string())
        );
        assert_eq!(
            check_nock(&state, player, loose),
            Err("You do not have that.".to_string())
        );
    }

    #[test]
    fn there_is_nothing_to_nock_to_without_a_launcher() {
        let mut state = bare_state();
        let spec = arrow().carried_by(state.player).make(&mut state);
        assert_eq!(
            check_nock(&state, state.player, spec),
            Err("You have nothing to nock it to.".to_string())
        );
    }

    #[test]
    fn the_quiver_takes_arrows_up_to_its_capacity() {
        let mut state = bare_state();
        let quiver = quiver_of(&mut state, 2);
        let arrows: Vec<Entity> = (0..3)
            .map(|_| arrow().carried_by(state.player).make(&mut state))
            .collect();
        for &arrow in &arrows {
            state.run(Command::Quiver {
                actor: state.player,
                arrow,
            });
        }
        assert_eq!(stored(&state.world, quiver), 2);
        assert_eq!(holding::carried(&state.world, state.player), [arrows[2]]);
        assert_eq!(
            check_quiver(&state, state.player, arrows[2]),
            Err("You have no quiver with room for that.".to_string())
        );
    }

    #[test]
    fn an_unequipped_quiver_does_not_count() {
        let mut state = bare_state();
        thing("quiver")
            .item()
            .quiver(AmmoKind::Arrow, 5)
            .carried_by(state.player)
            .make(&mut state);
        let loose = arrow().carried_by(state.player).make(&mut state);
        assert_eq!(quiver_with_room(&state.world, state.player, loose), None);
    }

    #[test]
    fn the_next_arrow_is_the_nocked_one_before_the_quiver() {
        let mut state = bare_state();
        let bow = bow(&mut state);
        let quiver = quiver_of(&mut state, 5);
        let quivered = arrow().make(&mut state);
        holding::stow(&mut state.world, quiver, quivered);
        let player = state.player;
        assert_eq!(
            next_arrow(&state.world, player, bow, AmmoKind::Arrow),
            Some((quivered, quiver))
        );

        let special = arrow().carried_by(player).make(&mut state);
        state.run(Command::Nock {
            actor: player,
            arrow: special,
        });
        assert_eq!(
            next_arrow(&state.world, player, bow, AmmoKind::Arrow),
            Some((special, bow))
        );
    }

    #[test]
    fn a_launcher_leaving_the_hands_returns_its_nocked_arrow() {
        let mut state = bare_state();
        let bow = bow(&mut state);
        let spec = arrow().carried_by(state.player).make(&mut state);
        state.run(Command::Nock {
            actor: state.player,
            arrow: spec,
        });
        state.run(Command::Unequip {
            actor: state.player,
            item: bow,
        });
        assert_eq!(nocked(&state.world, bow), None);
        assert!(holding::carried(&state.world, state.player).contains(&spec));
    }
    #[test]
    fn picked_up_arrows_go_into_an_equipped_quiver_until_it_is_full() {
        let mut state = bare_state();
        let quiver = quiver_of(&mut state, 1);
        let first = arrow().at(1, 1).make(&mut state);
        let second = arrow().at(1, 1).make(&mut state);
        state.run(Command::PickUp(state.player));
        assert_eq!(holding::carried(&state.world, quiver), [first]);
        assert_eq!(holding::carried(&state.world, state.player), [second]);
        assert!(state.world.get::<&crate::entity::Position>(first).is_err());
        let log: Vec<_> = state.log.recent(2).cloned().collect();
        assert_eq!(
            log,
            [
                "You pick up the arrow.",
                "You put the arrow in your quiver."
            ]
        );
    }

    #[test]
    fn without_a_quiver_picked_up_arrows_go_to_the_pack() {
        let mut state = bare_state();
        let loose = arrow().at(1, 1).make(&mut state);
        state.run(Command::PickUp(state.player));
        assert_eq!(holding::carried(&state.world, state.player), [loose]);
    }
}
