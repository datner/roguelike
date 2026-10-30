//! Legacy (docs/plans/legacy.md, ADR 0026): what a life leaves when it ends, as a plain record
//! the next characters of the same lineage are built from. Everything in it is a name, never an
//! id, so content can change under it: a name that no longer exists is skipped, never an error.

use serde::{Deserialize, Serialize};

use crate::dive::Place;
use crate::entity::{Ability, Discovered, Item, Made, Origin, Position, Skills, Stashed};
use crate::state::GameState;

/// Bumped on any change to the lineage file's format; a file of another version is ignored.
pub const LINEAGE_VERSION: u32 = 1;

/// How many lives a lineage remembers.
// shortcut: a constant until `data/legacy.ron` sets `keep` (legacy.md, slice 3).
pub const KEEP: usize = 3;

/// How a life ended. Retiring will be another variant, so nothing may read a record as a death.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ending {
    /// "the goblin's blow": the death chain's last link, named at the moment of death.
    Died { by: String },
    /// Deleted while alive. It counts as an ending so a new character can't reroll the ruins.
    Abandoned,
}

/// A life, as the lineage keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    /// The origin's key; `None` for a character made without one.
    pub origin: Option<String>,
    pub turn: u64,
    pub ending: Ending,
    pub place: Place,
    /// Each skill with a level, by name, highest first.
    pub skills: Vec<(String, u32)>,
    /// Known styles by blueprint name.
    pub styles: Vec<String>,
    /// Known techniques by blueprint name, abilities left out: they come from what a creature
    /// is and can't be passed on.
    pub techniques: Vec<String>,
    pub discoveries: Vec<String>,
    /// What they wore and carried, by blueprint name; it died with them. What a container
    /// holds (a quiver's arrows) is not listed.
    pub gear: Vec<String>,
    /// What they left in camp, by blueprint name.
    pub stash: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Lineage {
    version: u32,
    records: Vec<Record>,
}

/// Reads only the version, so another version's layout can't fail first.
#[derive(Deserialize)]
struct Header {
    version: u32,
}

/// The life `name` led in `state`: a death if it ended in one, otherwise abandoned (the only way
/// a living character's life is recorded).
pub fn record(state: &GameState, name: &str) -> Record {
    let world = &state.world;
    let player = state.player;
    let made = |things: Vec<hecs::Entity>| -> Vec<String> {
        things
            .into_iter()
            .filter_map(|thing| world.get::<&Made>(thing).ok().map(|made| made.0.clone()))
            .collect()
    };
    let book = state.blueprints.skills();
    let mut skills: Vec<(String, u32)> = world
        .get::<&Skills>(player)
        .map(|skills| {
            skills
                .0
                .iter()
                .filter_map(|&(id, xp)| {
                    let level = crate::skill::level(xp);
                    (level > 0).then_some((book.get(id)?.name.clone(), level))
                })
                .collect()
        })
        .unwrap_or_default();
    skills.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut gear = crate::holding::attached(world, player);
    gear.extend(crate::holding::carried(world, player));
    gear.retain(|&thing| world.satisfies::<&Item>(thing));
    // What camp keeps: stashed while away, on its floor if the life ended there.
    let mut stash: Vec<hecs::Entity> = world
        .query::<(hecs::Entity, &Stashed)>()
        .iter()
        .map(|(item, _)| item)
        .collect();
    if state.place == Place::Camp {
        stash.extend(
            world
                .query::<(hecs::Entity, &Position, &Item)>()
                .iter()
                .map(|(item, _, _)| item),
        );
    }
    let mut techniques = crate::technique::known(world, player);
    techniques.retain(|&technique| !world.satisfies::<&Ability>(technique));
    Record {
        name: name.to_string(),
        origin: world
            .get::<&Origin>(player)
            .ok()
            .map(|origin| origin.0.clone()),
        turn: state.now.turn(),
        ending: state
            .death
            .as_ref()
            .map_or(Ending::Abandoned, |death| Ending::Died {
                by: death.killer.clone(),
            }),
        place: state.place.clone(),
        skills,
        styles: made(crate::style::known(world, player)),
        techniques: made(techniques),
        discoveries: world
            .get::<&Discovered>(player)
            .map(|discovered| discovered.0.clone())
            .unwrap_or_default(),
        gear: made(gear),
        stash: made(stash),
    }
}

