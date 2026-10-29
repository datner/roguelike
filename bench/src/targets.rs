//! Pacing targets (`docs/vision.md`, Pacing; docs/plans/floor-bot.md): the ranges in
//! `bench/targets.ron`, the floors a build takes to reach a level, and which measured numbers fall
//! outside their range. A moved target is a design change, committed with its reason.

use serde::Deserialize;

use game::balance::DiveRun;

use crate::floors::{DiveRow, floors};

/// `bench/targets.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct Targets {
    /// What converts keypresses to time.
    pub actions_per_hour: f64,
    /// Mean fight length, in keypresses.
    pub fight: (f64, f64),
    /// Mean keypresses per floor.
    pub floor: (f64, f64),
    /// Floors for the most-trained skill to reach each level.
    pub levels: Vec<LevelTarget>,
    /// How many runs of some builds should reach a floor.
    #[serde(default)]
    pub survival: Vec<Survival>,
}

/// The share of runs of `builds` that should reach `floor`, for a tier of
/// character (starting characters should usually make the first landing).
#[derive(Debug, Clone, Deserialize)]
pub struct Survival {
    pub builds: Vec<String>,
    /// The dives (by name) it applies to: where such a character would go.
    pub dives: Vec<String>,
    pub floor: u32,
    /// Fractions of the runs, low to high.
    pub share: (f64, f64),
}

#[derive(Debug, Clone, Deserialize)]
pub struct LevelTarget {
    pub level: u32,
    pub floors: (f64, f64),
    /// What the level stands for ("the first style principle").
    pub stands_for: String,
}

impl Default for Targets {
    fn default() -> Self {
        Self {
            actions_per_hour: 1000.0,
            fight: (0.0, f64::INFINITY),
            floor: (0.0, f64::INFINITY),
            levels: Vec::new(),
            survival: Vec::new(),
        }
    }
}

/// How many floors the dive's most-trained skill takes to reach a level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reach {
    /// Already there as the dive began.
    Start,
    /// On this floor, within the floors played.
    Played(f64),
    /// Beyond them, at the last floor's rate.
    Extrapolated(f64),
    /// Never, at the rate the dive ended on.
    Never,
}

impl Reach {
    pub fn floors(self) -> Option<f64> {
        match self {
            Reach::Start => Some(0.0),
            Reach::Played(n) | Reach::Extrapolated(n) => Some(n),
            Reach::Never => None,
        }
    }

    /// "4", "~12" (extrapolated), "start", "never".
    pub fn label(self) -> String {
        match self {
            Reach::Start => "start".into(),
            Reach::Played(n) => format!("{n:.0}"),
            Reach::Extrapolated(n) => format!("~{n:.0}"),
            Reach::Never => "never".into(),
        }
    }
}

/// The skill that gained most over the runs, by name.
pub fn top_skill(runs: &[DiveRun]) -> Option<String> {
    let mut by_skill = std::collections::BTreeMap::<&str, u64>::new();
    for floor in runs.iter().flat_map(|run| &run.floors) {
        for (skill, xp) in &floor.xp {
            *by_skill.entry(skill).or_default() += u64::from(*xp);
        }
    }
    by_skill
        .into_iter()
        .max_by_key(|&(name, xp)| (xp, std::cmp::Reverse(name)))
        .map(|(name, _)| name.to_string())
}

/// Floors for `skill` to reach `level`: the mean XP it starts with, plus each
/// floor's mean gain over the runs that reached it, while at least half the
/// runs did, then the last such floor's gain repeated.
pub fn reach(runs: &[DiveRun], skill: &str, level: u32) -> Reach {
    let needed = f64::from(game::skill::xp_for(level));
    let count = runs.len().max(1) as f64;
    let mut xp = runs
        .iter()
        .map(|run| f64::from(run.start.get(skill).copied().unwrap_or(0)))
        .sum::<f64>()
        / count;
    if xp >= needed {
        return Reach::Start;
    }
    // Floors only a few runs survived to say little: count those at least
    // half the runs reached (the first always), then extrapolate.
    let deepest = (1..)
        .take_while(|&depth| {
            depth == 1
                || runs.iter().filter(|run| run.floors.len() >= depth).count() * 2 >= runs.len()
        })
        .take_while(|&depth| runs.iter().any(|run| run.floors.len() >= depth))
        .count();
    let mut last = 0.0;
    for index in 0..deepest {
        let gains: Vec<f64> = runs
            .iter()
            .filter_map(|run| run.floors.get(index))
            .map(|floor| f64::from(floor.xp.get(skill).copied().unwrap_or(0)))
            .collect();
        last = gains.iter().sum::<f64>() / gains.len().max(1) as f64;
        if xp + last >= needed {
            return Reach::Played(index as f64 + (needed - xp) / last);
        }
        xp += last;
    }
    if last <= 0.0 {
        return Reach::Never;
    }
    Reach::Extrapolated(deepest as f64 + (needed - xp) / last)
}

/// One measured number outside its target range.
#[derive(Debug, Clone, PartialEq)]
pub struct Miss {
    pub build: String,
    pub dive: String,
    /// "fight length", "keypresses per floor", "floors to level 5".
    pub what: String,
    pub value: String,
    pub range: (f64, f64),
}

