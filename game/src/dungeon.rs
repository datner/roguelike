//! Dungeons (docs/plans/dives.md): `data/dungeons/<Name>.ron`, one per themed dungeon: what it is called, what you hear arriving, how its floors are generated and what lives on them. Loaded and checked with the blueprints, so a missing name fails at load, never on a floor.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::blueprint::{BlueprintError, Blueprints, ron_options};
use rand_pcg::Pcg64;

use crate::map::Map;
use crate::rooms::{RoomsParams, generate_rooms};
use crate::zone::{CaveParams, generate_cave};

/// A dungeon, as its file describes it. Known by its file stem.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dungeon {
    /// What the player reads: "the caves".
    pub name: String,
    /// Logged on arriving on each floor.
    pub arrival: String,
    /// How its floors are laid out.
    pub generator: Generator,
    /// How deep it goes: the last floor, its bottom, has no stairs down
    /// (docs/plans/depth.md).
    pub floors: u32,
    /// Every this many floors, and on the bottom, a way-landing: stairs up
    /// to camp where you arrive. Between landings the only way is down.
    pub landing_every: u32,
    /// Patches of chasm on each floor but the bottom: a shove next to one is
    /// a kill (docs/plans/fights.md). None draws nothing.
    #[serde(default)]
    pub chasms: u32,
    /// Patches of tall grass on each floor: it hides what stands in it.
    #[serde(default)]
    pub grass: u32,
    /// Pools of deep water on each floor: a shove into one drowns a creature
    /// and costs the player a turn (docs/plans/fights-2.md).
    #[serde(default)]
    pub water: u32,
    /// Patches of rubble on each floor: a step onto it costs double
    /// (docs/plans/fights-2.md).
    #[serde(default)]
    pub rubble: u32,
    /// Creatures on each floor, placed in this order.
    pub creatures: Vec<Inhabitant>,
    /// Items scattered on each floor, after the creatures, each drawn from
    /// every blueprint with an `Item`.
    #[serde(default)]
    pub items: Vec<Scatter>,
}

impl Dungeon {
    /// Whether `floor` has a way back to camp.
    pub fn is_landing(&self, floor: u32) -> bool {
        self.is_bottom(floor) || floor.is_multiple_of(self.landing_every.max(1))
    }

    pub fn is_bottom(&self, floor: u32) -> bool {
        floor >= self.floors
    }

    /// The first landing at or below `floor`.
    pub fn way_back(&self, floor: u32) -> u32 {
        (floor..self.floors)
            .find(|&at| self.is_landing(at))
            .unwrap_or(self.floors)
    }
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
    /// This item only, such as a phase's find (docs/plans/phases.md);
    /// none picks each from every item that can be found lying about.
    #[serde(default)]
    pub blueprint: Option<String>,
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

/// How a dungeon's floors are laid out: grown caves or built rooms.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Generator {
    Cave(CaveParams),
    Rooms(RoomsParams),
}

impl Generator {
    pub fn check(&self) -> Result<(), &'static str> {
        match self {
            Generator::Cave(params) => params.check(),
            Generator::Rooms(params) => params.check(),
        }
    }

    /// A floor from `zone_seed`, the map stream it leaves, and where to
    /// arrive if the layout has a better place than the floor nearest the
    /// middle.
    pub fn generate(&self, zone_seed: u64) -> (Map, Pcg64, Option<(i32, i32)>) {
        match self {
            Generator::Cave(params) => {
                let (map, stream) = generate_cave(zone_seed, params);
                (map, stream, None)
            }
            Generator::Rooms(params) => {
                let (map, stream, arrival) = generate_rooms(zone_seed, params);
                (map, stream, Some(arrival))
            }
        }
    }
}

/// A kind of creature on a dungeon's floors.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inhabitant {
    pub blueprint: String,
    /// How many on the first floor they live on.
    pub count: usize,
    /// How many more on each floor below that.
    #[serde(default)]
    pub per_floor: usize,
    /// The first floor they live on (the vision's "deeper is newer"): a new
    /// kind of creature is new to fight, and novelty is what trains.
    #[serde(default = "first_floor")]
    pub from: u32,
    /// The phases these live in; none means every phase.
    #[serde(default)]
    pub phases: Vec<String>,
}

fn first_floor() -> u32 {
    1
}

