//! The hovering panel: a modal box drawn over the map. Pure state plus
//! drawing; `app::run` feeds it keys and performs the actions it returns.

use game::{ItemView, PlayerAction};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::input::OverlayInput;
use crate::widgets::color;

/// What the panel asks `app::run` to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Close,
    /// Take this as the player's turn; the panel stays open.
    Act(PlayerAction),
}

/// What the panel shows, from `game`'s views, fetched fresh for each key
/// and frame so it never goes stale after a turn.
pub struct Contents<'a> {
    pub inventory: &'a [ItemView],
}

#[derive(Debug, Default)]
pub struct Overlay {
    selected: usize,
}

impl Overlay {
    pub fn handle(&mut self, input: OverlayInput, contents: &Contents) -> Option<OverlayAction> {
        let items = contents.inventory;
        self.selected = self.selected.min(items.len().saturating_sub(1));
        let chosen = items.get(self.selected).map(|item| item.item);
        match input {
            OverlayInput::Up => self.selected = self.selected.saturating_sub(1),
            OverlayInput::Down => {
                self.selected = (self.selected + 1).min(items.len().saturating_sub(1));
            }
            OverlayInput::Close => return Some(OverlayAction::Close),
            OverlayInput::Select => {
                let item = chosen?;
                return Some(OverlayAction::Act(PlayerAction::Equip { item, part: None }));
            }
            OverlayInput::Drop => return Some(OverlayAction::Act(PlayerAction::Drop(chosen?))),
        }
        None
    }
}

/// Half the map panel, at least 36×8 where it fits, centred over `area`.
pub fn draw(frame: &mut Frame, area: Rect, overlay: &Overlay, contents: &Contents) {
    let width = (area.width / 2).max(36).min(area.width);
    let height = (area.height / 2).max(8).min(area.height);
    let panel = area.centered(Constraint::Length(width), Constraint::Length(height));

    let rows = inventory_rows(contents.inventory, overlay.selected);
    let hint = "Enter equip  d drop  Esc close";
    let block = Block::bordered()
        .title("Inventory")
        .title_bottom(Line::styled(hint, Style::new().add_modifier(Modifier::DIM)));
    frame.render_widget(Clear, panel);
    frame.render_widget(Paragraph::new(rows).block(block), panel);
}

fn inventory_rows(items: &[ItemView], selected: usize) -> Vec<Line<'static>> {
    if items.is_empty() {
        return vec![Line::from("You carry nothing.")];
    }
    let selected = selected.min(items.len() - 1);
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut row = item_spans(item);
            if let Some(kind) = item.fits {
                row.push(Span::styled(
                    format!("  ({})", kind.noun()),
                    Style::new().add_modifier(Modifier::DIM),
                ));
            }
            highlight(Line::from(row), i == selected)
        })
        .collect()
}

/// The item's glyph in its colour, then its name.
pub fn item_spans(item: &ItemView) -> Vec<Span<'static>> {
    vec![
        Span::styled(item.glyph.to_string(), Style::new().fg(color(item.fg))),
        Span::raw(format!(" {}", item.name)),
    ]
}

fn highlight(line: Line<'static>, selected: bool) -> Line<'static> {
    if selected {
        line.patch_style(Style::new().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use game::{ItemRef, PartKind, Rgb, SpawnSeq};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn items(names: &[&str]) -> Vec<ItemView> {
        (0..)
            .zip(names)
            .map(|(seq, name)| ItemView {
                item: ItemRef(SpawnSeq(seq)),
                name: name.to_string(),
                glyph: '/',
                fg: Rgb(200, 100, 50),
                fits: Some(PartKind::Hand),
            })
            .collect()
    }

    fn screen(overlay: &Overlay, contents: &Contents) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|frame| draw(frame, frame.area(), overlay, contents))
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
    fn enter_equips_and_d_drops_the_selected_item() {
        let inventory = items(&["knife", "dagger"]);
        let contents = Contents {
            inventory: &inventory,
        };
        let mut overlay = Overlay::default();
        assert_eq!(overlay.handle(OverlayInput::Down, &contents), None);
        assert_eq!(overlay.handle(OverlayInput::Down, &contents), None);
        assert_eq!(
            overlay.handle(OverlayInput::Select, &contents),
            Some(OverlayAction::Act(PlayerAction::Equip {
                item: inventory[1].item,
                part: None
            }))
        );
        overlay.handle(OverlayInput::Up, &contents);
        assert_eq!(
            overlay.handle(OverlayInput::Drop, &contents),
            Some(OverlayAction::Act(PlayerAction::Drop(inventory[0].item)))
        );
    }

    #[test]
    fn an_empty_pack_offers_nothing_but_closing() {
        let contents = Contents { inventory: &[] };
        let mut overlay = Overlay::default();
        assert_eq!(overlay.handle(OverlayInput::Select, &contents), None);
        assert_eq!(overlay.handle(OverlayInput::Drop, &contents), None);
        assert_eq!(
            overlay.handle(OverlayInput::Close, &contents),
            Some(OverlayAction::Close)
        );
        assert!(screen(&overlay, &contents).contains("You carry nothing."));
    }

    #[test]
    fn a_shrunken_list_keeps_the_selection_inside_it() {
        let three = items(&["a", "b", "c"]);
        let mut overlay = Overlay::default();
        for _ in 0..2 {
            overlay.handle(OverlayInput::Down, &Contents { inventory: &three });
        }
        let one = &three[..1];
        let contents = Contents { inventory: one };
        assert_eq!(
            overlay.handle(OverlayInput::Drop, &contents),
            Some(OverlayAction::Act(PlayerAction::Drop(three[0].item)))
        );
    }

    #[test]
    fn the_panel_lists_items_with_where_they_go() {
        let inventory = items(&["knife"]);
        let screen = screen(
            &Overlay::default(),
            &Contents {
                inventory: &inventory,
            },
        );
        assert!(screen.contains("Inventory"));
        assert!(screen.contains("/ knife  (hand)"));
        assert!(screen.contains("Enter equip"));
    }
}
