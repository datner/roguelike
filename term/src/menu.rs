//! Draws the title menu (`front::menu`): new game, continue, load, and save management.

use front::menu::{Body, Menu, Row};
use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

pub fn draw(frame: &mut Frame, menu: &Menu) {
    let view = menu.view();
    let mut lines: Vec<Line> = match view.body {
        Body::List(rows) => rows.into_iter().map(item).collect(),
        Body::Text(lines) => lines.into_iter().map(Line::from).collect(),
        Body::Field { prompt, text } => vec![Line::from(prompt), Line::from(format!("> {text}_"))],
    };
    lines.push(Line::default());
    if let Some(message) = view.message {
        lines.push(Line::from(message));
    }
    lines.push(Line::styled(
        view.hint,
        Style::new().add_modifier(Modifier::DIM),
    ));

    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = frame
        .area()
        .centered(Constraint::Length(48), Constraint::Length(height));
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(view.title)),
        area,
    );
}

fn item(row: Row) -> Line<'static> {
    let marker = if row.selected { "> " } else { "  " };
    let mut style = Style::new();
    if row.selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if row.disabled {
        style = style.add_modifier(Modifier::DIM);
    }
    Line::styled(format!("{marker}{}", row.text), style)
}

#[cfg(test)]
mod tests {
    use front::input::MenuInput;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn screen(menu: &Menu) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|frame| draw(frame, menu)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn draws_rows_messages_and_the_text_field() {
        let mut menu = Menu::new(vec!["recent".into()]);
        assert!(screen(&menu).contains("> New game"));
        assert!(screen(&menu).contains("Continue (recent)"));
        menu.handle(MenuInput::Select);
        menu.handle(MenuInput::Char('a'));
        assert!(screen(&menu).contains("> a_"));
        menu.show("Saved recent.");
        assert!(screen(&menu).contains("Saved recent."));
    }
}
