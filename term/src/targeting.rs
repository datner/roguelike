//! Throwing and casting from the keyboard: a small list to pick what to
//! throw or cast, then a cursor to pick where. Pure state plus drawing, like
//! `overlay`; `app::run` feeds it keys and takes the turn it returns. What a
//! shot would do comes from `game::aim`, never worked out here.

use game::camera;
use game::{
    Dir, GameState, ItemRef, ItemView, PlayerAction, Rgb, Shape, Shot, SpellView, aim, aim_range,
    aim_targets,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::input::{AimInput, OverlayInput};
use crate::overlay::item_spans;
use crate::widgets::color;
use crate::widgets::map_widget::{tile_to_screen, viewport_tiles};

/// A list to pick one throwable or spell from.
#[derive(Debug, Default)]
pub struct Chooser {
    selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooserAction {
    Cancel,
    Pick(ItemRef),
}

impl Chooser {
    /// `items` are the listed entries' refs, in the order drawn.
    pub fn handle(&mut self, input: OverlayInput, items: &[ItemRef]) -> Option<ChooserAction> {
        self.selected = self.selected.min(items.len().saturating_sub(1));
        match input {
            OverlayInput::Up => self.selected = self.selected.saturating_sub(1),
            OverlayInput::Down => {
                self.selected = (self.selected + 1).min(items.len().saturating_sub(1));
            }
            OverlayInput::Select => {
                return items.get(self.selected).copied().map(ChooserAction::Pick);
            }
            OverlayInput::Close => return Some(ChooserAction::Cancel),
            _ => {}
        }
        None
    }
}

/// One line per throwable.
pub fn throwable_lines(items: &[ItemView]) -> Vec<Line<'static>> {
    items
        .iter()
        .map(|item| Line::from(item_spans(item)))
        .collect()
}

/// One line per spell: its reach and damage, then, dimmed, its price and
/// why it can't be cast now, if it can't.
pub fn spell_lines(spells: &[SpellView]) -> Vec<Line<'static>> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    spells
        .iter()
        .map(|spell| {
            let shape = match spell.shape {
                None => String::new(),
                Some(Shape::Burst { radius }) => format!("  burst {radius}"),
                Some(Shape::Cone) => "  cone".to_string(),
                Some(Shape::Beam) => "  beam".to_string(),
                Some(Shape::Nova { radius }) => format!("  nova {radius}"),
            };
            let damage = spell
                .damage
                .map_or_else(String::new, |(low, high)| format!("  {low}-{high}"));
            let summons = spell
                .summons
                .as_ref()
                .map_or_else(String::new, |(count, noun)| format!("  {count} {noun}"));
            let mut spans = vec![Span::raw(format!(
                "{}  r{}{shape}{damage}{summons}",
                spell.name, spell.range
            ))];
            let mut costs: Vec<String> = spell
                .preview
                .costs
                .iter()
                .map(|cost| format!("{} {}", cost.amount, cost.resource))
                .collect();
            if spell.preview.fizzle_bp > 0 {
                costs.push(format!("{}% fizzle", spell.preview.fizzle_bp / 100));
            }
            if !costs.is_empty() {
                spans.push(Span::styled(format!("  {}", costs.join(", ")), dim));
            }
            if let Some(why) = &spell.preview.blocked {
                spans.push(Span::styled(format!("  {why}"), dim));
            }
            Line::from(spans)
        })
        .collect()
}

pub fn draw_chooser(
    frame: &mut Frame,
    area: Rect,
    chooser: &Chooser,
    title: &str,
    lines: Vec<Line<'static>>,
) {
    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 48.min(area.width);
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let lines: Vec<Line> = (0..)
        .zip(lines)
        .map(|(index, line)| {
            if index == chooser.selected {
                line.patch_style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title.to_string())),
        panel,
    );
}

