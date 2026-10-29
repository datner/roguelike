//! Key bindings: the only place an egui key event becomes a `game::InputEvent` or a `front::input` value. Printable keys come from text events, so shifted symbols work on any layout, like `term`'s `KeyCode::Char`; bindings match `term/src/input.rs`.

use eframe::egui::{Event, Key};
use front::input::{AimInput, MenuInput, OverlayInput};
use game::{Dir, InputEvent};

/// One key press: a typed character, or a named key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    /// Shows sprites or glyphs: the window's own setting, no game key.
    F2,
}

/// This frame's key presses, in order. Releases are ignored.
pub fn presses(events: &[Event]) -> Vec<Press> {
    let mut presses = Vec::new();
    for event in events {
        match event {
            Event::Text(text) => presses.extend(text.chars().map(Press::Char)),
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => presses.extend(named(*key, modifiers.shift)),
            _ => {}
        }
    }
    presses
}

/// Keys that type nothing; the rest arrive as text.
fn named(key: Key, shift: bool) -> Option<Press> {
    Some(match key {
        Key::ArrowUp => Press::Up,
        Key::ArrowDown => Press::Down,
        Key::ArrowLeft => Press::Left,
        Key::ArrowRight => Press::Right,
        Key::Enter => Press::Enter,
        Key::Escape => Press::Esc,
        Key::Tab if shift => Press::BackTab,
        Key::Tab => Press::Tab,
        Key::Backspace => Press::Backspace,
        Key::Delete => Press::Delete,
        Key::F2 => Press::F2,
        _ => return None,
    })
}

pub fn map_game(press: Press) -> Option<InputEvent> {
    match press {
        Press::Up | Press::Char('k') => Some(InputEvent::Move(Dir::N)),
        Press::Down | Press::Char('j') => Some(InputEvent::Move(Dir::S)),
        Press::Left | Press::Char('h') => Some(InputEvent::Move(Dir::W)),
        Press::Right | Press::Char('l') => Some(InputEvent::Move(Dir::E)),
        Press::Char('y') => Some(InputEvent::Move(Dir::NW)),
        Press::Char('u') => Some(InputEvent::Move(Dir::NE)),
        Press::Char('b') => Some(InputEvent::Move(Dir::SW)),
        Press::Char('n') => Some(InputEvent::Move(Dir::SE)),
        Press::Char('.') => Some(InputEvent::Wait),
        Press::Char('x') => Some(InputEvent::Explore),
        Press::Char('>') => Some(InputEvent::Descend),
        Press::Char('<') => Some(InputEvent::Ascend),
        Press::Char('W') => Some(InputEvent::WaitForPhase),
        Press::Char('p') => Some(InputEvent::Practice),
        Press::Char('R') => Some(InputEvent::Rest),
        Press::Char('C') => Some(InputEvent::Close),
        Press::Char('Z') => Some(InputEvent::Sprint),
        Press::Char(';') => Some(InputEvent::Look),
        Press::Char('X') => Some(InputEvent::CycleExploreItems),
        Press::Char('g' | ',') => Some(InputEvent::PickUp),
        Press::Char('i') => Some(InputEvent::Inventory),
        Press::Char('e') => Some(InputEvent::Equipment),
        Press::Char('s') => Some(InputEvent::Skills),
        Press::Char('S') => Some(InputEvent::Styles),
        Press::Char('t') => Some(InputEvent::Throw),
        Press::Char('f') => Some(InputEvent::Fire),
        Press::Char('c') => Some(InputEvent::Cast),
        Press::Char('a') => Some(InputEvent::Techniques),
        Press::Char('q') | Press::Esc => Some(InputEvent::Quit),
        _ => None,
    }
}

pub fn map_list(press: Press) -> Option<OverlayInput> {
    match press {
        Press::Up | Press::Char('k') => Some(OverlayInput::Up),
        Press::Down | Press::Char('j') => Some(OverlayInput::Down),
        Press::Enter => Some(OverlayInput::Select),
        Press::Char('d') => Some(OverlayInput::Drop),
        Press::Char('n') => Some(OverlayInput::Nock),
        Press::Char('v') => Some(OverlayInput::Quiver),
        Press::Char('r') => Some(OverlayInput::Study),
        Press::Esc | Press::Char('q') => Some(OverlayInput::Close),
        Press::Char('i') => Some(OverlayInput::Inventory),
        Press::Char('e') => Some(OverlayInput::Equipment),
        Press::Char('s') => Some(OverlayInput::Skills),
        Press::Char('S') => Some(OverlayInput::Styles),
        Press::Tab => Some(OverlayInput::SwitchTab),
        _ => None,
    }
}

