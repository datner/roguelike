//! Discovery (ADR 0018): combinations of skills that, once reached, inspire
//! the learner with something new. Listed in `data/discoveries.ron`; checked
//! only after a use with stakes, so practice never inspires.

use std::path::Path;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintError, BlueprintId, Blueprints, ComponentDef};
use crate::entity::{Discovered, Proficiency};
use crate::grammar::Subject;
use crate::holding;
use crate::skill::{self, SkillId};
use crate::spawn::Spawn;
use crate::state::GameState;

/// What a discovery needs, as written in RON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Requirement {
    /// A skill, by name, at least at this level.
    Skill(String, u32),
}

/// What a discovery gives, as written in RON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grant {
    /// A proficiency: this blueprint, attached to the learner.
    Proficiency(String),
    /// A style (ADR 0019): this blueprint, known but not yet followed.
    Style(String),
    /// A technique (ADR 0024): this blueprint, attached to the learner.
    Technique(String),
}

/// One entry of `data/discoveries.ron`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryDef {
    pub name: String,
    pub requires: Vec<Requirement>,
    pub grants: Grant,
}

/// A discovery after load: every name resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Discovery {
    pub name: String,
    pub requires: Vec<(SkillId, u32)>,
    pub grants: BlueprintId,
    /// A style, carried until followed; else a proficiency or a technique,
    /// attached.
    pub carried: bool,
}

/// Reads `data/discoveries.ron`: a list of `DiscoveryDef`s.
pub(crate) fn read(path: &Path) -> Result<Vec<DiscoveryDef>, BlueprintError> {
    let text = std::fs::read_to_string(path).map_err(|err| BlueprintError::Io(path.into(), err))?;
    ron::from_str(&text).map_err(|err| BlueprintError::Parse(path.into(), Box::new(err)))
}

/// Resolves `defs` against `blueprints`: every skill and blueprint named
/// must exist, and the blueprint granted must be a proficiency, a style or a
/// technique, as the grant says.
pub(crate) fn resolve(
    defs: Vec<DiscoveryDef>,
    blueprints: &Blueprints,
) -> Result<Vec<Discovery>, BlueprintError> {
    defs.into_iter()
        .map(|def| {
            let unknown = |what: String| BlueprintError::UnknownInDiscovery {
                discovery: def.name.clone(),
                what,
            };
            let requires = def
                .requires
                .iter()
                .map(|Requirement::Skill(name, level)| {
                    let id = blueprints
                        .skills()
                        .id(name)
                        .ok_or_else(|| unknown(name.clone()))?;
                    Ok((id, *level))
                })
                .collect::<Result<_, BlueprintError>>()?;
            let (granted, carried) = match &def.grants {
                Grant::Proficiency(name) | Grant::Technique(name) => (name, false),
                Grant::Style(name) => (name, true),
            };
            let fits = |bp: &crate::blueprint::ResolvedBlueprint| match def.grants {
                Grant::Proficiency(_) => bp.components.contains(&ComponentDef::Proficiency),
                Grant::Style(_) => bp.style.is_some(),
                Grant::Technique(_) => bp.technique.is_some(),
            };
            let grants = blueprints
                .id(granted)
                .filter(|&id| blueprints.by_id(id).is_some_and(fits))
                .ok_or_else(|| unknown(granted.clone()))?;
            if blueprints.by_id(grants).is_some_and(|bp| bp.is_ability()) {
                return Err(BlueprintError::AbilityHandedOut {
                    by: format!("discovery {:?}", def.name),
                    ability: granted.clone(),
                });
            }
            Ok(Discovery {
                name: def.name,
                requires,
                grants,
                carried,
            })
        })
        .collect()
}

fn met(state: &GameState, learner: Entity, (skill, level): (SkillId, u32)) -> bool {
    skill::level(skill::xp(&state.world, learner, skill)) >= level
}

