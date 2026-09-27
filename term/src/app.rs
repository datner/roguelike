use std::io;

use game::{apply_commands, render_view, Command, GameState, InputEvent};
use ratatui::crossterm::event::{self, Event};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::input::map_key_event;
use crate::tui::Backend;
use crate::widgets::map_widget::MapWidget;

/// Blocks on `event::read()` and redraws after every event (including
/// resize) — no tick loop, this is a purely turn-based game.
pub fn run(terminal: &mut ratatui::Terminal<Backend>, state: &mut GameState) -> io::Result<()> {
    terminal.draw(|frame| draw(frame, state))?;

    loop {
        match event::read()? {
            Event::Key(key) => {
                let Some(input_event) = map_key_event(key) else {
                    continue;
                };
                match input_event {
                    InputEvent::Quit => return Ok(()),
                    InputEvent::Move(dir) => {
                        state.commands.push_back(Command::TryMove(state.player, dir));
                        apply_commands(state);
                    }
                    InputEvent::Wait => {}
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

    let view = render_view(state);
    frame.render_widget(MapWidget::new(&view), chunks[0]);

    let mut log_lines: Vec<Line> = state.log.recent(5).map(|m| Line::from(m.as_str())).collect();
    log_lines.reverse();
    frame.render_widget(
        Paragraph::new(log_lines).block(Block::bordered().title("Log")),
        chunks[1],
    );

    let status = format!("Seed: {}", state.seed);
    frame.render_widget(
        Paragraph::new(status).block(Block::bordered().title("Status")),
        chunks[2],
    );
}
