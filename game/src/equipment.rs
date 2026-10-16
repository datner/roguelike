//! Equipping items on body parts (Qud-style anatomy). An equipped item is
//! in its owner's `Attachments` with `EquippedOn(part)`, so its parts take
//! part in the owner's events; a carried one is in `Inventory` and inert.

use hecs::{Entity, World};

use crate::entity::{Body, BodyPart, Equippable, EquippedOn, PartKind};
use crate::grammar::{Subject, capitalize};
use crate::holding;
use crate::state::GameState;

/// The first part of `kind` not in `occupied`, by index.
pub(crate) fn free_part(body: &[BodyPart], occupied: &[u32], kind: PartKind) -> Option<u32> {
    (0..)
        .zip(body)
        .find(|&(index, part)| part.kind == kind && !occupied.contains(&index))
        .map(|(index, _)| index)
}

/// `owner`'s equipped items with the part each occupies, in `Attachments` order.
pub(crate) fn equipped(world: &World, owner: Entity) -> Vec<(Entity, u32)> {
    holding::attached(world, owner)
        .into_iter()
        .filter_map(|item| Some((item, world.get::<&EquippedOn>(item).ok()?.0)))
        .collect()
}

/// Where `item` would go if `actor` equipped it (on `part`, or the first
/// free part that fits, else the first that fits), and what it would
/// displace there. `Err` is the message for why it can't.
pub(crate) fn plan_equip(
    state: &GameState,
    actor: Entity,
    item: Entity,
    part: Option<u32>,
) -> Result<(u32, Option<Entity>), String> {
    let world = &state.world;
    let you = Subject::of(world, actor);
    let what = Subject::of(world, item);
    if !holding::carried(world, actor).contains(&item) {
        return Err(you.sentence("do", "not carry that"));
    }
    let Ok(kind) = world.get::<&Equippable>(item).map(|e| e.part) else {
        return Err(format!("{} cannot be equipped.", capitalize(what.noun())));
    };
    let body = world
        .get::<&Body>(actor)
        .map(|b| b.0.clone())
        .unwrap_or_default();
    let worn = equipped(world, actor);
    let part = match part {
        Some(index) => {
            let fits = body.get(index as usize).is_some_and(|p| p.kind == kind);
            if !fits {
                return Err(format!("{} does not fit there.", capitalize(what.noun())));
            }
            index
        }
        None => {
            let occupied: Vec<u32> = worn.iter().map(|&(_, index)| index).collect();
            let Some(index) =
                free_part(&body, &occupied, kind).or_else(|| free_part(&body, &[], kind))
            else {
                let rest = format!("no {} to equip {} on", kind.noun(), what.noun());
                return Err(you.sentence("have", &rest));
            };
            index
        }
    };
    let displaced = worn.iter().find(|&&(_, at)| at == part).map(|&(e, _)| e);
    Ok((part, displaced))
}

pub(crate) fn equip(state: &mut GameState, actor: Entity, item: Entity, part: Option<u32>) {
    let (part, displaced) = match plan_equip(state, actor, item, part) {
        Ok(plan) => plan,
        Err(message) => {
            state.log.push(message);
            return;
        }
    };
    if let Some(old) = displaced {
        unequip(state, actor, old);
    }
    holding::unstow(&mut state.world, actor, item);
    holding::put_on(&mut state.world, actor, item, part);

    let part_name = state
        .world
        .get::<&Body>(actor)
        .ok()
        .and_then(|body| body.0.get(part as usize).map(|p| p.name.clone()))
        .unwrap_or_default();
    let you = Subject::of(&state.world, actor);
    let what = Subject::of(&state.world, item);
    let rest = format!("{} on {} {part_name}", what.noun(), you.possessive());
    state.log.push(you.sentence("equip", &rest));
}

/// `Err` is the message for why `actor` can't unequip `item`.
pub(crate) fn check_unequip(state: &GameState, actor: Entity, item: Entity) -> Result<(), String> {
    if equipped(&state.world, actor)
        .iter()
        .any(|&(e, _)| e == item)
    {
        return Ok(());
    }
    let you = Subject::of(&state.world, actor);
    Err(you.sentence("do", "not have that equipped"))
}

