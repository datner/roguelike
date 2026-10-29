//! The floor bot's half of the bench (docs/plans/floor-bot.md): the dives in `bench/dives.ron`,
//! each listed build played through each over the seeds by `game::balance::dive`, and the report's
//! Floors section: what a floor costs and gives, and how dives end.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use game::Blueprints;
use game::balance::{self, Build, Dive, DiveRun, End, FloorRun};
use serde::Deserialize;

use crate::targets::{self, Targets};
use crate::{number, text};

/// `bench/dives.ron`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Dives {
    /// Seeds `0..seeds` per dive, unless `--seeds` says otherwise.
    pub seeds: u64,
    pub sets: Vec<DiveSet>,
}

/// Every build listed played through every dive listed.
#[derive(Debug, Clone, Deserialize)]
pub struct DiveSet {
    /// Build names, from `bench/builds/`.
    pub builds: Vec<String>,
    pub dives: Vec<Dive>,
}

/// One build through one dive, every seed.
#[derive(Debug, Clone, PartialEq)]
pub struct DiveRow {
    pub build: String,
    pub dive: Dive,
    pub runs: Vec<DiveRun>,
}

/// Every set's builds through its dives, over seeds `0..seeds`.
pub fn measure(
    blueprints: &Blueprints,
    builds: &[Build],
    dives: &Dives,
    seeds: u64,
) -> Result<Vec<DiveRow>, String> {
    let mut rows = Vec::new();
    for set in &dives.sets {
        for dive in &set.dives {
            for name in &set.builds {
                let build = builds
                    .iter()
                    .find(|build| &build.name == name)
                    .ok_or_else(|| format!("dives.ron: no build named {name:?}"))?;
                let runs = (0..seeds)
                    .map(|seed| balance::dive(blueprints, build, dive, seed))
                    .collect::<Result<_, _>>()?;
                rows.push(DiveRow {
                    build: name.clone(),
                    dive: dive.clone(),
                    runs,
                });
            }
        }
    }
    Ok(rows)
}

/// One floor over the runs that reached it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloorSummary {
    /// Runs that reached the floor.
    pub reached: usize,
    pub keypresses: Spread,
    pub turns: f64,
    pub xp: f64,
    /// The skill that gained most on the floor over all its runs, and its
    /// mean gain per run.
    pub top_skill: Option<(String, f64)>,
    pub hp_lost: f64,
    /// Turns spent resting.
    pub rested: f64,
    /// Fights per run, and every fight's length.
    pub fights: f64,
    pub fight: Spread,
}

/// A mean with its 10th and 90th percentiles.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spread {
    pub mean: f64,
    pub low: f64,
    pub high: f64,
}

impl Spread {
    pub fn of(values: impl IntoIterator<Item = f64>) -> Self {
        let mut values: Vec<f64> = values.into_iter().collect();
        if values.is_empty() {
            return Self::default();
        }
        values.sort_by(f64::total_cmp);
        let at = |share: f64| values[((values.len() - 1) as f64 * share).round() as usize];
        Self {
            mean: values.iter().sum::<f64>() / values.len() as f64,
            low: at(0.1),
            high: at(0.9),
        }
    }
}

/// Floor by floor: index 0 is the first floor.
pub fn floors(runs: &[DiveRun]) -> Vec<FloorSummary> {
    let deepest = runs.iter().map(|run| run.floors.len()).max().unwrap_or(0);
    (0..deepest)
        .map(|index| {
            let reached: Vec<&FloorRun> = runs
                .iter()
                .filter_map(|run| run.floors.get(index))
                .collect();
            let count = reached.len().max(1) as f64;
            let mean =
                |of: &dyn Fn(&FloorRun) -> f64| reached.iter().map(|f| of(f)).sum::<f64>() / count;
            let mut by_skill: BTreeMap<&str, u32> = BTreeMap::new();
            for floor in &reached {
                for (skill, xp) in &floor.xp {
                    *by_skill.entry(skill).or_default() += xp;
                }
            }
            let top_skill = by_skill
                .iter()
                .max_by_key(|&(name, xp)| (*xp, std::cmp::Reverse(*name)))
                .map(|(name, xp)| (name.to_string(), f64::from(*xp) / count));
            FloorSummary {
                reached: reached.len(),
                keypresses: Spread::of(reached.iter().map(|f| f64::from(f.keypresses))),
                turns: mean(&|f| f64::from(f.turns)),
                xp: mean(&|f| f64::from(f.xp_total())),
                top_skill,
                hp_lost: mean(&|f| f64::from(f.hp_lost)),
                rested: mean(&|f| f64::from(f.rested)),
                fights: mean(&|f| f.fights.len() as f64),
                fight: Spread::of(
                    reached
                        .iter()
                        .flat_map(|f| f.fights.iter().map(|&length| f64::from(length))),
                ),
            }
        })
        .collect()
}

