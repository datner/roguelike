//! Legacy (docs/plans/legacy.md, ADR 0026): what a life leaves when it ends, as a plain record
//! the next characters of the same lineage are built from. Everything in it is a name, never an
//! id, so content can change under it: a name that no longer exists is skipped, never an error.

use std::path::Path;

use rand::RngExt;
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintError, ron_options};
use crate::dive::Place;
use crate::entity::{Ability, Discovered, Item, Made, Note, Origin, Position, Skills, Stashed};
use crate::spawn::Spawn;
use crate::state::GameState;

/// Bumped on any change to the lineage file's format; a file of another version is ignored.
pub const LINEAGE_VERSION: u32 = 1;

/// How much a legacy is worth (`data/legacy.ron`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Legacy {
    /// How many lives a lineage remembers; camp has a ruin spot for each.
    pub keep: usize,
    /// Shortest first.
    pub gaps: Vec<Gap>,
}

/// How long ago a life ended, and what that left.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gap {
    /// "years": "An abandoned camp, left years ago."
    pub name: String,
    pub weight: u32,
    /// % chance each stashed item lasts.
    pub survives: u32,
    /// % of a predecessor's level a memoir can teach.
    pub lesson: u32,
    /// Whether their journal can still be read.
    pub journal: bool,
}

/// `legacy.ron` in `data`; content without one keeps no lives.
pub(crate) fn read(data: &Path) -> Result<Legacy, BlueprintError> {
    let path = data.join("legacy.ron");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Legacy::default()),
        Err(err) => return Err(BlueprintError::Io(path, err)),
    };
    let legacy: Legacy = ron_options()
        .from_str(&text)
        .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
    check(&legacy)?;
    Ok(legacy)
}

fn check(legacy: &Legacy) -> Result<(), BlueprintError> {
    let bad = |why: String| Err(BlueprintError::BadLegacy(why));
    if legacy.keep > 0 && legacy.gaps.is_empty() {
        return bad("lives are kept, so there must be a gap".into());
    }
    for gap in &legacy.gaps {
        if gap.weight == 0 {
            return bad(format!("gap {:?} has weight 0", gap.name));
        }
        if gap.survives > 100 || gap.lesson > 100 {
            return bad(format!("gap {:?} has a percentage over 100", gap.name));
        }
    }
    Ok(())
}

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

/// `record` added to a lineage, newest first, trimmed to the `keep` the
/// content sets.
pub fn add(records: &mut Vec<Record>, record: Record, legacy: &Legacy) {
    records.insert(0, record);
    records.truncate(legacy.keep);
}

/// The ruins `records` (newest first) left at camp's ruin spots, newest
/// nearest the fire: an abandoned camp saying how long ago, and what
/// survived of what they left. Built once, when a game starts at camp; the
/// legacy stream is seeded from the world seed and draws, per record, the gap
/// and then each item in order, so the same seed and records give the same
/// ruins.
pub(crate) fn build_ruins(state: &mut GameState, records: &[Record]) -> Result<(), BlueprintError> {
    let Some(camp) = state.blueprints.camp().cloned() else {
        return Ok(());
    };
    if state.place != Place::Camp {
        return Ok(());
    }
    let legacy = state.blueprints.legacy().clone();
    // Only here, once, at creation: never part of a running game's `Rngs`.
    let mut rng = Pcg64::new(u128::from(state.seed), crate::rng::stream::LEGACY);
    let mut gap = 0;
    let lives = records.iter().take(legacy.keep).zip(&camp.ruins);
    for (i, (record, &spot)) in lives.enumerate() {
        let roll = weighted(&mut rng, &legacy.gaps);
        // Older ruins are never better kept than newer ones.
        gap = if i == 0 { roll } else { gap + roll }.min(legacy.gaps.len().saturating_sub(1));
        let Some(this) = legacy.gaps.get(gap) else {
            break;
        };
        state.spawn(
            Spawn::blueprint("AbandonedCamp")
                .at(spot.0, spot.1)
                .with(Note(format!("An abandoned camp, left {} ago.", this.name))),
        )?;
        let mut left = record.stash.clone();
        // Gear died with them, unless they died at camp.
        if record.place == Place::Camp {
            left.extend(record.gear.iter().cloned());
        }
        let mut around = around(&camp.map, spot).into_iter().cycle();
        for name in left {
            let lasts = rng.random_range(0..100) < this.survives;
            if !lasts || state.blueprints.id(&name).is_none() {
                continue;
            }
            let (x, y) = around.next().unwrap_or(spot);
            state.spawn(Spawn::blueprint(&name).at(x, y))?;
        }
    }
    Ok(())
}

/// An index into `gaps`, by weight.
fn weighted(rng: &mut Pcg64, gaps: &[Gap]) -> usize {
    let total: u32 = gaps.iter().map(|gap| gap.weight).sum();
    let mut roll = rng.random_range(0..total.max(1));
    gaps.iter()
        .position(|gap| {
            let here = roll < gap.weight;
            roll = roll.saturating_sub(gap.weight);
            here
        })
        .unwrap_or(0)
}