pub(crate) fn unequip(state: &mut GameState, actor: Entity, item: Entity) {
    if let Err(message) = check_unequip(state, actor, item) {
        state.log.push(message);
        return;
    }
    holding::take_off(&mut state.world, actor, item);
    holding::stow(&mut state.world, actor, item);
    let you = Subject::of(&state.world, actor);
    let what = Subject::of(&state.world, item);
    state.log.push(you.sentence("unequip", what.noun()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::command::tests::BLOW;
    use crate::entity::{Armor, Health};
    use crate::input::PlayerAction;
    use crate::spawn::Spawn;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};
    use crate::time::take_turn;
    use crate::view::{ItemRef, equipment_view, inventory_view};

    /// An item of `kind` in the player's pack.
    fn carry(state: &mut GameState, name: &str, kind: Option<PartKind>) -> Entity {
        let item = thing(name).item().carried_by(state.player);
        match kind {
            Some(part) => item.fits(part),
            None => item,
        }
        .make(state)
    }

    fn equip_cmd(state: &mut GameState, item: Entity, part: Option<u32>) {
        state.run(Command::Equip {
            actor: state.player,
            item,
            part,
        });
    }

    fn part_items(state: &GameState) -> Vec<(String, Option<String>)> {
        equipment_view(state)
            .into_iter()
            .map(|part| (part.name, part.item.map(|item| item.name)))
            .collect()
    }

    #[test]
    fn equipping_fills_the_first_free_part_that_fits() {
        let mut state = bare_state();
        let first = carry(&mut state, "knife", Some(PartKind::Hand));
        let second = carry(&mut state, "dagger", Some(PartKind::Hand));

        equip_cmd(&mut state, first, None);
        assert_eq!(last_log(&state), "You equip the knife on your right hand.");
        equip_cmd(&mut state, second, None);
        assert_eq!(last_log(&state), "You equip the dagger on your left hand.");

        assert!(inventory_view(&state).is_empty());
        assert_eq!(
            part_items(&state),
            [
                ("head".to_string(), None),
                ("body".to_string(), None),
                ("right hand".to_string(), Some("knife".to_string())),
                ("left hand".to_string(), Some("dagger".to_string())),
            ]
        );
    }

    #[test]
    fn equipping_onto_a_taken_part_swaps_the_old_item_out() {
        let mut state = bare_state();
        let knife = carry(&mut state, "knife", Some(PartKind::Hand));
        let dagger = carry(&mut state, "dagger", Some(PartKind::Hand));
        equip_cmd(&mut state, knife, Some(2));
        equip_cmd(&mut state, dagger, Some(2));

        let log: Vec<&String> = state.log.recent(2).collect();
        assert_eq!(
            log,
            [
                "You equip the dagger on your right hand.",
                "You unequip the knife."
            ]
        );
        assert_eq!(equipped(&state.world, state.player), [(dagger, 2)]);
        assert_eq!(
            inventory_view(&state)
                .into_iter()
                .map(|item| item.name)
                .collect::<Vec<_>>(),
            ["knife"]
        );
    }

    #[test]
    fn equipping_refuses_what_cannot_go_on() {
        let mut state = bare_state();
        let rock = carry(&mut state, "rock", None);
        let boots = carry(&mut state, "boots", Some(PartKind::Feet));
        let knife = carry(&mut state, "knife", Some(PartKind::Hand));
        let floor = thing("helm").item().make(&mut state);
        let player = state.player;
        let plan = |state: &GameState, item, part| plan_equip(state, player, item, part);

        assert_eq!(
            plan(&state, rock, None),
            Err("The rock cannot be equipped.".to_string())
        );
        assert_eq!(
            plan(&state, boots, None),
            Err("You have no feet to equip the boots on.".to_string())
        );
        assert_eq!(
            plan(&state, knife, Some(0)),
            Err("The knife does not fit there.".to_string())
        );
        assert_eq!(
            plan(&state, knife, Some(99)),
            Err("The knife does not fit there.".to_string())
        );
        assert_eq!(
            plan(&state, floor, None),
            Err("You do not carry that.".to_string())
        );
    }

    #[test]
    fn unequipping_returns_the_item_to_the_pack() {
        let mut state = bare_state();
        let knife = carry(&mut state, "knife", Some(PartKind::Hand));
        equip_cmd(&mut state, knife, None);
        state.run(Command::Unequip {
            actor: state.player,
            item: knife,
        });

        assert_eq!(last_log(&state), "You unequip the knife.");
        assert!(equipped(&state.world, state.player).is_empty());
        assert!(!state.world.satisfies::<&EquippedOn>(knife));
        assert_eq!(inventory_view(&state).len(), 1);
        assert_eq!(
            check_unequip(&state, state.player, knife),
            Err("You do not have that equipped.".to_string())
        );
    }

    #[test]
    fn a_refused_equip_takes_no_time_and_a_real_one_takes_a_turn() {
        let mut state = bare_state();
        let rock = carry(&mut state, "rock", None);
        let knife = carry(&mut state, "knife", Some(PartKind::Hand));
        let reference = |item| ItemRef::of(&state.world, item).unwrap();
        let (rock, knife) = (reference(rock), reference(knife));
        let start = state.now;

        take_turn(
            &mut state,
            PlayerAction::Equip {
                item: rock,
                part: None,
            },
        );
        assert_eq!(state.now, start);
        assert_eq!(last_log(&state), "The rock cannot be equipped.");

        take_turn(
            &mut state,
            PlayerAction::Equip {
                item: knife,
                part: None,
            },
        );
        assert!(state.now > start);
        let equipped_at = state.now;
        take_turn(&mut state, PlayerAction::Unequip(knife));
        assert!(state.now > equipped_at);
        assert_eq!(last_log(&state), "You unequip the knife.");
    }

    #[test]
    fn only_equipped_armor_protects() {
        let mut state = bare_state();
        let armor = carry(&mut state, "mail", Some(PartKind::Body));
        state.world.insert_one(armor, Armor::uniform(1)).unwrap();
        let hit = |state: &mut GameState| {
            let before = state.world.get::<&Health>(state.player).unwrap().current;
            state.run(Command::DealDamage {
                target: state.player,
                amount: 3,
                cause: BLOW,
            });
            before - state.world.get::<&Health>(state.player).unwrap().current
        };

        assert_eq!(hit(&mut state), 3, "carried armor is inert");
        equip_cmd(&mut state, armor, None);
        assert_eq!(hit(&mut state), 2);
    }

    #[test]
    fn spawned_equipment_sits_on_its_body_part() {
        let mut state = bare_state();
        let goblin = state.spawn(Spawn::blueprint("Goblin")).unwrap();
        let worn = equipped(&state.world, goblin);
        assert_eq!(worn.len(), 1);
        // Leather armor goes on the body, the test anatomy's second part.
        assert_eq!(worn[0].1, 1);
    }
}
