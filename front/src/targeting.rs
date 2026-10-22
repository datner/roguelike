//! Throwing, casting and looking from the keyboard: a small list to pick what to throw or cast, then a cursor to pick where, and the look cursor. Pure state; what a shot would do comes from `game::aim`, what looking reveals from `game::inspect`, never worked out here.

use game::{
    Dir, GameState, ItemRef, PlayerAction, Shape, Shot, SpellView, aim, aim_range, aim_targets,
    capitalize, inspect, look_targets,
};

use crate::input::{AimInput, OverlayInput};

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
    pub fn selected(&self) -> usize {
        self.selected
    }

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
            OverlayInput::Focus(row) => self.selected = row.min(items.len().saturating_sub(1)),
            OverlayInput::Close => return Some(ChooserAction::Cancel),
            _ => {}
        }
        None
    }
}

/// A spell as a list row: its reach and damage, then the notes a frontend
/// shows quieter: its price and why it can't be cast now, if it can't.
pub fn spell_text(spell: &SpellView) -> (String, Vec<String>) {
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
    let main = format!("{}  r{}{shape}{damage}{summons}", spell.name, spell.range);
    let mut costs: Vec<String> = spell
        .preview
        .costs
        .iter()
        .map(|cost| format!("{} {}", cost.amount, cost.resource))
        .collect();
    if spell.preview.fizzle_bp > 0 {
        costs.push(format!("{}% fizzle", spell.preview.fizzle_bp / 100));
    }
    let mut notes = Vec::new();
    if !costs.is_empty() {
        notes.push(costs.join(", "));
    }
    notes.extend(spell.preview.blocked.clone());
    (main, notes)
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

    pub fn shot(&self) -> Shot {
        self.shot
    }

    /// World tile.
    pub fn cursor(&self) -> (i32, i32) {
        self.cursor
    }

    pub fn handle(&mut self, input: AimInput, state: &GameState) -> Option<AimAction> {
        match input {
            AimInput::Move(dir) => {
                let (dx, dy) = Dir::delta(dir);
                self.cursor = (self.cursor.0 + dx, self.cursor.1 + dy);
            }
            AimInput::Set(at) => self.cursor = at,
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

/// How looking ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookAction {
    Done,
    /// Walk to the looked-at tile.
    Travel((i32, i32)),
}

/// Looking around: a cursor over the map, and the status bar says what is
/// under it, as much as the player's lore reads (`game::inspect`).
#[derive(Debug)]
pub struct Looking {
    /// World tile.
    cursor: (i32, i32),
}

impl Looking {
    /// The cursor starts on the nearest creature in view, else the player.
    pub fn new(state: &GameState) -> Option<Self> {
        let cursor = *look_targets(state).first()?;
        Some(Self { cursor })
    }

    /// World tile.
    pub fn cursor(&self) -> (i32, i32) {
        self.cursor
    }

    /// Moves the cursor or cycles creatures; says when looking is over.
    pub fn handle(&mut self, input: AimInput, state: &GameState) -> Option<LookAction> {
        match input {
            AimInput::Move(dir) => {
                let (dx, dy) = Dir::delta(dir);
                self.cursor = (self.cursor.0 + dx, self.cursor.1 + dy);
            }
            AimInput::Set(at) => self.cursor = at,
            AimInput::Next | AimInput::Previous => {
                let targets = look_targets(state);
                let at = targets.iter().position(|&target| target == self.cursor);
                let next = match (input, at) {
                    (AimInput::Next, Some(at)) => (at + 1) % targets.len(),
                    (AimInput::Previous, Some(at)) => (at + targets.len() - 1) % targets.len(),
                    _ => 0,
                };
                if let Some(&tile) = targets.get(next) {
                    self.cursor = tile;
                }
            }
            AimInput::Confirm => return Some(LookAction::Travel(self.cursor)),
            AimInput::Cancel => return Some(LookAction::Done),
        }
        None
    }
}

/// The status line while looking: what is under the cursor.
pub fn look_status(state: &GameState, looking: &Looking) -> String {
    let keys = "Tab: next  Enter: go there  Esc: done";
    match inspect(state, looking.cursor) {
        None => format!("You see nothing there.  {keys}"),
        Some(seen) if seen.lines.is_empty() => format!("{}  {keys}", capitalize(&seen.name)),
        Some(seen) => format!(
            "{}: {}  {keys}",
            capitalize(&seen.name),
            seen.lines.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::state;

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
    fn a_spell_row_shows_its_price_its_fizzle_chance_and_why_not() {
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
        assert_eq!(
            spell_text(&spell),
            (
                "scroll of fireball  r6  burst 1  3-6".to_string(),
                vec![
                    "1 scroll, 14% fizzle".to_string(),
                    "You can't see that spot.".to_string()
                ]
            )
        );
    }

    #[test]
    fn a_shot_that_is_gone_cannot_be_aimed() {
        let state = state();
        assert!(Aiming::new(&state, Shot::Throw(ItemRef(game::SpawnSeq(u64::MAX)))).is_none());
    }

    #[test]
    fn a_pointer_sets_the_cursor_and_focuses_chooser_rows() {
        let state = state();
        let mut aiming = Aiming::new(&state, Shot::Fire).unwrap();
        aiming.handle(AimInput::Set((3, 4)), &state);
        assert_eq!(aiming.cursor(), (3, 4));
        let mut looking = Looking::new(&state).unwrap();
        assert_eq!(looking.handle(AimInput::Set((5, 6)), &state), None);
        assert_eq!(looking.cursor(), (5, 6));
        let items = [item(1), item(2), item(3)];
        let mut chooser = Chooser::default();
        chooser.handle(OverlayInput::Focus(2), &items);
        assert_eq!(
            chooser.handle(OverlayInput::Select, &items),
            Some(ChooserAction::Pick(items[2]))
        );
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

    #[test]
    fn looking_starts_on_what_is_nearest_and_says_what_lore_reads() {
        let state = state();
        let mut looking = Looking::new(&state).unwrap();
        let status = look_status(&state, &looking);
        assert!(
            status.starts_with("You: health 20/20") || status.contains("know too little"),
            "{status}"
        );
        looking.handle(AimInput::Move(Dir::N), &state);
        looking.handle(AimInput::Move(Dir::N), &state);
        assert_eq!(looking.handle(AimInput::Next, &state), None);
        assert_eq!(
            looking.handle(AimInput::Cancel, &state),
            Some(LookAction::Done)
        );
        let cursor = looking.cursor();
        assert_eq!(
            looking.handle(AimInput::Confirm, &state),
            Some(LookAction::Travel(cursor))
        );
    }
}