fn made(state: &GameState, learner: Entity) -> Vec<String> {
    state
        .world
        .get::<&Discovered>(learner)
        .map(|discovered| discovered.0.clone())
        .unwrap_or_default()
}

/// A moment of inspiration: every discovery `learner` has newly met the
/// requirements of, in file order, is made: a proficiency or a technique is
/// attached, a style becomes known. Called only from `skill::train`, after a use with
/// stakes.
pub(crate) fn inspire(state: &mut GameState, learner: Entity) {
    let mut discovered = made(state, learner);
    let new: Vec<(String, BlueprintId, bool)> = state
        .blueprints
        .discoveries()
        .iter()
        .filter(|d| !discovered.contains(&d.name))
        .filter(|d| d.requires.iter().all(|&req| met(state, learner, req)))
        .map(|d| (d.name.clone(), d.grants, d.carried))
        .collect();
    for (name, grants, carried) in new {
        // A technique may already be known, from a manual.
        let noun = state.blueprints.by_id(grants).and_then(|bp| bp.noun());
        if noun.is_some_and(|noun| crate::technique::knows(&state.world, learner, noun)) {
            discovered.push(name);
            continue;
        }
        let spawn = Spawn::id(grants);
        let spawn = if carried {
            spawn.carried_by(learner)
        } else {
            spawn.attached_to(learner)
        };
        if state.spawn(spawn).is_err() {
            continue;
        }
        let message = Subject::of(&state.world, learner).sentence("discover", &name);
        state.log.push(format!("Inspiration! {message}"));
        discovered.push(name);
    }
    discovered.sort();
    let _ = state.world.insert_one(learner, Discovered(discovered));
}

/// A half-formed idea, for the journal: a discovery not yet made with at
/// least half its requirements met. Only the met ones are named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaView {
    pub name: String,
    /// Skill and level, for each requirement already met.
    pub met: Vec<(String, u32)>,
    /// How many requirements are still unmet (and unnamed).
    pub unmet: usize,
}

/// The player's half-formed ideas, in file order, then what their followed
/// styles are close to inventing.
pub fn ideas(state: &GameState) -> Vec<IdeaView> {
    let player = state.player;
    let discovered = made(state, player);
    let book = state.blueprints.skills();
    state
        .blueprints
        .discoveries()
        .iter()
        .filter(|d| !discovered.contains(&d.name))
        .filter_map(|d| {
            let met: Vec<(String, u32)> = d
                .requires
                .iter()
                .filter(|&&req| met(state, player, req))
                .filter_map(|&(id, level)| Some((book.get(id)?.name.clone(), level)))
                .collect();
            let unmet = d.requires.len() - met.len();
            (!met.is_empty() && met.len() >= unmet).then(|| IdeaView {
                name: d.name.clone(),
                met,
                unmet,
            })
        })
        // A followed style close to inventing something (ADR 0024).
        .chain(
            crate::style::near_inventions(state, player)
                .into_iter()
                .filter_map(|(style, level, noun)| {
                    Some(IdeaView {
                        name: noun,
                        met: vec![(book.get(style)?.name.clone(), level)],
                        unmet: 1,
                    })
                }),
        )
        .collect()
}