/// `record` added to a lineage, newest first, trimmed to `KEEP`.
pub fn add(records: &mut Vec<Record>, record: Record) {
    records.insert(0, record);
    records.truncate(KEEP);
}

pub fn to_ron(records: &[Record]) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(
        &Lineage {
            version: LINEAGE_VERSION,
            records: records.to_vec(),
        },
        ron::ser::PrettyConfig::default(),
    )
}

#[derive(Debug)]
pub enum LineageError {
    Parse(ron::error::SpannedError),
    Version { found: u32, expected: u32 },
}

impl std::fmt::Display for LineageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::Version { found, expected } => {
                write!(f, "lineage version {found} (this build reads {expected})")
            }
        }
    }
}

pub fn from_ron(text: &str) -> Result<Vec<Record>, LineageError> {
    let header: Header = ron::from_str(text).map_err(LineageError::Parse)?;
    if header.version != LINEAGE_VERSION {
        return Err(LineageError::Version {
            found: header.version,
            expected: LINEAGE_VERSION,
        });
    }
    let lineage: Lineage = ron::from_str(text).map_err(LineageError::Parse)?;
    Ok(lineage.records)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::blueprint::Blueprints;
    use crate::command::{Command, DamageCause};

    fn pit_fighter() -> GameState {
        let data =
            Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap();
        GameState::new_as(1, data, Some("PitFighter")).unwrap()
    }

    #[test]
    fn a_death_records_its_cause_kit_and_skills() {
        let mut state = pit_fighter();
        let player = state.player;
        state.run(Command::DealDamage {
            target: player,
            amount: 1000,
            cause: DamageCause::Fall,
        });
        assert!(state.game_over);
        let record = record(&state, "Ana");
        assert_eq!(record.name, "Ana");
        assert_eq!(record.origin.as_deref(), Some("PitFighter"));
        assert_eq!(
            record.ending,
            Ending::Died {
                by: "the fall into the chasm".into()
            }
        );
        assert_eq!(record.place, state.place);
        assert_eq!(record.techniques, ["Trip"], "Headbutt is an ability");
        assert_eq!(record.styles, ["Brawling"]);
        for kit in ["LeatherArmor", "Torch"] {
            assert!(record.gear.contains(&kit.to_string()), "{kit}");
        }
        assert!(!record.skills.is_empty());
        assert!(record.skills.iter().all(|&(_, level)| level > 0));
        assert!(record.skills.is_sorted_by(|a, b| a.1 >= b.1));
    }

    #[test]
    fn the_stash_is_what_camp_keeps() {
        let mut state = pit_fighter();
        assert_eq!(state.place, Place::Camp);
        let spot = crate::spawn::Spawn::blueprint("RustyKnife");
        let at = state
            .world
            .get::<&Position>(state.player)
            .map(|p| *p)
            .unwrap();
        state.spawn(spot.at(at.x, at.y)).unwrap();
        let stashed = state
            .spawn(crate::spawn::Spawn::blueprint("Torch"))
            .unwrap();
        let _ = state.world.insert_one(stashed, Stashed(at));
        let mut stash = record(&state, "Ana").stash;
        stash.sort();
        assert_eq!(
            stash,
            ["RustyKnife", "Torch"],
            "the camp floor counts at camp"
        );
        state.place = Place::Floor {
            dungeon: "Caves".into(),
            phase: "Stillness".into(),
            dive: 1,
            floor: 1,
        };
        assert_eq!(
            record(&state, "Ana").stash,
            ["Torch"],
            "a floor's items are not camp's"
        );
    }

    #[test]
    fn a_living_character_is_recorded_as_abandoned() {
        assert_eq!(record(&pit_fighter(), "Ana").ending, Ending::Abandoned);
    }

    #[test]
    fn a_lineage_round_trips_newest_first_and_trimmed() {
        let one = record(&pit_fighter(), "One");
        let mut records = Vec::new();
        for name in ["A", "B", "C", "D"] {
            add(
                &mut records,
                Record {
                    name: name.into(),
                    ..one.clone()
                },
            );
        }
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["D", "C", "B"]);
        let text = to_ron(&records).unwrap();
        assert_eq!(from_ron(&text).unwrap(), records);
    }

    #[test]
    fn another_version_is_refused_before_its_layout_is_read() {
        let text = "(version: 0, records: [(nonsense: true)])";
        assert!(matches!(
            from_ron(text),
            Err(LineageError::Version { found: 0, .. })
        ));
    }
}
