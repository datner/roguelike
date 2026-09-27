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
        KeyCode::Char('q') | KeyCode::Esc => Some(InputEvent::Quit),
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
        assert_eq!(map_key_event(key(KeyCode::Up)), map_key_event(key(KeyCode::Char('k'))));
        assert_eq!(map_key_event(key(KeyCode::Down)), map_key_event(key(KeyCode::Char('j'))));
        assert_eq!(map_key_event(key(KeyCode::Left)), map_key_event(key(KeyCode::Char('h'))));
        assert_eq!(map_key_event(key(KeyCode::Right)), map_key_event(key(KeyCode::Char('l'))));
    }

    #[test]
    fn diagonals_map_correctly() {
        assert_eq!(map_key_event(key(KeyCode::Char('y'))), Some(InputEvent::Move(Dir::NW)));
        assert_eq!(map_key_event(key(KeyCode::Char('u'))), Some(InputEvent::Move(Dir::NE)));
        assert_eq!(map_key_event(key(KeyCode::Char('b'))), Some(InputEvent::Move(Dir::SW)));
        assert_eq!(map_key_event(key(KeyCode::Char('n'))), Some(InputEvent::Move(Dir::SE)));
    }

    #[test]
    fn q_and_esc_quit() {
        assert_eq!(map_key_event(key(KeyCode::Char('q'))), Some(InputEvent::Quit));
        assert_eq!(map_key_event(key(KeyCode::Esc)), Some(InputEvent::Quit));
    }

    #[test]
    fn release_events_are_ignored() {
        let mut release = key(KeyCode::Char('h'));
        release.kind = KeyEventKind::Release;
        assert_eq!(map_key_event(release), None);
    }
}
