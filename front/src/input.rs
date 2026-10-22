//! What a frontend sends a `Session` or `Menu`, once its own key bindings have decided what a key means. Each mode reads one of these; `Mode::keys` says which.

use game::{Dir, InputEvent};

/// Input while a list is up: the hovering panel (`overlay`) or a chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayInput {
    Up,
    Down,
    Select,
    Drop,
    /// Nock the selected arrow on the wielded bow.
    Nock,
    /// Put the selected arrow in an equipped quiver.
    Quiver,
    /// Study the selected item's lesson.
    Study,
    Close,
    /// Show the inventory view, or close if it is showing.
    Inventory,
    /// Show the equipment view, or close if it is showing.
    Equipment,
    /// Show the skills view, or close if it is showing.
    Skills,
    /// Show the styles view, or close if it is showing.
    Styles,
    SwitchTab,
    /// Focus this row of the list: a pointer on it.
    Focus(usize),
}

/// Input while a cursor is on the map: aiming or looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimInput {
    /// Nudge the cursor one tile.
    Move(Dir),
    /// Jump to the next or previous target in view.
    Next,
    Previous,
    /// Put the cursor on this world tile: a pointer hovering the map.
    Set((i32, i32)),
    Confirm,
    Cancel,
}

/// Title-menu input. Menus are a frontend concern, so this never reaches `game`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuInput {
    Up,
    Down,
    Select,
    Back,
    Delete,
    Rename,
    Char(char),
    Backspace,
}

/// Input to a `Session`, already mapped by the frontend for the current
/// mode (`Mode::keys`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Game(InputEvent),
    List(OverlayInput),
    Cursor(AimInput),
    /// Walk to this known world tile: a pointer clicking the map.
    TravelTo((i32, i32)),
    /// Any key at all: what the game-over screen waits for.
    Any,
}
