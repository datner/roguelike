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
    /// The rules it bends, for everyone.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// The skill a use with stakes during it trains (`skill::train`), whose
    /// level softens its rules for the one who has it.
    #[serde(default)]
    pub attunement: Option<String>,
}

/// Each level of a phase's attunement takes this much off its rules' strength
/// for the attuned, in percent, up to `MOST_SOFTENED`.
const SOFTEN_PER_LEVEL: u32 = 5;
const MOST_SOFTENED: u32 = 75;

/// A rule a phase bends, with its strength in percent (docs/plans/phases.md).
/// Each is read in one place only, where its system already decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Rule {
    /// Held light reaches this much farther (`light.rs`).
    LightReach(u32),
    /// Stamina comes back this much more slowly (`technique::stamina`).
    StaminaRecovery(u32),
    /// A missile loses this much of its hit chance per tile it flies
    /// (`combat::odds`).
    RangedFalloff(u32),
    /// A step costs this much more energy (`apply_commands`).
    MoveCost(u32),
}

/// The current phase's strength for the rule `pick` names, as it bears on
/// `who` (summed, 0 when the phase doesn't bend it): their attunement to the
/// phase softens it, by `SOFTEN_PER_LEVEL` a level, never past
/// `MOST_SOFTENED`. `rule(state, who, |r| match r { Rule::MoveCost(p) =>
/// Some(p), _ => None })`.
pub(crate) fn rule(state: &GameState, who: Entity, pick: impl Fn(Rule) -> Option<u32>) -> u32 {
    let Some(phase) = current(state) else {
        return 0;
    };
    let (hard, kind): (Vec<_>, Vec<_>) = phase
        .rules
        .iter()
        .filter_map(|&r| pick(r).map(|pct| (r.is_hardship(), pct)))
        .partition(|&(hardship, _)| hardship);
    let boon: u32 = kind.iter().map(|&(_, pct)| pct).sum();
    let strength: u32 = hard.iter().map(|&(_, pct)| pct).sum();
    let level = attunement(state, phase).map_or(0, |skill| {
        crate::skill::level(crate::skill::xp(&state.world, who, skill))
    });
    let softened = (level * SOFTEN_PER_LEVEL).min(MOST_SOFTENED);
    boon + strength * (100 - softened) / 100
}

impl Rule {
    /// Whether it makes things harder, so attunement softens it; a boon
    /// (light reaching farther) is left whole.
    fn is_hardship(self) -> bool {
        !matches!(self, Rule::LightReach(_))
    }
}

/// `phase`'s attunement skill, if it has one the content knows.
fn attunement(state: &GameState, phase: &Phase) -> Option<crate::skill::SkillId> {
    let name = phase.attunement.as_ref()?;
    state.blueprints.skills().id(name)
}

/// The attunement a use with stakes trains now: the phase's of the dive the
/// player is on. Camp is in no phase yet.
pub(crate) fn attuning(state: &GameState) -> Option<crate::skill::SkillId> {
    on_dive(state).and_then(|phase| attunement(state, phase))
}

/// The phase of the dive the player is on; `None` at camp, which is in no
/// phase yet. What the character systems read (attunement, thriving styles,
/// discoveries), where `current` is what the world's rules read.
pub(crate) fn on_dive(state: &GameState) -> Option<&Phase> {
    match state.place {
        Place::Floor { .. } => current(state),
        Place::Camp => None,
    }
}