/// The floor tiles around `spot`, in a fixed order; the spot itself if none.
fn around(map: &crate::map::Map, spot: (i32, i32)) -> Vec<(i32, i32)> {
    let tiles: Vec<(i32, i32)> = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ]
    .iter()
    .map(|(dx, dy)| (spot.0 + dx, spot.1 + dy))
    .filter(|&(x, y)| map.get(x, y) == Some(&crate::map::Tile::Floor))
    .collect();
    if tiles.is_empty() { vec![spot] } else { tiles }
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

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    /// A life that left `stash` at camp and died on a floor with `gear`.
    fn life(name: &str, stash: &[&str], gear: &[&str]) -> Record {
        Record {
            stash: stash.iter().map(|s| s.to_string()).collect(),
            gear: gear.iter().map(|s| s.to_string()).collect(),
            place: Place::Floor {
                dungeon: "Caves".into(),
                phase: "Stillness".into(),
                dive: 1,
                floor: 2,
            },
            ending: Ending::Died {
                by: "the goblin's blow".into(),
            },
            ..record(&pit_fighter(), name)
        }
    }

    /// What lies at camp besides the player: (blueprint, x, y), sorted.
    fn ruins(state: &GameState) -> Vec<(String, i32, i32)> {
        let mut found: Vec<(String, i32, i32)> = state
            .world
            .query::<(hecs::Entity, &Made, &Position)>()
            .iter()
            .filter(|&(entity, ..)| entity != state.player)
            .map(|(_, made, at)| (made.0.clone(), at.x, at.y))
            .collect();
        found.sort();
        found
    }

    fn new_game(seed: u64, records: &[Record]) -> GameState {
        GameState::new_in_lineage(seed, data(), Some("PitFighter"), records).unwrap()
    }

    #[test]
    fn no_records_give_todays_camp() {
        assert!(ruins(&new_game(1, &[])).is_empty());
    }

    #[test]
    fn the_same_seed_and_records_give_the_same_ruins() {
        let records = [life("Ana", &["Torch", "RustyKnife", "Quiver"], &[])];
        assert_eq!(ruins(&new_game(4, &records)), ruins(&new_game(4, &records)));
    }

    #[test]
    fn ruins_stand_at_the_spot_nearest_the_fire_and_say_how_long_ago() {
        let state = new_game(4, &[life("Ana", &[], &[])]);
        let spot = state.blueprints.camp().unwrap().ruins[0];
        assert_eq!(
            ruins(&state),
            [("AbandonedCamp".to_string(), spot.0, spot.1)]
        );
        let note = state
            .world
            .query::<&Note>()
            .iter()
            .next()
            .map(|note| note.0.clone())
            .unwrap();
        let gaps: Vec<String> = state
            .blueprints
            .legacy()
            .gaps
            .iter()
            .map(|gap| format!("An abandoned camp, left {} ago.", gap.name))
            .collect();
        assert!(gaps.contains(&note), "{note}");
    }

    #[test]
    fn unknown_names_and_gear_from_a_floor_death_are_not_spawned() {
        let records = [life("Ana", &["NoSuchThing"; 20], &["Torch"; 20])];
        for seed in 0..10 {
            let names: Vec<String> = ruins(&new_game(seed, &records))
                .into_iter()
                .map(|(name, ..)| name)
                .collect();
            assert_eq!(names, ["AbandonedCamp"], "seed {seed}");
        }
    }

    #[test]
    fn gear_is_left_when_the_death_was_at_camp() {
        let at_camp = Record {
            place: Place::Camp,
            ..life("Ana", &[], &["Torch"; 20])
        };
        let torches = ruins(&new_game(4, &[at_camp]))
            .iter()
            .filter(|(name, ..)| name == "Torch")
            .count();
        assert!(torches > 0);
    }

    #[test]
    fn older_ruins_are_never_better_kept() {
        let full = ["Torch"; 30];
        let records = [life("New", &full, &[]), life("Old", &full, &[])];
        let spots = data().camp().unwrap().ruins.clone();
        let near = |state: &GameState, spot: (i32, i32)| {
            ruins(state)
                .iter()
                .filter(|(name, x, y)| {
                    name == "Torch" && (x - spot.0).abs() <= 1 && (y - spot.1).abs() <= 1
                })
                .count()
        };
        for seed in 0..20 {
            let state = new_game(seed, &records);
            let gap_at = |spot: (i32, i32)| {
                state
                    .world
                    .query::<(&Note, &Position)>()
                    .iter()
                    .find(|(_, at)| (at.x, at.y) == spot)
                    .and_then(|(note, _)| {
                        let gaps = &state.blueprints.legacy().gaps;
                        gaps.iter().position(|gap| note.0.contains(&gap.name))
                    })
                    .unwrap()
            };
            let gaps = [gap_at(spots[0]), gap_at(spots[1])];
            assert!(gaps[0] <= gaps[1], "seed {seed}: {gaps:?}");
            assert!(
                near(&state, spots[0]) + near(&state, spots[1]) > 0,
                "seed {seed}"
            );
        }
    }

    #[test]
    fn kept_lives_need_a_gap() {
        let legacy = Legacy {
            keep: 1,
            gaps: Vec::new(),
        };
        assert!(check(&legacy).is_err());
    }

    #[test]
    fn a_living_character_is_recorded_as_abandoned() {
        assert_eq!(record(&pit_fighter(), "Ana").ending, Ending::Abandoned);
    }

    #[test]
    fn a_lineage_round_trips_newest_first_and_trimmed() {
        let state = pit_fighter();
        let one = record(&state, "One");
        let mut records = Vec::new();
        for name in ["A", "B", "C", "D"] {
            add(
                &mut records,
                Record {
                    name: name.into(),
                    ..one.clone()
                },
                state.blueprints.legacy(),
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
