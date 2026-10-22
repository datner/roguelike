//! Phases (`docs/vision.md`, docs/plans/dives.md): the world turns through phases, and at camp you choose which one to wait for. A dive is fixed to the phase it began in; a dungeon's spawn entries may belong to some phases only. Listed in `data/phases.ron`; the first is where the world starts.

use std::path::Path;

use hecs::Entity;
use serde::Deserialize;

use crate::blueprint::{BlueprintError, ron_options};
use crate::dive::Place;
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
    for (i, phase) in phases.iter().enumerate() {
        if phases[..i].iter().any(|other| other.key == phase.key) {
            return Err(BlueprintError::BadPhases(format!(
                "{:?} is listed twice",
                phase.key
            )));
        }
    }
    Ok(phases)
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
}

/// Every phase in order, for choosing which to wait for.
pub fn phases(state: &GameState) -> Vec<PhaseView> {
    state
        .blueprints
        .phases()
        .iter()
        .map(|phase| PhaseView {
            name: phase.name.clone(),
            chosen: phase.key == state.phase,
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
        };
        vec![
            phase("Still", "the stillness", "The air settles."),
            phase("Embers", "the embers", "Heat rises from below."),
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
        assert!(read(&data).unwrap().len() >= 2);
    }
}
