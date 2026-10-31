//! The rendering seam's input (ADR 0001): frontends translate raw keys into `InputEvent`.

use crate::view::ItemRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Dir {
    /// Clockwise from north. Wherever a choice among directions must be
    /// deterministic, ties go to the earlier entry.
    pub const ALL: [Dir; 8] = [
        Dir::N,
        Dir::NE,
        Dir::E,
        Dir::SE,
        Dir::S,
        Dir::SW,
        Dir::W,
        Dir::NW,
    ];

    /// (dx, dy) to add to a position to move one step in this direction.
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (0, -1),
            Dir::S => (0, 1),
            Dir::E => (1, 0),
            Dir::W => (-1, 0),
            Dir::NE => (1, -1),
            Dir::NW => (-1, -1),
            Dir::SE => (1, 1),
            Dir::SW => (-1, 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Move(Dir),
    Wait,
    PickUp,
    /// Repeat `explore::explore_step` until it stops.
    Explore,
    /// `explore::cycle_explore_items`: what auto-explore does at items.
    CycleExploreItems,
    /// Frontend only: a cursor to look at what is in view. No game effect.
    Look,
    /// Close an open door beside you: `PlayerAction::Close`.
    Close,
    /// Start or stop sprinting: `PlayerAction::Sprint`.
    Sprint,
    /// Frontend only: practise until done or interrupted, repeating
    /// `training::practice_step`.
    Practice,
    /// Frontend only: show why the latest log lines happened, until the next
    /// key (docs/plans/legibility.md).
    Why,
    /// Frontend only: rest until healed, the floor's budget is spent or an
    /// enemy comes into view, repeating `rest::rest_step`.
    Rest,
    /// Frontend only, like `Quit`: open the inventory, equipment or skills
    /// view. No game effect.
    Inventory,
    Equipment,
    Skills,
    Styles,
    /// Frontend only: pick a throwable and aim it. The throw itself is
    /// `PlayerAction::Throw`.
    Throw,
    /// Frontend only: fire the wielded launcher. The shot is `PlayerAction::Fire`.
    Fire,
    /// Frontend only: pick a known spell and aim it. The cast is
    /// `PlayerAction::Cast`.
    Cast,
    /// Frontend only: pick a known technique and aim it. Performing it is
    /// `PlayerAction::Perform`.
    Techniques,
    /// Frontend only: the recipe list (docs/plans/crafting.md). Making one is
    /// `PlayerAction::Craft`.
    Craft,
    /// Take the stairs down: `PlayerAction::Descend`.
    Descend,
    /// Take the stairs up: `PlayerAction::Ascend`.
    Ascend,
    /// Frontend only: pick a phase to wait for. The wait itself is
    /// `PlayerAction::WaitForPhase`.
    WaitForPhase,
    Quit,
}

/// What the player chose to do with their turn: the actions among
/// `InputEvent`s. `take_turn` resolves the actor and the commands, so a
/// frontend sends intent only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerAction {
    Move(Dir),
    Wait,
    PickUp,
    Drop(ItemRef),
    /// `part` indexes `equipment_view`; `None` picks the first that fits.
    Equip {
        item: ItemRef,
        part: Option<u32>,
    },
    Unequip(ItemRef),
    /// Hurl a carried or equipped throwable at a map tile.
    Throw {
        item: ItemRef,
        at: (i32, i32),
    },
    /// Shoot the wielded launcher at a map tile.
    Fire {
        at: (i32, i32),
    },
    /// Nock a carried arrow on the wielded launcher.
    Nock(ItemRef),
    /// Put carried ammunition in an equipped quiver.
    Quiver(ItemRef),
    /// Cast a known spell at a map tile (ADR 0014).
    Cast {
        spell: ItemRef,
        at: (i32, i32),
    },
    /// Perform a known technique at a map tile (ADR 0024).
    Perform {
        technique: ItemRef,
        at: (i32, i32),
    },
    /// Take up a known style, or set aside the one followed (ADR 0019).
    FollowStyle(ItemRef),
    /// Study a carried item's lesson (ADR 0017).
    Study(ItemRef),
    /// A turn of drills with what you wield; see `training::practice_step`.
    Practice,
    /// Close an open door beside you.
    Close,
    /// Start or stop sprinting (`sprint`).
    Sprint,
    /// Take the stairs down (`dive`).
    Descend,
    /// Take the stairs up, back to camp (`dive`).
    Ascend,
    /// At camp: the next dive will be in this phase, an index into
    /// `phase::phases` (`phase`).
    WaitForPhase(u32),
    /// Make a known recipe, an index into `craft::recipes`; a slot's chosen
    /// carried material, or `None` for the first that fits
    /// (docs/plans/crafting.md).
    Craft {
        recipe: u32,
        inputs: [Option<ItemRef>; crate::craft::MAX_SLOTS],
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_directions_have_opposite_deltas() {
        assert_eq!(Dir::N.delta(), (0, -1));
        assert_eq!(Dir::S.delta(), (0, 1));
        assert_eq!(Dir::NE.delta(), (1, -1));
        assert_eq!(Dir::SW.delta(), (-1, 1));
    }
}