/// The names of the proficiencies the player has.
pub fn proficiencies(state: &GameState) -> Vec<String> {
    holding::attached(&state.world, state.player)
        .into_iter()
        .filter(|&thing| state.world.satisfies::<&Proficiency>(thing))
        .map(|thing| {
            Subject::of(&state.world, thing)
                .noun()
                .trim_start_matches("the ")
                .to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat;
    use crate::entity::{Footwork, Learner, Skills};
    use crate::state::tests::bare_state_with;
    use crate::testkit::{last_log, thing};

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    /// The player as a learner at these levels.
    fn learner(levels: &[(&str, u32)]) -> GameState {
        let mut state = bare_state_with(data());
        let player = state.player;
        let mut skills = Skills::default();
        for &(name, level) in levels {
            let id = state.blueprints.skills().id(name).unwrap();
            skills.set(id, skill::xp_for(level));
        }
        state.world.insert(player, (Learner, skills)).unwrap();
        state
    }

    /// One training use with stakes, of a skill unrelated to any discovery.
    fn train_once(state: &mut GameState) {
        let melee = state.blueprints.skills().id("Melee").unwrap();
        let goblin = thing("goblin").hostile().make(state);
        let player = state.player;
        skill::train(state, player, &[melee], goblin, 1);
    }

    #[test]
    fn reaching_the_requirements_inspires_once_and_attaches_the_proficiency() {
        let mut state = learner(&[("Dodge", 4), ("Poise", 1)]);
        train_once(&mut state);
        assert!(proficiencies(&state).is_empty());

        let mut state = learner(&[("Dodge", 4), ("Poise", 2)]);
        train_once(&mut state);
        assert_eq!(last_log(&state), "Inspiration! You discover Footwork.");
        assert_eq!(proficiencies(&state), ["footwork"]);
        train_once(&mut state);
        assert_eq!(proficiencies(&state), ["footwork"], "made once");
    }

    #[test]
    fn archery_and_unarmed_inspire_bow_dancing() {
        let mut state = learner(&[("Archery", 3), ("Unarmed", 2)]);
        train_once(&mut state);
        assert_eq!(last_log(&state), "Inspiration! You discover Bow dancing.");
        let player = state.player;
        assert!(
            crate::style::known(&state.world, player)
                .into_iter()
                .any(
                    |style| state.world.get::<&crate::entity::Name>(style).unwrap().0
                        == "bow dancing"
                )
        );
    }

    #[test]
    fn footwork_helps_only_its_holder_dodge() {
        let mut state = learner(&[("Dodge", 4), ("Poise", 2)]);
        train_once(&mut state);
        let player = state.player;
        let goblin = thing("goblin")
            .melee(1, 1)
            .hostile()
            .at(2, 1)
            .make(&mut state);
        state
            .world
            .insert_one(player, crate::entity::Melee { min: 1, max: 1 })
            .unwrap();
        let strike = |state: &GameState, who| {
            combat::strike(&state.world, state.blueprints.skills(), who).unwrap()
        };
        let defending = combat::odds(&state, &strike(&state, goblin), goblin, player);
        assert_eq!(
            defending.perfect_bp,
            2 * 100 + Footwork { perfect_bp: 500 }.perfect_bp
        );
        let attacking = combat::odds(&state, &strike(&state, player), player, goblin);
        assert_eq!(attacking.perfect_bp, 0);
    }

    #[test]
    fn the_journal_shows_ideas_half_met_naming_only_what_is_met() {
        let half = ideas(&learner(&[("Poise", 2)]));
        assert_eq!(
            half,
            [IdeaView {
                name: "Footwork".into(),
                met: vec![("Poise".into(), 2)],
                unmet: 1,
            }]
        );
        // In no discovery at all.
        assert!(ideas(&learner(&[("Thrown", 9)])).is_empty());
    }

    #[test]
    fn a_discovery_must_name_real_skills_and_a_proficiency() {
        let def = |skill: &str, grants: &str| DiscoveryDef {
            name: "Odd".into(),
            requires: vec![Requirement::Skill(skill.into(), 1)],
            grants: Grant::Proficiency(grants.into()),
        };
        let error = |defs| data().with_discoveries(defs).unwrap_err().to_string();
        assert_eq!(
            error(vec![def("Juggling", "Footwork")]),
            r#"discovery "Odd" names unknown "Juggling" (or not a proficiency)"#
        );
        assert_eq!(
            error(vec![def("Dodge", "Goblin")]),
            r#"discovery "Odd" names unknown "Goblin" (or not a proficiency)"#
        );
    }
}
