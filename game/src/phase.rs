//! Phases (`docs/vision.md`, docs/plans/dives.md): the world turns through phases, and at camp you choose which one to wait for. A dive is fixed to the phase it began in; a dungeon's spawn entries may belong to some phases only. Listed in `data/phases.ron`; the first is where the world starts.

use std::path::Path;

use hecs::Entity;
use serde::Deserialize;

use crate::blueprint::{BlueprintError, ron_options};
use crate::dive::Place;
use crate::entity::{DamageType, DamageTypes};
use crate::state::GameState;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    /// How content and saves name it.
    pub key: String,
    /// What the player reads: "the embers".
    pub name: String,
    /// Logged when you wait for it.
    pub wait: String,
    /// The element it leans toward, if any (docs/plans/phases.md). One kind
    /// of lean among others to come, never what a phase is: nothing may
    /// decide behaviour by matching on it.
    #[serde(default)]
    pub element: Option<DamageType>,
    /// How much stronger damage of `element` is while it holds, in percent,
    /// for everyone alike.
    #[serde(default)]
    pub lean: u32,
    /// The phase it is opposed to, by key; that one names it back.
    #[serde(default)]
    pub opposite: Option<String>,
}

/// `phases.ron` in `data`; at least one phase, keys unique.
pub(crate) fn read(data: &Path) -> Result<Vec<Phase>, BlueprintError> {
    let path = data.join("phases.ron");
    let text =
        std::fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
    let phases: Vec<Phase> = ron_options()
        .from_str(&text)
        .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
    if phases.is_empty() {
        return Err(BlueprintError::BadPhases(
            "there must be at least one phase".into(),
        ));
    }
    check(&phases)?;
    Ok(phases)
}