/// `value` grown by `pct` percent.
pub(crate) fn grown(value: i64, pct: u32) -> i64 {
    value + value * i64::from(pct) / 100
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
            rules: Vec::new(),
            attunement: None,
        };
        vec![
            phase("Still", "the stillness", "The air settles."),
            Phase {
                element: Some(DamageType::Fire),
                lean: 50,
                rules: vec![
                    Rule::LightReach(50),
                    Rule::StaminaRecovery(100),
                    Rule::RangedFalloff(10),
                    Rule::MoveCost(50),
                ],
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

    /// A bare state in the test embers, which bend every rule.
    fn in_embers() -> GameState {
        let mut state = crate::state::tests::bare_state();
        set_floor_phase(&mut state, "Embers");
        state
    }

    /// The bare state's floor in `phase`: a dive's own phase is what holds.
    pub(crate) fn set_floor_phase(state: &mut GameState, key: &str) {
        if let Place::Floor { phase, .. } = &mut state.place {
            *phase = key.into();
        }
    }

    #[test]
    fn move_cost_makes_a_step_dearer_only_in_its_phase() {
        use crate::entity::Energy;
        for (mut state, cost) in [
            (crate::state::tests::bare_state(), crate::cost::MOVE),
            (in_embers(), crate::cost::MOVE * 3 / 2),
        ] {
            let player = state.player;
            state.world.insert_one(player, Energy(0)).unwrap();
            state.run(crate::command::Command::TryMove(
                player,
                crate::input::Dir::E,
            ));
            assert_eq!(state.world.get::<&Energy>(player).unwrap().0, -cost);
        }
    }

    #[test]
    fn stamina_recovery_slows_stamina_only_in_its_phase() {
        use crate::entity::{Pool, Stamina};
        let pool = Stamina(Pool {
            value: 0,
            max: 10,
            as_of: crate::time::Tick::default(),
            regen_ticks: 100,
            growth: Vec::new(),
        });
        for (mut state, ticks) in [(crate::state::tests::bare_state(), 100), (in_embers(), 200)] {
            let player = state.player;
            state.world.insert_one(player, pool.clone()).unwrap();
            let (_, rates) = crate::technique::stamina(&state, player).unwrap();
            assert_eq!(rates.regen_ticks, ticks);
        }
    }

    #[test]
    fn light_reach_carries_a_held_light_farther_only_in_its_phase() {
        use crate::entity::Light;
        let lit_at_edge = |mut state: GameState| {
            let player = state.player;
            crate::testkit::thing("torch")
                .item()
                .with(Light {
                    radius: 2,
                    tint: crate::cell::Rgb(255, 150, 50),
                    tint_bp: 5_000,
                })
                .equipped_on(player, 0)
                .make(&mut state);
            crate::testkit::see_everything(&mut state);
            // Three tiles east of the player at (1, 1): past a radius of 2.
            crate::light::lit_by(&state)(4, 1).is_some()
        };
        assert!(!lit_at_edge(crate::state::tests::bare_state()));
        assert!(lit_at_edge(in_embers()));
    }

    /// A learner on a floor of the shipped data in `phase`, and a goblin.
    fn attuning_in(phase: &str) -> (GameState, Entity) {
        let data = crate::blueprint::Blueprints::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap();
        let mut state = crate::state::tests::bare_state_with(data);
        set_floor_phase(&mut state, phase);
        let player = state.player;
        let _ = state.world.insert_one(player, crate::entity::Learner);
        let goblin = state
            .spawn(crate::spawn::Spawn::blueprint("Goblin").at(3, 1))
            .unwrap();
        (state, goblin)
    }

    fn level_of(state: &GameState, who: Entity, skill: &str) -> u32 {
        let id = state.blueprints.skills().id(skill).unwrap();
        crate::skill::level(crate::skill::xp(&state.world, who, id))
    }

    #[test]
    fn a_use_with_stakes_on_a_dive_trains_its_phases_attunement_and_camp_none() {
        let (mut state, goblin) = attuning_in("Rime");
        let player = state.player;
        let xp = |state: &GameState| {
            let id = state.blueprints.skills().id("Rime attunement").unwrap();
            crate::skill::xp(&state.world, player, id)
        };
        crate::skill::train(&mut state, player, &[], goblin, 10);
        assert!(xp(&state) > 0);
        assert_eq!(level_of(&state, player, "Mire attunement"), 0);
        let before = xp(&state);
        state.place = Place::Camp;
        state.phase = "Rime".into();
        crate::skill::train(&mut state, player, &[], goblin, 10);
        assert_eq!(xp(&state), before, "camp is in no phase yet");
    }

    #[test]
    fn attunement_softens_a_hardship_for_the_attuned_only_and_never_the_lean() {
        let (mut state, goblin) = attuning_in("Mire");
        let player = state.player;
        let mud = |state: &GameState, who| {
            rule(state, who, |r| match r {
                Rule::MoveCost(pct) => Some(pct),
                _ => None,
            })
        };
        assert_eq!(mud(&state, player), 25);
        let id = state.blueprints.skills().id("Mire attunement").unwrap();
        let attuned = vec![(id, crate::skill::xp_for(10))];
        crate::skill::gain(&mut state, player, &attuned);
        assert_eq!(
            mud(&state, player),
            12,
            "half of 25, rounded down: 10 levels"
        );
        assert_eq!(mud(&state, goblin), 25, "the goblin is not attuned");
        let acid = DamageTypes::single(DamageType::Acid);
        assert_eq!(leaned(&state, 10, acid), 15, "the lean is everyone's");
    }

    #[test]
    fn a_style_thriving_in_the_dives_phase_gains_more() {
        let brawling_xp = |phase: &str| {
            let (mut state, goblin) = attuning_in(phase);
            let player = state.player;
            state
                .spawn(crate::spawn::Spawn::blueprint("Brawling").attached_to(player))
                .unwrap();
            let book = state.blueprints.skills();
            let used = [book.id("Melee").unwrap(), book.id("Unarmed").unwrap()];
            crate::skill::train(&mut state, player, &used, goblin, 10);
            let id = state.blueprints.skills().id("Brawling").unwrap();
            crate::skill::xp(&state.world, player, id)
        };
        assert_eq!(brawling_xp("Stillness"), 10);
        assert_eq!(brawling_xp("Embers"), 15, "brawling thrives in the embers");
    }

    #[test]
    fn a_phase_discovery_inspires_only_in_its_phase_with_stakes() {
        let hardened = |phase: &str| {
            let (mut state, goblin) = attuning_in(phase);
            let player = state.player;
            let book = state.blueprints.skills();
            let ready = vec![
                (book.id("Toughness").unwrap(), crate::skill::xp_for(3)),
                (
                    book.id("Embers attunement").unwrap(),
                    crate::skill::xp_for(2),
                ),
            ];
            crate::skill::gain(&mut state, player, &ready);
            let idea = crate::discovery::ideas(&state)
                .into_iter()
                .find(|idea| idea.name == "Ember-hardened")
                .unwrap();
            assert_eq!(
                idea.phase,
                Some(("the embers".to_string(), phase == "Embers"))
            );
            crate::skill::train(&mut state, player, &[], goblin, 10);
            crate::discovery::proficiencies(&state).contains(&"ember-hardened".to_string())
        };
        assert!(!hardened("Mire"));
        assert!(hardened("Embers"));
    }

    #[test]
    fn a_phase_naming_an_unknown_attunement_fails_to_load() {
        let mut phases = test_phases();
        phases[1].attunement = Some("Lava attunement".into());
        let loaded = crate::state::tests::test_blueprints().with_phases(phases);
        assert!(matches!(loaded, Err(BlueprintError::BadPhases(why)) if why.contains("Lava")));
    }

    #[test]
    fn every_phase_is_a_choice_at_camp_and_none_away_from_it() {
        let state = at_camp();
        assert!(phases(&state).iter().all(|phase| phase.refused.is_none()));
        let state = crate::state::tests::bare_state();
        assert!(phases(&state).iter().all(|phase| phase.refused.is_some()));
    }
}