pub fn map_cursor(press: Press) -> Option<AimInput> {
    match press {
        Press::Up | Press::Char('k') => Some(AimInput::Move(Dir::N)),
        Press::Down | Press::Char('j') => Some(AimInput::Move(Dir::S)),
        Press::Left | Press::Char('h') => Some(AimInput::Move(Dir::W)),
        Press::Right | Press::Char('l') => Some(AimInput::Move(Dir::E)),
        Press::Char('y') => Some(AimInput::Move(Dir::NW)),
        Press::Char('u') => Some(AimInput::Move(Dir::NE)),
        Press::Char('b') => Some(AimInput::Move(Dir::SW)),
        Press::Char('n') => Some(AimInput::Move(Dir::SE)),
        Press::Tab => Some(AimInput::Next),
        Press::BackTab => Some(AimInput::Previous),
        Press::Enter | Press::Char(' ') => Some(AimInput::Confirm),
        Press::Esc | Press::Char('q') => Some(AimInput::Cancel),
        _ => None,
    }
}

/// `typing` is true while a text field has focus: every printable key is
/// then text, so `j` or `q` type a letter instead of navigating.
pub fn map_menu(press: Press, typing: bool) -> Option<MenuInput> {
    match press {
        Press::Enter => Some(MenuInput::Select),
        Press::Esc => Some(MenuInput::Back),
        Press::Backspace if typing => Some(MenuInput::Backspace),
        Press::Char(c) if typing => Some(MenuInput::Char(c)),
        Press::Up | Press::Char('k') => Some(MenuInput::Up),
        Press::Down | Press::Char('j') => Some(MenuInput::Down),
        Press::Char('q') => Some(MenuInput::Back),
        Press::Char('d') | Press::Delete => Some(MenuInput::Delete),
        Press::Char('r') => Some(MenuInput::Rename),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::Modifiers;

    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn text_gives_characters_and_named_keys_the_rest() {
        let events = [
            key(Key::X, Modifiers::SHIFT),
            Event::Text("X".into()),
            key(Key::Tab, Modifiers::SHIFT),
            key(Key::Escape, Modifiers::NONE),
            Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ];
        assert_eq!(
            presses(&events),
            [Press::Char('X'), Press::BackTab, Press::Esc]
        );
    }

    #[test]
    fn game_keys_match_the_terminal() {
        let cases = [
            (Press::Char('k'), InputEvent::Move(Dir::N)),
            (Press::Down, InputEvent::Move(Dir::S)),
            (Press::Char('y'), InputEvent::Move(Dir::NW)),
            (Press::Char('n'), InputEvent::Move(Dir::SE)),
            (Press::Char('.'), InputEvent::Wait),
            (Press::Char(','), InputEvent::PickUp),
            (Press::Char(';'), InputEvent::Look),
            (Press::Char('x'), InputEvent::Explore),
            (Press::Char('X'), InputEvent::CycleExploreItems),
            (Press::Char('S'), InputEvent::Styles),
            (Press::Char('>'), InputEvent::Descend),
            (Press::Char('<'), InputEvent::Ascend),
            (Press::Char('W'), InputEvent::WaitForPhase),
            (Press::Char('c'), InputEvent::Cast),
            (Press::Esc, InputEvent::Quit),
        ];
        for (press, event) in cases {
            assert_eq!(map_game(press), Some(event), "{press:?}");
        }
        assert_eq!(map_game(Press::Enter), None);
    }

    #[test]
    fn list_and_cursor_keys_match_the_terminal() {
        assert_eq!(map_list(Press::Enter), Some(OverlayInput::Select));
        assert_eq!(map_list(Press::Tab), Some(OverlayInput::SwitchTab));
        assert_eq!(map_list(Press::Char('v')), Some(OverlayInput::Quiver));
        assert_eq!(map_cursor(Press::BackTab), Some(AimInput::Previous));
        assert_eq!(map_cursor(Press::Char(' ')), Some(AimInput::Confirm));
        assert_eq!(map_cursor(Press::Char('u')), Some(AimInput::Move(Dir::NE)));
    }

    #[test]
    fn typing_in_the_menu_takes_letters_as_text() {
        assert_eq!(map_menu(Press::Char('j'), false), Some(MenuInput::Down));
        assert_eq!(map_menu(Press::Char('j'), true), Some(MenuInput::Char('j')));
        assert_eq!(map_menu(Press::Backspace, false), None);
        assert_eq!(map_menu(Press::Esc, true), Some(MenuInput::Back));
    }
}
