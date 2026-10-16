//! Blocking, turn-based event loop and panel layout.

use std::io;

use game::{GameState, InputEvent, PlayerAction, render_view, take_turn};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::input::map_key_event;
use crate::tui::Backend;
use crate::widgets::map_widget::{MapWidget, viewport_tiles};

/// Blocks on `event::read()` and redraws after every event (including
/// resize) — no tick loop: `game::take_turn` runs the world until the
/// player is ready again.
pub fn run(terminal: &mut ratatui::Terminal<Backend>, state: &mut GameState) -> io::Result<()> {
    terminal.draw(|frame| draw(frame, state))?;

    loop {
        match event::read()? {
            Event::Key(key) if state.game_over => {
                if key.kind != KeyEventKind::Release {
                    return Ok(());
                }
                continue;
            }
            Event::Key(key) => {
                let Some(input_event) = map_key_event(key) else {
                    continue;
                };
                match input_event {
                    InputEvent::Quit => return Ok(()),
                    InputEvent::Move(dir) => take_turn(state, PlayerAction::Move(dir)),
                    InputEvent::Wait => take_turn(state, PlayerAction::Wait),
                }
            }
            Event::Resize(_, _) => {}
            _ => continue,
        }
        terminal.draw(|frame| draw(frame, state))?;
    }
}

fn draw(frame: &mut Frame, state: &GameState) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(3),
        ],
    )
    .split(frame.area());

    let (width, height) = viewport_tiles(chunks[0]);
    let view = render_view(state, width, height);
    frame.render_widget(MapWidget::new(&view), chunks[0]);

    let mut log_lines: Vec<Line> = state
        .log
        .recent(5)
        .map(|m| Line::from(m.as_str()))
        .collect();
    log_lines.reverse();
    frame.render_widget(
        Paragraph::new(log_lines).block(Block::bordered().title("Log")),
        chunks[1],
    );

    let status = if state.game_over {
        format!(
            "You died on turn {}. Press any key to quit.",
            state.now.turn()
        )
    } else {
        format!("Seed: {}  Turn: {}", state.seed, state.now.turn())
    };
    frame.render_widget(
        Paragraph::new(status).block(Block::bordered().title("Status")),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use game::{Blueprints, CaveParams, Tick};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn state() -> GameState {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load_dir(&data.join("entities")).unwrap();
        let cave = CaveParams::load(&data.join("cave.ron")).unwrap();
        GameState::new(3, blueprints, &cave).unwrap()
    }

    /// 60×30: a 60×20 map panel above the 7-row log and 3-row status.
    fn rows(state: &GameState) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal.draw(|frame| draw(frame, state)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    fn screen(state: &GameState) -> String {
        rows(state).concat()
    }

    #[test]
    fn the_map_panel_shows_the_viewport_around_the_player() {
        let state = state();
        let rows = rows(&state);
        // The player starts mid-zone, so the camera centres on them.
        assert_eq!(rows[10].chars().nth(30), Some('☺'));
        let panel: String = rows[..20].concat();
        assert_eq!(panel.matches('☺').count(), 1);
        assert!(panel.contains('#') && panel.contains('.'));
    }

    #[test]
    fn status_shows_the_turn() {
        let mut state = state();
        state.now = Tick(250);
        assert!(screen(&state).contains("Seed: 3  Turn: 2"));
    }

    #[test]
    fn status_announces_game_over() {
        let mut state = state();
        state.now = Tick(700);
        // What the frontend reads of a death; how dying works is `game`'s.
        state.game_over = true;
        let screen = screen(&state);
        assert!(screen.contains("You died on turn 7. Press any key to quit."));
        assert!(screen.contains('☺'), "the dead player is still drawn");
    }
}
