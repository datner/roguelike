//! Key bindings: the only place a `KeyEvent` becomes a `game::InputEvent`, `MenuInput` or `OverlayInput`.

use game::{Dir, InputEvent};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

pub fn map_key_event(key: KeyEvent) -> Option<InputEvent> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(InputEvent::Move(Dir::N)),
        KeyCode::Down | KeyCode::Char('j') => Some(InputEvent::Move(Dir::S)),
        KeyCode::Left | KeyCode::Char('h') => Some(InputEvent::Move(Dir::W)),
        KeyCode::Right | KeyCode::Char('l') => Some(InputEvent::Move(Dir::E)),
        KeyCode::Char('y') => Some(InputEvent::Move(Dir::NW)),
        KeyCode::Char('u') => Some(InputEvent::Move(Dir::NE)),
        KeyCode::Char('b') => Some(InputEvent::Move(Dir::SW)),
        KeyCode::Char('n') => Some(InputEvent::Move(Dir::SE)),
        KeyCode::Char('.') => Some(InputEvent::Wait),
        KeyCode::Char('x') => Some(InputEvent::Explore),
        KeyCode::Char('X') => Some(InputEvent::CycleExploreItems),
        KeyCode::Char('g' | ',') => Some(InputEvent::PickUp),
        KeyCode::Char('i') => Some(InputEvent::Inventory),
        KeyCode::Char('e') => Some(InputEvent::Equipment),
        KeyCode::Char('t') => Some(InputEvent::Throw),
        KeyCode::Char('f') => Some(InputEvent::Fire),
        KeyCode::Char('q') | KeyCode::Esc => Some(InputEvent::Quit),
        _ => None,
    }
}

/// Input while the hovering panel (`overlay`) is open.
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
    Close,
    /// Show the inventory view, or close if it is showing.
    Inventory,
    /// Show the equipment view, or close if it is showing.
    Equipment,
    SwitchTab,
}

pub fn map_overlay_key(key: KeyEvent) -> Option<OverlayInput> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(OverlayInput::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(OverlayInput::Down),
        KeyCode::Enter => Some(OverlayInput::Select),
        KeyCode::Char('d') => Some(OverlayInput::Drop),
        KeyCode::Char('n') => Some(OverlayInput::Nock),
        KeyCode::Char('v') => Some(OverlayInput::Quiver),
        KeyCode::Esc | KeyCode::Char('q') => Some(OverlayInput::Close),
        KeyCode::Char('i') => Some(OverlayInput::Inventory),
        KeyCode::Char('e') => Some(OverlayInput::Equipment),
        KeyCode::Tab => Some(OverlayInput::SwitchTab),
        _ => None,
    }
}

/// Input while choosing a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimInput {
    /// Nudge the cursor one tile.
    Move(Dir),
    /// Jump to the next or previous target in view.
    Next,
    Previous,
    Confirm,
    Cancel,
}

pub fn map_aim_key(key: KeyEvent) -> Option<AimInput> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(AimInput::Move(Dir::N)),
        KeyCode::Down | KeyCode::Char('j') => Some(AimInput::Move(Dir::S)),
        KeyCode::Left | KeyCode::Char('h') => Some(AimInput::Move(Dir::W)),
        KeyCode::Right | KeyCode::Char('l') => Some(AimInput::Move(Dir::E)),
        KeyCode::Char('y') => Some(AimInput::Move(Dir::NW)),
        KeyCode::Char('u') => Some(AimInput::Move(Dir::NE)),
        KeyCode::Char('b') => Some(AimInput::Move(Dir::SW)),
        KeyCode::Char('n') => Some(AimInput::Move(Dir::SE)),
        KeyCode::Tab => Some(AimInput::Next),
        KeyCode::BackTab => Some(AimInput::Previous),
        KeyCode::Enter | KeyCode::Char(' ') => Some(AimInput::Confirm),
        KeyCode::Esc | KeyCode::Char('q') => Some(AimInput::Cancel),
        _ => None,
    }
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

