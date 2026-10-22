//! Dungeons (docs/plans/dives.md): `data/dungeons/<Name>.ron`, one per themed dungeon: what it is called, what you hear arriving, how its floors are generated and what lives on them. Loaded and checked with the blueprints, so a missing name fails at load, never on a floor.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::blueprint::{BlueprintError, Blueprints, ron_options};
use crate::zone::CaveParams;

/// A dungeon, as its file describes it. Known by its file stem.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dungeon {
    /// What the player reads: "the caves".
    pub name: String,
    /// Logged on arriving on each floor.
    pub arrival: String,
    pub cave: CaveParams,
    /// Creatures on each floor, placed in this order.
    pub creatures: Vec<Inhabitant>,
    /// Items scattered on each floor, after the creatures, each drawn from
    /// every blueprint with an `Item`.
    #[serde(default)]
    pub items: Vec<Scatter>,
}

/// Items on a dungeon's floors.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scatter {
    pub count: usize,
    #[serde(default)]
    pub per_floor: usize,
    /// The phases these turn up in; none means every phase.
    #[serde(default)]
    pub phases: Vec<String>,
}

impl Scatter {
    pub fn on_floor(&self, floor: u32) -> usize {
        grow(self.count, self.per_floor, floor)
    }

    pub fn in_phase(&self, phase: &str) -> bool {
        self.phases.is_empty() || self.phases.iter().any(|p| p == phase)
    }
}

/// `count` on the first floor and `per_floor` more on each one below.
fn grow(count: usize, per_floor: usize, floor: u32) -> usize {
    let below = usize::try_from(floor.saturating_sub(1)).unwrap_or(usize::MAX);
    count.saturating_add(per_floor.saturating_mul(below))
}

/// A kind of creature on a dungeon's floors.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inhabitant {
    pub blueprint: String,
    /// How many on the first floor.
    pub count: usize,
    /// How many more on each floor below the first.
    #[serde(default)]
    pub per_floor: usize,
    /// The phases these live in; none means every phase.
    #[serde(default)]
    pub phases: Vec<String>,
}

impl Inhabitant {
    /// How many on `floor`, counted from 1.
    pub fn on_floor(&self, floor: u32) -> usize {
        grow(self.count, self.per_floor, floor)
    }

    pub fn in_phase(&self, phase: &str) -> bool {
        self.phases.is_empty() || self.phases.iter().any(|p| p == phase)
    }
}

/// Every `*.ron` in `dir`, by file stem.
pub(crate) fn read_dir(dir: &Path) -> Result<BTreeMap<String, Dungeon>, BlueprintError> {
    let io_err = |err| BlueprintError::Io(dir.to_path_buf(), err);
    let mut dungeons = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(io_err)? {
        let path = entry.map_err(io_err)?.path();
        if path.extension().is_none_or(|ext| ext != "ron") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let key = key.to_string();
        let text =
            std::fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
        let dungeon = ron_options()
            .from_str(&text)
            .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
        dungeons.insert(key, dungeon);
    }
    Ok(dungeons)
}

/// Every blueprint a dungeon names exists, and its cave can be generated.
pub(crate) fn check(
    key: &str,
    dungeon: &Dungeon,
    blueprints: &Blueprints,
) -> Result<(), BlueprintError> {
    if let Err(why) = dungeon.cave.check() {
        return Err(BlueprintError::BadDungeon {
            dungeon: key.to_string(),
            why: why.to_string(),
        });
    }
    let bad = |why: String| BlueprintError::BadDungeon {
        dungeon: key.to_string(),
        why,
    };
    for inhabitant in &dungeon.creatures {
        if blueprints.id(&inhabitant.blueprint).is_none() {
            return Err(bad(format!("unknown blueprint {:?}", inhabitant.blueprint)));
        }
    }
    let named = dungeon
        .creatures
        .iter()
        .flat_map(|inhabitant| &inhabitant.phases)
        .chain(dungeon.items.iter().flat_map(|scatter| &scatter.phases));
    for phase in named {
        if !blueprints.phases().iter().any(|known| &known.key == phase) {
            return Err(bad(format!("unknown phase {phase:?}")));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::zone::tests::test_cave;

    /// A dungeon for tests: goblins on every floor, one more each floor down.
    pub(crate) fn test_dungeon() -> Dungeon {
        Dungeon {
            name: "the test caves".into(),
            arrival: "Goblins prowl these caves.".into(),
            cave: test_cave(),
            creatures: vec![Inhabitant {
                blueprint: "Goblin".into(),
                count: 6,
                per_floor: 1,
                phases: Vec::new(),
            }],
            items: vec![Scatter {
                count: 5,
                per_floor: 0,
                phases: Vec::new(),
            }],
        }
    }

    #[test]
    fn deeper_floors_hold_more() {
        let goblins = Inhabitant {
            blueprint: "Goblin".into(),
            count: 6,
            per_floor: 2,
            phases: Vec::new(),
        };
        assert_eq!(goblins.on_floor(1), 6);
        assert_eq!(goblins.on_floor(3), 10);
    }

    #[test]
    fn the_shipped_dungeons_load() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load(&data).unwrap();
        let caves = blueprints.dungeon("Caves").expect("the caves ship");
        assert_eq!(caves.name, "the caves");
        assert!(!caves.creatures.is_empty());
    }

    #[test]
    fn a_dungeon_naming_an_unknown_creature_fails_at_load() {
        let mut dungeon = test_dungeon();
        dungeon.creatures[0].blueprint = "Dragon".into();
        let loaded = crate::state::tests::test_blueprints()
            .with_dungeons(BTreeMap::from([("Lair".to_string(), dungeon)]));
        assert!(matches!(
            loaded,
            Err(BlueprintError::BadDungeon { dungeon, why }) if dungeon == "Lair" && why.contains("Dragon")
        ));
    }
}
