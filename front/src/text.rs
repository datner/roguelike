//! Text the player reads in panels, as rows of segments a frontend styles: the character sheet, the equipment list, an item's name. Built once here so every frontend says the same thing.

use game::{
    DamageType, ExploreItems, GameState, ItemView, PhaseView, PlaceView, Rgb, Stat, capitalize,
    character_sheet, equipment_view, inspect, place_view, styles,
};

/// A run of text and how to show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Plain(String),
    /// Labels and asides: shown quieter than the values.
    Quiet(String),
    /// A thing's glyph in its own colour.
    Glyph(char, Rgb),
}

pub type Row = Vec<Seg>;

fn plain(text: impl Into<String>) -> Seg {
    Seg::Plain(text.into())
}

fn quiet(text: impl Into<String>) -> Seg {
    Seg::Quiet(text.into())
}

/// A row's text without styling, for tests and plain text.
pub fn flat(row: &[Seg]) -> String {
    row.iter()
        .map(|seg| match seg {
            Seg::Plain(text) | Seg::Quiet(text) => text.clone(),
            Seg::Glyph(glyph, _) => glyph.to_string(),
        })
        .collect()
}

/// A phase in the camp's list: its name, and a quiet note on the one the
/// next dive will be in.
pub fn phase(phase: &PhaseView) -> Row {
    let mut row = vec![Seg::Plain(capitalize(&phase.name))];
    if phase.chosen {
        row.push(Seg::Quiet("  (the next dive)".into()));
    }
    row
}

/// The item's glyph, name and note.
pub fn item(item: &ItemView) -> Row {
    let mut row = vec![
        Seg::Glyph(item.glyph, item.fg),
        plain(format!(" {}", item.name)),
    ];
    if let Some(note) = &item.note {
        row.push(quiet(format!(" ({note})")));
    }
    row
}

/// The place in a few words: "Camp", or "The caves, floor 2 (the embers)".
pub fn place(state: &GameState) -> String {
    match place_view(state) {
        PlaceView::Camp { .. } => "Camp".to_string(),
        PlaceView::Floor {
            dungeon,
            floor,
            phase,
        } => format!("{}, floor {floor} ({phase})", capitalize(&dungeon)),
    }
}

/// The side panel's account of where you are and what comes next.
pub fn whereabouts(state: &GameState) -> Vec<Row> {
    let plain = |text: String| Seg::Plain(text);
    let quiet = |text: &str| Seg::Quiet(text.to_string());
    match place_view(state) {
        PlaceView::Camp { next_phase } => vec![
            vec![plain("Camp".into())],
            vec![quiet("Next dive  "), plain(next_phase)],
            vec![quiet("W waits for another phase")],
        ],
        PlaceView::Floor {
            dungeon,
            floor,
            phase,
        } => vec![
            vec![plain(format!("{}, floor {floor}", capitalize(&dungeon)))],
            vec![quiet("Phase  "), plain(phase)],
            vec![quiet("> deeper  < back to camp")],
        ],
    }
}

