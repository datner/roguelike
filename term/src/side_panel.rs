//! The side column: a stack of panels to the right of the map, log and
//! status. Each `Panel` supplies a title and lines; `draw` stacks them in
//! `PANELS` order. Hidden on narrow terminals so the map keeps its room.

use game::{DamageType, ExploreItems, GameState, Stat, character_sheet, equipment_view};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::overlay::item_spans;

/// Columns the side column takes, borders included.
pub const WIDTH: u16 = 30;
/// Narrower terminals get no side column.
pub const MIN_TERMINAL_WIDTH: u16 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Character,
    Equipment,
}

/// Top to bottom.
pub const PANELS: [Panel; 2] = [Panel::Character, Panel::Equipment];

/// What a panel can draw from: the game, plus what only the frontend knows.
pub struct Context<'a> {
    pub state: &'a GameState,
    /// The save's name; `game` never sees it.
    pub character: &'a str,
}

impl Panel {
    fn title(self, context: &Context) -> String {
        match self {
            Panel::Character => context.character.to_string(),
            Panel::Equipment => "Equipment".to_string(),
        }
    }

    fn lines(self, context: &Context) -> Vec<Line<'static>> {
        match self {
            Panel::Character => character_lines(context.state),
            Panel::Equipment => equipment_lines(context.state),
        }
    }
}

/// Splits the frame into the main area and, if wide enough, the side column.
pub fn split(area: Rect) -> (Rect, Option<Rect>) {
    if area.width < MIN_TERMINAL_WIDTH {
        return (area, None);
    }
    let [main, side] = Layout::new(
        Direction::Horizontal,
        [Constraint::Min(0), Constraint::Length(WIDTH)],
    )
    .areas(area);
    (main, Some(side))
}

/// Each panel as tall as its lines; space left over goes at the bottom.
pub fn draw(frame: &mut Frame, area: Rect, context: &Context) {
    let contents: Vec<(String, Vec<Line>)> = PANELS
        .iter()
        .map(|panel| (panel.title(context), panel.lines(context)))
        .collect();
    let mut constraints: Vec<Constraint> = contents
        .iter()
        .map(|(_, lines)| Constraint::Length(u16::try_from(lines.len() + 2).unwrap_or(u16::MAX)))
        .collect();
    constraints.push(Constraint::Fill(1));
    let areas = Layout::new(Direction::Vertical, constraints).split(area);
    for ((title, lines), &panel_area) in contents.into_iter().zip(areas.iter()) {
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(title)),
            panel_area,
        );
    }
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().add_modifier(Modifier::DIM))
}

fn character_lines(state: &GameState) -> Vec<Line<'static>> {
    let sheet = character_sheet(state);
    let mut lines = vec![Line::from(vec![
        dim("HP "),
        Span::raw(format!("{}/{}", sheet.health, sheet.max_health)),
    ])];
    if let Some((mana, max)) = sheet.mana {
        lines.push(Line::from(vec![
            dim("MP "),
            Span::raw(format!("{mana}/{max}")),
        ]));
    }
    if let Some(stats) = sheet.stats {
        let stat = |label: &str, which| {
            vec![
                dim(format!("{label} ")),
                Span::raw(format!(
                    "{} ({:+})",
                    stats.score(which),
                    stats.modifier(which)
                )),
            ]
        };
        let pair = |left: (&str, Stat), right: (&str, Stat)| {
            let mut row = stat(left.0, left.1);
            row.push(Span::raw("  "));
            row.extend(stat(right.0, right.1));
            Line::from(row)
        };
        lines.push(pair(("STR", Stat::Strength), ("AGI", Stat::Agility)));
        lines.push(pair(("INT", Stat::Intelligence), ("WIL", Stat::Willpower)));
    }
    let damage = sheet
        .damage
        .map_or_else(|| "-".to_string(), |(min, max)| format!("{min}-{max}"));
    let kinds = sheet
        .damage_types
        .iter()
        .map(DamageType::noun)
        .collect::<Vec<_>>()
        .join("+");
    lines.push(Line::from(vec![
        dim("Armor "),
        Span::raw(format!(
            "S{} B{} P{}",
            sheet.armor.slashing, sheet.armor.blunt, sheet.armor.piercing
        )),
    ]));
    lines.push(Line::from(vec![
        dim("Dmg "),
        Span::raw(format!("{damage} {kinds}")),
    ]));
    let wielding = sheet.wielding.unwrap_or_else(|| "bare hands".to_string());
    lines.push(Line::from(vec![dim("Wielding "), Span::raw(wielding)]));
    lines.push(Line::from(vec![
        dim("Speed "),
        Span::raw(sheet.speed.to_string()),
    ]));
    let explore = match state.explore_items {
        ExploreItems::Find => "note items",
        ExploreItems::Stop => "stop at items",
        ExploreItems::PickUp => "pick up items",
    };
    lines.push(Line::from(vec![dim("Explore "), Span::raw(explore)]));
    lines
}