/// How the runs ended: reached the depth, died (with what killed them,
/// commonest first), stuck.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Endings {
    pub depth: usize,
    pub died: usize,
    pub stuck: usize,
    pub causes: Vec<(String, usize)>,
}

pub fn endings(runs: &[DiveRun]) -> Endings {
    let mut endings = Endings::default();
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    for run in runs {
        match run.ended {
            End::Depth => endings.depth += 1,
            End::Stuck => endings.stuck += 1,
            End::Died => {
                endings.died += 1;
                let cause = run.floors.last().and_then(|floor| floor.died_to.clone());
                *causes.entry(cause.unwrap_or_default()).or_default() += 1;
            }
        }
    }
    endings.causes = causes.into_iter().collect();
    endings
        .causes
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    endings
}

/// The report's Floors section: what is off target, then one table per
/// dive, how its runs ended and how fast they level.
pub fn report(out: &mut String, rows: &[DiveRow], seeds: u64, targets: &Targets) {
    if rows.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## Floors\n");
    let _ = writeln!(
        out,
        "The floor bot (`docs/plans/floor-bot.md`) plays each build from camp through real floors, \
         {seeds} seeds per dive: it fights what it sees with its policy and explores the rest. \
         Keypresses count a run of explore steps as one. Per floor, over the runs that reached \
         it: mean (10th–90th percentile). Targets are `bench/targets.ron`; minutes assume {} \
         keypresses an hour.",
        targets.actions_per_hour
    );
    let misses = targets::misses(rows, targets);
    if !misses.is_empty() {
        let _ = writeln!(out, "\n### Off target\n");
        for miss in &misses {
            let _ = writeln!(
                out,
                "- {} in {}: {} {} (target {}–{})",
                miss.build, miss.dive, miss.what, miss.value, miss.range.0, miss.range.1
            );
        }
    }
    let mut dives: Vec<&Dive> = Vec::new();
    for row in rows {
        if !dives.contains(&&row.dive) {
            dives.push(&row.dive);
        }
    }
    for dive in dives {
        let phase = dive.phase.as_deref().unwrap_or("the starting phase");
        let _ = writeln!(
            out,
            "\n### {}\n\n{}, {phase}, {} floors.\n",
            dive.name, dive.dungeon, dive.floors
        );
        let _ = writeln!(
            out,
            "| build | floor | reached | keypresses | turns | XP (most to) | HP lost | rested | fights | fight length |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|");
        let mine: Vec<&DiveRow> = rows.iter().filter(|row| &row.dive == dive).collect();
        for row in &mine {
            for (index, floor) in floors(&row.runs).iter().enumerate() {
                let top = floor
                    .top_skill
                    .as_ref()
                    .map_or_else(String::new, |(skill, xp)| format!(" ({skill} {xp:.0})"));
                let _ = writeln!(
                    out,
                    "| {} | {} | {}/{} | {} | {:.0} | {:.0}{top} | {:.1} | {:.0} | {:.1} | {} |",
                    row.build,
                    index + 1,
                    floor.reached,
                    row.runs.len(),
                    spread(&floor.keypresses, 0),
                    floor.turns,
                    floor.xp,
                    floor.hp_lost,
                    floor.rested,
                    floor.fights,
                    spread(&floor.fight, 1),
                );
            }
        }
        let _ = writeln!(out);
        for row in &mine {
            let ended = endings(&row.runs);
            let causes: Vec<String> = ended
                .causes
                .iter()
                .map(|(cause, count)| format!("{cause} {count}"))
                .collect();
            let causes = if causes.is_empty() {
                String::new()
            } else {
                format!(" ({})", causes.join(", "))
            };
            let _ = writeln!(
                out,
                "- **{}**: {} reached floor {}, {} died{causes}, {} stuck.",
                row.build, ended.depth, dive.floors, ended.died, ended.stuck
            );
        }
        progression(out, &mine, targets);
    }
}

