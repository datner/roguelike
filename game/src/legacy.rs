//! Legacy (docs/plans/legacy.md, ADR 0026): what a life leaves when it ends, as a plain record
//! the next characters of the same lineage are built from. Everything in it is a name, never an
//! id, so content can change under it: a name that no longer exists is skipped, never an error.

use std::path::Path;

use rand::RngExt;
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintError, ron_options};
use crate::dive::Place;
use crate::entity::{
    Ability, Discovered, Item, Made, Memoir, Name, Note, Origin, Position, Reveals, Skills,
    Stashed, Teaches,
};
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

/// A life as the player reads it, its names resolved against the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeView {
    pub name: String,
    /// The origin's name, as the menu shows it.
    pub origin: Option<String>,
    /// "killed by the goblin's blow", "abandoned".
    pub ending: String,
    /// "camp", "the caves, floor 3".
    pub place: String,
    pub turn: u64,
    /// The highest skills, at most three.
    pub best: Vec<(String, u32)>,
}

/// How many skills a summary names.
const BEST: usize = 3;

/// `record` for reading: what the death panel and the title's Fallen list
/// say. A name the content no longer has is shown as it was recorded.
pub fn summary(blueprints: &crate::blueprint::Blueprints, record: &Record) -> LifeView {
    let origin = record.origin.as_ref().map(|key| {
        blueprints
            .origins()
            .iter()
            .find(|origin| &origin.key == key)
            .map_or_else(|| key.clone(), |origin| origin.name.clone())
    });
    let place = match &record.place {
        Place::Camp => "camp".to_string(),
        Place::Floor { dungeon, floor, .. } => {
            let name = blueprints
                .dungeon(dungeon)
                .map_or(dungeon.as_str(), |found| found.name.as_str());
            format!("{name}, floor {floor}")
        }
    };
    LifeView {
        name: record.name.clone(),
        origin,
        ending: match &record.ending {
            Ending::Died { by } => format!("killed by {by}"),
            Ending::Abandoned => "abandoned".to_string(),
        },
        place,
        turn: record.turn,
        best: record.skills.iter().take(BEST).cloned().collect(),
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
        let (x, y) = around.next().unwrap_or(spot);
        state.spawn(memoir(state, record, this).at(x, y))?;
        let (x, y) = around.next().unwrap_or(spot);
        state.spawn(if this.journal {
            Spawn::blueprint("Journal")
                .with(Name(format!("journal of {}", record.name)))
                .with(Reveals(record.discoveries.clone()))
                .at(x, y)
        } else {
            Spawn::blueprint("RuinedPage").at(x, y)
        })?;
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

/// `record`'s memoir: who they were and how it ended, skills it can raise to
/// `gap.lesson`% of theirs, and the techniques they knew. Names the content
/// no longer has are left out.
fn memoir(state: &GameState, record: &Record, gap: &Gap) -> Spawn {
    let caps: Vec<(String, u32)> = record
        .skills
        .iter()
        .map(|(skill, level)| (skill.clone(), level * gap.lesson / 100))
        .filter(|&(_, cap)| cap > 0)
        .collect();
    let teaches: Vec<_> = record
        .techniques
        .iter()
        .filter_map(|name| state.blueprints.id(name))
        .collect();
    let life = summary(&state.blueprints, record);
    let origin = life
        .origin
        .map_or_else(String::new, |origin| format!(", {}", origin.to_lowercase()));
    let story = format!(
        "The memoir of {}{origin}. It ends on turn {}: {}.",
        life.name, life.turn, life.ending
    );
    let spawn = Spawn::blueprint("Memoir")
        .with(Name(format!("memoir of {}", record.name)))
        .with(Note(story))
        .with(Memoir(caps));
    if teaches.is_empty() {
        spawn
    } else {
        spawn.with(Teaches(teaches))
    }
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

    use super::*;
    use crate::blueprint::Blueprints;
    use crate::command::{Command, DamageCause};

    fn pit_fighter() -> GameState {
        let data = Blueprints::load(&crate::testkit::data()).unwrap();
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

    fn level(state: &GameState, skill: &str) -> u32 {
        let id = state.blueprints.skills().id(skill).unwrap();
        crate::skill::level(crate::skill::xp(&state.world, state.player, id))
    }

    #[test]
    fn a_memoir_raises_skills_to_a_share_of_theirs_once_and_never_lowers_one() {
        let mut state = pit_fighter();
        let player = state.player;
        let dodge = state.blueprints.skills().id("Dodge").unwrap();
        state
            .world
            .get::<&mut Skills>(player)
            .unwrap()
            .set(dodge, crate::skill::xp_for(3));
        let gap = Gap {
            name: "years".into(),
            weight: 1,
            survives: 0,
            lesson: 50,
            journal: true,
        };
        let record = Record {
            skills: vec![("Bow".into(), 8), ("Dodge".into(), 2), ("Gone".into(), 9)],
            techniques: vec!["PushKick".into(), "NoSuchKick".into()],
            ..life("Ana", &[], &[])
        };
        let book = memoir(&state, &record, &gap).carried_by(player);
        let book = state.spawn(book).unwrap();
        let discovered = state
            .world
            .get::<&Discovered>(player)
            .ok()
            .map(|d| d.0.clone());
        state.run(Command::Study {
            actor: player,
            item: book,
        });
        assert_eq!(level(&state, "Bow"), 4, "half of 8");
        assert_eq!(level(&state, "Dodge"), 3, "above its cap of 1, untouched");
        let known: Vec<String> = crate::technique::known(&state.world, player)
            .into_iter()
            .filter_map(|t| state.world.get::<&Made>(t).ok().map(|m| m.0.clone()))
            .collect();
        assert!(known.contains(&"PushKick".to_string()), "{known:?}");
        assert!(state.log.recent(6).any(|line| line.starts_with(
            "The memoir of Ana, pit fighter. It ends on turn 0: killed by the goblin's blow."
        )));
        assert_eq!(
            state
                .world
                .get::<&Discovered>(player)
                .ok()
                .map(|d| d.0.clone()),
            discovered,
            "a lesson has no stakes, so no inspiration"
        );
        state.run(Command::Study {
            actor: player,
            item: book,
        });
        assert_eq!(
            crate::testkit::last_log(&state),
            "You have nothing more to learn from the memoir of Ana."
        );
    }

    #[test]
    fn a_journal_reveals_what_they_discovered_and_a_ruined_page_teaches_nothing() {
        let mut state = pit_fighter();
        let player = state.player;
        let journal = Spawn::blueprint("Journal")
            .with(Name("journal of Ana".into()))
            .with(Reveals(vec!["Footwork".into()]))
            .carried_by(player);
        let journal = state.spawn(journal).unwrap();
        state.run(Command::Study {
            actor: player,
            item: journal,
        });
        assert_eq!(crate::testkit::last_log(&state), "You read of Footwork.");
        let footwork = crate::discovery::ideas(&state)
            .into_iter()
            .find(|idea| idea.name == "Footwork")
            .unwrap();
        assert_eq!(footwork.unmet, footwork.missing.len());
        let page = state
            .spawn(Spawn::blueprint("RuinedPage").carried_by(player))
            .unwrap();
        state.run(Command::Study {
            actor: player,
            item: page,
        });
        assert_eq!(
            crate::testkit::last_log(&state),
            "You find nothing to learn in the ruined page."
        );
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
        Blueprints::load(&crate::testkit::data()).unwrap()
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

    const WRITINGS: [&str; 3] = ["Memoir", "Journal", "RuinedPage"];

    /// What lies at camp besides the player and the writings every ruin
    /// has: (blueprint, x, y), sorted.
    fn ruins(state: &GameState) -> Vec<(String, i32, i32)> {
        let mut found: Vec<(String, i32, i32)> = state
            .world
            .query::<(hecs::Entity, &Made, &Position)>()
            .iter()
            .filter(|&(entity, made, _)| {
                entity != state.player && !WRITINGS.contains(&made.0.as_str())
            })
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
            .query::<(&Note, &Made)>()
            .iter()
            .find(|(_, made)| made.0 == "AbandonedCamp")
            .map(|(note, _)| note.0.clone())
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
    fn writings_are_never_found_lying_about() {
        let data = data();
        let pool: Vec<&str> = data
            .items()
            .into_iter()
            .filter_map(|id| data.by_id(id).map(|bp| bp.name.as_str()))
            .collect();
        for writing in WRITINGS {
            assert!(!pool.contains(&writing), "{writing}");
        }
        assert!(pool.contains(&"Torch"));
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