/// `typing` is true while a text field has focus: every printable key is
/// then text, so `j` or `q` type a letter instead of navigating.
pub fn map_menu_key(key: KeyEvent, typing: bool) -> Option<MenuInput> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Enter => Some(MenuInput::Select),
        KeyCode::Esc => Some(MenuInput::Back),
        KeyCode::Backspace if typing => Some(MenuInput::Backspace),
        KeyCode::Char(c) if typing => Some(MenuInput::Char(c)),
        KeyCode::Up | KeyCode::Char('k') => Some(MenuInput::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(MenuInput::Down),
        KeyCode::Char('q') => Some(MenuInput::Back),
        KeyCode::Char('d') | KeyCode::Delete => Some(MenuInput::Delete),
        KeyCode::Char('r') => Some(MenuInput::Rename),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn arrows_and_vi_keys_agree() {
        assert_eq!(
            map_key_event(key(KeyCode::Up)),
            map_key_event(key(KeyCode::Char('k')))
        );
        assert_eq!(
            map_key_event(key(KeyCode::Down)),
            map_key_event(key(KeyCode::Char('j')))
        );
        assert_eq!(
            map_key_event(key(KeyCode::Left)),
            map_key_event(key(KeyCode::Char('h')))
        );
        assert_eq!(
            map_key_event(key(KeyCode::Right)),
            map_key_event(key(KeyCode::Char('l')))
        );
    }

    #[test]
    fn diagonals_map_correctly() {
        assert_eq!(
            map_key_event(key(KeyCode::Char('y'))),
            Some(InputEvent::Move(Dir::NW))
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('u'))),
            Some(InputEvent::Move(Dir::NE))
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('b'))),
            Some(InputEvent::Move(Dir::SW))
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('n'))),
            Some(InputEvent::Move(Dir::SE))
        );
    }

    #[test]
    fn q_and_esc_quit() {
        assert_eq!(
            map_key_event(key(KeyCode::Char('q'))),
            Some(InputEvent::Quit)
        );
        assert_eq!(map_key_event(key(KeyCode::Esc)), Some(InputEvent::Quit));
    }

    #[test]
    fn g_and_comma_pick_up() {
        for c in ['g', ','] {
            assert_eq!(
                map_key_event(key(KeyCode::Char(c))),
                Some(InputEvent::PickUp)
            );
        }
    }

    #[test]
    fn x_explores() {
        assert_eq!(
            map_key_event(key(KeyCode::Char('x'))),
            Some(InputEvent::Explore)
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('X'))),
            Some(InputEvent::CycleExploreItems)
        );
    }

    #[test]
    fn i_and_e_open_the_panel_and_its_keys_navigate() {
        assert_eq!(
            map_key_event(key(KeyCode::Char('i'))),
            Some(InputEvent::Inventory)
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('e'))),
            Some(InputEvent::Equipment)
        );
        assert_eq!(
            map_overlay_key(key(KeyCode::Char('i'))),
            Some(OverlayInput::Inventory)
        );
        assert_eq!(
            map_overlay_key(key(KeyCode::Tab)),
            Some(OverlayInput::SwitchTab)
        );
        for code in [KeyCode::Esc, KeyCode::Char('q')] {
            assert_eq!(map_overlay_key(key(code)), Some(OverlayInput::Close));
        }
        assert_eq!(
            map_overlay_key(key(KeyCode::Char('j'))),
            Some(OverlayInput::Down)
        );
        assert_eq!(map_overlay_key(key(KeyCode::Char('l'))), None);
    }

    #[test]
    fn release_events_are_ignored() {
        let mut release = key(KeyCode::Char('h'));
        release.kind = KeyEventKind::Release;
        assert_eq!(map_key_event(release), None);
    }

    #[test]
    fn menu_letters_navigate_unless_typing() {
        let j = key(KeyCode::Char('j'));
        assert_eq!(map_menu_key(j, false), Some(MenuInput::Down));
        assert_eq!(map_menu_key(j, true), Some(MenuInput::Char('j')));
        let q = key(KeyCode::Char('q'));
        assert_eq!(map_menu_key(q, false), Some(MenuInput::Back));
        assert_eq!(map_menu_key(q, true), Some(MenuInput::Char('q')));
        assert_eq!(map_menu_key(key(KeyCode::Esc), true), Some(MenuInput::Back));
        assert_eq!(
            map_menu_key(key(KeyCode::Enter), true),
            Some(MenuInput::Select)
        );
    }
}
