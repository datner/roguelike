//! Helpers for other crates' tests, behind the `testing` feature that only their dev-dependencies turn on: a frontend's flow test sets up what play would take many turns to reach. Never in play.

use crate::spawn::Spawn;
use crate::state::GameState;
use crate::view::ItemRef;

/// One of blueprint `name` in the player's pack, as if picked up.
pub fn give(state: &mut GameState, name: &str) -> Option<ItemRef> {
    let player = state.player;
    let thing = state
        .spawn(Spawn::blueprint(name).carried_by(player))
        .ok()?;
    ItemRef::of(&state.world, thing)
}

/// The player stands at `at`, on whatever is there.
pub fn put_player(state: &mut GameState, at: (i32, i32)) {
    let player = state.player;
    let _ = state
        .world
        .insert_one(player, crate::entity::Position { x: at.0, y: at.1 });
}