/// Health, mana, stats, armor, damage and the rest of the player's sheet.
pub fn character(state: &GameState) -> Vec<Row> {
    let sheet = character_sheet(state);
    let mut rows: Vec<Row> = sheet
        .origin
        .iter()
        .map(|origin| vec![plain(origin.clone())])
        .collect();
    rows.push(vec![
        quiet("HP "),
        plain(format!("{}/{}", sheet.health, sheet.max_health)),
    ]);
    if let Some((mana, max)) = sheet.mana {
        let mut row = vec![quiet("MP "), plain(format!("{mana}/{max}"))];
        // Turns per point, to a tenth: the pace Willpower buys.
        if let Some(ticks) = sheet.mana_regen_ticks.filter(|&ticks| ticks > 0) {
            let tenths = (ticks * 10).div_ceil(game::cost::TICKS_PER_TURN);
            row.push(quiet(format!(
                "  +1 per {}.{} turns",
                tenths / 10,
                tenths % 10
            )));
        }
        rows.push(row);
    }
    if let Some(stats) = sheet.stats {
        let stat = |label: &str, which| {
            vec![
                quiet(format!("{label} ")),
                plain(format!(
                    "{} ({:+})",
                    stats.score(which),
                    stats.modifier(which)
                )),
            ]
        };
        let pair = |left: (&str, Stat), right: (&str, Stat)| {
            let mut row = stat(left.0, left.1);
            row.push(plain("  "));
            row.extend(stat(right.0, right.1));
            row
        };
        rows.push(pair(("STR", Stat::Strength), ("AGI", Stat::Agility)));
        rows.push(pair(("INT", Stat::Intelligence), ("WIL", Stat::Willpower)));
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
    let armor = sheet.armor;
    rows.push(vec![
        quiet("Armor "),
        plain(format!(
            "S{} B{} P{}",
            armor.slashing, armor.blunt, armor.piercing
        )),
    ]);
    if (armor.fire, armor.cold, armor.shock) != (0, 0, 0) {
        rows.push(vec![
            quiet("Resist "),
            plain(format!("F{} C{} E{}", armor.fire, armor.cold, armor.shock)),
        ]);
    }
    rows.push(vec![quiet("Dmg "), plain(format!("{damage} {kinds}"))]);
    let wielding = sheet.wielding.unwrap_or_else(|| "bare hands".to_string());
    rows.push(vec![quiet("Wielding "), plain(wielding)]);
    let followed: Vec<String> = styles(state)
        .into_iter()
        .filter(|style| style.followed)
        .map(|style| format!("{} {}", style.name, style.level))
        .collect();
    if !followed.is_empty() {
        rows.push(vec![quiet("Style "), plain(followed.join(", "))]);
    }
    rows.push(vec![quiet("Speed "), plain(sheet.speed.to_string())]);
    let explore = match state.explore_items {
        ExploreItems::Find => "note items",
        ExploreItems::Stop => "stop at items",
        ExploreItems::PickUp => "pick up items",
    };
    rows.push(vec![quiet("Explore "), plain(explore)]);
    rows
}

/// What the player can tell about the tile at `at` (`game::inspect`): the
/// thing's name, then one row per thing their lore reads.
pub fn inspection(state: &GameState, at: (i32, i32)) -> Vec<Row> {
    match inspect(state, at) {
        None => vec![vec![quiet("You see nothing there.")]],
        Some(seen) => {
            let mut rows = vec![vec![plain(capitalize(&seen.name))]];
            rows.extend(seen.lines.into_iter().map(|line| vec![quiet(line)]));
            rows
        }
    }
}

/// One row per body part: its name, padded to line up, and what is on it.
pub fn equipment(state: &GameState) -> Vec<Row> {
    let parts = equipment_view(state);
    if parts.is_empty() {
        return vec![vec![quiet("no body parts")]];
    }
    let width = parts
        .iter()
        .map(|part| part.name.chars().count())
        .max()
        .unwrap_or(0);
    parts
        .into_iter()
        .map(|part| {
            let mut row = vec![quiet(format!("{:<width$} ", part.name))];
            match &part.item {
                Some(held) => row.extend(item(held)),
                None => row.push(quiet("-")),
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::state;

    #[test]
    fn the_place_is_named_by_its_content() {
        let camp = state();
        assert_eq!(place(&camp), "Camp");
        assert_eq!(flat(&whereabouts(&camp)[1]), "Next dive  the stillness");

        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = game::Blueprints::load(&data).unwrap().with_camp(None);
        let floor = GameState::new(3, blueprints).unwrap();
        assert_eq!(place(&floor), "The castle, floor 1 (the stillness)");
    }

    #[test]
    fn the_sheet_lines_up_labels_and_values() {
        let rows: Vec<String> = character(&state()).iter().map(|row| flat(row)).collect();
        assert_eq!(rows[0], "Wanderer", "the origin heads the sheet");
        assert!(rows[1].starts_with("HP 20/20"), "{rows:?}");
        assert!(rows.iter().any(|row| row.starts_with("STR ")), "{rows:?}");
        assert!(
            rows.iter().any(|row| row == "Explore note items"),
            "{rows:?}"
        );
    }

    #[test]
    fn equipment_shows_each_part_and_what_it_holds() {
        let rows: Vec<String> = equipment(&state()).iter().map(|row| flat(row)).collect();
        assert!(rows.iter().any(|row| row.contains("short bow")), "{rows:?}");
        assert!(rows.iter().any(|row| row.ends_with('-')), "{rows:?}");
    }
}