/// Per build: the most-trained skill, the floors it takes to reach each
/// target level, and minutes per floor.
fn progression(out: &mut String, rows: &[&DiveRow], targets: &Targets) {
    if targets.levels.is_empty() {
        return;
    }
    let levels: Vec<String> = targets
        .levels
        .iter()
        .map(|target| format!("floors to {}", target.level))
        .collect();
    let _ = writeln!(
        out,
        "\n| build | most trained | {} | minutes per floor |",
        levels.join(" | ")
    );
    let _ = writeln!(out, "|---|---|{}---|", "---|".repeat(levels.len()));
    for row in rows {
        let Some(skill) = targets::top_skill(&row.runs) else {
            continue;
        };
        let reaches: Vec<String> = targets
            .levels
            .iter()
            .map(|target| targets::reach(&row.runs, &skill, target.level).label())
            .collect();
        let summaries = floors(&row.runs);
        let keypresses = summaries
            .iter()
            .map(|floor| floor.keypresses.mean)
            .sum::<f64>()
            / summaries.len().max(1) as f64;
        let _ = writeln!(
            out,
            "| {} | {skill} | {} | {:.1} |",
            row.build,
            reaches.join(" | "),
            keypresses * 60.0 / targets.actions_per_hour
        );
    }
}

fn spread(spread: &Spread, decimals: usize) -> String {
    format!(
        "{:.*} ({:.*}–{:.*})",
        decimals, spread.mean, decimals, spread.low, decimals, spread.high
    )
}

