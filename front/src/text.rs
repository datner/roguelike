//! Text the player reads in panels, as rows of segments a frontend styles: the character sheet, the equipment list, an item's name. Built once here so every frontend says the same thing.

use game::{
    ExploreItems, GameState, ItemView, PhaseView, PlaceView, Rgb, Stat, capitalize,
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
/// next dive will be in, or on why it can't be chosen.
pub fn phase(phase: &PhaseView) -> Row {
    let mut row = vec![Seg::Plain(capitalize(&phase.name))];
    if phase.chosen {
        row.push(Seg::Quiet("  (the next dive)".into()));
    }
    if let Some(why) = &phase.refused {
        row.push(Seg::Quiet(format!("  ({why})")));
    }
    row
}

/// What a phase does, a line each, for under the camp's list: its lean,
/// the rules it bends and its opposite. The stillness has none.
pub fn phase_details(phase: &PhaseView) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some((element, lean)) = phase.leans {
        lines.push(format!(
            "{} hits {lean}% harder, for everyone",
            capitalize(element.noun())
        ));
    }
    lines.extend(phase.rules.iter().map(|&rule| match rule {
        game::Rule::LightReach(pct) => format!("Light reaches {pct}% farther"),
        game::Rule::StaminaRecovery(pct) => format!("Stamina comes back {pct}% slower"),
        game::Rule::RangedFalloff(pct) => format!("Missiles lose {pct}% to hit per tile"),
        game::Rule::MoveCost(pct) => format!("Each step costs {pct}% more"),
    }));
    if let Some(opposite) = &phase.opposite {
        lines.push(format!("Opposed to {opposite}"));
    }
    if lines.is_empty() {
        lines.push("Nothing leans".to_string());
    }
    lines
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

/// The place in a few words: "Camp", or "The caves, floor 2 of 9 (the
/// embers)".
pub fn place(state: &GameState) -> String {
    match place_view(state) {
        PlaceView::Camp { .. } => "Camp".to_string(),
        PlaceView::Floor {
            dungeon,
            floor,
            of,
            phase,
            leans,
            ..
        } => {
            let leans = leans.map_or_else(String::new, |element| format!(": {element}"));
            format!(
                "{}, floor {floor} of {of} ({phase}{leans})",
                capitalize(&dungeon)
            )
        }
    }
}

/// The rest budget left on this floor ("  Rest: 18"), for the status line;
/// empty at camp, where resting is not counted.
pub fn rest(state: &GameState) -> String {
    match (place_view(state), game::rest::budget(state)) {
        (PlaceView::Floor { .. }, Some(left)) => format!("  Rest: {left}"),
        _ => String::new(),
    }
}

/// "  Outnumbered: 2" when that many enemies beyond the first stand beside
/// the player, for the status line; empty otherwise.
pub fn outnumbered(state: &GameState) -> String {
    match game::outnumbered(state) {
        0 => String::new(),
        extra => format!("  Outnumbered: {extra}"),
    }
}

/// The log's last `rows` rows, oldest first; with `why`, each line that says
/// why it happened is followed by that, indented and quiet, and older lines
/// make room for it.
pub fn log(state: &GameState, rows: usize, why: bool) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for (line, because) in state.log.recent_with_why(rows) {
        if why && let Some(because) = because {
            out.push(vec![Seg::Quiet(format!("  {because}"))]);
        }
        out.push(vec![Seg::Plain(line.clone())]);
        if out.len() >= rows {
            break;
        }
    }
    out.truncate(rows);
    out.reverse();
    out
}

/// A death's chain for the status line: ": killed by the goblin's blow (5);
/// outnumbered by 2; 3 rest left; The caves, floor 3".
pub fn death_chain(death: &game::DeathView) -> String {
    let mut parts = vec![format!("killed by {} ({})", death.killer, death.amount)];
    if death.outnumbered > 0 {
        parts.push(format!("outnumbered by {}", death.outnumbered));
    }
    if let Some(left) = death.rest_left {
        parts.push(format!("{left} rest left"));
    }
    parts.push(death.place.clone());
    format!(": {}", parts.join("; "))
}

