//! Effects (ADR 0022): what just happened, for a frontend to animate: missiles in flight, damage dealt, misses and the like, deaths. The world has already moved on by the time they are read; nothing in the simulation reads them and they are never saved.

use hecs::Entity;

use crate::entity::Position;
use crate::ranged::Trail;
use crate::state::GameState;

/// One thing worth showing, in the order it happened. Positions are world
/// tiles, read when it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fx {
    Flight(Trail),
    /// `amount` got through; `blocked` was stopped by armor.
    Damage {
        at: (i32, i32),
        amount: i32,
        blocked: i32,
        to_player: bool,
    },
    Outcome {
        at: (i32, i32),
        kind: Outcome,
    },
    Death {
        at: (i32, i32),
    },
}

/// A blow that did no damage, or only some.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Miss,
    Graze,
    Dodge,
    Deflect,
    /// Armor took all of it.
    Absorbed,
}

/// The effects since the last call, oldest first.
pub fn take_fx(state: &mut GameState) -> Vec<Fx> {
    std::mem::take(&mut state.fx)
}

/// Records an effect at `entity`'s tile, if the player sees it: the
/// effects show no more than the log and the map do.
pub(crate) fn at(state: &mut GameState, entity: Entity, fx: impl FnOnce((i32, i32)) -> Fx) {
    let Ok(at) = state
        .world
        .get::<&Position>(entity)
        .map(|position| (position.x, position.y))
    else {
        return;
    };
    if state.visible.get(at.0, at.1) == Some(&true) {
        state.fx.push(fx(at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::bare_state;
    use crate::testkit::thing;

    #[test]
    fn effects_are_taken_once_and_only_where_the_player_sees() {
        let mut state = bare_state();
        let seen = thing("seen").at(2, 1).make(&mut state);
        let unseen = thing("unseen").at(3, 1).make(&mut state);
        state.visible.set(2, 1, true);
        state.visible.set(3, 1, false);
        at(&mut state, seen, |at| Fx::Death { at });
        at(&mut state, unseen, |at| Fx::Death { at });
        assert_eq!(take_fx(&mut state), [Fx::Death { at: (2, 1) }]);
        assert!(take_fx(&mut state).is_empty());
    }
}