/// The Floors section as JSON: every dive row with its floors, endings and
/// progression, then the targets and what misses them.
pub fn json(rows: &[DiveRow], targets: &Targets) -> String {
    let misses = targets::misses(rows, targets);
    let spread = |s: &Spread| {
        format!(
            "{{\"mean\":{},\"low\":{},\"high\":{}}}",
            number(s.mean),
            number(s.low),
            number(s.high)
        )
    };
    let rows: Vec<String> = rows
        .iter()
        .map(|row| {
            let floors: Vec<String> = floors(&row.runs)
                .iter()
                .map(|floor| {
                    let top = floor.top_skill.as_ref().map_or_else(
                        || "null".to_string(),
                        |(skill, xp)| format!("{{\"skill\":{},\"xp\":{}}}", text(skill), number(*xp)),
                    );
                    format!(
                        "{{\"reached\":{},\"keypresses\":{},\"turns\":{},\"xp\":{},\"top_skill\":{top},\
                         \"hp_lost\":{},\"rested\":{},\"fights\":{},\"fight\":{}}}",
                        floor.reached,
                        spread(&floor.keypresses),
                        number(floor.turns),
                        number(floor.xp),
                        number(floor.hp_lost),
                        number(floor.rested),
                        number(floor.fights),
                        spread(&floor.fight)
                    )
                })
                .collect();
            let ended = endings(&row.runs);
            let causes: Vec<String> = ended
                .causes
                .iter()
                .map(|(cause, count)| format!("[{},{count}]", text(cause)))
                .collect();
            let skill = targets::top_skill(&row.runs);
            let reaches: Vec<String> = skill.as_deref().map_or_else(Vec::new, |skill| {
                targets
                    .levels
                    .iter()
                    .map(|target| {
                        let reach = targets::reach(&row.runs, skill, target.level);
                        format!(
                            "{{\"level\":{},\"floors\":{},\"extrapolated\":{}}}",
                            target.level,
                            reach.floors().map_or_else(|| "null".to_string(), number),
                            matches!(reach, targets::Reach::Extrapolated(_))
                        )
                    })
                    .collect()
            });
            format!(
                "{{\"build\":{},\"dive\":{},\"dungeon\":{},\"phase\":{},\"floors_asked\":{},\
                 \"runs\":{},\"floors\":[{}],\"depth\":{},\"died\":{},\"stuck\":{},\"causes\":[{}],\
                 \"top_skill\":{},\"reach\":[{}]}}",
                text(&row.build),
                text(&row.dive.name),
                text(&row.dive.dungeon),
                row.dive
                    .phase
                    .as_deref()
                    .map_or_else(|| "null".to_string(), text),
                row.dive.floors,
                row.runs.len(),
                floors.join(","),
                ended.depth,
                ended.died,
                ended.stuck,
                causes.join(","),
                skill.as_deref().map_or_else(|| "null".to_string(), text),
                reaches.join(",")
            )
        })
        .collect();
    let levels: Vec<String> = targets
        .levels
        .iter()
        .map(|target| {
            format!(
                "{{\"level\":{},\"floors\":[{},{}],\"stands_for\":{}}}",
                target.level,
                number(target.floors.0),
                number(target.floors.1),
                text(&target.stands_for)
            )
        })
        .collect();
    let misses: Vec<String> = misses
        .iter()
        .map(|miss| {
            format!(
                "{{\"build\":{},\"dive\":{},\"what\":{},\"value\":{},\"range\":[{},{}]}}",
                text(&miss.build),
                text(&miss.dive),
                text(&miss.what),
                text(&miss.value),
                number(miss.range.0),
                number(miss.range.1)
            )
        })
        .collect();
    format!(
        "{{\"rows\":[{}],\"targets\":{{\"actions_per_hour\":{},\"fight\":[{},{}],\"floor\":[{},{}],\
         \"levels\":[{}]}},\"off_target\":[{}]}}",
        rows.join(","),
        number(targets.actions_per_hour),
        number(targets.fight.0),
        number(targets.fight.1),
        number(targets.floor.0),
        number(targets.floor.1),
        levels.join(","),
        misses.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(keypresses: u32, fights: Vec<u32>) -> FloorRun {
        FloorRun {
            turns: keypresses * 10,
            keypresses,
            ticks: u64::from(keypresses) * 1000,
            xp: BTreeMap::from([("Melee".to_string(), 30), ("Dodge".to_string(), 10)]),
            hp_lost: 4,
            hp_left: 16,
            died_to: None,
            rested: 0,
            fights,
        }
    }

    fn row() -> DiveRow {
        let mut died = floor(20, vec![6]);
        died.died_to = Some("goblin".into());
        DiveRow {
            build: "bumper".into(),
            dive: Dive {
                name: "Caves".into(),
                dungeon: "Caves".into(),
                phase: None,
                floors: 2,
            },
            runs: vec![
                DiveRun {
                    start: BTreeMap::new(),
                    floors: vec![floor(30, vec![4, 8]), floor(40, vec![])],
                    ended: End::Depth,
                },
                DiveRun {
                    start: BTreeMap::new(),
                    floors: vec![floor(50, vec![3]), died],
                    ended: End::Died,
                },
            ],
        }
    }

    /// The Floors section's shape, pinned without running the bot.
    #[test]
    fn a_floors_table_shows_each_floor_and_how_dives_ended() {
        let mut out = String::new();
        report(&mut out, &[row()], 2, &Targets::default());
        let lines: Vec<&str> = out.lines().collect();
        for expected in [
            "### Caves",
            "| bumper | 1 | 2/2 | 40 (30–50) | 400 | 40 (Melee 30) | 4.0 | 0 | 1.5 | 5.0 (3.0–8.0) |",
            "| bumper | 2 | 2/2 | 30 (20–40) | 300 | 40 (Melee 30) | 4.0 | 0 | 0.5 | 6.0 (6.0–6.0) |",
            "- **bumper**: 1 reached floor 2, 1 died (goblin 1), 0 stuck.",
        ] {
            assert!(lines.contains(&expected), "{expected:?} not in\n{out}");
        }
    }

    #[test]
    fn floors_json_has_every_floor() {
        let json = json(&[row()], &Targets::default());
        assert_eq!(json.matches("\"reached\":2").count(), 2);
        assert!(json.contains("\"causes\":[[\"goblin\",1]]"));
        assert_eq!(
            json.matches(['{', '[']).count(),
            json.matches(['}', ']']).count()
        );
    }
}