/// A cursor on the map, for one shot.
#[derive(Debug)]
pub struct Aiming {
    shot: Shot,
    /// World tile.
    cursor: (i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimAction {
    Cancel,
    Take(PlayerAction),
}

impl Aiming {
    /// The cursor starts on the nearest target in view, else on the player.
    /// `None` if the shot is no longer available.
    pub fn new(state: &GameState, shot: Shot) -> Option<Self> {
        let (origin, _) = aim_range(state, shot)?;
        let cursor = aim_targets(state, shot).first().copied().unwrap_or(origin);
        Some(Self { shot, cursor })
    }

    pub fn handle(&mut self, input: AimInput, state: &GameState) -> Option<AimAction> {
        match input {
            AimInput::Move(dir) => {
                let (dx, dy) = Dir::delta(dir);
                self.cursor = (self.cursor.0 + dx, self.cursor.1 + dy);
            }
            AimInput::Next | AimInput::Previous => {
                let targets = aim_targets(state, self.shot);
                if targets.is_empty() {
                    return None;
                }
                let at = targets.iter().position(|&target| target == self.cursor);
                let next = match (input, at) {
                    (AimInput::Next, Some(at)) => (at + 1) % targets.len(),
                    (AimInput::Previous, Some(at)) => (at + targets.len() - 1) % targets.len(),
                    (AimInput::Previous, None) => targets.len() - 1,
                    _ => 0,
                };
                self.cursor = targets[next];
            }
            AimInput::Confirm => return Some(AimAction::Take(action(self.shot, self.cursor))),
            AimInput::Cancel => return Some(AimAction::Cancel),
        }
        None
    }
}

fn action(shot: Shot, at: (i32, i32)) -> PlayerAction {
    match shot {
        Shot::Throw(item) => PlayerAction::Throw { item, at },
        Shot::Fire => PlayerAction::Fire { at },
        Shot::Cast(spell) => PlayerAction::Cast { spell, at },
    }
}

/// One line for the status bar: what the shot would do, or why it can't.
pub fn status(state: &GameState, aiming: &Aiming) -> String {
    let Some(shot) = aim(state, aiming.shot, aiming.cursor) else {
        return "Nothing to aim.".to_string();
    };
    let verb = match aiming.shot {
        Shot::Throw(_) => "throw",
        Shot::Fire => "fire",
        Shot::Cast(_) => "cast",
    };
    let keys = format!("Enter: {verb}  Tab: next target  Esc: cancel");
    if let Some(problem) = shot.problem {
        return format!("{problem}  {keys}");
    }
    let kinds: Vec<_> = shot.types.iter().map(game::DamageType::noun).collect();
    let kinds = if kinds.is_empty() {
        String::new()
    } else {
        format!(" {}", kinds.join("+"))
    };
    let (low, high) = shot.damage;
    if !shot.summons.is_empty() {
        return format!("Calls {} here  {keys}", shot.summons.len());
    }
    if !shot.area.is_empty() {
        return match shot.caught {
            0 => format!("Catches no one  {keys}"),
            1 => format!("Catches 1: {low}-{high}{kinds}  {keys}"),
            n => format!("Catches {n}: {low}-{high}{kinds} each  {keys}"),
        };
    }
    let odds = shot.chances.map_or_else(String::new, |chances| {
        format!(
            " ({}% hit, {}% graze)",
            chances.hit_bp / 100,
            chances.graze_bp / 100
        )
    });
    match shot.target {
        Some(target) => format!("{target}: {low}-{high}{kinds}{odds}  {keys}"),
        None => format!("Lands here  {keys}"),
    }
}

const PATH: Rgb = Rgb(110, 90, 20);
const LANDING: Rgb = Rgb(170, 60, 20);
pub(crate) const AREA: Rgb = Rgb(120, 40, 90);

/// Marks the missile's path, a spell's area and the landing tile on the
/// already-drawn map, and reverses the cursor's tile.
pub fn draw_aim(frame: &mut Frame, map: Rect, state: &GameState, aiming: &Aiming) {
    let Some(shot) = aim(state, aiming.shot, aiming.cursor) else {
        return;
    };
    let (width, height) = viewport_tiles(map);
    let view = camera(state, width, height);
    let mut mark = |tile: (i32, i32), style: Style| {
        let (x, y) = tile_to_screen(view.world_to_screen(tile).0, view.world_to_screen(tile).1);
        if !(0..width).contains(&x) || !(0..height).contains(&y) {
            return;
        }
        if let Some(cell) = frame
            .buffer_mut()
            .cell_mut((map.x + x as u16, map.y + y as u16))
        {
            cell.set_style(style);
        }
    };
    for &tile in &shot.path {
        mark(tile, Style::new().bg(color(PATH)));
    }
    for &tile in &shot.area {
        mark(tile, Style::new().bg(color(AREA)));
    }
    for &tile in &shot.summons {
        mark(tile, Style::new().bg(color(AREA)));
    }
    mark(shot.landing, Style::new().bg(color(LANDING)));
    mark(aiming.cursor, Style::new().add_modifier(Modifier::REVERSED));
}

/// One frame of a missile's flight: its glyph at one world tile.
#[derive(Debug, Clone, Copy)]
pub struct Flying {
    pub glyph: char,
    pub fg: Rgb,
    pub at: (i32, i32),
}

/// Draws the flying missile over the already-drawn map.
pub fn draw_missile(frame: &mut Frame, map: Rect, state: &GameState, flying: Flying) {
    let (width, height) = viewport_tiles(map);
    let (x, y) = camera(state, width, height).world_to_screen(flying.at);
    let (x, y) = tile_to_screen(x, y);
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return;
    }
    if let Some(cell) = frame
        .buffer_mut()
        .cell_mut((map.x + x as u16, map.y + y as u16))
    {
        cell.set_char(flying.glyph).set_fg(color(flying.fg));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use game::{Blueprints, CaveParams};

    use super::*;

    fn state() -> GameState {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load(&data).unwrap();
        let cave = CaveParams::load(&data.join("cave.ron")).unwrap();
        GameState::new(3, blueprints, &cave).unwrap()
    }

    fn item(seq: u64) -> ItemRef {
        ItemRef(game::SpawnSeq(seq))
    }

    #[test]
    fn the_chooser_picks_the_selected_item_and_cancels() {
        let items = [item(1), item(2)];
        let mut chooser = Chooser::default();
        assert_eq!(chooser.handle(OverlayInput::Down, &items), None);
        assert_eq!(
            chooser.handle(OverlayInput::Select, &items),
            Some(ChooserAction::Pick(items[1]))
        );
        assert_eq!(
            chooser.handle(OverlayInput::Close, &items),
            Some(ChooserAction::Cancel)
        );
    }

    #[test]
    fn the_chooser_stays_inside_the_list() {
        let items = [item(1)];
        let mut chooser = Chooser::default();
        chooser.handle(OverlayInput::Down, &items);
        chooser.handle(OverlayInput::Up, &items);
        chooser.handle(OverlayInput::Up, &items);
        assert_eq!(
            chooser.handle(OverlayInput::Select, &items),
            Some(ChooserAction::Pick(items[0]))
        );
    }

    #[test]
    fn a_spell_line_shows_its_price_its_fizzle_chance_and_why_not() {
        let spell = SpellView {
            spell: item(9),
            name: "scroll of fireball".to_string(),
            range: 6,
            shape: Some(Shape::Burst { radius: 1 }),
            damage: Some((3, 6)),
            summons: None,
            preview: game::CastPreview {
                costs: vec![game::CostLine {
                    resource: "scroll".to_string(),
                    amount: 1,
                }],
                blocked: Some("You can't see that spot.".to_string()),
                fizzle_bp: 1_400,
            },
        };
        let line: String = spell_lines(&[spell])[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            line,
            "scroll of fireball  r6  burst 1  3-6  1 scroll, 14% fizzle  You can't see that spot."
        );
    }

    #[test]
    fn a_shot_that_is_gone_cannot_be_aimed() {
        let state = state();
        assert!(Aiming::new(&state, Shot::Throw(ItemRef(game::SpawnSeq(u64::MAX)))).is_none());
    }

    #[test]
    fn confirming_throws_at_the_cursor_and_moving_nudges_it() {
        let state = state();
        let shot = Shot::Throw(ItemRef(game::SpawnSeq(5)));
        let mut aiming = Aiming {
            shot,
            cursor: (10, 10),
        };
        assert_eq!(aiming.handle(AimInput::Move(Dir::E), &state), None);
        assert_eq!(aiming.handle(AimInput::Move(Dir::N), &state), None);
        assert_eq!(
            aiming.handle(AimInput::Confirm, &state),
            Some(AimAction::Take(PlayerAction::Throw {
                item: ItemRef(game::SpawnSeq(5)),
                at: (11, 9)
            }))
        );
        assert_eq!(
            aiming.handle(AimInput::Cancel, &state),
            Some(AimAction::Cancel)
        );
    }
}
