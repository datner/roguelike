//! Discovery (ADR 0018): combinations of skills that, once reached, inspire
//! the learner with something new. Listed in `data/discoveries.ron`; checked
//! only after a use with stakes, so practice never inspires.

use std::path::Path;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintError, BlueprintId, Blueprints, ComponentDef};
use crate::entity::{Discovered, Proficiency, Revealed};
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
    /// Being on a dive in this phase, by key (docs/plans/phases.md): the
    /// moment of inspiration must happen there.
    Phase(String),
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
    /// A recipe (docs/plans/crafting.md), by name: known from then on.
    Recipe(String),
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
    /// The phase it must happen in, by key, if any.
    pub phase: Option<String>,
    pub grants: Granted,
}

/// What a discovery gives, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Granted {
    /// A style, carried until followed; else a proficiency or a technique,
    /// attached.
    Blueprint { id: BlueprintId, carried: bool },
    /// A recipe by name; `Blueprints::with_crafting` checks it exists.
    Recipe(String),
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
                .filter_map(|req| match req {
                    Requirement::Skill(name, level) => Some((name, *level)),
                    Requirement::Phase(_) => None,
                })
                .map(|(name, level)| {
                    let id = blueprints
                        .skills()
                        .id(name)
                        .ok_or_else(|| unknown(name.clone()))?;
                    Ok((id, level))
                })
                .collect::<Result<_, BlueprintError>>()?;
            // Phases load after discoveries; `Blueprints::with_phases` checks it.
            let phase = def.requires.iter().find_map(|req| match req {
                Requirement::Phase(key) => Some(key.clone()),
                Requirement::Skill(..) => None,
            });
            let (granted, carried) = match &def.grants {
                Grant::Proficiency(name) | Grant::Technique(name) => (name, false),
                Grant::Style(name) => (name, true),
                // Recipes load after discoveries; `with_crafting` checks it.
                Grant::Recipe(name) => {
                    return Ok(Discovery {
                        name: def.name.clone(),
                        requires,
                        phase,
                        grants: Granted::Recipe(name.clone()),
                    });
                }
            };
            let fits = |bp: &crate::blueprint::ResolvedBlueprint| match def.grants {
                Grant::Proficiency(_) => bp.components.contains(&ComponentDef::Proficiency),
                Grant::Style(_) => bp.style.is_some(),
                Grant::Technique(_) => bp.technique.is_some(),
                Grant::Recipe(_) => false,
            };
            let id = blueprints
                .id(granted)
                .filter(|&id| blueprints.by_id(id).is_some_and(fits))
                .ok_or_else(|| unknown(granted.clone()))?;
            if blueprints.by_id(id).is_some_and(|bp| bp.is_ability()) {
                return Err(BlueprintError::AbilityHandedOut {
                    by: format!("discovery {:?}", def.name),
                    ability: granted.clone(),
                });
            }
            Ok(Discovery {
                name: def.name,
                requires,
                phase,
                grants: Granted::Blueprint { id, carried },
            })
        })
        .collect()
}

fn met(state: &GameState, learner: Entity, (skill, level): (SkillId, u32)) -> bool {
    skill::level(skill::xp(&state.world, learner, skill)) >= level
}

/// Whether the player is on a dive in the phase `key`.
fn in_phase(state: &GameState, key: &str) -> bool {
    crate::phase::on_dive(state).is_some_and(|phase| phase.key == key)
}

pub(crate) fn made(state: &GameState, learner: Entity) -> Vec<String> {
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
    let new: Vec<(String, Granted)> = state
        .blueprints
        .discoveries()
        .iter()
        .filter(|d| !discovered.contains(&d.name))
        .filter(|d| d.requires.iter().all(|&req| met(state, learner, req)))
        .filter(|d| d.phase.as_ref().is_none_or(|key| in_phase(state, key)))
        .map(|d| (d.name.clone(), d.grants.clone()))
        .collect();
    for (name, granted) in new {
        let (grants, carried) = match granted {
            Granted::Blueprint { id, carried } => (id, carried),
            Granted::Recipe(recipe) => {
                crate::craft::learn(state, learner, &recipe);
                let message = Subject::of(&state.world, learner).sentence("discover", &name);
                state.log.push(format!("Inspiration! {message}"));
                discovered.push(name);
                continue;
            }
        };
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
/// least half its requirements met, or one read of in a predecessor's
/// journal (docs/plans/legacy.md). Only the met requirements are named,
/// unless it was read of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaView {
    pub name: String,
    /// Skill and level, for each requirement already met.
    pub met: Vec<(String, u32)>,
    /// How many requirements are still unmet.
    pub unmet: usize,
    /// The unmet ones by skill and level, when it was read of; else empty.
    pub missing: Vec<(String, u32)>,
    /// The phase it must happen in, by name ("the embers"), and whether the
    /// player is on a dive in it now: where to go is always said.
    pub phase: Option<(String, bool)>,
}

/// The player's half-formed ideas, in file order, then what their followed
/// styles are close to inventing.
pub fn ideas(state: &GameState) -> Vec<IdeaView> {
    let player = state.player;
    let discovered = made(state, player);
    let revealed = state
        .world
        .get::<&Revealed>(player)
        .map(|r| r.0.clone())
        .unwrap_or_default();
    let book = state.blueprints.skills();
    let named = |reqs: Vec<&(SkillId, u32)>| -> Vec<(String, u32)> {
        reqs.into_iter()
            .filter_map(|&(id, level)| Some((book.get(id)?.name.clone(), level)))
            .collect()
    };
    state
        .blueprints
        .discoveries()
        .iter()
        .filter(|d| !discovered.contains(&d.name))
        .filter_map(|d| {
            let (met, missing): (Vec<_>, Vec<_>) =
                d.requires.iter().partition(|&&req| met(state, player, req));
            let phase = d.phase.as_ref().map(|key| {
                let name = state
                    .blueprints
                    .phases()
                    .iter()
                    .find(|phase| &phase.key == key)
                    .map_or_else(|| key.clone(), |phase| phase.name.clone());
                (name, in_phase(state, key))
            });
            let away = usize::from(phase.as_ref().is_some_and(|&(_, here)| !here));
            let (met, unmet) = (named(met), missing.len() + away);
            let read_of = revealed.contains(&d.name);
            (read_of || (!met.is_empty() && met.len() >= unmet)).then(|| IdeaView {
                name: d.name.clone(),
                met,
                unmet,
                missing: if read_of { named(missing) } else { Vec::new() },
                phase,
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
                        missing: Vec::new(),
                        phase: None,
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
        Blueprints::load(&crate::testkit::data()).unwrap()
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
                missing: Vec::new(),
                phase: None,
            }]
        );
        // In no discovery at all.
        assert!(ideas(&learner(&[("Thrown", 9)])).is_empty());
    }

    #[test]
    fn a_discovery_read_of_is_listed_with_all_it_needs_before_any_is_met() {
        let mut state = learner(&[]);
        let player = state.player;
        let _ = state
            .world
            .insert_one(player, Revealed(vec!["Footwork".into()]));
        let idea = ideas(&state)
            .into_iter()
            .find(|i| i.name == "Footwork")
            .unwrap();
        assert!(idea.met.is_empty());
        assert_eq!(idea.unmet, idea.missing.len());
        assert_eq!(
            idea.missing,
            [("Dodge".to_string(), 4), ("Poise".to_string(), 2)]
        );
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