/// Keys unique; an opposite exists and names this one back.
fn check(phases: &[Phase]) -> Result<(), BlueprintError> {
    let bad = |why: String| Err(BlueprintError::BadPhases(why));
    for (i, phase) in phases.iter().enumerate() {
        if phases[..i].iter().any(|other| other.key == phase.key) {
            return bad(format!("{:?} is listed twice", phase.key));
        }
        let Some(opposite) = &phase.opposite else {
            continue;
        };
        match phases.iter().find(|other| &other.key == opposite) {
            None => {
                return bad(format!(
                    "{:?}'s opposite {opposite:?} is no phase",
                    phase.key
                ));
            }
            Some(other) if other.opposite.as_ref() != Some(&phase.key) => {
                return bad(format!(
                    "{opposite:?} must name {:?} back as its opposite",
                    phase.key
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The phase the world is in for whoever reads it: a dive's own (fixed when
/// it began), or at camp the one being waited for. Every reader of the phase
/// goes through here, so how it is decided can change in one place.
pub(crate) fn current(state: &GameState) -> Option<&Phase> {
    let key = match &state.place {
        Place::Floor { phase, .. } => phase,
        Place::Camp => &state.phase,
    };
    state
        .blueprints
        .phases()
        .iter()
        .find(|phase| &phase.key == key)
}

/// `amount` of damage of `types` as the current phase leans it: stronger by
/// its `lean` when it carries the phase's element, for anyone who deals it.
pub(crate) fn leaned(state: &GameState, amount: i32, types: DamageTypes) -> i32 {
    match current(state) {
        Some(Phase {
            element: Some(element),
            lean,
            ..
        }) if types.contains(*element) => {
            let lean = i32::try_from(*lean).unwrap_or(0);
            amount + amount * lean / 100
        }
        _ => amount,
    }
}

/// Why `phases[index]` can't be waited for now, if it can't. Choosing a
/// phase goes through here and `WaitForPhase` only, so how phases are
/// earned can change without touching the frontends. Today any phase can
/// be chosen at camp.
pub(crate) fn refusal(state: &GameState, index: u32) -> Option<String> {
    if let Some(why) = wait_refusal(state) {
        return Some(why.to_string());
    }
    let known = usize::try_from(index).is_ok_and(|i| i < state.blueprints.phases().len());
    (!known).then(|| "There is no such phase.".to_string())
}

/// Why the player can't wait for a phase now, if they can't: only at camp.
pub fn wait_refusal(state: &GameState) -> Option<&'static str> {
    (state.place != Place::Camp).then_some("You can only wait for a phase at camp.")
}

/// `Command::WaitForPhase` applier: the next dive will be in `phases[index]`.
/// `reject_input` has checked the player is at camp and the phase exists.
pub(crate) fn wait_for(state: &mut GameState, actor: Entity, index: u32) {
    if actor != state.player {
        return;
    }
    let Some(phase) = state
        .blueprints
        .phases()
        .get(usize::try_from(index).unwrap_or(usize::MAX))
        .cloned()
    else {
        return;
    };
    state.phase = phase.key;
    state.log.push(phase.wait);
}

/// A phase as the camp's chooser shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseView {
    pub name: String,
    /// The phase the next dive will be in.
    pub chosen: bool,
    /// Why it can't be waited for now, if it can't (`refusal`).
    pub refused: Option<String>,
}

/// Every phase in order, the choices for which to wait for, each with why
/// it can't be chosen if it can't.
pub fn phases(state: &GameState) -> Vec<PhaseView> {
    (0..)
        .zip(state.blueprints.phases())
        .map(|(index, phase)| PhaseView {
            name: phase.name.clone(),
            chosen: phase.key == state.phase,
            refused: refusal(state, index),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::input::PlayerAction;
    use crate::testkit::last_log;
    use crate::time::take_turn;

    /// Phases for tests: a still one, where the world starts, and embers.
    pub(crate) fn test_phases() -> Vec<Phase> {
        let phase = |key: &str, name: &str, wait: &str| Phase {
            key: key.into(),
            name: name.into(),
            wait: wait.into(),
            element: None,
            lean: 0,
            opposite: None,
        };
        vec![
            phase("Still", "the stillness", "The air settles."),
            Phase {
                element: Some(DamageType::Fire),
                lean: 50,
                ..phase("Embers", "the embers", "Heat rises from below.")
            },
        ]
    }

    fn at_camp() -> GameState {
        let blueprints =
            crate::state::tests::test_blueprints().with_camp(Some(crate::camp::tests::test_camp()));
        GameState::new(9, blueprints).unwrap()
    }

    #[test]
    fn waiting_at_camp_chooses_the_next_dives_phase_and_takes_a_turn() {
        let mut state = at_camp();
        assert_eq!(state.phase, "Still", "the world starts in the first phase");
        let before = state.now;
        take_turn(&mut state, PlayerAction::WaitForPhase(1));
        assert_eq!(state.phase, "Embers");
        assert!(state.now > before);
        assert_eq!(last_log(&state), "Heat rises from below.");
        assert_eq!(
            phases(&state).iter().map(|p| p.chosen).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn waiting_is_refused_away_from_camp_or_for_no_phase() {
        let mut state = crate::state::tests::bare_state();
        take_turn(&mut state, PlayerAction::WaitForPhase(1));
        assert_eq!(state.phase, "Still");
        assert_eq!(last_log(&state), "You can only wait for a phase at camp.");

        let mut state = at_camp();
        let before = state.now;
        take_turn(&mut state, PlayerAction::WaitForPhase(7));
        assert_eq!(state.now, before);
        assert_eq!(last_log(&state), "There is no such phase.");
    }

    #[test]
    fn the_shipped_phases_load() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let phases = read(&data).unwrap();
        let keys: Vec<&str> = phases.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["Stillness", "Embers", "Rime", "Tempest", "Mire"]);
        assert_eq!(phases[0].element, None, "the stillness leans nowhere");
    }

    #[test]
    fn an_opposite_must_exist_and_name_its_phase_back() {
        let mut phases = test_phases();
        phases[1].opposite = Some("Rime".into());
        assert!(check(&phases).is_err(), "no such phase");
        phases[1].opposite = Some("Still".into());
        assert!(
            check(&phases).is_err(),
            "the stillness doesn't name it back"
        );
        phases[0].opposite = Some("Embers".into());
        assert!(check(&phases).is_ok());
    }

    #[test]
    fn the_lean_strengthens_its_element_only_and_only_in_its_phase() {
        let mut state = crate::state::tests::bare_state();
        let fire = DamageTypes::single(DamageType::Fire);
        let cold = DamageTypes::single(DamageType::Cold);
        assert_eq!(
            leaned(&state, 10, fire),
            10,
            "the stillness changes nothing"
        );
        state.phase = "Embers".into();
        state.place = Place::Camp;
        assert_eq!(leaned(&state, 10, fire), 15);
        assert_eq!(leaned(&state, 10, cold), 10);
        assert_eq!(leaned(&state, 10, DamageTypes::default()), 10);
        state.place = Place::Floor {
            dungeon: "Test".into(),
            phase: "Still".into(),
            dive: 1,
            floor: 1,
        };
        assert_eq!(leaned(&state, 10, fire), 10, "a dive keeps its own phase");
    }

    #[test]
    fn every_phase_is_a_choice_at_camp_and_none_away_from_it() {
        let state = at_camp();
        assert!(phases(&state).iter().all(|phase| phase.refused.is_none()));
        let state = crate::state::tests::bare_state();
        assert!(phases(&state).iter().all(|phase| phase.refused.is_some()));
    }
}
