//! Read-only views of the player's state for frontends: plain data, no hecs
//! types, so `term` can draw panels without touching the world.

use hecs::{Entity, World};

use crate::cell::Rgb;
use crate::entity::{Appearance, Body, Equippable, Inventory, Name, PartKind, SpawnSeq};
use crate::equipment::equipped;
use crate::state::GameState;

/// A frontend's handle on an item: its `SpawnSeq`, which is unique, stable
/// across saves, and says nothing about hecs ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemRef(SpawnSeq);

impl ItemRef {
    pub(crate) fn of(world: &World, item: Entity) -> Option<Self> {
        world.get::<&SpawnSeq>(item).ok().map(|seq| Self(*seq))
    }

    /// The entity among `candidates` this refers to.
    pub(crate) fn find(self, world: &World, candidates: &[Entity]) -> Option<Entity> {
        candidates
            .iter()
            .copied()
            .find(|&item| world.get::<&SpawnSeq>(item).is_ok_and(|seq| *seq == self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub item: ItemRef,
    /// Bare noun, e.g. "rusty knife".
    pub name: String,
    pub glyph: char,
    pub fg: Rgb,
    /// The kind of body part it can be equipped on, if any.
    pub fits: Option<PartKind>,
}

impl ItemView {
    fn of(world: &World, item: Entity) -> Option<Self> {
        let name = world
            .get::<&Name>(item)
            .map_or_else(|_| "thing".to_string(), |n| n.0.clone());
        let (glyph, fg) = world
            .get::<&Appearance>(item)
            .map_or(('?', Rgb(200, 200, 200)), |look| (look.glyph, look.fg));
        Some(Self {
            item: ItemRef::of(world, item)?,
            name,
            glyph,
            fg,
            fits: world.get::<&Equippable>(item).ok().map(|e| e.part),
        })
    }
}

/// What the player carries, in the order it was picked up.
pub fn inventory_view(state: &GameState) -> Vec<ItemView> {
    carried(state)
        .into_iter()
        .filter_map(|item| ItemView::of(&state.world, item))
        .collect()
}

/// One of the player's body parts and what is equipped on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartView {
    /// Pass back as `PlayerAction::Equip { part: Some(index), .. }`.
    pub index: u32,
    pub name: String,
    pub kind: PartKind,
    pub item: Option<ItemView>,
}

/// The player's body parts in anatomy order, equipped or empty.
pub fn equipment_view(state: &GameState) -> Vec<PartView> {
    let world = &state.world;
    let Ok(body) = world.get::<&Body>(state.player) else {
        return Vec::new();
    };
    let worn = equipped(world, state.player);
    (0..)
        .zip(&body.0)
        .map(|(index, part)| PartView {
            index,
            name: part.name.clone(),
            kind: part.kind,
            item: worn
                .iter()
                .find(|&&(_, at)| at == index)
                .and_then(|&(item, _)| ItemView::of(world, item)),
        })
        .collect()
}

pub(crate) fn carried(state: &GameState) -> Vec<Entity> {
    state
        .world
        .get::<&Inventory>(state.player)
        .map(|inventory| inventory.0.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Item, Position};
    use crate::input::PlayerAction;
    use crate::state::tests::{bare_state, spawn_with};
    use crate::time::take_turn;

    fn knife_at(state: &mut GameState, x: i32, y: i32) -> Entity {
        spawn_with(
            state,
            (
                Name("knife".into()),
                Appearance {
                    glyph: '/',
                    fg: Rgb(1, 2, 3),
                    bg: Rgb(0, 0, 0),
                },
                Item,
                Position { x, y },
            ),
        )
    }

    #[test]
    fn the_inventory_view_lists_what_was_picked_up() {
        let mut state = bare_state();
        knife_at(&mut state, 1, 1);
        assert!(inventory_view(&state).is_empty());

        take_turn(&mut state, PlayerAction::PickUp);

        let view = inventory_view(&state);
        assert_eq!(view.len(), 1);
        assert_eq!((view[0].name.as_str(), view[0].glyph), ("knife", '/'));
        assert_eq!(view[0].fg, Rgb(1, 2, 3));
    }

    #[test]
    fn picking_up_and_dropping_take_a_turn_each() {
        let mut state = bare_state();
        knife_at(&mut state, 1, 1);
        let start = state.now;
        take_turn(&mut state, PlayerAction::PickUp);
        let picked = state.now;
        assert!(picked > start);

        let item = inventory_view(&state)[0].item;
        take_turn(&mut state, PlayerAction::Drop(item));
        assert!(state.now > picked);
        assert!(inventory_view(&state).is_empty());
    }

    #[test]
    fn picking_up_nothing_is_refused_without_spending_time() {
        let mut state = bare_state();
        let start = state.now;
        take_turn(&mut state, PlayerAction::PickUp);
        assert_eq!(state.now, start);
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("There is nothing here to pick up.")
        );
    }

    #[test]
    fn dropping_an_item_ref_you_do_not_carry_is_refused() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 3, 3);
        let item = ItemRef::of(&state.world, knife).unwrap();
        let start = state.now;
        take_turn(&mut state, PlayerAction::Drop(item));
        assert_eq!(state.now, start);
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("You do not carry that.")
        );
    }
}