impl Inhabitant {
    /// How many on `floor`, counted from 1.
    pub fn on_floor(&self, floor: u32) -> usize {
        if floor < self.from {
            return 0;
        }
        grow(self.count, self.per_floor, floor + 1 - self.from.max(1))
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
    if let Err(why) = dungeon.generator.check() {
        return Err(BlueprintError::BadDungeon {
            dungeon: key.to_string(),
            why: why.to_string(),
        });
    }
    let bad = |why: String| BlueprintError::BadDungeon {
        dungeon: key.to_string(),
        why,
    };
    if dungeon.floors == 0 || dungeon.landing_every == 0 {
        return Err(bad("floors and landing_every must be at least 1".into()));
    }
    let blueprints_named = dungeon
        .creatures
        .iter()
        .map(|inhabitant| &inhabitant.blueprint)
        .chain(
            dungeon
                .items
                .iter()
                .filter_map(|scatter| scatter.blueprint.as_ref()),
        );
    for name in blueprints_named {
        if blueprints.id(name).is_none() {
            return Err(bad(format!("unknown blueprint {name:?}")));
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
            generator: Generator::Cave(test_cave()),
            // Every floor a landing, so tests can climb back from any;
            // depth's own tests set theirs (docs/plans/depth.md).
            floors: 9,
            landing_every: 1,
            chasms: 0,
            grass: 0,
            water: 0,
            rubble: 0,
            creatures: vec![Inhabitant {
                blueprint: "Goblin".into(),
                count: 6,
                per_floor: 1,
                from: 1,
                phases: Vec::new(),
            }],
            items: vec![Scatter {
                count: 5,
                per_floor: 0,
                phases: Vec::new(),
                blueprint: None,
            }],
        }
    }

    #[test]
    fn deeper_floors_hold_more() {
        let goblins = Inhabitant {
            blueprint: "Goblin".into(),
            count: 6,
            per_floor: 2,
            from: 1,
            phases: Vec::new(),
        };
        assert_eq!(goblins.on_floor(1), 6);
        assert_eq!(goblins.on_floor(3), 10);
    }

    #[test]
    fn deeper_kinds_start_on_their_floor_and_grow_from_it() {
        let brutes = Inhabitant {
            blueprint: "GoblinBrute".into(),
            count: 1,
            per_floor: 1,
            from: 4,
            phases: Vec::new(),
        };
        assert_eq!(brutes.on_floor(3), 0);
        assert_eq!(brutes.on_floor(4), 1);
        assert_eq!(brutes.on_floor(6), 3);
    }

    #[test]
    fn the_shipped_dungeons_load() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load(&data).unwrap();
        let caves = blueprints.dungeon("Caves").expect("the caves ship");
        assert_eq!(caves.name, "the caves");
        assert!(!caves.creatures.is_empty());
        let castle = blueprints.dungeon("Castle").expect("the castle ships");
        assert!(matches!(castle.generator, Generator::Rooms(_)));
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
        let mut dungeon = test_dungeon();
        dungeon.items[0].blueprint = Some("Grail".into());
        let loaded = crate::state::tests::test_blueprints()
            .with_dungeons(BTreeMap::from([("Lair".to_string(), dungeon)]));
        assert!(
            matches!(loaded, Err(BlueprintError::BadDungeon { why, .. }) if why.contains("Grail"))
        );
    }

    /// The first floor of the caves in `phase`, from camp, by its stairs.
    fn caves_in(phase: &str) -> crate::state::GameState {
        use crate::entity::Position;
        let data = crate::blueprint::Blueprints::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap();
        let mut state = crate::state::GameState::new(5, data).unwrap();
        state.phase = phase.into();
        let camp = state.blueprints.camp().unwrap();
        let (x, y) = camp.entrances.iter().find(|(_, d)| d == "Caves").unwrap().0;
        let player = state.player;
        *state.world.get::<&mut Position>(player).unwrap() = Position { x, y };
        state.run(crate::command::Command::Descend(player));
        state
    }

    fn made(state: &crate::state::GameState) -> Vec<String> {
        state
            .world
            .query::<&crate::entity::Made>()
            .iter()
            .map(|made| made.0.clone())
            .collect()
    }

    #[test]
    fn each_phase_has_its_natives_and_find_and_never_anothers() {
        let natives = [
            ("Embers", "EmberImp", "AshMantle"),
            ("Rime", "RimeWisp", "RimeCharm"),
            ("Tempest", "StormHawk", "StormJavelin"),
            ("Mire", "BogCrawler", "BogWaders"),
        ];
        for (phase, native, find) in natives {
            let here = made(&caves_in(phase));
            assert!(here.iter().any(|m| m == native), "{phase}: {native}");
            assert!(here.iter().any(|m| m == find), "{phase}: {find}");
            for (other, native, find) in natives.iter().filter(|(p, ..)| *p != phase) {
                assert!(
                    !here.iter().any(|m| m == native || m == find),
                    "{phase} has {other}'s"
                );
            }
        }
        let still = made(&caves_in("Stillness"));
        assert!(
            natives
                .iter()
                .all(|(_, n, f)| !still.iter().any(|m| m == n || m == f))
        );
    }

    #[test]
    fn natives_resist_and_deal_their_element() {
        use crate::entity::{Armor, DamageType, DamageTypes, Natural};
        let data = crate::blueprint::Blueprints::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap();
        let mut state = crate::state::GameState::new(5, data).unwrap();
        for (native, element) in [
            ("EmberImp", DamageType::Fire),
            ("RimeWisp", DamageType::Cold),
            ("StormHawk", DamageType::Shock),
            ("BogCrawler", DamageType::Acid),
        ] {
            let it = state.spawn(crate::spawn::Spawn::blueprint(native)).unwrap();
            assert!(
                state.world.get::<&Armor>(it).unwrap().of(element) > 0,
                "{native}"
            );
            assert_eq!(
                *state.world.get::<&Natural>(it).unwrap(),
                Natural(DamageTypes::single(element))
            );
            let strike = crate::combat::strike(&state.world, state.blueprints.skills(), it);
            assert_eq!(
                strike.unwrap().types,
                DamageTypes::single(element),
                "{native}'s blow"
            );
        }
    }
}
