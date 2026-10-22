//! Camp (docs/plans/dives.md): the safe place between dives, drawn in `data/camp.ron` as rows of characters. Each entrance is stairs down into a dungeon. Nothing hostile and nothing left there persists yet.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::blueprint::{BlueprintError, Blueprints, ron_options};
use crate::map::{Grid, Map, Tile};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CampDef {
    arrival: String,
    /// `#` wall, `.` floor, `@` where you stand on arriving, and an
    /// entrance's character for its stairs.
    layout: Vec<String>,
    /// Entrance characters, each to the dungeon (`data/dungeons/<Key>.ron`)
    /// it leads into.
    entrances: BTreeMap<char, String>,
}

/// The camp, checked: its map, where you arrive, and which dungeon each
/// entrance leads into.
#[derive(Debug, Clone, PartialEq)]
pub struct Camp {
    pub arrival_line: String,
    pub map: Map,
    pub arrival: (i32, i32),
    pub entrances: Vec<((i32, i32), String)>,
}

impl Camp {
    /// The dungeon whose entrance is at `at`.
    pub fn entrance_at(&self, at: (i32, i32)) -> Option<&str> {
        self.entrances
            .iter()
            .find(|(tile, _)| *tile == at)
            .map(|(_, dungeon)| dungeon.as_str())
    }
}

fn bad(why: impl Into<String>) -> BlueprintError {
    BlueprintError::BadCamp(why.into())
}

/// Reads `camp.ron` from `data`, if there is one. A layout must be
/// rectangular with exactly one `@`, and every entrance must lead into a
/// dungeon `blueprints` has.
pub(crate) fn load(data: &Path, blueprints: &Blueprints) -> Result<Option<Camp>, BlueprintError> {
    let path = data.join("camp.ron");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(BlueprintError::Io(path, err)),
    };
    let def: CampDef = ron_options()
        .from_str(&text)
        .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
    parse(def, blueprints).map(Some)
}

fn parse(def: CampDef, blueprints: &Blueprints) -> Result<Camp, BlueprintError> {
    let height = i32::try_from(def.layout.len()).map_err(|_| bad("too tall"))?;
    let width = def.layout.first().map_or(0, |row| row.chars().count());
    if height == 0 || width == 0 {
        return Err(bad("the layout is empty"));
    }
    if def.layout.iter().any(|row| row.chars().count() != width) {
        return Err(bad("every row of the layout must be as long as the first"));
    }
    for dungeon in def.entrances.values() {
        if blueprints.dungeon(dungeon).is_none() {
            return Err(bad(format!(
                "an entrance leads into unknown dungeon {dungeon:?}"
            )));
        }
    }
    let width = i32::try_from(width).map_err(|_| bad("too wide"))?;
    let mut map = Grid::new(width, height, Tile::Wall);
    let mut arrival = None;
    let mut entrances = Vec::new();
    for (y, row) in (0..).zip(&def.layout) {
        for (x, ch) in (0..).zip(row.chars()) {
            let tile = match ch {
                '#' => Tile::Wall,
                '.' => Tile::Floor,
                '@' => {
                    if arrival.replace((x, y)).is_some() {
                        return Err(bad("the layout has more than one @"));
                    }
                    Tile::Floor
                }
                other => {
                    let dungeon = def.entrances.get(&other).ok_or_else(|| {
                        bad(format!("{other:?} is not a wall, floor or entrance"))
                    })?;
                    entrances.push(((x, y), dungeon.clone()));
                    Tile::StairsDown
                }
            };
            map.set(x, y, tile);
        }
    }
    let arrival = arrival.ok_or_else(|| bad("the layout has no @"))?;
    Ok(Camp {
        arrival_line: def.arrival,
        map,
        arrival,
        entrances,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::tests::test_blueprints;

    /// A camp for tests: a small room, the entrance to the test dungeon
    /// two steps east of where you arrive.
    pub(crate) fn test_camp() -> Camp {
        let def = CampDef {
            arrival: "The fire is warm.".into(),
            layout: vec![
                "#######".into(),
                "#.@.1.#".into(),
                "#.....#".into(),
                "#######".into(),
            ],
            entrances: BTreeMap::from([('1', "Test".to_string())]),
        };
        parse(def, &test_blueprints()).unwrap()
    }

    #[test]
    fn a_layout_becomes_a_map_with_its_entrances() {
        let camp = test_camp();
        assert_eq!((camp.map.width, camp.map.height), (7, 4));
        assert_eq!(camp.arrival, (2, 1));
        assert_eq!(camp.map.get(4, 1), Some(&Tile::StairsDown));
        assert_eq!(camp.entrance_at((4, 1)), Some("Test"));
        assert_eq!(camp.entrance_at((3, 1)), None);
    }

    #[test]
    fn a_bad_layout_fails_at_load() {
        let def = |layout: &[&str], dungeon: &str| CampDef {
            arrival: String::new(),
            layout: layout.iter().map(|row| row.to_string()).collect(),
            entrances: BTreeMap::from([('1', dungeon.to_string())]),
        };
        let blueprints = test_blueprints();
        let fails = |layout: &[&str], dungeon: &str, why: &str| {
            let Err(BlueprintError::BadCamp(message)) = parse(def(layout, dungeon), &blueprints)
            else {
                panic!("{layout:?} should fail");
            };
            assert!(message.contains(why), "{message}");
        };
        fails(&["#.#", "#@##"], "Test", "as long as");
        fails(&["@.@"], "Test", "more than one");
        fails(&["..1"], "Test", "no @");
        fails(&["@.x"], "Test", "'x'");
        fails(&["@.1"], "Lair", "unknown dungeon");
    }

    #[test]
    fn the_shipped_camp_loads() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load(&data).unwrap();
        let camp = blueprints.camp().expect("a camp ships");
        assert!(camp.entrances.iter().any(|(_, dungeon)| dungeon == "Caves"));
    }
}
