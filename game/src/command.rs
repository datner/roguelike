use hecs::Entity;

use crate::input::Dir;
use crate::map::Tile;
use crate::state::GameState;
use crate::entity::{BlocksMovement, Position};

#[derive(Debug, Clone, Copy)]
pub enum Command {
    TryMove(Entity, Dir),
}

pub fn apply_commands(state: &mut GameState) {
    while let Some(command) = state.commands.pop_front() {
        match command {
            Command::TryMove(entity, dir) => try_move(state, entity, dir),
        }
    }
}

fn try_move(state: &mut GameState, entity: Entity, dir: Dir) {
    let (dx, dy) = dir.delta();

    let target = {
        let Ok(current) = state.world.get::<&Position>(entity) else {
            return;
        };
        (current.x + dx, current.y + dy)
    };

    match state.map.get(target.0, target.1) {
        None | Some(Tile::Wall) => {
            state.log.push("You bump into a wall.");
            return;
        }
        Some(Tile::Floor) => {}
    }

    let blocked = state
        .world
        .query::<(&Position, &BlocksMovement)>()
        .iter()
        .any(|(pos, _)| (pos.x, pos.y) == target);
    if blocked {
        state.log.push("Something blocks your way.");
        return;
    }

    if let Ok(mut pos) = state.world.get::<&mut Position>(entity) {
        pos.x = target.0;
        pos.y = target.1;
    }
}