fn equipment_lines(state: &GameState) -> Vec<Line<'static>> {
    let parts = equipment_view(state);
    if parts.is_empty() {
        return vec![Line::from(dim("no body parts"))];
    }
    let width = parts
        .iter()
        .map(|part| part.name.chars().count())
        .max()
        .unwrap_or(0);
    parts
        .into_iter()
        .map(|part| {
            let mut row = vec![dim(format!("{:<width$} ", part.name))];
            match &part.item {
                Some(item) => row.extend(item_spans(item)),
                None => row.push(dim("-")),
            }
            Line::from(row)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use game::{Blueprints, CaveParams, PlayerAction, take_turn};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn state() -> GameState {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load_dir(&data.join("entities")).unwrap();
        let cave = CaveParams::load(&data.join("cave.ron")).unwrap();
        GameState::new(3, blueprints, &cave).unwrap()
    }

    fn screen(state: &GameState, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        let context = Context {
            state,
            character: "hero",
        };
        terminal
            .draw(|frame| draw(frame, frame.area(), &context))
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn narrow_terminals_get_no_side_column() {
        let (main, side) = split(Rect::new(0, 0, MIN_TERMINAL_WIDTH - 1, 30));
        assert_eq!((main.width, side), (MIN_TERMINAL_WIDTH - 1, None));
        let (main, side) = split(Rect::new(0, 0, 100, 30));
        assert_eq!(main.width, 100 - WIDTH);
        assert_eq!(
            side.map(|side| (side.x, side.width)),
            Some((100 - WIDTH, WIDTH))
        );
    }

    #[test]
    fn the_character_panel_shows_the_sheet() {
        let state = state();
        let screen = screen(&state, WIDTH);
        assert!(screen.contains("hero"), "{screen}");
        assert!(screen.contains("HP 20/20"));
        assert!(screen.contains("MP 10/10"), "{screen}");
        assert!(screen.contains("STR 16 (+0)"));
        assert!(screen.contains("INT 18 (+1)  WIL 18 (+1)"), "{screen}");
        assert!(screen.contains("Armor S0 B0 P0"));
        assert!(screen.contains("Dmg 2-4 blunt"), "{screen}");
        assert!(screen.contains("Wielding bare hands"));
        assert!(screen.contains("Explore note items"));
    }

    #[test]
    fn the_equipment_panel_lists_every_body_part() {
        let mut state = state();
        let screen_before = screen(&state, WIDTH);
        for part in ["head", "body", "right hand", "left hand", "feet"] {
            assert!(screen_before.contains(part), "{part}");
        }
        assert!(screen_before.contains("Equipment"));

        game::cycle_explore_items(&mut state);
        take_turn(&mut state, PlayerAction::Wait);
        assert!(screen(&state, WIDTH).contains("Explore stop at items"));
    }
}