fn outside(value: f64, (low, high): (f64, f64)) -> bool {
    value < low || value > high
}

/// Every row's numbers checked against `targets`.
pub fn misses(rows: &[DiveRow], targets: &Targets) -> Vec<Miss> {
    let mut misses = Vec::new();
    for row in rows {
        let floors = floors(&row.runs);
        let miss = |what: &str, value: String, range| Miss {
            build: row.build.clone(),
            dive: row.dive.name.clone(),
            what: what.into(),
            value,
            range,
        };
        let fights: Vec<f64> = row
            .runs
            .iter()
            .flat_map(|run| &run.floors)
            .flat_map(|floor| floor.fights.iter().map(|&l| f64::from(l)))
            .collect();
        if !fights.is_empty() {
            let fight = fights.iter().sum::<f64>() / fights.len() as f64;
            if outside(fight, targets.fight) {
                misses.push(miss("fight length", format!("{fight:.1}"), targets.fight));
            }
        }
        if !floors.is_empty() {
            let floor = floors
                .iter()
                .map(|floor| floor.keypresses.mean)
                .sum::<f64>()
                / floors.len() as f64;
            if outside(floor, targets.floor) {
                misses.push(miss(
                    "keypresses per floor",
                    format!("{floor:.0}"),
                    targets.floor,
                ));
            }
        }
        for target in targets.survival.iter().filter(|target| {
            target.builds.contains(&row.build) && target.dives.contains(&row.dive.name)
        }) {
            let floor = usize::try_from(target.floor).unwrap_or(usize::MAX);
            let reached = row
                .runs
                .iter()
                .filter(|run| run.floors.len() >= floor)
                .count();
            let share = reached as f64 / row.runs.len().max(1) as f64;
            if outside(share, target.share) {
                misses.push(miss(
                    &format!("runs reaching floor {}", target.floor),
                    format!("{:.0}%", share * 100.0),
                    target.share,
                ));
            }
        }
        let Some(skill) = top_skill(&row.runs) else {
            continue;
        };
        for target in &targets.levels {
            let reach = reach(&row.runs, &skill, target.level);
            if reach.floors().is_none_or(|n| outside(n, target.floors)) {
                misses.push(miss(
                    &format!("floors to {skill} {}", target.level),
                    reach.label(),
                    target.floors,
                ));
            }
        }
    }
    misses
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use game::balance::{Dive, End, FloorRun};

    use super::*;

    fn run(start: u32, per_floor: &[u32]) -> DiveRun {
        DiveRun {
            start: BTreeMap::from([("Melee".to_string(), start)]),
            floors: per_floor
                .iter()
                .map(|&xp| FloorRun {
                    keypresses: 40,
                    xp: BTreeMap::from([("Melee".to_string(), xp)]),
                    fights: vec![5],
                    ..FloorRun::default()
                })
                .collect(),
            ended: End::Depth,
        }
    }

    #[test]
    fn floors_to_a_level_add_up_then_extrapolate() {
        // Level 5 is 1250 XP: 250 a floor from 0 reaches it on the fifth.
        let runs = [run(0, &[250, 250, 250, 250, 250, 250])];
        assert_eq!(reach(&runs, "Melee", 5), Reach::Played(5.0));
        // Level 10 is 5000: 1500 in six floors, then 3500 more at 250.
        assert_eq!(reach(&runs, "Melee", 10), Reach::Extrapolated(20.0));
        assert_eq!(reach(&[run(1250, &[10])], "Melee", 5), Reach::Start);
        assert_eq!(reach(&[run(0, &[100, 0])], "Melee", 5), Reach::Never);
        // The second floor, reached by one run in three, is not counted.
        let runs = [run(0, &[250]), run(0, &[250]), run(0, &[250, 1000])];
        assert_eq!(reach(&runs, "Melee", 5), Reach::Extrapolated(5.0));
    }

    #[test]
    fn only_numbers_outside_their_range_are_marked() {
        let row = DiveRow {
            build: "bumper".into(),
            dive: Dive {
                name: "Caves".into(),
                dungeon: "Caves".into(),
                phase: None,
                floors: 6,
            },
            runs: vec![run(0, &[250, 250, 250, 250, 250, 250])],
        };
        let targets = Targets {
            actions_per_hour: 1000.0,
            fight: (3.0, 8.0),
            floor: (150.0, 300.0),
            levels: vec![
                LevelTarget {
                    level: 5,
                    floors: (3.0, 12.0),
                    stands_for: String::new(),
                },
                LevelTarget {
                    level: 10,
                    floors: (12.0, 18.0),
                    stands_for: String::new(),
                },
            ],
            survival: vec![Survival {
                builds: vec!["bumper".into()],
                dives: vec!["Caves".into()],
                floor: 3,
                share: (0.6, 0.9),
            }],
        };
        let misses = misses(&[row], &targets);
        let what: Vec<&str> = misses.iter().map(|miss| miss.what.as_str()).collect();
        assert_eq!(
            what,
            [
                "keypresses per floor",
                "runs reaching floor 3",
                "floors to Melee 10"
            ]
        );
        assert_eq!(misses[1].value, "100%", "every run reached it: too easy");
        assert_eq!(misses[2].value, "~20");
    }
}
