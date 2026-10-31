//! Properties over generated play (docs/plans/content-free-tests.md): what
//! the architecture promises, checked on many games instead of a few
//! examples. A game is a seed and a list of `Act`s played through the real
//! turn loop, so every state checked is one play can reach, and a failure
//! shrinks to a short list.

use std::sync::OnceLock;

use proptest::prelude::*;

use crate::aim::{Shot, aim, aim_targets};
use crate::blueprint::Blueprints;
use crate::entity::{BlocksMovement, Dead, Health, Position};
use crate::fx::{Fx, take_fx};
use crate::input::{Dir, PlayerAction};
use crate::map::Tile;
use crate::save::{from_ron, to_ron};
use crate::state::GameState;
use crate::time::take_turn;

/// One thing the player does, resolved against the state it is done in:
/// indices pick among what is there (a direction, a target in range, a known
/// spell), so any list of them is playable.
#[derive(Debug, Clone)]
enum Act {
    Move(usize),
    Wait,
    PickUp,
    Close,
    Sprint,
    Fire(usize),
    Throw(usize, usize),
    Cast(usize, usize),
    Perform(usize, usize),
}

fn act() -> impl Strategy<Value = Act> {
    prop_oneof![
        4 => (0..8usize).prop_map(Act::Move),
        1 => Just(Act::Wait),
        1 => Just(Act::PickUp),
        1 => Just(Act::Close),
        1 => Just(Act::Sprint),
        2 => any::<usize>().prop_map(Act::Fire),
        1 => (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Act::Throw(a, b)),
        2 => (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Act::Cast(a, b)),
        2 => (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Act::Perform(a, b)),
    ]
}

fn acts() -> impl Strategy<Value = Vec<Act>> {
    proptest::collection::vec(act(), 0..40)
}

/// Cases per property: 64 in `check.sh`, more with `PROPTEST_CASES`.
fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(64);
    ProptestConfig::with_cases(cases)
}

/// The test content, loaded once; games start on the first floor, not camp.
fn blueprints() -> Blueprints {
    static LOADED: OnceLock<Blueprints> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            Blueprints::load(&crate::testkit::data())
                .unwrap()
                .with_camp(None)
        })
        .clone()
}

fn pick<T: Clone>(list: &[T], index: usize) -> Option<T> {
    (!list.is_empty()).then(|| list[index % list.len()].clone())
}

/// The shot `act` takes and where, if there is anything to take it with.
fn shot(state: &GameState, act: &Act) -> Option<(Shot, (i32, i32))> {
    let (shot, at) = match *act {
        Act::Fire(at) => (Shot::Fire, at),
        Act::Throw(item, at) => {
            let items = crate::view::throwables(state);
            (Shot::Throw(pick(&items, item)?.item), at)
        }
        Act::Cast(spell, at) => {
            let spells = crate::view::spells(state);
            (Shot::Cast(pick(&spells, spell)?.spell), at)
        }
        Act::Perform(technique, at) => {
            let techniques = crate::technique::techniques(state);
            (Shot::Technique(pick(&techniques, technique)?.technique), at)
        }
        _ => return None,
    };
    let targets = aim_targets(state, shot);
    Some((shot, pick(&targets, at)?))
}

fn action(state: &GameState, act: &Act) -> PlayerAction {
    if let Some((shot, at)) = shot(state, act) {
        return match shot {
            Shot::Fire => PlayerAction::Fire { at },
            Shot::Throw(item) => PlayerAction::Throw { item, at },
            Shot::Cast(spell) => PlayerAction::Cast { spell, at },
            Shot::Technique(technique) => PlayerAction::Perform { technique, at },
        };
    }
    match *act {
        Act::Move(dir) => PlayerAction::Move(Dir::ALL[dir % Dir::ALL.len()]),
        Act::PickUp => PlayerAction::PickUp,
        Act::Close => PlayerAction::Close,
        Act::Sprint => PlayerAction::Sprint,
        _ => PlayerAction::Wait,
    }
}

fn alive(state: &GameState) -> bool {
    !state.world.satisfies::<&Dead>(state.player)
}

/// Plays `acts` from where `state` is, stopping if the player dies, checking
/// `each` after every turn.
fn play(
    state: &mut GameState,
    acts: &[Act],
    mut each: impl FnMut(&GameState) -> Result<(), TestCaseError>,
) -> Result<(), TestCaseError> {
    for act in acts {
        if !alive(state) {
            break;
        }
        take_turn(state, action(state, act));
        each(state)?;
    }
    Ok(())
}

