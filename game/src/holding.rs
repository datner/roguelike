//! Who holds what: the owner-side lists `Attachments` (in use, so its parts
//! fire) and `Inventory` (carried, inert). Every change to either list goes
//! through here, so how holding is stored can change in one place.

use hecs::{Entity, World};

use crate::entity::{Attachments, EquippedOn, Inventory};

/// What `owner` has attached, in attachment order.
pub(crate) fn attached(world: &World, owner: Entity) -> Vec<Entity> {
    world
        .get::<&Attachments>(owner)
        .map(|attachments| attachments.0.clone())
        .unwrap_or_default()
}

/// What `owner` carries, in the order it was stowed.
pub(crate) fn carried(world: &World, owner: Entity) -> Vec<Entity> {
    world
        .get::<&Inventory>(owner)
        .map(|inventory| inventory.0.clone())
        .unwrap_or_default()
}

pub(crate) fn attach(world: &mut World, owner: Entity, thing: Entity) {
    if let Ok(mut attachments) = world.get::<&mut Attachments>(owner) {
        attachments.0.push(thing);
        return;
    }
    let _ = world.insert_one(owner, Attachments(vec![thing]));
}

pub(crate) fn detach(world: &mut World, owner: Entity, thing: Entity) {
    if let Ok(mut attachments) = world.get::<&mut Attachments>(owner) {
        attachments.0.retain(|&other| other != thing);
    }
}

pub(crate) fn stow(world: &mut World, owner: Entity, item: Entity) {
    if let Ok(mut inventory) = world.get::<&mut Inventory>(owner) {
        inventory.0.push(item);
        return;
    }
    let _ = world.insert_one(owner, Inventory(vec![item]));
}

pub(crate) fn unstow(world: &mut World, owner: Entity, item: Entity) {
    if let Ok(mut inventory) = world.get::<&mut Inventory>(owner) {
        inventory.0.retain(|&other| other != item);
    }
}

/// Equipped = attached and marked with the body part it occupies (ADR 0010).
pub(crate) fn put_on(world: &mut World, owner: Entity, item: Entity, part: u32) {
    attach(world, owner, item);
    let _ = world.insert_one(item, EquippedOn(part));
}

pub(crate) fn take_off(world: &mut World, owner: Entity, item: Entity) {
    detach(world, owner, item);
    let _ = world.remove_one::<EquippedOn>(item);
}
