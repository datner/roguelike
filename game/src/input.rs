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
    /// Frontend only, like `Quit`: open the inventory or equipment view. No
    /// game effect.
    Inventory,
    Equipment,
    /// Frontend only: pick a throwable and aim it. The throw itself is
    /// `PlayerAction::Throw`.
    Throw,
    /// Frontend only: fire the wielded launcher. The shot is `PlayerAction::Fire`.
    Fire,
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