fn new_game(seed: u64) -> GameState {
    GameState::new(seed, blueprints()).unwrap()
}

/// What must hold after any turn.
fn invariants(state: &GameState) -> Result<(), TestCaseError> {
    for health in state.world.query::<&Health>().iter() {
        prop_assert!(
            health.current <= health.max,
            "health over its max: {health:?}"
        );
    }
    let mut standing: Vec<(i32, i32)> = state
        .world
        .query::<&Position>()
        .with::<&BlocksMovement>()
        .without::<&Dead>()
        .iter()
        .map(|position| (position.x, position.y))
        .collect();
    for &(x, y) in &standing {
        prop_assert!(
            Tile::is_open(state.map.get(x, y)),
            "a creature stands in {:?} at ({x}, {y})",
            state.map.get(x, y)
        );
    }
    standing.sort_unstable();
    let before = standing.len();
    standing.dedup();
    prop_assert_eq!(standing.len(), before, "two creatures share a tile");
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    /// ADR 0005: the same seed and the same inputs make the same game.
    #[test]
    fn the_same_seed_and_acts_make_the_same_game(seed in 0..10_000u64, acts in acts()) {
        let mut first = new_game(seed);
        play(&mut first, &acts, |_| Ok(()))?;
        let mut second = new_game(seed);
        play(&mut second, &acts, |_| Ok(()))?;
        prop_assert_eq!(to_ron(&first).unwrap(), to_ron(&second).unwrap());
    }

    /// ADR 0008: a save loads into the same game, and that game goes on as
    /// the unsaved one would.
    #[test]
    fn a_save_loads_into_the_same_game(
        seed in 0..10_000u64,
        before in acts(),
        after in acts(),
    ) {
        let mut kept = new_game(seed);
        play(&mut kept, &before, |_| Ok(()))?;
        let saved = to_ron(&kept).unwrap();
        let mut loaded = from_ron(&saved, blueprints()).unwrap();
        prop_assert_eq!(&to_ron(&loaded).unwrap(), &saved, "save, load, save");
        play(&mut kept, &after, |_| Ok(()))?;
        play(&mut loaded, &after, |_| Ok(()))?;
        prop_assert_eq!(to_ron(&kept).unwrap(), to_ron(&loaded).unwrap(), "played on");
    }

    /// ADR 0013, 0014, 0024: a preview with no problem is an action that
    /// happens, one with a problem is refused and costs no time; a missile
    /// flies the previewed path and deals no more than the previewed damage.
    #[test]
    fn a_preview_says_what_the_action_does(seed in 0..10_000u64, acts in acts()) {
        let mut state = new_game(seed);
        for act in &acts {
            if !alive(&state) {
                break;
            }
            let Some((shot, at)) = shot(&state, act) else {
                let action = action(&state, act);
            take_turn(&mut state, action);
                continue;
            };
            let Some(preview) = aim(&state, shot, at) else {
                let action = action(&state, act);
            take_turn(&mut state, action);
                continue;
            };
            take_fx(&mut state);
            let before = state.now;
            let action = action(&state, act);
            take_turn(&mut state, action);
            let fx = take_fx(&mut state);
            prop_assert_eq!(
                state.now > before,
                preview.problem.is_none(),
                "{:?} at {:?}: previewed {:?}, last log {:?}",
                shot, at, preview.problem, crate::testkit::last_log(&state)
            );
            if preview.problem.is_some() || !matches!(shot, Shot::Fire | Shot::Throw(_)) {
                continue;
            }
            if let Some(Fx::Flight(trail)) = fx.first() {
                prop_assert_eq!(&trail.path, &preview.path, "the flight");
            }
            if preview.target.is_some()
                && let Some(&Fx::Damage { amount, .. }) = fx
                    .iter()
                    .find(|fx| matches!(fx, Fx::Damage { at, .. } if *at == preview.landing))
            {
                prop_assert!(amount <= preview.damage.1, "{} over {:?}", amount, preview.damage);
            }
        }
    }

    /// What must hold after every turn of any game.
    #[test]
    fn every_turn_keeps_the_invariants(seed in 0..10_000u64, acts in acts()) {
        let mut state = new_game(seed);
        invariants(&state)?;
        play(&mut state, &acts, invariants)?;
    }
}
