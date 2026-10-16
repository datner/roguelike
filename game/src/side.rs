//! Who fights whom (ADR 0015). Until there are factions there are two sides:
//! hostile creatures and the player's. A summon is on its owner's side, so
//! a goblin shaman's summons are hostile and the player's fight for them.

use hecs::{Entity, World};

use crate::entity::{Hostile, Summoned};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Player,
    Hostile,
}

/// Summons of summons are followed this far; deeper chains count as the
/// last owner reached.
const MAX_OWNERS: usize = 4;

pub(crate) fn side(world: &World, entity: Entity) -> Side {
    let mut who = entity;
    for _ in 0..MAX_OWNERS {
        match world.get::<&Summoned>(who) {
            Ok(summoned) => who = summoned.owner,
            Err(_) => break,
        }
    }
    if world.satisfies::<&Hostile>(who) {
        Side::Hostile
    } else {
        Side::Player
    }
}

pub(crate) fn enemies(world: &World, a: Entity, b: Entity) -> bool {
    side(world, a) != side(world, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::bare_state;
    use crate::testkit::thing;

    #[test]
    fn a_summon_takes_its_owners_side() {
        let mut state = bare_state();
        let player = state.player;
        let shaman = thing("shaman").health(5).hostile().make(&mut state);
        let wolf = thing("wolf").health(5).summoned_by(player).make(&mut state);
        let imp = thing("imp").health(5).summoned_by(shaman).make(&mut state);
        let world = &state.world;
        assert_eq!(side(world, wolf), Side::Player);
        assert_eq!(side(world, imp), Side::Hostile);
        assert!(enemies(world, wolf, imp));
        assert!(!enemies(world, player, wolf));
        assert!(enemies(world, player, shaman));
    }
}