/// A life in one line, for the death panel and the Fallen list: "Ana, Pit
/// fighter: killed by the goblin's blow in the caves, floor 3, turn 812.
/// Best: Bow 8, Dodge 4."
pub fn life(life: &game::LifeView) -> String {
    let origin = life
        .origin
        .as_ref()
        .map_or_else(String::new, |origin| format!(", {origin}"));
    let best: Vec<String> = life
        .best
        .iter()
        .map(|(skill, level)| format!("{skill} {level}"))
        .collect();
    let best = if best.is_empty() {
        String::new()
    } else {
        format!(" Best: {}.", best.join(", "))
    };
    format!(
        "{}{origin}: {} in {}, turn {}.{best}",
        life.name, life.ending, life.place, life.turn
    )
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
            of,
            way_back,
            phase,
            leans,
        } => {
            let leans = leans.map_or_else(String::new, |element| format!(", leaning {element}"));
            let back = if way_back == floor {
                "here, on the stairs up".to_string()
            } else {
                format!("floor {way_back}")
            };
            let keys = match (floor == of, way_back == floor) {
                (true, _) => "The bottom  < back to camp",
                (false, true) => "> deeper  < back to camp",
                (false, false) => "> deeper",
            };
            vec![
                vec![plain(format!(
                    "{}, floor {floor} of {of}",
                    capitalize(&dungeon)
                ))],
                vec![quiet("Phase  "), plain(format!("{phase}{leans}"))],
                vec![quiet("Way back  "), plain(back)],
                vec![quiet(keys)],
            ]
        }
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
        let stat = |which: Stat| {
            vec![
                quiet(format!("{} ", which.abbr())),
                plain(format!(
                    "{} ({:+})",
                    stats.score(which),
                    stats.modifier(which)
                )),
            ]
        };
        let pair = |left: Stat, right: Stat| {
            let mut row = stat(left);
            row.push(plain("  "));
            row.extend(stat(right));
            row
        };
        rows.push(pair(Stat::Strength, Stat::Agility));
        rows.push(pair(Stat::Intelligence, Stat::Willpower));
    }
    let damage = sheet
        .damage
        .map_or_else(|| "-".to_string(), |(min, max)| format!("{min}-{max}"));
    let kinds = sheet.damage_types.words();
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
    fn a_phases_details_say_its_lean_rules_and_opposite_and_the_stillness_nothing() {
        let state = crate::tests::state();
        let all = game::phases(&state);
        assert_eq!(phase_details(&all[0]), ["Nothing leans"]);
        assert_eq!(
            phase_details(&all[4]),
            [
                "Acid hits 50% harder, for everyone",
                "Each step costs 25% more",
                "Opposed to the tempest",
            ]
        );
    }

    #[test]
    fn a_death_chain_reads_in_order_and_leaves_out_what_did_not_matter() {
        let death = game::DeathView {
            killer: "the goblin's blow".into(),
            amount: 5,
            outnumbered: 2,
            rest_left: Some(3),
            place: "The caves, floor 3".into(),
        };
        assert_eq!(
            death_chain(&death),
            ": killed by the goblin's blow (5); outnumbered by 2; 3 rest left; The caves, floor 3"
        );
        let alone = game::DeathView {
            outnumbered: 0,
            rest_left: None,
            ..death
        };
        assert_eq!(
            death_chain(&alone),
            ": killed by the goblin's blow (5); The caves, floor 3"
        );
    }

    #[test]
    fn the_place_is_named_by_its_content() {
        let camp = state();
        assert_eq!(place(&camp), "Camp");
        assert_eq!(flat(&whereabouts(&camp)[1]), "Next dive  the stillness");

        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = game::Blueprints::load(&data).unwrap().with_camp(None);
        let floor = GameState::new(3, blueprints).unwrap();
        assert_eq!(place(&floor), "The castle, floor 1 of 12 (the stillness)");
        let rows: Vec<String> = whereabouts(&floor).iter().map(|row| flat(row)).collect();
        assert_eq!(rows[2], "Way back  floor 3");
        assert_eq!(rows[3], "> deeper", "no way back between landings");

        let mut landing = floor;
        let at = |floor| game::dive::Place::Floor {
            dungeon: "Castle".into(),
            phase: "Stillness".into(),
            dive: 1,
            floor,
        };
        landing.place = at(3);
        let rows: Vec<String> = whereabouts(&landing).iter().map(|row| flat(row)).collect();
        assert_eq!(rows[2], "Way back  here, on the stairs up");
        assert_eq!(rows[3], "> deeper  < back to camp");
        landing.place = at(12);
        let rows: Vec<String> = whereabouts(&landing).iter().map(|row| flat(row)).collect();
        assert_eq!(rows[3], "The bottom  < back to camp");
    }

    #[test]
    fn the_sheet_lines_up_labels_and_values() {
        let rows: Vec<String> = character(&state()).iter().map(|row| flat(row)).collect();
        assert_eq!(rows[0], "Wanderer", "the origin heads the sheet");
        assert!(rows[1].starts_with("HP 22/22"), "{rows:?}");
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
